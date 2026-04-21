use std::collections::{HashMap, HashSet};
use std::io::Write;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::*;
use x11rb::protocol::Event;
use x11rb::wrapper::ConnectionExt as _;
use x11rb::COPY_DEPTH_FROM_PARENT;

// Layout constants
const WIN_W: u16 = 250;
const WIN_H: u16 = 600;
const ITEM_MARGIN: i16 = 8; // margin on each side
const ITEM_H: u16 = 28;
const ITEM_SPACING: i16 = 2;
const HEADER_H: u16 = ITEM_H; // "+ New terminal" button row at the top
const ITEM_Y_START: i16 = HEADER_H as i16 + ITEM_SPACING + 8;
const DRAG_THRESHOLD: i16 = 5;
const GROUP_INDENT: i16 = 16;
const MENU_ITEM_H: u16 = 24;
const MENU_PADDING: i16 = 4;
const MENU_MIN_W: u16 = 180;
const CHAR_WIDTH: i16 = 8; // approximate for Nimbus Mono L 13px

// How long a pending attach claim stays live waiting for its new window to
// appear before giving up. Cheap upper bound — typical gnome-terminal launch
// is ~300 ms on this machine; anything past a few seconds means the spawn
// failed or the user closed the window before it registered.
const PENDING_ATTACH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

// Colors — OneDark-inspired palette
const BG_COLOR: u32 = 0x282c34;
const TEXT_COLOR: u32 = 0xabb2bf;
const TEXT_DIM_COLOR: u32 = 0x5c6370;
const INDICATOR_COLOR: u32 = 0x61afef;
const GHOST_COLOR: u32 = 0x3e4451;
const ITEM_COLOR: u32 = 0x2c313a;
const ITEM_HOVER_COLOR: u32 = 0x333842;
const ITEM_ACTIVE_COLOR: u32 = 0x2d3340;
const ACTIVE_STRIPE_COLOR: u32 = 0x61afef;
const MENU_BG_COLOR: u32 = 0x21252b;
const MENU_BORDER_COLOR: u32 = 0x3e4451;
const MENU_HOVER_COLOR: u32 = 0x2c313a;
const GROUP_HEADER_COLOR: u32 = 0x21252b;
const SESSION_MARKER_COLOR: u32 = 0x98c379; // OneDark green — tmux-backed window indicator

// Accent colors for left-edge stripe (OneDark)
const ACCENT_COLORS: &[u32] = &[0xe06c75, 0x98c379, 0x61afef, 0xc678dd, 0xe5c07b, 0x56b6c2];
const GROUP_COLORS: &[u32] = &[0x61afef, 0xe06c75, 0x98c379, 0xc678dd, 0xe5c07b, 0x56b6c2];


// ── Atoms ──

struct Atoms {
    net_client_list: Atom,
    net_active_window: Atom,
    net_wm_name: Atom,
    net_wm_desktop: Atom,
    net_current_desktop: Atom,
    net_frame_extents: Atom,
    net_workarea: Atom,
    utf8_string: Atom,
    net_wm_window_type: Atom,
    net_wm_window_type_normal: Atom,
    wm_protocols: Atom,
    wm_delete_window: Atom,
    net_wm_pid: Atom,
    ptm_wake: Atom,
}

impl Atoms {
    fn new(conn: &impl Connection) -> Result<Self, Box<dyn std::error::Error>> {
        let c0 = conn.intern_atom(false, b"_NET_CLIENT_LIST")?;
        let c1 = conn.intern_atom(false, b"_NET_ACTIVE_WINDOW")?;
        let c2 = conn.intern_atom(false, b"_NET_WM_NAME")?;
        let c3 = conn.intern_atom(false, b"_NET_WM_DESKTOP")?;
        let c4 = conn.intern_atom(false, b"_NET_CURRENT_DESKTOP")?;
        let c5 = conn.intern_atom(false, b"_NET_FRAME_EXTENTS")?;
        let c6 = conn.intern_atom(false, b"_NET_WORKAREA")?;
        let c7 = conn.intern_atom(false, b"UTF8_STRING")?;
        let c8 = conn.intern_atom(false, b"_NET_WM_WINDOW_TYPE")?;
        let c9 = conn.intern_atom(false, b"_NET_WM_WINDOW_TYPE_NORMAL")?;
        let c10 = conn.intern_atom(false, b"WM_PROTOCOLS")?;
        let c11 = conn.intern_atom(false, b"WM_DELETE_WINDOW")?;
        let c12 = conn.intern_atom(false, b"_NET_WM_PID")?;
        let c13 = conn.intern_atom(false, b"_PTM_WAKE")?;
        Ok(Self {
            net_client_list: c0.reply()?.atom,
            net_active_window: c1.reply()?.atom,
            net_wm_name: c2.reply()?.atom,
            net_wm_desktop: c3.reply()?.atom,
            net_current_desktop: c4.reply()?.atom,
            net_frame_extents: c5.reply()?.atom,
            net_workarea: c6.reply()?.atom,
            utf8_string: c7.reply()?.atom,
            net_wm_window_type: c8.reply()?.atom,
            net_wm_window_type_normal: c9.reply()?.atom,
            wm_protocols: c10.reply()?.atom,
            wm_delete_window: c11.reply()?.atom,
            net_wm_pid: c12.reply()?.atom,
            ptm_wake: c13.reply()?.atom,
        })
    }
}

// Spawn a thread that wakes PTM every `interval` by sending an X11
// ClientMessage to our own window. The message carries the `ptm_wake` atom as
// its type, which the main event loop recognises and uses as a cue to
// refresh items. This is how tmux state changes (sessions created or
// destroyed outside PTM) get picked up — tmux doesn't fire X11 events of
// its own, so without this poll the sidebar would only update when some
// other X activity happens to trigger a refresh.
fn spawn_tmux_poll_thread(window: Window, wake_atom: Atom, interval: std::time::Duration) {
    std::thread::spawn(move || {
        // Give the main loop a moment to reach wait_for_event before we start
        // pinging, so our first wake doesn't race the initial refresh.
        std::thread::sleep(interval);
        let Ok((c, _)) = x11rb::connect(None) else {
            return;
        };
        loop {
            let data = ClientMessageData::from([0u32; 5]);
            let ev = ClientMessageEvent {
                response_type: 33, // ClientMessage
                format: 32,
                sequence: 0,
                window,
                type_: wake_atom,
                data,
            };
            let _ = c.send_event(false, window, EventMask::NO_EVENT, ev);
            let _ = c.flush();
            std::thread::sleep(interval);
        }
    });
}

// ── Data model ──

struct Item {
    wid: u32,
    label: String,
    #[allow(dead_code)]
    wm_class: String,
    accent_pixel: u32,
    custom_prefix: String,
    session: Option<String>,
}

impl Item {
    fn display_label(&self) -> String {
        if self.custom_prefix.is_empty() {
            self.label.clone()
        } else {
            format!("{}: {}", self.custom_prefix, self.label)
        }
    }
}

struct Group {
    id: u32,
    name: String,
    collapsed: bool,
    member_wids: Vec<u32>,
}

#[derive(Clone, Debug)]
enum DisplaySlot {
    Window(u32),
    Group(u32),
    Session(String), // orphan tmux session — exists but has no attached window
}

#[derive(Clone, Debug)]
enum DisplayRow {
    GroupHeader { group_id: u32 },
    Window { wid: u32, group_id: Option<u32> },
    Session { name: String, group_id: Option<u32> },
}

#[derive(Clone, Debug)]
enum MenuAction {
    CreateGroup,
    AddToGroup(u32),
    RemoveFromGroup,
    RenameGroup,
    DeleteGroup,
    RenameTab,
    AttachSession,
    RenameSession,
    KillSession,
}

struct MenuEntry {
    label: String,
    action: MenuAction,
}

struct ContextMenu {
    window: Window,
    pixmap: Pixmap,
    entries: Vec<MenuEntry>,
    target_row: usize,
    #[allow(dead_code)]
    x: i16,
    #[allow(dead_code)]
    y: i16,
    width: u16,
    height: u16,
    hover_index: Option<usize>,
}

struct DragState {
    source_row: usize,
    start_y: i16,
    current_y: i16,
    started: bool,
}

enum RenameTarget {
    Group(u32),
    Window(u32),
    Session(String), // rename a tmux session via `tmux rename-session`
}

struct RenameState {
    target: RenameTarget,
    text: String,
    cursor: usize, // byte position in text
}

// ── App ──

struct App {
    items: Vec<Item>,
    groups: Vec<Group>,
    display_order: Vec<DisplaySlot>,
    display_rows: Vec<DisplayRow>,
    next_group_id: u32,
    context_menu: Option<ContextMenu>,
    rename: Option<RenameState>,
    active_wid: Option<u32>,
    hover_row: Option<usize>,
    header_hovered: bool,
    drag: Option<DragState>,
    x: i16,
    y: i16,
    width: u16,
    height: u16,
    our_wid: u32,
    subscribed_wids: HashSet<u32>,
    // When a user clicks an orphan session row, PTM spawns a terminal that
    // will attach to that session. Process-tree-based marker detection can
    // fail for terminals that fork through a shared server pid (gnome-
    // terminal, konsole), so we instead watch for the next newly-appearing
    // window and claim it for the pending session.
    pending_attach: Option<(String, std::time::Instant)>,
}

impl App {
    fn new(our_wid: u32) -> Self {
        Self {
            items: Vec::new(),
            groups: Vec::new(),
            display_order: Vec::new(),
            display_rows: Vec::new(),
            next_group_id: 0,
            context_menu: None,
            rename: None,
            active_wid: None,
            hover_row: None,
            header_hovered: false,
            drag: None,
            x: 0,
            y: 0,
            width: WIN_W,
            height: WIN_H,
            our_wid,
            subscribed_wids: HashSet::new(),
            pending_attach: None,
        }
    }

    fn hit_test_header_button(&self, y: i16) -> bool {
        y >= 0 && y < HEADER_H as i16
    }

    fn build_display_rows(&mut self) {
        self.display_rows.clear();
        for slot in &self.display_order {
            match slot {
                DisplaySlot::Window(wid) => {
                    self.display_rows.push(DisplayRow::Window {
                        wid: *wid,
                        group_id: None,
                    });
                }
                DisplaySlot::Group(gid) => {
                    self.display_rows.push(DisplayRow::GroupHeader { group_id: *gid });
                    if let Some(group) = self.groups.iter().find(|g| g.id == *gid) {
                        if !group.collapsed {
                            for member_wid in &group.member_wids {
                                self.display_rows.push(DisplayRow::Window {
                                    wid: *member_wid,
                                    group_id: Some(*gid),
                                });
                            }
                        }
                    }
                }
                DisplaySlot::Session(name) => {
                    self.display_rows.push(DisplayRow::Session {
                        name: name.clone(),
                        group_id: None,
                    });
                }
            }
        }
    }

    fn item_x(&self) -> i16 {
        ITEM_MARGIN
    }

    fn item_w(&self) -> u16 {
        (self.width as i16 - ITEM_MARGIN * 2).max(20) as u16
    }

    fn row_y(&self, index: usize) -> i16 {
        ITEM_Y_START + (index as i16) * (ITEM_H as i16 + ITEM_SPACING)
    }

    fn hit_test_row(&self, y: i16) -> Option<usize> {
        for i in 0..self.display_rows.len() {
            let iy = self.row_y(i);
            if y >= iy && y < iy + ITEM_H as i16 {
                return Some(i);
            }
        }
        None
    }

    fn drop_index_from_y(&self, y: i16) -> usize {
        for i in 0..self.display_rows.len() {
            let mid = self.row_y(i) + (ITEM_H as i16 / 2);
            if y < mid {
                return i;
            }
        }
        self.display_rows.len()
    }

    fn find_item(&self, wid: u32) -> Option<&Item> {
        self.items.iter().find(|i| i.wid == wid)
    }

    // ── Group operations ──

    fn create_group(&mut self, wid: u32) {
        let gid = self.next_group_id;
        self.next_group_id += 1;
        let name = format!("Group {}", gid + 1);
        self.groups.push(Group {
            id: gid,
            name,
            collapsed: false,
            member_wids: vec![wid],
        });
        for slot in &mut self.display_order {
            if matches!(slot, DisplaySlot::Window(w) if *w == wid) {
                *slot = DisplaySlot::Group(gid);
                break;
            }
        }
        self.build_display_rows();
    }

    fn add_to_group(&mut self, gid: u32, wid: u32) {
        self.display_order
            .retain(|s| !matches!(s, DisplaySlot::Window(w) if *w == wid));
        for group in &mut self.groups {
            group.member_wids.retain(|w| *w != wid);
        }
        if let Some(group) = self.groups.iter_mut().find(|g| g.id == gid) {
            group.member_wids.push(wid);
        }
        self.build_display_rows();
    }

    fn remove_from_group(&mut self, wid: u32) {
        let mut group_gid = None;
        for group in &mut self.groups {
            if let Some(pos) = group.member_wids.iter().position(|w| *w == wid) {
                group_gid = Some(group.id);
                group.member_wids.remove(pos);
                break;
            }
        }
        if let Some(gid) = group_gid {
            let pos = self
                .display_order
                .iter()
                .position(|s| matches!(s, DisplaySlot::Group(g) if *g == gid));
            let insert_at = pos.map(|p| p + 1).unwrap_or(self.display_order.len());
            self.display_order
                .insert(insert_at, DisplaySlot::Window(wid));
        }
        self.build_display_rows();
    }

    fn delete_group(&mut self, gid: u32) {
        let group_pos = self.groups.iter().position(|g| g.id == gid);
        if let Some(gpos) = group_pos {
            let members = self.groups[gpos].member_wids.clone();
            self.groups.remove(gpos);
            let slot_pos = self
                .display_order
                .iter()
                .position(|s| matches!(s, DisplaySlot::Group(g) if *g == gid));
            if let Some(sp) = slot_pos {
                self.display_order.remove(sp);
                for (i, wid) in members.iter().enumerate() {
                    self.display_order
                        .insert(sp + i, DisplaySlot::Window(*wid));
                }
            }
        }
        self.build_display_rows();
    }

    fn start_rename(&mut self, gid: u32) {
        let text = self
            .groups
            .iter()
            .find(|g| g.id == gid)
            .map(|g| g.name.clone())
            .unwrap_or_default();
        let cursor = text.len();
        self.rename = Some(RenameState {
            target: RenameTarget::Group(gid),
            text,
            cursor,
        });
    }

    fn start_session_rename(&mut self, session_name: &str) {
        let text = session_name.to_string();
        let cursor = text.len();
        self.rename = Some(RenameState {
            target: RenameTarget::Session(session_name.to_string()),
            text,
            cursor,
        });
    }

    fn start_tab_rename(&mut self, wid: u32) {
        let text = self
            .items
            .iter()
            .find(|i| i.wid == wid)
            .map(|i| i.custom_prefix.clone())
            .unwrap_or_default();
        let cursor = text.len();
        self.rename = Some(RenameState {
            target: RenameTarget::Window(wid),
            text,
            cursor,
        });
    }

    fn commit_rename(&mut self) {
        if let Some(rs) = self.rename.take() {
            match rs.target {
                RenameTarget::Group(gid) => {
                    let name = rs.text.trim().to_string();
                    if !name.is_empty() {
                        if let Some(group) =
                            self.groups.iter_mut().find(|g| g.id == gid)
                        {
                            group.name = name;
                        }
                    }
                }
                RenameTarget::Window(wid) => {
                    let prefix = rs.text.trim().to_string();
                    if let Some(item) =
                        self.items.iter_mut().find(|i| i.wid == wid)
                    {
                        item.custom_prefix = prefix;
                    }
                }
                RenameTarget::Session(old) => {
                    let new_name = rs.text.trim().to_string();
                    if new_name.is_empty() || new_name == old {
                        return;
                    }
                    // Ask tmux to rename the session. If it fails (bad name,
                    // collision, server gone), leave the UI unchanged and
                    // let the next refresh reconcile with reality.
                    let ok = std::process::Command::new("tmux")
                        .args(["rename-session", "-t", &old, &new_name])
                        .status()
                        .map(|s| s.success())
                        .unwrap_or(false);
                    if ok {
                        // Rewrite the slot in-place so the row keeps its
                        // position in display_order across the next refresh.
                        for slot in &mut self.display_order {
                            if let DisplaySlot::Session(n) = slot {
                                if *n == old {
                                    *n = new_name.clone();
                                }
                            }
                        }
                        self.build_display_rows();
                    }
                }
            }
        }
    }

    fn cancel_rename(&mut self) {
        self.rename = None;
    }

    fn toggle_collapse(&mut self, gid: u32) {
        if let Some(group) = self.groups.iter_mut().find(|g| g.id == gid) {
            group.collapsed = !group.collapsed;
        }
        self.build_display_rows();
    }

    fn remove_wid_from_group(&mut self, gid: u32, wid: u32) {
        if let Some(group) = self.groups.iter_mut().find(|g| g.id == gid) {
            group.member_wids.retain(|w| *w != wid);
        }
    }

    // ── Drag-and-drop ──

    fn is_gap_in_group(&self, gap: usize, gid: u32) -> bool {
        let header_row = self
            .display_rows
            .iter()
            .position(|r| matches!(r, DisplayRow::GroupHeader { group_id } if *group_id == gid));
        let Some(hr) = header_row else {
            return false;
        };
        let mut last_member = hr;
        for (i, row) in self.display_rows.iter().enumerate().skip(hr + 1) {
            if matches!(row, DisplayRow::Window { group_id: Some(g), .. } if *g == gid) {
                last_member = i;
            } else {
                break;
            }
        }
        gap > hr && gap <= last_member + 1
    }

    fn display_row_to_slot_position(&self, gap: usize) -> usize {
        let mut row_count = 0;
        for (slot_idx, slot) in self.display_order.iter().enumerate() {
            if row_count >= gap {
                return slot_idx;
            }
            match slot {
                DisplaySlot::Window(_) => {
                    row_count += 1;
                }
                DisplaySlot::Session(_) => {
                    row_count += 1;
                }
                DisplaySlot::Group(gid) => {
                    row_count += 1;
                    if let Some(group) = self.groups.iter().find(|g| g.id == *gid) {
                        if !group.collapsed {
                            row_count += group.member_wids.len();
                        }
                    }
                }
            }
        }
        self.display_order.len()
    }

    fn move_slot_to(&mut self, target: &DisplaySlot, drop_gap: usize) {
        let src_pos = self.display_order.iter().position(|s| match (target, s) {
            (DisplaySlot::Window(a), DisplaySlot::Window(b)) => a == b,
            (DisplaySlot::Group(a), DisplaySlot::Group(b)) => a == b,
            (DisplaySlot::Session(a), DisplaySlot::Session(b)) => a == b,
            _ => false,
        });
        if let Some(sp) = src_pos {
            let dst = self.display_row_to_slot_position(drop_gap);
            let slot = self.display_order.remove(sp);
            let insert_at = if dst > sp {
                (dst - 1).min(self.display_order.len())
            } else {
                dst.min(self.display_order.len())
            };
            self.display_order.insert(insert_at, slot);
        }
    }

    fn reorder_within_group(&mut self, gid: u32, wid: u32, drop_gap: usize) {
        let header_row = self
            .display_rows
            .iter()
            .position(|r| matches!(r, DisplayRow::GroupHeader { group_id } if *group_id == gid));
        if let Some(hr) = header_row {
            if let Some(group) = self.groups.iter_mut().find(|g| g.id == gid) {
                let src_pos = group.member_wids.iter().position(|w| *w == wid);
                if let Some(sp) = src_pos {
                    let target_member = if drop_gap > hr + 1 {
                        drop_gap - hr - 1
                    } else {
                        0
                    };
                    group.member_wids.remove(sp);
                    let insert_at = if target_member > sp {
                        (target_member - 1).min(group.member_wids.len())
                    } else {
                        target_member.min(group.member_wids.len())
                    };
                    group.member_wids.insert(insert_at, wid);
                }
            }
        }
    }

    fn handle_drop(&mut self, source_row: usize, current_y: i16) {
        if source_row >= self.display_rows.len() {
            return;
        }
        let source = self.display_rows[source_row].clone();

        let on_header_gid = self.hit_test_row(current_y).and_then(|r| {
            if let DisplayRow::GroupHeader { group_id } = &self.display_rows[r] {
                Some(*group_id)
            } else {
                None
            }
        });

        let drop_gap = self.drop_index_from_y(current_y);

        match source {
            DisplayRow::GroupHeader { group_id } => {
                self.move_slot_to(&DisplaySlot::Group(group_id), drop_gap);
            }
            DisplayRow::Window {
                wid,
                group_id: src_gid,
            } => {
                if let Some(target_gid) = on_header_gid {
                    if src_gid == Some(target_gid) {
                        return;
                    }
                    if let Some(gid) = src_gid {
                        self.remove_wid_from_group(gid, wid);
                    } else {
                        self.display_order
                            .retain(|s| !matches!(s, DisplaySlot::Window(w) if *w == wid));
                    }
                    if let Some(g) = self.groups.iter_mut().find(|g| g.id == target_gid) {
                        g.member_wids.push(wid);
                    }
                } else if let Some(src_gid) = src_gid {
                    if self.is_gap_in_group(drop_gap, src_gid) {
                        self.reorder_within_group(src_gid, wid, drop_gap);
                    } else {
                        self.remove_wid_from_group(src_gid, wid);
                        let slot_pos = self.display_row_to_slot_position(drop_gap);
                        self.display_order
                            .insert(slot_pos, DisplaySlot::Window(wid));
                    }
                } else {
                    self.move_slot_to(&DisplaySlot::Window(wid), drop_gap);
                }
            }
            DisplayRow::Session { name, .. } => {
                // Sessions are standalone rows for now (no groups until C.5);
                // dragging just reorders them inside display_order.
                self.move_slot_to(&DisplaySlot::Session(name), drop_gap);
            }
        }

        self.build_display_rows();
    }
}

// ── EWMH helpers ──

fn get_client_list(
    conn: &impl Connection,
    root: Window,
    atoms: &Atoms,
) -> Result<Vec<u32>, Box<dyn std::error::Error>> {
    let reply = conn
        .get_property(
            false,
            root,
            atoms.net_client_list,
            AtomEnum::WINDOW,
            0,
            4096,
        )?
        .reply()?;
    Ok(reply
        .value
        .chunks_exact(4)
        .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect())
}

fn get_active_window(
    conn: &impl Connection,
    root: Window,
    atoms: &Atoms,
) -> Result<Option<u32>, Box<dyn std::error::Error>> {
    let reply = conn
        .get_property(
            false,
            root,
            atoms.net_active_window,
            AtomEnum::WINDOW,
            0,
            1,
        )?
        .reply()?;
    if reply.value.len() >= 4 {
        let wid = u32::from_le_bytes([
            reply.value[0],
            reply.value[1],
            reply.value[2],
            reply.value[3],
        ]);
        if wid == 0 {
            Ok(None)
        } else {
            Ok(Some(wid))
        }
    } else {
        Ok(None)
    }
}

fn get_window_pid(conn: &impl Connection, wid: u32, atoms: &Atoms) -> Option<u32> {
    let reply = conn
        .get_property(false, wid, atoms.net_wm_pid, AtomEnum::CARDINAL, 0, 1)
        .ok()?
        .reply()
        .ok()?;
    if reply.value.len() >= 4 {
        Some(u32::from_le_bytes([
            reply.value[0],
            reply.value[1],
            reply.value[2],
            reply.value[3],
        ]))
    } else {
        None
    }
}

fn get_window_title(
    conn: &impl Connection,
    wid: u32,
    atoms: &Atoms,
) -> Result<String, Box<dyn std::error::Error>> {
    let reply = conn
        .get_property(false, wid, atoms.net_wm_name, atoms.utf8_string, 0, 1024)?
        .reply()?;
    let title = String::from_utf8_lossy(&reply.value).into_owned();
    if !title.is_empty() {
        return Ok(title);
    }
    let reply = conn
        .get_property(false, wid, AtomEnum::WM_NAME, AtomEnum::STRING, 0, 1024)?
        .reply()?;
    Ok(String::from_utf8_lossy(&reply.value).into_owned())
}

fn get_wm_class(
    conn: &impl Connection,
    wid: u32,
) -> Result<(String, String), Box<dyn std::error::Error>> {
    let reply = conn
        .get_property(false, wid, AtomEnum::WM_CLASS, AtomEnum::STRING, 0, 256)?
        .reply()?;
    let s = String::from_utf8_lossy(&reply.value);
    let mut parts = s.split('\0').filter(|p| !p.is_empty());
    let instance = parts.next().unwrap_or("").to_string();
    let class = parts.next().unwrap_or("").to_string();
    Ok((instance, class))
}

fn get_current_desktop(
    conn: &impl Connection,
    root: Window,
    atoms: &Atoms,
) -> Result<u32, Box<dyn std::error::Error>> {
    let reply = conn
        .get_property(
            false,
            root,
            atoms.net_current_desktop,
            AtomEnum::CARDINAL,
            0,
            1,
        )?
        .reply()?;
    if reply.value.len() >= 4 {
        Ok(u32::from_le_bytes([
            reply.value[0],
            reply.value[1],
            reply.value[2],
            reply.value[3],
        ]))
    } else {
        Ok(0)
    }
}

fn get_window_desktop(
    conn: &impl Connection,
    wid: u32,
    atoms: &Atoms,
) -> Result<Option<u32>, Box<dyn std::error::Error>> {
    let reply = conn
        .get_property(
            false,
            wid,
            atoms.net_wm_desktop,
            AtomEnum::CARDINAL,
            0,
            1,
        )?
        .reply()?;
    if reply.value.len() >= 4 {
        let d = u32::from_le_bytes([
            reply.value[0],
            reply.value[1],
            reply.value[2],
            reply.value[3],
        ]);
        if d == 0xFFFFFFFF {
            Ok(None)
        } else {
            Ok(Some(d))
        }
    } else {
        Ok(None)
    }
}

fn is_normal_window(
    conn: &impl Connection,
    wid: u32,
    atoms: &Atoms,
) -> Result<bool, Box<dyn std::error::Error>> {
    let reply = conn
        .get_property(
            false,
            wid,
            atoms.net_wm_window_type,
            AtomEnum::ATOM,
            0,
            32,
        )?
        .reply()?;
    if reply.value.is_empty() {
        return Ok(true); // No type set → treat as normal
    }
    let type_atoms: Vec<u32> = reply
        .value
        .chunks_exact(4)
        .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect();
    Ok(type_atoms.contains(&atoms.net_wm_window_type_normal))
}

fn get_frame_extents(
    conn: &impl Connection,
    wid: u32,
    atoms: &Atoms,
) -> Result<(i32, i32, i32, i32), Box<dyn std::error::Error>> {
    let reply = conn
        .get_property(
            false,
            wid,
            atoms.net_frame_extents,
            AtomEnum::CARDINAL,
            0,
            4,
        )?
        .reply()?;
    if reply.value.len() >= 16 {
        let left =
            u32::from_le_bytes([reply.value[0], reply.value[1], reply.value[2], reply.value[3]])
                as i32;
        let right =
            u32::from_le_bytes([reply.value[4], reply.value[5], reply.value[6], reply.value[7]])
                as i32;
        let top = u32::from_le_bytes([
            reply.value[8],
            reply.value[9],
            reply.value[10],
            reply.value[11],
        ]) as i32;
        let bottom = u32::from_le_bytes([
            reply.value[12],
            reply.value[13],
            reply.value[14],
            reply.value[15],
        ]) as i32;
        Ok((left, right, top, bottom))
    } else {
        Ok((0, 0, 0, 0))
    }
}

fn get_workarea(
    conn: &impl Connection,
    root: Window,
    atoms: &Atoms,
    desktop: u32,
) -> Result<(i32, i32, u32, u32), Box<dyn std::error::Error>> {
    let reply = conn
        .get_property(
            false,
            root,
            atoms.net_workarea,
            AtomEnum::CARDINAL,
            0,
            1024,
        )?
        .reply()?;
    let offset = (desktop as usize) * 16;
    if reply.value.len() >= offset + 16 {
        let v = &reply.value[offset..];
        let x = u32::from_le_bytes([v[0], v[1], v[2], v[3]]) as i32;
        let y = u32::from_le_bytes([v[4], v[5], v[6], v[7]]) as i32;
        let w = u32::from_le_bytes([v[8], v[9], v[10], v[11]]);
        let h = u32::from_le_bytes([v[12], v[13], v[14], v[15]]);
        Ok((x, y, w, h))
    } else {
        Ok((0, 0, 1920, 1080))
    }
}

fn get_window_geometry(
    conn: &impl Connection,
    wid: u32,
) -> Result<(i32, i32, u32, u32), Box<dyn std::error::Error>> {
    let geo = conn.get_geometry(wid)?.reply()?;
    let trans = conn.translate_coordinates(wid, geo.root, 0, 0)?.reply()?;
    Ok((
        trans.dst_x as i32,
        trans.dst_y as i32,
        geo.width as u32,
        geo.height as u32,
    ))
}

fn activate_window(
    conn: &impl Connection,
    root: Window,
    wid: u32,
    atoms: &Atoms,
) -> Result<(), Box<dyn std::error::Error>> {
    let data = ClientMessageData::from([2u32, 0, 0, 0, 0]);
    let event = ClientMessageEvent {
        response_type: 33,
        format: 32,
        sequence: 0,
        window: wid,
        type_: atoms.net_active_window,
        data,
    };
    conn.send_event(
        false,
        root,
        EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
        event,
    )?;
    conn.flush()?;
    Ok(())
}

fn move_window(
    conn: &impl Connection,
    wid: u32,
    x: i32,
    y: i32,
) -> Result<(), Box<dyn std::error::Error>> {
    conn.configure_window(wid, &ConfigureWindowAux::new().x(x).y(y))?;
    conn.flush()?;
    Ok(())
}

fn snap_to_sidebar(
    conn: &impl Connection,
    root: Window,
    our_wid: u32,
    target_wid: u32,
    atoms: &Atoms,
) -> Result<(), Box<dyn std::error::Error>> {
    let (our_x, our_y, our_w, _our_h) = get_window_geometry(conn, our_wid)?;
    let our_frame = get_frame_extents(conn, our_wid, atoms)?;
    let target_frame = get_frame_extents(conn, target_wid, atoms)?;

    let desktop = get_current_desktop(conn, root, atoms)?;
    let (wa_x, wa_y, wa_w, _wa_h) = get_workarea(conn, root, atoms, desktop)?;

    let our_frame_right = our_x + our_w as i32 + our_frame.1;
    let target_x = our_frame_right + target_frame.0;
    let our_frame_top = our_y - our_frame.2;
    let target_y = our_frame_top + target_frame.2;

    let max_x = wa_x + wa_w as i32;
    let target_x = target_x.min(max_x);
    let target_y = target_y.max(wa_y + target_frame.2);

    move_window(conn, target_wid, target_x, target_y)?;
    Ok(())
}

// ── Refresh ──

fn refresh_items(
    conn: &impl Connection,
    root: Window,
    atoms: &Atoms,
    app: &mut App,
    colormap: Colormap,
) -> Result<(), Box<dyn std::error::Error>> {
    let wids = get_client_list(conn, root, atoms)?;
    let current_desktop = get_current_desktop(conn, root, atoms).unwrap_or(0);
    let tmux_clients = list_tmux_clients();

    // Snapshot wids we already knew about so claim_pending_attach can spot
    // windows that appeared since the last refresh.
    let prior_wids: HashSet<u32> = app.items.iter().map(|i| i.wid).collect();

    let mut live_wids = HashSet::new();
    let mut new_items = Vec::new();
    let mut color_idx = 0usize;
    // Multiple windows can share a PID (e.g. every gnome-terminal window
    // reports the server PID). Keep a list so the walk can detect collisions
    // and refuse to guess, rather than silently attributing a tmux session
    // to whichever window happened to enumerate first.
    let mut pid_to_wid: HashMap<u32, Vec<u32>> = HashMap::new();

    for wid in wids {
        if wid == app.our_wid {
            continue;
        }
        match get_window_desktop(conn, wid, atoms) {
            Ok(Some(d)) if d != current_desktop => continue,
            _ => {}
        }
        if !is_normal_window(conn, wid, atoms).unwrap_or(true) {
            continue;
        }

        live_wids.insert(wid);

        // Subscribe to property changes on this window once, so title edits
        // and other updates land in the main event loop without polling.
        if !app.subscribed_wids.contains(&wid) {
            let _ = conn.change_window_attributes(
                wid,
                &ChangeWindowAttributesAux::new().event_mask(EventMask::PROPERTY_CHANGE),
            );
            app.subscribed_wids.insert(wid);
        }

        let title = get_window_title(conn, wid, atoms).unwrap_or_default();
        let (_instance, class) = get_wm_class(conn, wid).unwrap_or_default();
        let display = if title.is_empty() { class.clone() } else { title };

        let accent = ACCENT_COLORS[color_idx % ACCENT_COLORS.len()];
        let accent_pixel = alloc_color(conn, colormap, accent)?;
        color_idx += 1;

        let custom_prefix = app
            .items
            .iter()
            .find(|i| i.wid == wid)
            .map(|i| i.custom_prefix.clone())
            .unwrap_or_default();

        if let Some(pid) = get_window_pid(conn, wid, atoms) {
            pid_to_wid.entry(pid).or_insert_with(Vec::new).push(wid);
        }

        new_items.push(Item {
            wid,
            label: display,
            wm_class: class,
            accent_pixel,
            custom_prefix,
            session: None,
        });
    }

    // Pending-attach takes priority over the ancestor walk. If the user
    // just clicked an orphan row, we know which session to expect on the
    // next new window — much more reliable than walking through a
    // gnome-terminal-server pid collision.
    let pre_assigned = claim_pending_attach(
        &mut app.pending_attach,
        &prior_wids,
        &mut new_items,
        PENDING_ATTACH_TIMEOUT,
        std::time::Instant::now(),
    );

    // Assign tmux sessions by walking UP from each tmux client's PID until
    // we hit a pid that's a tracked window's _NET_WM_PID. This is the
    // closest-window-to-client match — walking DOWN from a window's pid is
    // wrong when many windows share a launcher pid (e.g. cinnamon-session),
    // because the launcher's descendants include unrelated terminals too.
    if !tmux_clients.is_empty() && !pid_to_wid.is_empty() {
        for (&client_pid, session_name) in &tmux_clients {
            if pre_assigned.contains(session_name) {
                continue;
            }
            if let Some(wid) =
                walk_to_window_owner(client_pid, &pid_to_wid, read_ppid, 20)
            {
                if let Some(item) = new_items.iter_mut().find(|i| i.wid == wid) {
                    item.session = Some(session_name.clone());
                }
            }
        }
    }

    // Remove dead wids from groups
    for group in &mut app.groups {
        group.member_wids.retain(|w| live_wids.contains(w));
    }

    // Drop subscription bookkeeping for wids that have gone away.
    // (X11 already stopped delivering events for the destroyed window; we just
    // prune our HashSet so a wid that's later re-used gets a fresh subscribe.)
    app.subscribed_wids.retain(|w| live_wids.contains(w));

    // Orphan tmux sessions — sessions that exist but aren't attached to any
    // window PTM already tracks. These get standalone rows so the user can
    // see them and reattach; without this they'd be invisible outside
    // `tmux ls`.
    let attached_session_names: HashSet<String> = new_items
        .iter()
        .filter_map(|i| i.session.clone())
        .collect();
    let live_sessions = list_tmux_sessions();
    let live_session_names: HashSet<String> = live_sessions
        .iter()
        .map(|(n, _)| n.clone())
        .collect();
    let orphan_session_names: HashSet<String> = live_session_names
        .difference(&attached_session_names)
        .cloned()
        .collect();

    // Remove dead entries from display_order. Sessions are kept iff they
    // still exist in tmux AND aren't currently attached to a tracked window
    // (attached sessions are represented by the window row, not a Session row).
    app.display_order.retain(|slot| match slot {
        DisplaySlot::Window(wid) => live_wids.contains(wid),
        DisplaySlot::Group(gid) => app.groups.iter().any(|g| g.id == *gid),
        DisplaySlot::Session(name) => orphan_session_names.contains(name),
    });

    // Collect wids already tracked
    let mut known_wids = HashSet::new();
    for slot in &app.display_order {
        if let DisplaySlot::Window(wid) = slot {
            known_wids.insert(*wid);
        }
    }
    for group in &app.groups {
        for wid in &group.member_wids {
            known_wids.insert(*wid);
        }
    }

    // Add new wids to display_order
    for wid in &live_wids {
        if !known_wids.contains(wid) {
            app.display_order.push(DisplaySlot::Window(*wid));
        }
    }

    // Add new orphan sessions we haven't seen before, appended at the bottom.
    let known_session_names: HashSet<String> = app
        .display_order
        .iter()
        .filter_map(|s| {
            if let DisplaySlot::Session(n) = s {
                Some(n.clone())
            } else {
                None
            }
        })
        .collect();
    for name in &orphan_session_names {
        if !known_session_names.contains(name) {
            app.display_order
                .push(DisplaySlot::Session(name.clone()));
        }
    }

    // Update active window
    app.active_wid = get_active_window(conn, root, atoms).unwrap_or(None);

    app.items = new_items;
    app.build_display_rows();
    Ok(())
}

// ── tmux session detection ──
//
// Strategy: ask tmux for its attached clients (PID + session name), then for
// each client walk UP the /proc parent chain until we hit a PID that matches
// some tracked window's _NET_WM_PID. That ancestor is the window hosting the
// tmux client, so we tag it with the session name.
//
// The obvious alternative — walk DOWN from each window's _NET_WM_PID looking
// for tmux-client PIDs — is wrong on desktops where the WM sets _NET_WM_PID
// to a shared launcher process (e.g. cinnamon-session) for many windows.
// The launcher's descendant tree contains every GUI app the user ever
// launched, so every one of those windows would falsely match the tmux
// client. Walking up from the client instead finds the CLOSEST owning
// window and stops there.

fn parse_tmux_list_clients(stdout: &str) -> HashMap<u32, String> {
    let mut map = HashMap::new();
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some((pid_str, name)) = line.split_once(char::is_whitespace) {
            if let Ok(pid) = pid_str.parse::<u32>() {
                let name = name.trim();
                if !name.is_empty() {
                    map.insert(pid, name.to_string());
                }
            }
        }
    }
    map
}

fn list_tmux_clients() -> HashMap<u32, String> {
    match std::process::Command::new("tmux")
        .args(["list-clients", "-F", "#{client_pid} #{session_name}"])
        .output()
    {
        Ok(o) => parse_tmux_list_clients(&String::from_utf8_lossy(&o.stdout)),
        Err(_) => HashMap::new(),
    }
}

// Parse `tmux list-sessions -F '#{session_name} #{session_attached}'` output
// into (name, attached_count>0) pairs. Sessions whose first token after the
// name isn't a number are silently dropped.
fn parse_tmux_list_sessions(stdout: &str) -> Vec<(String, bool)> {
    let mut out = Vec::new();
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // Split on the LAST whitespace so session names with spaces survive.
        if let Some(idx) = line.rfind(char::is_whitespace) {
            let name = line[..idx].trim();
            let attached_str = line[idx + 1..].trim();
            if name.is_empty() {
                continue;
            }
            if let Ok(n) = attached_str.parse::<u32>() {
                out.push((name.to_string(), n > 0));
            }
        }
    }
    out
}

fn list_tmux_sessions() -> Vec<(String, bool)> {
    match std::process::Command::new("tmux")
        .args([
            "list-sessions",
            "-F",
            "#{session_name} #{session_attached}",
        ])
        .output()
    {
        Ok(o) => parse_tmux_list_sessions(&String::from_utf8_lossy(&o.stdout)),
        Err(_) => Vec::new(),
    }
}

fn parse_proc_status_ppid(s: &str) -> Option<u32> {
    for line in s.lines() {
        if let Some(rest) = line.strip_prefix("PPid:") {
            return rest.trim().parse::<u32>().ok();
        }
    }
    None
}

fn read_ppid(pid: u32) -> Option<u32> {
    let s = std::fs::read_to_string(format!("/proc/{}/status", pid)).ok()?;
    parse_proc_status_ppid(&s)
}

// Walk up from `start_pid` via `read_ppid_fn` until we hit a pid that exists
// in `pid_to_wid`, or we hit init (ppid=1), or we run out of depth. Returns
// the matched wid of the closest owning window, if any. Pure — the /proc
// reader is injected so this can be tested without a running process tree.
//
// Collision semantics: if the hit pid maps to multiple windows (e.g.
// gnome-terminal-server hosts every gnome-terminal window under one pid),
// we give up and return None rather than pick arbitrarily. "No marker" is
// strictly better than "wrong marker." Climbing further past an ambiguous
// hit would only find unrelated ancestors (systemd, init), so stop here.
fn walk_to_window_owner(
    start_pid: u32,
    pid_to_wid: &HashMap<u32, Vec<u32>>,
    mut read_ppid_fn: impl FnMut(u32) -> Option<u32>,
    max_depth: usize,
) -> Option<u32> {
    let mut cur = start_pid;
    for _ in 0..max_depth {
        if let Some(wids) = pid_to_wid.get(&cur) {
            if wids.len() == 1 {
                return Some(wids[0]);
            }
            return None;
        }
        match read_ppid_fn(cur) {
            Some(ppid) if ppid > 1 => cur = ppid,
            _ => return None,
        }
    }
    None
}

// Consume a pending attach: if a new wid has appeared since the previous
// refresh, assign the pending session to it. Returns the set of sessions
// that were pre-assigned so the ancestor-walk loop can skip them.
//
// "Exactly one new wid" is the safe case — we know which window is ours.
// Zero new wids → window hasn't appeared yet, keep waiting. Multiple new
// wids → can't disambiguate (user opened something else in parallel), also
// keep waiting. Either way, the pending entry stays until it times out.
//
// Pure: `now` is injected so tests can simulate timeout without sleeping.
fn claim_pending_attach(
    pending: &mut Option<(String, std::time::Instant)>,
    prior_wids: &HashSet<u32>,
    new_items: &mut [Item],
    timeout: std::time::Duration,
    now: std::time::Instant,
) -> HashSet<String> {
    let mut pre_assigned = HashSet::new();
    let Some((session_name, spawn_time)) = pending.as_ref() else {
        return pre_assigned;
    };
    if now.saturating_duration_since(*spawn_time) > timeout {
        *pending = None;
        return pre_assigned;
    }
    let new_wids: Vec<u32> = new_items
        .iter()
        .map(|i| i.wid)
        .filter(|w| !prior_wids.contains(w))
        .collect();
    if new_wids.len() != 1 {
        return pre_assigned;
    }
    let claimed_wid = new_wids[0];
    let session = session_name.clone();
    if let Some(item) = new_items.iter_mut().find(|i| i.wid == claimed_wid) {
        item.session = Some(session.clone());
        pre_assigned.insert(session);
    }
    *pending = None;
    pre_assigned
}

// ── Terminal launch ──
//
// PTM delegates terminal configuration to the system: whatever the user has
// set up as their default is what PTM launches. No tmux wrapping, no shell
// arguments, no session naming owned by PTM. If the user's shell rc
// auto-attaches to tmux, they get tmux; otherwise they get a plain shell.

fn detect_terminal_command(
    env_terminal: Option<&str>,
    has_binary: impl Fn(&str) -> bool,
) -> Vec<String> {
    if let Some(term) = env_terminal {
        let trimmed = term.trim();
        if !trimmed.is_empty() {
            return trimmed.split_whitespace().map(String::from).collect();
        }
    }
    for candidate in ["x-terminal-emulator", "xdg-terminal-exec"] {
        if has_binary(candidate) {
            return vec![candidate.to_string()];
        }
    }
    vec!["xterm".to_string()]
}

fn binary_on_path(name: &str) -> bool {
    let Ok(path) = std::env::var("PATH") else {
        return false;
    };
    for dir in path.split(':') {
        if dir.is_empty() {
            continue;
        }
        let mut p = std::path::PathBuf::from(dir);
        p.push(name);
        if p.is_file() {
            return true;
        }
    }
    false
}

fn spawn_default_terminal() {
    let argv = detect_terminal_command(
        std::env::var("TERMINAL").ok().as_deref(),
        binary_on_path,
    );
    if argv.is_empty() {
        return;
    }
    let _ = std::process::Command::new(&argv[0])
        .args(&argv[1..])
        .spawn();
}

// Build the argv for launching a terminal attached to an existing tmux
// session. Different terminal emulators use different separators before
// the command: gnome-terminal / ptyxis need `--`, almost everything else
// (xterm, urxvt, alacritty, kitty, st, konsole) uses `-e`. Unknown
// terminals fall through to `-e` as a reasonable default.
fn terminal_argv_for_attach(term_argv: &[String], session_name: &str) -> Vec<String> {
    if term_argv.is_empty() {
        return Vec::new();
    }
    let term_name = std::path::Path::new(&term_argv[0])
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    let separator = match term_name {
        "gnome-terminal" | "ptyxis" => "--",
        _ => "-e",
    };
    let mut out: Vec<String> = term_argv.to_vec();
    out.push(separator.to_string());
    out.push("tmux".to_string());
    out.push("attach-session".to_string());
    out.push("-t".to_string());
    out.push(session_name.to_string());
    out
}

fn spawn_attach_terminal(session_name: &str) {
    let term = detect_terminal_command(
        std::env::var("TERMINAL").ok().as_deref(),
        binary_on_path,
    );
    let argv = terminal_argv_for_attach(&term, session_name);
    if argv.is_empty() {
        return;
    }
    let _ = std::process::Command::new(&argv[0])
        .args(&argv[1..])
        .spawn();
}

// ── Property-change classification (pure, testable) ──

#[derive(Debug, PartialEq, Eq)]
enum PropertyAction {
    RefreshClientList,
    UpdateActiveWindow,
    UpdateWindowTitle,
    Ignore,
}

fn classify_property_event(
    atom: u32,
    is_root: bool,
    net_client_list: u32,
    net_active_window: u32,
    net_wm_name: u32,
) -> PropertyAction {
    if is_root {
        if atom == net_client_list {
            PropertyAction::RefreshClientList
        } else if atom == net_active_window {
            PropertyAction::UpdateActiveWindow
        } else {
            PropertyAction::Ignore
        }
    } else if atom == net_wm_name || atom == u32::from(AtomEnum::WM_NAME) {
        PropertyAction::UpdateWindowTitle
    } else {
        PropertyAction::Ignore
    }
}

// ── Helpers ──

fn alloc_color(
    conn: &impl Connection,
    cmap: Colormap,
    rgb: u32,
) -> Result<u32, Box<dyn std::error::Error>> {
    let r = ((rgb >> 16) & 0xff) as u16 * 257;
    let g = ((rgb >> 8) & 0xff) as u16 * 257;
    let b = (rgb & 0xff) as u16 * 257;
    let reply = conn.alloc_color(cmap, r, g, b)?.reply()?;
    Ok(reply.pixel)
}

// ── Renderer ──

struct Renderer {
    window: Window,
    pixmap: Pixmap,
    gc: Gcontext,
    depth: u8,
    #[allow(dead_code)]
    colormap: Colormap,
    bg_pixel: u32,
    text_pixel: u32,
    text_dim_pixel: u32,
    indicator_pixel: u32,
    ghost_pixel: u32,
    item_pixel: u32,
    item_hover_pixel: u32,
    item_active_pixel: u32,
    active_stripe_pixel: u32,
    menu_bg_pixel: u32,
    menu_border_pixel: u32,
    menu_hover_pixel: u32,
    group_header_pixel: u32,
    group_color_pixels: Vec<u32>,
    session_marker_pixel: u32,
}

impl Renderer {
    fn new(
        conn: &impl Connection,
        screen: &Screen,
        window: Window,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let colormap = screen.default_colormap;
        let depth = screen.root_depth;

        let bg_pixel = alloc_color(conn, colormap, BG_COLOR)?;
        let text_pixel = alloc_color(conn, colormap, TEXT_COLOR)?;
        let text_dim_pixel = alloc_color(conn, colormap, TEXT_DIM_COLOR)?;
        let indicator_pixel = alloc_color(conn, colormap, INDICATOR_COLOR)?;
        let ghost_pixel = alloc_color(conn, colormap, GHOST_COLOR)?;
        let item_pixel = alloc_color(conn, colormap, ITEM_COLOR)?;
        let item_hover_pixel = alloc_color(conn, colormap, ITEM_HOVER_COLOR)?;
        let item_active_pixel = alloc_color(conn, colormap, ITEM_ACTIVE_COLOR)?;
        let active_stripe_pixel = alloc_color(conn, colormap, ACTIVE_STRIPE_COLOR)?;
        let menu_bg_pixel = alloc_color(conn, colormap, MENU_BG_COLOR)?;
        let menu_border_pixel = alloc_color(conn, colormap, MENU_BORDER_COLOR)?;
        let menu_hover_pixel = alloc_color(conn, colormap, MENU_HOVER_COLOR)?;
        let group_header_pixel = alloc_color(conn, colormap, GROUP_HEADER_COLOR)?;
        let session_marker_pixel = alloc_color(conn, colormap, SESSION_MARKER_COLOR)?;

        let mut group_color_pixels = Vec::new();
        for &c in GROUP_COLORS {
            group_color_pixels.push(alloc_color(conn, colormap, c)?);
        }

        // Try scalable Nimbus Mono L, fall back to fixed 13px
        let font = conn.generate_id()?;
        let opened = conn.open_font(
            font,
            b"-urw-nimbus mono l-regular-r-normal--13-*-*-*-*-*-iso8859-1",
        );
        if opened.is_err() {
            conn.open_font(
                font,
                b"-misc-fixed-medium-r-normal--13-120-75-75-c-70-iso8859-1",
            )?;
        }

        let gc = conn.generate_id()?;
        let gc_aux = CreateGCAux::new()
            .foreground(text_pixel)
            .background(bg_pixel)
            .font(font)
            .graphics_exposures(0);
        conn.create_gc(gc, window, &gc_aux)?;

        let pixmap = conn.generate_id()?;
        conn.create_pixmap(depth, pixmap, window, WIN_W, WIN_H)?;

        Ok(Self {
            window,
            pixmap,
            gc,
            depth,
            colormap,
            bg_pixel,
            text_pixel,
            text_dim_pixel,
            indicator_pixel,
            ghost_pixel,
            item_pixel,
            item_hover_pixel,
            item_active_pixel,
            active_stripe_pixel,
            menu_bg_pixel,
            menu_border_pixel,
            menu_hover_pixel,
            group_header_pixel,
            group_color_pixels,
            session_marker_pixel,
        })
    }

    fn resize(
        &mut self,
        conn: &impl Connection,
        width: u16,
        height: u16,
    ) -> Result<(), Box<dyn std::error::Error>> {
        conn.free_pixmap(self.pixmap)?;
        self.pixmap = conn.generate_id()?;
        conn.create_pixmap(self.depth, self.pixmap, self.window, width, height)?;
        Ok(())
    }

    fn redraw(
        &self,
        conn: &impl Connection,
        app: &App,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let pix = self.pixmap;

        // Clear background
        conn.change_gc(self.gc, &ChangeGCAux::new().foreground(self.bg_pixel))?;
        conn.poly_fill_rectangle(
            pix,
            self.gc,
            &[Rectangle {
                x: 0,
                y: 0,
                width: app.width,
                height: app.height,
            }],
        )?;

        // Draw the "+ New terminal" header button above the item list.
        self.draw_new_terminal_button(conn, pix, app)?;

        let dragged_row = app
            .drag
            .as_ref()
            .filter(|d| d.started)
            .map(|d| d.source_row);

        // Draw display rows
        for (i, row) in app.display_rows.iter().enumerate() {
            if Some(i) == dragged_row {
                continue;
            }
            let y = app.row_y(i);
            let hovered = app.hover_row == Some(i);
            match row {
                DisplayRow::GroupHeader { group_id } => {
                    self.draw_group_header(conn, pix, app, *group_id, y, hovered)?;
                }
                DisplayRow::Window { wid, group_id } => {
                    if let Some(item) = app.find_item(*wid) {
                        let is_active = app.active_wid == Some(*wid);
                        let ix = app.item_x();
                        let iw = app.item_w();
                        let (x, w) = if group_id.is_some() {
                            (ix + GROUP_INDENT, iw - GROUP_INDENT as u16)
                        } else {
                            (ix, iw)
                        };
                        self.draw_item(conn, pix, x, y, w, ITEM_H as u16, item, false, hovered, is_active)?;
                    }
                }
                DisplayRow::Session { name, group_id } => {
                    let ix = app.item_x();
                    let iw = app.item_w();
                    let (x, w) = if group_id.is_some() {
                        (ix + GROUP_INDENT, iw - GROUP_INDENT as u16)
                    } else {
                        (ix, iw)
                    };
                    self.draw_session_row(conn, pix, x, y, w, ITEM_H as u16, name, hovered)?;
                }
            }
        }

        // Draw rename overlay (draws on top of the target row)
        if let Some(ref rs) = app.rename {
            let row_idx = match &rs.target {
                RenameTarget::Group(gid) => app.display_rows.iter().position(
                    |r| matches!(r, DisplayRow::GroupHeader { group_id } if group_id == gid),
                ),
                RenameTarget::Window(wid) => app.display_rows.iter().position(
                    |r| matches!(r, DisplayRow::Window { wid: w, .. } if w == wid),
                ),
                RenameTarget::Session(name) => app.display_rows.iter().position(
                    |r| matches!(r, DisplayRow::Session { name: n, .. } if n == name),
                ),
            };
            if let Some(row_idx) = row_idx {
                self.draw_rename_input(conn, pix, app, rs, app.row_y(row_idx))?;
            }
        }

        // Draw drag visuals
        if let Some(drag) = &app.drag {
            if drag.started {
                let drop_idx = app.drop_index_from_y(drag.current_y);
                let indicator_y = if drop_idx < app.display_rows.len() {
                    app.row_y(drop_idx) - (ITEM_SPACING / 2)
                } else if !app.display_rows.is_empty() {
                    app.row_y(app.display_rows.len() - 1) + ITEM_H as i16 + (ITEM_SPACING / 2)
                } else {
                    ITEM_Y_START
                };
                conn.change_gc(
                    self.gc,
                    &ChangeGCAux::new().foreground(self.indicator_pixel),
                )?;
                conn.poly_fill_rectangle(
                    pix,
                    self.gc,
                    &[Rectangle {
                        x: app.item_x(),
                        y: indicator_y,
                        width: app.item_w(),
                        height: 2,
                    }],
                )?;

                // Ghost at cursor
                if drag.source_row < app.display_rows.len() {
                    let ghost_y = drag.current_y - (ITEM_H as i16 / 2);
                    match &app.display_rows[drag.source_row] {
                        DisplayRow::GroupHeader { group_id } => {
                            self.draw_ghost_header(conn, pix, app, *group_id, ghost_y)?;
                        }
                        DisplayRow::Window { wid, group_id } => {
                            if let Some(item) = app.find_item(*wid) {
                                let ix = app.item_x();
                                let iw = app.item_w();
                                let (x, w) = if group_id.is_some() {
                                    (ix + GROUP_INDENT, iw - GROUP_INDENT as u16)
                                } else {
                                    (ix, iw)
                                };
                                self.draw_item(
                                    conn, pix, x, ghost_y, w, ITEM_H as u16, item, true, false, false,
                                )?;
                            }
                        }
                        DisplayRow::Session { name, group_id } => {
                            let ix = app.item_x();
                            let iw = app.item_w();
                            let (x, w) = if group_id.is_some() {
                                (ix + GROUP_INDENT, iw - GROUP_INDENT as u16)
                            } else {
                                (ix, iw)
                            };
                            // Ghost reuses the session-row drawing; hovered=false, no special ghost style.
                            self.draw_session_row(conn, pix, x, ghost_y, w, ITEM_H as u16, name, false)?;
                        }
                    }
                }
            }
        }

        conn.copy_area(pix, self.window, self.gc, 0, 0, 0, 0, app.width, app.height)?;
        conn.flush()?;
        Ok(())
    }

    fn draw_new_terminal_button(
        &self,
        conn: &impl Connection,
        drawable: Drawable,
        app: &App,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let x = ITEM_MARGIN;
        let y: i16 = 4;
        let w = (app.width as i16 - ITEM_MARGIN * 2).max(20) as u16;
        let h = HEADER_H;

        let bg = if app.header_hovered {
            self.item_hover_pixel
        } else {
            self.item_pixel
        };
        conn.change_gc(self.gc, &ChangeGCAux::new().foreground(bg))?;
        conn.poly_fill_rectangle(
            drawable,
            self.gc,
            &[Rectangle { x, y, width: w, height: h }],
        )?;

        // Label centred horizontally.
        let label = "+ New terminal";
        let label_width = label.len() as i16 * CHAR_WIDTH;
        let text_x = x + (w as i16 - label_width) / 2;
        let text_y = y + (h as i16 / 2) + 4;
        conn.change_gc(self.gc, &ChangeGCAux::new().foreground(self.text_pixel))?;
        conn.image_text8(drawable, self.gc, text_x, text_y, label.as_bytes())?;

        Ok(())
    }

    fn draw_item(
        &self,
        conn: &impl Connection,
        drawable: Drawable,
        x: i16,
        y: i16,
        w: u16,
        h: u16,
        item: &Item,
        ghost: bool,
        hovered: bool,
        is_active: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let bg = if ghost {
            self.ghost_pixel
        } else if is_active {
            self.item_active_pixel
        } else if hovered {
            self.item_hover_pixel
        } else {
            self.item_pixel
        };
        conn.change_gc(self.gc, &ChangeGCAux::new().foreground(bg))?;
        conn.poly_fill_rectangle(
            drawable,
            self.gc,
            &[Rectangle {
                x,
                y,
                width: w,
                height: h,
            }],
        )?;

        // Left accent stripe (3px) — blue override for active window
        let stripe_color = if is_active {
            self.active_stripe_pixel
        } else {
            item.accent_pixel
        };
        conn.change_gc(self.gc, &ChangeGCAux::new().foreground(stripe_color))?;
        conn.poly_fill_rectangle(
            drawable,
            self.gc,
            &[Rectangle {
                x,
                y,
                width: 3,
                height: h,
            }],
        )?;

        // Reserve right-side space for the session marker if present,
        // so the label truncates cleanly instead of overlapping the dot.
        let marker_reserve: i16 = if item.session.is_some() { 14 } else { 0 };

        // Label
        conn.change_gc(self.gc, &ChangeGCAux::new().foreground(self.text_pixel))?;
        let text_x = x + 8;
        let text_y = y + (h as i16 / 2) + 4;
        let max_chars = ((w as i16 - 12 - marker_reserve) / CHAR_WIDTH).max(0) as usize;
        let full_label = item.display_label();
        let display: String = full_label.chars().take(max_chars).collect();
        if !display.is_empty() {
            conn.image_text8(drawable, self.gc, text_x, text_y, display.as_bytes())?;
        }

        // Session marker: small filled circle on the right edge when this
        // window is attached to a tmux session.
        if item.session.is_some() {
            let marker_size: u16 = 6;
            let marker_x = x + w as i16 - marker_size as i16 - 6;
            let marker_y = y + (h as i16 - marker_size as i16) / 2;
            conn.change_gc(
                self.gc,
                &ChangeGCAux::new().foreground(self.session_marker_pixel),
            )?;
            conn.poly_fill_arc(
                drawable,
                self.gc,
                &[Arc {
                    x: marker_x,
                    y: marker_y,
                    width: marker_size,
                    height: marker_size,
                    angle1: 0,
                    angle2: 360 * 64,
                }],
            )?;
        }

        Ok(())
    }

    fn draw_session_row(
        &self,
        conn: &impl Connection,
        drawable: Drawable,
        x: i16,
        y: i16,
        w: u16,
        h: u16,
        name: &str,
        hovered: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Background
        let bg = if hovered {
            self.item_hover_pixel
        } else {
            self.item_pixel
        };
        conn.change_gc(self.gc, &ChangeGCAux::new().foreground(bg))?;
        conn.poly_fill_rectangle(
            drawable,
            self.gc,
            &[Rectangle { x, y, width: w, height: h }],
        )?;

        // Grey left-edge stripe so orphan sessions read as "not a window".
        conn.change_gc(
            self.gc,
            &ChangeGCAux::new().foreground(self.text_dim_pixel),
        )?;
        conn.poly_fill_rectangle(
            drawable,
            self.gc,
            &[Rectangle { x, y, width: 3, height: h }],
        )?;

        // Label (reserve the right-edge marker area so text doesn't overlap).
        let marker_reserve: i16 = 14;
        conn.change_gc(self.gc, &ChangeGCAux::new().foreground(self.text_pixel))?;
        let text_x = x + 8;
        let text_y = y + (h as i16 / 2) + 4;
        let max_chars = ((w as i16 - 12 - marker_reserve) / CHAR_WIDTH).max(0) as usize;
        let display: String = name.chars().take(max_chars).collect();
        if !display.is_empty() {
            conn.image_text8(drawable, self.gc, text_x, text_y, display.as_bytes())?;
        }

        // Hollow ring on the right: "session exists but no attached terminal".
        let marker_size: u16 = 6;
        let marker_x = x + w as i16 - marker_size as i16 - 6;
        let marker_y = y + (h as i16 - marker_size as i16) / 2;
        conn.change_gc(
            self.gc,
            &ChangeGCAux::new().foreground(self.session_marker_pixel),
        )?;
        conn.poly_arc(
            drawable,
            self.gc,
            &[Arc {
                x: marker_x,
                y: marker_y,
                width: marker_size,
                height: marker_size,
                angle1: 0,
                angle2: 360 * 64,
            }],
        )?;

        Ok(())
    }

    fn draw_group_header(
        &self,
        conn: &impl Connection,
        drawable: Drawable,
        app: &App,
        group_id: u32,
        y: i16,
        hovered: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let group = match app.groups.iter().find(|g| g.id == group_id) {
            Some(g) => g,
            None => return Ok(()),
        };

        let ix = app.item_x();
        let iw = app.item_w();

        // Background
        let bg = if hovered {
            self.item_hover_pixel
        } else {
            self.group_header_pixel
        };
        conn.change_gc(self.gc, &ChangeGCAux::new().foreground(bg))?;
        conn.poly_fill_rectangle(
            drawable,
            self.gc,
            &[Rectangle {
                x: ix,
                y,
                width: iw,
                height: ITEM_H as u16,
            }],
        )?;

        // Top edge line for visual separation
        conn.change_gc(
            self.gc,
            &ChangeGCAux::new().foreground(self.menu_border_pixel),
        )?;
        conn.poly_fill_rectangle(
            drawable,
            self.gc,
            &[Rectangle {
                x: ix,
                y,
                width: iw,
                height: 1,
            }],
        )?;

        // Left accent stripe
        let color_idx = group_id as usize % self.group_color_pixels.len();
        conn.change_gc(
            self.gc,
            &ChangeGCAux::new().foreground(self.group_color_pixels[color_idx]),
        )?;
        conn.poly_fill_rectangle(
            drawable,
            self.gc,
            &[Rectangle {
                x: ix,
                y,
                width: 3,
                height: ITEM_H as u16,
            }],
        )?;

        // Arrow + name
        let arrow = if group.collapsed { "+" } else { "-" };
        let name_text = format!("{} {}", arrow, group.name);

        conn.change_gc(self.gc, &ChangeGCAux::new().foreground(self.text_pixel))?;
        let text_x = ix + 8;
        let text_y = y + (ITEM_H as i16 / 2) + 4;
        let max_chars = ((iw as i16 - 12) / CHAR_WIDTH).max(0) as usize;
        let display: String = name_text.chars().take(max_chars).collect();
        conn.image_text8(drawable, self.gc, text_x, text_y, display.as_bytes())?;

        // Member count (dimmed) when collapsed
        if group.collapsed {
            let count_text = format!("({})", group.member_wids.len());
            let name_width = (display.len() as i16 + 1) * CHAR_WIDTH;
            conn.change_gc(
                self.gc,
                &ChangeGCAux::new().foreground(self.text_dim_pixel),
            )?;
            conn.image_text8(
                drawable,
                self.gc,
                text_x + name_width,
                text_y,
                count_text.as_bytes(),
            )?;
        }

        Ok(())
    }

    fn draw_ghost_header(
        &self,
        conn: &impl Connection,
        drawable: Drawable,
        app: &App,
        group_id: u32,
        y: i16,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let ix = app.item_x();
        let iw = app.item_w();
        conn.change_gc(self.gc, &ChangeGCAux::new().foreground(self.ghost_pixel))?;
        conn.poly_fill_rectangle(
            drawable,
            self.gc,
            &[Rectangle {
                x: ix,
                y,
                width: iw,
                height: ITEM_H as u16,
            }],
        )?;
        if let Some(group) = app.groups.iter().find(|g| g.id == group_id) {
            let text = format!("{} (+{})", group.name, group.member_wids.len());
            conn.change_gc(self.gc, &ChangeGCAux::new().foreground(self.text_pixel))?;
            conn.image_text8(
                drawable,
                self.gc,
                ix + 8,
                y + (ITEM_H as i16 / 2) + 4,
                text.as_bytes(),
            )?;
        }
        Ok(())
    }

    fn draw_rename_input(
        &self,
        conn: &impl Connection,
        drawable: Drawable,
        app: &App,
        rs: &RenameState,
        y: i16,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let ix = app.item_x();
        let iw = app.item_w();

        // Dark background
        conn.change_gc(self.gc, &ChangeGCAux::new().foreground(self.bg_pixel))?;
        conn.poly_fill_rectangle(
            drawable,
            self.gc,
            &[Rectangle {
                x: ix,
                y,
                width: iw,
                height: ITEM_H as u16,
            }],
        )?;

        // Border
        conn.change_gc(
            self.gc,
            &ChangeGCAux::new().foreground(self.indicator_pixel),
        )?;
        conn.poly_fill_rectangle(drawable, self.gc, &[Rectangle { x: ix, y, width: iw, height: 1 }])?;
        conn.poly_fill_rectangle(drawable, self.gc, &[Rectangle { x: ix, y: y + ITEM_H as i16 - 1, width: iw, height: 1 }])?;
        conn.poly_fill_rectangle(drawable, self.gc, &[Rectangle { x: ix, y, width: 1, height: ITEM_H as u16 }])?;
        conn.poly_fill_rectangle(drawable, self.gc, &[Rectangle { x: ix + iw as i16 - 1, y, width: 1, height: ITEM_H as u16 }])?;

        // Text
        conn.change_gc(self.gc, &ChangeGCAux::new().foreground(self.text_pixel))?;
        let text_x = ix + 8;
        let text_y = y + (ITEM_H as i16 / 2) + 4;
        let max_chars = ((iw as i16 - 16) / CHAR_WIDTH).max(0) as usize;
        let display: String = rs.text.chars().take(max_chars).collect();
        if !display.is_empty() {
            conn.image_text8(drawable, self.gc, text_x, text_y, display.as_bytes())?;
        }

        // Cursor bar
        let cursor_chars = rs.text[..rs.cursor].chars().count().min(max_chars);
        let cursor_x = text_x + (cursor_chars as i16) * CHAR_WIDTH;
        conn.poly_fill_rectangle(
            drawable,
            self.gc,
            &[Rectangle {
                x: cursor_x,
                y: y + 4,
                width: 1,
                height: ITEM_H as u16 - 8,
            }],
        )?;

        Ok(())
    }
}

// ── Context menu ──

fn build_menu_entries(app: &App, row: usize) -> Vec<MenuEntry> {
    if row >= app.display_rows.len() {
        return vec![];
    }
    match &app.display_rows[row] {
        DisplayRow::GroupHeader { .. } => {
            vec![
                MenuEntry {
                    label: "Rename Group".to_string(),
                    action: MenuAction::RenameGroup,
                },
                MenuEntry {
                    label: "Delete Group".to_string(),
                    action: MenuAction::DeleteGroup,
                },
            ]
        }
        DisplayRow::Window {
            group_id: Some(_), ..
        } => {
            vec![
                MenuEntry {
                    label: "Rename Tab".to_string(),
                    action: MenuAction::RenameTab,
                },
                MenuEntry {
                    label: "Remove from Group".to_string(),
                    action: MenuAction::RemoveFromGroup,
                },
            ]
        }
        DisplayRow::Window {
            group_id: None, ..
        } => {
            let mut entries = vec![
                MenuEntry {
                    label: "Rename Tab".to_string(),
                    action: MenuAction::RenameTab,
                },
                MenuEntry {
                    label: "New Group".to_string(),
                    action: MenuAction::CreateGroup,
                },
            ];
            for group in &app.groups {
                entries.push(MenuEntry {
                    label: format!("Add to {}", group.name),
                    action: MenuAction::AddToGroup(group.id),
                });
            }
            entries
        }
        DisplayRow::Session { .. } => {
            vec![
                MenuEntry {
                    label: "Attach".to_string(),
                    action: MenuAction::AttachSession,
                },
                MenuEntry {
                    label: "Rename Session".to_string(),
                    action: MenuAction::RenameSession,
                },
                MenuEntry {
                    label: "Kill Session".to_string(),
                    action: MenuAction::KillSession,
                },
            ]
        }
    }
}

fn open_context_menu(
    conn: &impl Connection,
    screen: &Screen,
    renderer: &Renderer,
    app: &mut App,
    target_row: usize,
    root_x: i16,
    root_y: i16,
) -> Result<(), Box<dyn std::error::Error>> {
    if app.context_menu.is_some() {
        close_context_menu(conn, app)?;
    }

    let entries = build_menu_entries(app, target_row);
    if entries.is_empty() {
        return Ok(());
    }

    let height = (entries.len() as u16) * MENU_ITEM_H + (MENU_PADDING as u16 * 2);
    let width = MENU_MIN_W;

    let x = root_x.min((screen.width_in_pixels as i16) - width as i16);
    let y = root_y.min((screen.height_in_pixels as i16) - height as i16);

    let win = conn.generate_id()?;
    conn.create_window(
        COPY_DEPTH_FROM_PARENT,
        win,
        screen.root,
        x,
        y,
        width,
        height,
        1,
        WindowClass::INPUT_OUTPUT,
        0,
        &CreateWindowAux::new()
            .override_redirect(1u32)
            .background_pixel(renderer.menu_bg_pixel)
            .border_pixel(renderer.menu_border_pixel)
            .event_mask(EventMask::EXPOSURE),
    )?;

    let pix = conn.generate_id()?;
    conn.create_pixmap(screen.root_depth, pix, win, width, height)?;

    conn.map_window(win)?;

    let _grab_reply = conn
        .grab_pointer(
            false,
            win,
            EventMask::BUTTON_PRESS | EventMask::BUTTON_RELEASE | EventMask::POINTER_MOTION,
            GrabMode::ASYNC,
            GrabMode::ASYNC,
            0u32,
            0u32,
            0u32,
        )?
        .reply()?;

    conn.flush()?;

    app.context_menu = Some(ContextMenu {
        window: win,
        pixmap: pix,
        entries,
        target_row,
        x,
        y,
        width,
        height,
        hover_index: None,
    });

    draw_context_menu(conn, renderer, app)?;
    Ok(())
}

fn close_context_menu(
    conn: &impl Connection,
    app: &mut App,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(menu) = app.context_menu.take() {
        conn.ungrab_pointer(0u32)?;
        conn.free_pixmap(menu.pixmap)?;
        conn.destroy_window(menu.window)?;
        conn.flush()?;
    }
    Ok(())
}

fn draw_context_menu(
    conn: &impl Connection,
    renderer: &Renderer,
    app: &App,
) -> Result<(), Box<dyn std::error::Error>> {
    let menu = match app.context_menu.as_ref() {
        Some(m) => m,
        None => return Ok(()),
    };
    let pix = menu.pixmap;
    let gc = renderer.gc;

    // Clear
    conn.change_gc(gc, &ChangeGCAux::new().foreground(renderer.menu_bg_pixel))?;
    conn.poly_fill_rectangle(
        pix,
        gc,
        &[Rectangle {
            x: 0,
            y: 0,
            width: menu.width,
            height: menu.height,
        }],
    )?;

    // Draw entries
    for (i, entry) in menu.entries.iter().enumerate() {
        let entry_y = MENU_PADDING + (i as i16) * MENU_ITEM_H as i16;

        if Some(i) == menu.hover_index {
            conn.change_gc(gc, &ChangeGCAux::new().foreground(renderer.menu_hover_pixel))?;
            conn.poly_fill_rectangle(
                pix,
                gc,
                &[Rectangle {
                    x: 2,
                    y: entry_y,
                    width: menu.width - 4,
                    height: MENU_ITEM_H,
                }],
            )?;
        }

        conn.change_gc(gc, &ChangeGCAux::new().foreground(renderer.text_pixel))?;
        let text_y = entry_y + (MENU_ITEM_H as i16 / 2) + 4;
        conn.image_text8(pix, gc, 8, text_y, entry.label.as_bytes())?;
    }

    conn.copy_area(pix, menu.window, gc, 0, 0, 0, 0, menu.width, menu.height)?;
    conn.flush()?;
    Ok(())
}

fn execute_menu_action(app: &mut App, action: MenuAction, target_row: usize) {
    if target_row >= app.display_rows.len() {
        return;
    }
    match action {
        MenuAction::CreateGroup => {
            if let DisplayRow::Window { wid, .. } = &app.display_rows[target_row] {
                app.create_group(*wid);
            }
        }
        MenuAction::AddToGroup(gid) => {
            if let DisplayRow::Window { wid, .. } = &app.display_rows[target_row] {
                app.add_to_group(gid, *wid);
            }
        }
        MenuAction::RemoveFromGroup => {
            if let DisplayRow::Window { wid, .. } = &app.display_rows[target_row] {
                app.remove_from_group(*wid);
            }
        }
        MenuAction::RenameGroup => {
            if let DisplayRow::GroupHeader { group_id } = &app.display_rows[target_row] {
                app.start_rename(*group_id);
            }
        }
        MenuAction::DeleteGroup => {
            if let DisplayRow::GroupHeader { group_id } = &app.display_rows[target_row] {
                app.delete_group(*group_id);
            }
        }
        MenuAction::RenameTab => {
            if let DisplayRow::Window { wid, .. } = &app.display_rows[target_row] {
                app.start_tab_rename(*wid);
            }
        }
        MenuAction::AttachSession => {
            if let DisplayRow::Session { name, .. } = &app.display_rows[target_row] {
                app.pending_attach = Some((name.clone(), std::time::Instant::now()));
                spawn_attach_terminal(name);
            }
        }
        MenuAction::RenameSession => {
            if let DisplayRow::Session { name, .. } = &app.display_rows[target_row] {
                app.start_session_rename(&name.clone());
            }
        }
        MenuAction::KillSession => {
            if let DisplayRow::Session { name, .. } = &app.display_rows[target_row] {
                let _ = std::process::Command::new("tmux")
                    .args(["kill-session", "-t", name])
                    .status();
                // Optimistically drop the slot; next refresh will re-confirm.
                let target = name.clone();
                app.display_order.retain(|s| {
                    !matches!(s, DisplaySlot::Session(n) if *n == target)
                });
                app.build_display_rows();
            }
        }
    }
}

// ── Keyboard helpers ──

fn keysym_from_keycode(
    conn: &impl Connection,
    keycode: u8,
    state: KeyButMask,
) -> Result<u32, Box<dyn std::error::Error>> {
    let setup = conn.setup();
    let min_kc = setup.min_keycode;
    let max_kc = setup.max_keycode;
    let reply = conn
        .get_keyboard_mapping(min_kc, max_kc - min_kc + 1)?
        .reply()?;
    let syms_per_kc = reply.keysyms_per_keycode as usize;
    let offset = (keycode - min_kc) as usize * syms_per_kc;
    if offset >= reply.keysyms.len() {
        return Ok(0);
    }
    // Column 0 = unshifted, column 1 = shifted
    let shifted = u16::from(state) & 1 != 0; // ShiftMask = bit 0
    let col = if shifted && syms_per_kc > 1 { 1 } else { 0 };
    Ok(reply.keysyms[offset + col])
}

fn printable_char_from_sym(sym: u32) -> Option<char> {
    // Latin-1 range: keysym 0x20..0xff maps directly to Unicode
    if (0x20..=0x7e).contains(&sym) || (0xa0..=0xff).contains(&sym) {
        char::from_u32(sym)
    } else {
        None
    }
}

// ── Geometry persistence ──

fn geometry_path() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let mut path = std::path::PathBuf::from(home);
    path.push(".config");
    path.push("ptm");
    path.push("geometry");
    path
}

fn save_geometry(x: i16, y: i16, w: u16, h: u16) {
    let path = geometry_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut f) = std::fs::File::create(&path) {
        let _ = write!(f, "{} {} {} {}\n", x, y, w, h);
    }
}

fn load_geometry() -> Option<(i16, i16, u16, u16)> {
    let data = std::fs::read_to_string(geometry_path()).ok()?;
    let parts: Vec<&str> = data.trim().split_whitespace().collect();
    if parts.len() != 4 {
        return None;
    }
    Some((
        parts[0].parse().ok()?,
        parts[1].parse().ok()?,
        parts[2].parse().ok()?,
        parts[3].parse().ok()?,
    ))
}

// ── Group persistence ──

struct SavedMember {
    label: String,
    wm_class: String,
    custom_prefix: String,
}

struct SavedGroup {
    name: String,
    collapsed: bool,
    members: Vec<SavedMember>,
}

fn groups_path() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let mut path = std::path::PathBuf::from(home);
    path.push(".config");
    path.push("ptm");
    path.push("groups");
    path
}

fn extract_saved_state(app: &App) -> Vec<SavedGroup> {
    let mut saved = Vec::new();
    for slot in &app.display_order {
        if let DisplaySlot::Group(gid) = slot {
            if let Some(group) = app.groups.iter().find(|g| g.id == *gid) {
                let members = group
                    .member_wids
                    .iter()
                    .filter_map(|wid| {
                        app.find_item(*wid).map(|item| SavedMember {
                            label: item.label.clone(),
                            wm_class: item.wm_class.clone(),
                            custom_prefix: item.custom_prefix.clone(),
                        })
                    })
                    .collect();
                saved.push(SavedGroup {
                    name: group.name.clone(),
                    collapsed: group.collapsed,
                    members,
                });
            }
        }
    }
    saved
}

fn save_groups_to(path: &std::path::Path, groups: &[SavedGroup]) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut f) = std::fs::File::create(path) {
        let _ = writeln!(f, "v1");
        for group in groups {
            let collapsed = if group.collapsed { "1" } else { "0" };
            let _ = writeln!(f, "GROUP\t{}\t{}", group.name, collapsed);
            for member in &group.members {
                let _ = writeln!(
                    f,
                    "MEMBER\t{}\t{}\t{}",
                    member.label, member.wm_class, member.custom_prefix
                );
            }
        }
    }
}

fn save_groups(app: &App) {
    let groups = extract_saved_state(app);
    save_groups_to(&groups_path(), &groups);
}

fn load_groups_from(path: &std::path::Path) -> Option<Vec<SavedGroup>> {
    let data = std::fs::read_to_string(path).ok()?;
    let mut lines = data.lines();
    if lines.next()? != "v1" {
        return None;
    }
    let mut groups: Vec<SavedGroup> = Vec::new();
    for line in lines {
        let parts: Vec<&str> = line.split('\t').collect();
        match parts.first() {
            Some(&"GROUP") => {
                if parts.len() != 3 {
                    return None;
                }
                let collapsed = match parts[2] {
                    "1" => true,
                    "0" => false,
                    _ => return None,
                };
                groups.push(SavedGroup {
                    name: parts[1].to_string(),
                    collapsed,
                    members: Vec::new(),
                });
            }
            Some(&"MEMBER") => {
                if parts.len() != 4 {
                    return None;
                }
                if groups.is_empty() {
                    return None;
                }
                groups.last_mut().unwrap().members.push(SavedMember {
                    label: parts[1].to_string(),
                    wm_class: parts[2].to_string(),
                    custom_prefix: parts[3].to_string(),
                });
            }
            _ => return None,
        }
    }
    Some(groups)
}

fn load_groups() -> Option<Vec<SavedGroup>> {
    load_groups_from(&groups_path())
}

fn restore_groups(app: &mut App, saved: &[SavedGroup]) {
    let available: Vec<(String, String, u32)> = app
        .items
        .iter()
        .map(|item| (item.label.clone(), item.wm_class.clone(), item.wid))
        .collect();
    let mut claimed: HashSet<u32> = HashSet::new();

    for sg in saved {
        let mut matched_wids: Vec<u32> = Vec::new();
        let mut matched_prefixes: Vec<String> = Vec::new();

        for sm in &sg.members {
            // Prefer exact match on (label, wm_class)
            let exact = available.iter().find(|(l, c, w)| {
                l == &sm.label && c == &sm.wm_class && !claimed.contains(w)
            });
            let matched = exact.or_else(|| {
                // Fall back to label-only match
                available
                    .iter()
                    .find(|(l, _, w)| l == &sm.label && !claimed.contains(w))
            });
            if let Some((_, _, wid)) = matched {
                matched_wids.push(*wid);
                matched_prefixes.push(sm.custom_prefix.clone());
                claimed.insert(*wid);
            }
        }

        if matched_wids.is_empty() {
            continue;
        }

        // Restore custom_prefix on matched items
        for (wid, prefix) in matched_wids.iter().zip(matched_prefixes.iter()) {
            if !prefix.is_empty() {
                if let Some(item) = app.items.iter_mut().find(|i| i.wid == *wid) {
                    item.custom_prefix = prefix.clone();
                }
            }
        }

        let gid = app.next_group_id;
        app.next_group_id += 1;
        app.groups.push(Group {
            id: gid,
            name: sg.name.clone(),
            collapsed: sg.collapsed,
            member_wids: matched_wids,
        });
    }

    // Rebuild display_order: groups first (in saved order), then ungrouped
    let mut new_order: Vec<DisplaySlot> = Vec::new();
    for group in &app.groups {
        new_order.push(DisplaySlot::Group(group.id));
    }
    for slot in &app.display_order {
        if let DisplaySlot::Window(wid) = slot {
            if !claimed.contains(wid) {
                new_order.push(DisplaySlot::Window(*wid));
            }
        }
    }
    app.display_order = new_order;
    app.build_display_rows();
}

#[cfg(test)]
fn geometry_path_in(dir: &std::path::Path) -> std::path::PathBuf {
    dir.join("geometry")
}

#[cfg(test)]
fn save_geometry_to(path: &std::path::Path, x: i16, y: i16, w: u16, h: u16) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut f) = std::fs::File::create(path) {
        let _ = write!(f, "{} {} {} {}\n", x, y, w, h);
    }
}

#[cfg(test)]
fn load_geometry_from(path: &std::path::Path) -> Option<(i16, i16, u16, u16)> {
    let data = std::fs::read_to_string(path).ok()?;
    let parts: Vec<&str> = data.trim().split_whitespace().collect();
    if parts.len() != 4 {
        return None;
    }
    Some((
        parts[0].parse().ok()?,
        parts[1].parse().ok()?,
        parts[2].parse().ok()?,
        parts[3].parse().ok()?,
    ))
}

// ── Main ──

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (conn, screen_num) = x11rb::connect(None)?;
    let screen = &conn.setup().roots[screen_num];
    let root = screen.root;
    let colormap = screen.default_colormap;

    let atoms = Atoms::new(&conn)?;

    // Subscribe to property changes on the root window so we're notified
    // when _NET_CLIENT_LIST, _NET_ACTIVE_WINDOW, etc. change — this is how
    // PTM learns about new/closed/focused windows without polling.
    conn.change_window_attributes(
        root,
        &ChangeWindowAttributesAux::new().event_mask(EventMask::PROPERTY_CHANGE),
    )?;

    let window: Window = conn.generate_id()?;
    let event_mask = EventMask::BUTTON_PRESS
        | EventMask::BUTTON_RELEASE
        | EventMask::POINTER_MOTION
        | EventMask::EXPOSURE
        | EventMask::STRUCTURE_NOTIFY
        | EventMask::LEAVE_WINDOW
        | EventMask::KEY_PRESS;

    conn.create_window(
        COPY_DEPTH_FROM_PARENT,
        window,
        root,
        0,
        0,
        WIN_W,
        WIN_H,
        0,
        WindowClass::INPUT_OUTPUT,
        0,
        &CreateWindowAux::new()
            .background_pixel(screen.black_pixel)
            .event_mask(event_mask),
    )?;

    conn.change_property8(
        PropMode::REPLACE,
        window,
        AtomEnum::WM_NAME,
        AtomEnum::STRING,
        b"ptm",
    )?;

    // Register WM_DELETE_WINDOW so the WM sends ClientMessage instead of killing us
    conn.change_property32(
        PropMode::REPLACE,
        window,
        atoms.wm_protocols,
        AtomEnum::ATOM,
        &[atoms.wm_delete_window],
    )?;

    let mut app = App::new(window);
    let mut renderer = Renderer::new(&conn, screen, window)?;

    conn.map_window(window)?;
    conn.flush()?;

    // Restore saved geometry (position + size) from previous session
    if let Some((x, y, w, h)) = load_geometry() {
        conn.configure_window(
            window,
            &ConfigureWindowAux::new()
                .x(x as i32)
                .y(y as i32)
                .width(w as u32)
                .height(h as u32),
        )?;
        conn.flush()?;
        app.x = x;
        app.y = y;
        app.width = w;
        app.height = h;
        renderer.resize(&conn, w, h)?;
    }

    refresh_items(&conn, root, &atoms, &mut app, colormap)?;

    if let Some(saved) = load_groups() {
        restore_groups(&mut app, &saved);
    }

    // Background thread that pokes the main loop every 5 s so tmux state
    // changes (sessions created or destroyed outside PTM) show up promptly.
    spawn_tmux_poll_thread(window, atoms.ptm_wake, std::time::Duration::from_secs(5));

    loop {
        let event = conn.wait_for_event()?;

        // Handle WM_DELETE_WINDOW and our own wake pings in any mode.
        if let Event::ClientMessage(ev) = &event {
            if ev.window == window && ev.data.as_data32()[0] == atoms.wm_delete_window {
                save_geometry(app.x, app.y, app.width, app.height);
                save_groups(&app);
                break;
            }
            if ev.type_ == atoms.ptm_wake {
                // Scheduled tmux-state poll from the background thread.
                // Same gating as PropertyNotify refresh: don't rebuild while
                // the user is mid-gesture.
                if app.drag.is_none()
                    && app.context_menu.is_none()
                    && app.rename.is_none()
                {
                    refresh_items(&conn, root, &atoms, &mut app, colormap)?;
                    renderer.redraw(&conn, &app)?;
                }
                continue;
            }
        }

        // Event-driven refresh: react to PropertyNotify before mode dispatch.
        // Handled here so menu/rename/drag states stay consistent.
        if let Event::PropertyNotify(ev) = &event {
            let action = classify_property_event(
                ev.atom,
                ev.window == root,
                atoms.net_client_list,
                atoms.net_active_window,
                atoms.net_wm_name,
            );
            match action {
                PropertyAction::RefreshClientList => {
                    // Skip full refresh while the user is mid-gesture — the next
                    // PropertyNotify (or the gesture's completion) will catch up.
                    if app.drag.is_none()
                        && app.context_menu.is_none()
                        && app.rename.is_none()
                    {
                        refresh_items(&conn, root, &atoms, &mut app, colormap)?;
                        renderer.redraw(&conn, &app)?;
                    }
                }
                PropertyAction::UpdateActiveWindow => {
                    app.active_wid =
                        get_active_window(&conn, root, &atoms).unwrap_or(None);
                    if app.context_menu.is_none() && app.rename.is_none() {
                        renderer.redraw(&conn, &app)?;
                    }
                }
                PropertyAction::UpdateWindowTitle => {
                    if let Some(item) = app.items.iter_mut().find(|i| i.wid == ev.window) {
                        if let Ok(t) = get_window_title(&conn, ev.window, &atoms) {
                            if !t.is_empty() {
                                item.label = t;
                            }
                        }
                    }
                    if app.drag.is_none()
                        && app.context_menu.is_none()
                        && app.rename.is_none()
                    {
                        renderer.redraw(&conn, &app)?;
                    }
                }
                PropertyAction::Ignore => {}
            }
            continue;
        }

        // ── Context menu mode (pointer grab routes events here) ──
        if app.context_menu.is_some() {
            match event {
                Event::ButtonPress(ev) => {
                    let menu = app.context_menu.as_ref().unwrap();
                    let in_menu = ev.event_x >= 0
                        && ev.event_y >= 0
                        && (ev.event_x as u16) < menu.width
                        && (ev.event_y as u16) < menu.height;
                    if in_menu && ev.detail == 1 {
                        let idx = ((ev.event_y - MENU_PADDING) as usize) / MENU_ITEM_H as usize;
                        if idx < menu.entries.len() {
                            let action = menu.entries[idx].action.clone();
                            let target_row = menu.target_row;
                            close_context_menu(&conn, &mut app)?;
                            execute_menu_action(&mut app, action, target_row);
                        } else {
                            close_context_menu(&conn, &mut app)?;
                        }
                    } else {
                        close_context_menu(&conn, &mut app)?;
                    }
                    renderer.redraw(&conn, &app)?;
                }
                Event::MotionNotify(ev) => {
                    if let Some(ref mut menu) = app.context_menu {
                        let in_menu = ev.event_x >= 0
                            && ev.event_y >= 0
                            && (ev.event_x as u16) < menu.width
                            && (ev.event_y as u16) < menu.height;
                        let new_hover = if in_menu {
                            let idx =
                                ((ev.event_y - MENU_PADDING) as usize) / MENU_ITEM_H as usize;
                            if idx < menu.entries.len() {
                                Some(idx)
                            } else {
                                None
                            }
                        } else {
                            None
                        };
                        if new_hover != menu.hover_index {
                            menu.hover_index = new_hover;
                            draw_context_menu(&conn, &renderer, &app)?;
                        }
                    }
                }
                Event::Expose(ev) if ev.count == 0 => {
                    if Some(ev.window) == app.context_menu.as_ref().map(|m| m.window) {
                        draw_context_menu(&conn, &renderer, &app)?;
                    }
                    if ev.window == window {
                        renderer.redraw(&conn, &app)?;
                    }
                }
                _ => {}
            }
            continue;
        }

        // ── Rename mode (inline text editing) ──
        if app.rename.is_some() {
            match event {
                Event::KeyPress(ev) => {
                    let sym = keysym_from_keycode(&conn, ev.detail, ev.state)?;
                    match sym {
                        0xff0d | 0xff8d => {
                            // Return / KP_Enter → commit
                            app.commit_rename();
                        }
                        0xff1b => {
                            // Escape → cancel
                            app.cancel_rename();
                        }
                        0xff08 => {
                            // Backspace
                            if let Some(ref mut rs) = app.rename {
                                if rs.cursor > 0 {
                                    // Remove the char before cursor
                                    let prev = rs.text[..rs.cursor]
                                        .char_indices()
                                        .next_back()
                                        .map(|(i, _)| i)
                                        .unwrap_or(0);
                                    rs.text.drain(prev..rs.cursor);
                                    rs.cursor = prev;
                                }
                            }
                        }
                        0xffff => {
                            // Delete
                            if let Some(ref mut rs) = app.rename {
                                if rs.cursor < rs.text.len() {
                                    let next = rs.text[rs.cursor..]
                                        .char_indices()
                                        .nth(1)
                                        .map(|(i, _)| rs.cursor + i)
                                        .unwrap_or(rs.text.len());
                                    rs.text.drain(rs.cursor..next);
                                }
                            }
                        }
                        0xff51 => {
                            // Left arrow
                            if let Some(ref mut rs) = app.rename {
                                if rs.cursor > 0 {
                                    rs.cursor = rs.text[..rs.cursor]
                                        .char_indices()
                                        .next_back()
                                        .map(|(i, _)| i)
                                        .unwrap_or(0);
                                }
                            }
                        }
                        0xff53 => {
                            // Right arrow
                            if let Some(ref mut rs) = app.rename {
                                if rs.cursor < rs.text.len() {
                                    rs.cursor = rs.text[rs.cursor..]
                                        .char_indices()
                                        .nth(1)
                                        .map(|(i, _)| rs.cursor + i)
                                        .unwrap_or(rs.text.len());
                                }
                            }
                        }
                        0xff50 => {
                            // Home
                            if let Some(ref mut rs) = app.rename {
                                rs.cursor = 0;
                            }
                        }
                        0xff57 => {
                            // End
                            if let Some(ref mut rs) = app.rename {
                                rs.cursor = rs.text.len();
                            }
                        }
                        _ => {
                            // Printable character — lookup string from keycode
                            if let Some(ch) = printable_char_from_sym(sym) {
                                if let Some(ref mut rs) = app.rename {
                                    rs.text.insert(rs.cursor, ch);
                                    rs.cursor += ch.len_utf8();
                                }
                            }
                        }
                    }
                    renderer.redraw(&conn, &app)?;
                }
                Event::ButtonPress(ev) if ev.event == window => {
                    // Click outside the rename row → commit
                    app.commit_rename();
                    renderer.redraw(&conn, &app)?;
                }
                Event::Expose(ev) if ev.count == 0 && ev.window == window => {
                    renderer.redraw(&conn, &app)?;
                }
                Event::ConfigureNotify(ev) if ev.window == window => {
                    app.x = ev.x;
                    app.y = ev.y;
                    let (new_w, new_h) = (ev.width, ev.height);
                    if new_w != app.width || new_h != app.height {
                        app.width = new_w;
                        app.height = new_h;
                        renderer.resize(&conn, new_w, new_h)?;
                        renderer.redraw(&conn, &app)?;
                    }
                }
                _ => {}
            }
            continue;
        }

        // ── Normal mode ──
        match event {
            Event::Expose(ev) if ev.count == 0 && ev.window == window => {
                renderer.redraw(&conn, &app)?;
            }
            Event::ConfigureNotify(ev) if ev.window == window => {
                app.x = ev.x;
                app.y = ev.y;
                let (new_w, new_h) = (ev.width, ev.height);
                if new_w != app.width || new_h != app.height {
                    app.width = new_w;
                    app.height = new_h;
                    renderer.resize(&conn, new_w, new_h)?;
                    renderer.redraw(&conn, &app)?;
                }
            }
            Event::ButtonPress(ev) if ev.event == window => {
                match ev.detail {
                    1 => {
                        if app.hit_test_header_button(ev.event_y) {
                            spawn_default_terminal();
                        } else if let Some(row) = app.hit_test_row(ev.event_y) {
                            app.drag = Some(DragState {
                                source_row: row,
                                start_y: ev.event_y,
                                current_y: ev.event_y,
                                started: false,
                            });
                        }
                    }
                    3 => {
                        if app.drag.is_none() && !app.hit_test_header_button(ev.event_y) {
                            if let Some(row) = app.hit_test_row(ev.event_y) {
                                open_context_menu(
                                    &conn,
                                    screen,
                                    &renderer,
                                    &mut app,
                                    row,
                                    ev.root_x,
                                    ev.root_y,
                                )?;
                            }
                        }
                    }
                    _ => {}
                }
                renderer.redraw(&conn, &app)?;
            }
            Event::MotionNotify(ev) if ev.event == window => {
                let mut needs_redraw = false;
                if let Some(ref mut drag) = app.drag {
                    drag.current_y = ev.event_y;
                    if !drag.started
                        && (drag.current_y - drag.start_y).abs() > DRAG_THRESHOLD
                    {
                        drag.started = true;
                    }
                    needs_redraw = true;

                    // Drain queued motion events during drag
                    while let Some(queued) = conn.poll_for_event()? {
                        if let Event::MotionNotify(mn) = queued {
                            if let Some(ref mut drag) = app.drag {
                                drag.current_y = mn.event_y;
                            }
                        } else {
                            match queued {
                                Event::ButtonRelease(br)
                                    if br.detail == 1 && br.event == window =>
                                {
                                    handle_release(&conn, root, &atoms, &mut app);
                                    needs_redraw = true;
                                }
                                Event::Expose(ex) if ex.count == 0 => {
                                    needs_redraw = true;
                                }
                                _ => {}
                            }
                            break;
                        }
                    }
                } else {
                    // Hover tracking (no drag active)
                    let new_hover = app.hit_test_row(ev.event_y);
                    let new_header = app.hit_test_header_button(ev.event_y);
                    if new_hover != app.hover_row {
                        app.hover_row = new_hover;
                        needs_redraw = true;
                    }
                    if new_header != app.header_hovered {
                        app.header_hovered = new_header;
                        needs_redraw = true;
                    }
                    // Drain queued motion for hover too
                    while let Some(queued) = conn.poll_for_event()? {
                        if let Event::MotionNotify(mn) = queued {
                            let h = app.hit_test_row(mn.event_y);
                            let hb = app.hit_test_header_button(mn.event_y);
                            if h != app.hover_row {
                                app.hover_row = h;
                                needs_redraw = true;
                            }
                            if hb != app.header_hovered {
                                app.header_hovered = hb;
                                needs_redraw = true;
                            }
                        } else {
                            // Non-motion event during hover drain — break and let main loop handle next
                            // We can't easily re-queue, so just handle common cases
                            match queued {
                                Event::Expose(ex) if ex.count == 0 => {
                                    needs_redraw = true;
                                }
                                _ => {}
                            }
                            break;
                        }
                    }
                }
                if needs_redraw {
                    renderer.redraw(&conn, &app)?;
                }
            }
            Event::LeaveNotify(_) => {
                let was_hovering = app.hover_row.is_some() || app.header_hovered;
                app.hover_row = None;
                app.header_hovered = false;
                if was_hovering {
                    renderer.redraw(&conn, &app)?;
                }
            }
            Event::ButtonRelease(ev) if ev.detail == 1 && ev.event == window => {
                handle_release(&conn, root, &atoms, &mut app);
                renderer.redraw(&conn, &app)?;
            }
            _ => {}
        }
    }

    Ok(())
}

fn handle_release(conn: &impl Connection, root: Window, atoms: &Atoms, app: &mut App) {
    if let Some(drag) = app.drag.take() {
        if drag.started {
            app.handle_drop(drag.source_row, drag.current_y);
        } else {
            // Click (no drag)
            if drag.source_row < app.display_rows.len() {
                let row = app.display_rows[drag.source_row].clone();
                match row {
                    DisplayRow::GroupHeader { group_id } => {
                        app.toggle_collapse(group_id);
                    }
                    DisplayRow::Window { wid, .. } => {
                        let _ = activate_window(conn, root, wid, atoms);
                        app.active_wid = Some(wid);
                        std::thread::sleep(std::time::Duration::from_millis(50));
                        let _ = snap_to_sidebar(conn, root, app.our_wid, wid, atoms);
                    }
                    DisplayRow::Session { name, .. } => {
                        app.pending_attach = Some((name.clone(), std::time::Instant::now()));
                        spawn_attach_terminal(&name);
                    }
                }
            }
        }
    }
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;

    fn make_app() -> App {
        App::new(999) // dummy wid for our sidebar window
    }

    fn add_item(app: &mut App, wid: u32, label: &str) {
        app.items.push(Item {
            wid,
            label: label.to_string(),
            wm_class: "test".to_string(),
            accent_pixel: 0,
            custom_prefix: String::new(),
            session: None,
        });
        app.display_order.push(DisplaySlot::Window(wid));
    }

    // ── build_display_rows ──

    #[test]
    fn empty_state_produces_no_rows() {
        let mut app = make_app();
        app.build_display_rows();
        assert_eq!(app.display_rows.len(), 0);
    }

    #[test]
    fn ungrouped_windows_produce_flat_rows() {
        let mut app = make_app();
        add_item(&mut app, 1, "A");
        add_item(&mut app, 2, "B");
        add_item(&mut app, 3, "C");
        app.build_display_rows();

        assert_eq!(app.display_rows.len(), 3);
        assert!(matches!(app.display_rows[0], DisplayRow::Window { wid: 1, group_id: None }));
        assert!(matches!(app.display_rows[1], DisplayRow::Window { wid: 2, group_id: None }));
        assert!(matches!(app.display_rows[2], DisplayRow::Window { wid: 3, group_id: None }));
    }

    #[test]
    fn group_produces_header_plus_members() {
        let mut app = make_app();
        add_item(&mut app, 1, "A");
        add_item(&mut app, 2, "B");
        app.create_group(1); // group containing window 1

        assert!(matches!(app.display_rows[0], DisplayRow::GroupHeader { group_id: 0 }));
        assert!(matches!(app.display_rows[1], DisplayRow::Window { wid: 1, group_id: Some(0) }));
        assert!(matches!(app.display_rows[2], DisplayRow::Window { wid: 2, group_id: None }));
    }

    #[test]
    fn collapsed_group_hides_members() {
        let mut app = make_app();
        add_item(&mut app, 1, "A");
        add_item(&mut app, 2, "B");
        app.create_group(1);
        app.toggle_collapse(0);

        assert_eq!(app.display_rows.len(), 2); // header + window B
        assert!(matches!(app.display_rows[0], DisplayRow::GroupHeader { group_id: 0 }));
        assert!(matches!(app.display_rows[1], DisplayRow::Window { wid: 2, group_id: None }));
    }

    #[test]
    fn expand_collapsed_group_shows_members() {
        let mut app = make_app();
        add_item(&mut app, 1, "A");
        app.create_group(1);
        app.toggle_collapse(0); // collapse
        assert_eq!(app.display_rows.len(), 1);
        app.toggle_collapse(0); // expand
        assert_eq!(app.display_rows.len(), 2);
        assert!(matches!(app.display_rows[1], DisplayRow::Window { wid: 1, group_id: Some(0) }));
    }

    // ── Hit testing + responsive layout ──

    #[test]
    fn row_y_is_sequential() {
        let app = make_app();
        let y0 = app.row_y(0);
        let y1 = app.row_y(1);
        assert_eq!(y0, ITEM_Y_START);
        assert_eq!(y1, ITEM_Y_START + ITEM_H as i16 + ITEM_SPACING);
    }

    #[test]
    fn item_width_adapts_to_window_width() {
        let mut app = make_app();
        app.width = 250;
        assert_eq!(app.item_w(), 250 - ITEM_MARGIN as u16 * 2);

        app.width = 400;
        assert_eq!(app.item_w(), 400 - ITEM_MARGIN as u16 * 2);

        app.width = 100;
        assert_eq!(app.item_w(), 100 - ITEM_MARGIN as u16 * 2);
    }

    #[test]
    fn hit_test_returns_correct_row() {
        let mut app = make_app();
        add_item(&mut app, 1, "A");
        add_item(&mut app, 2, "B");
        app.build_display_rows();

        let mid_row0 = app.row_y(0) + ITEM_H as i16 / 2;
        assert_eq!(app.hit_test_row(mid_row0), Some(0));

        let mid_row1 = app.row_y(1) + ITEM_H as i16 / 2;
        assert_eq!(app.hit_test_row(mid_row1), Some(1));
    }

    #[test]
    fn hit_test_returns_none_in_gap() {
        let mut app = make_app();
        add_item(&mut app, 1, "A");
        app.build_display_rows();

        // Below all items
        let below = app.row_y(0) + ITEM_H as i16 + 100;
        assert_eq!(app.hit_test_row(below), None);
        // Above all items
        assert_eq!(app.hit_test_row(0), None);
    }

    #[test]
    fn drop_index_at_top_is_zero() {
        let mut app = make_app();
        add_item(&mut app, 1, "A");
        add_item(&mut app, 2, "B");
        app.build_display_rows();

        assert_eq!(app.drop_index_from_y(0), 0);
    }

    #[test]
    fn drop_index_past_end() {
        let mut app = make_app();
        add_item(&mut app, 1, "A");
        app.build_display_rows();

        assert_eq!(app.drop_index_from_y(9999), 1);
    }

    // ── Group operations ──

    #[test]
    fn create_group_replaces_window_slot() {
        let mut app = make_app();
        add_item(&mut app, 1, "A");
        add_item(&mut app, 2, "B");
        app.create_group(1);

        assert_eq!(app.groups.len(), 1);
        assert_eq!(app.groups[0].member_wids, vec![1]);
        assert!(matches!(app.display_order[0], DisplaySlot::Group(0)));
        assert!(matches!(app.display_order[1], DisplaySlot::Window(2)));
    }

    #[test]
    fn add_to_group_moves_window() {
        let mut app = make_app();
        add_item(&mut app, 1, "A");
        add_item(&mut app, 2, "B");
        app.create_group(1);
        app.add_to_group(0, 2);

        assert_eq!(app.groups[0].member_wids, vec![1, 2]);
        assert_eq!(app.display_order.len(), 1); // only the group slot remains
    }

    #[test]
    fn remove_from_group_inserts_after_group() {
        let mut app = make_app();
        add_item(&mut app, 1, "A");
        add_item(&mut app, 2, "B");
        app.create_group(1);
        app.add_to_group(0, 2);
        app.remove_from_group(1); // remove window 1

        assert_eq!(app.groups[0].member_wids, vec![2]);
        assert_eq!(app.display_order.len(), 2);
        assert!(matches!(app.display_order[0], DisplaySlot::Group(0)));
        assert!(matches!(app.display_order[1], DisplaySlot::Window(1)));
    }

    #[test]
    fn delete_group_restores_members() {
        let mut app = make_app();
        add_item(&mut app, 1, "A");
        add_item(&mut app, 2, "B");
        add_item(&mut app, 3, "C");
        app.create_group(1);
        app.add_to_group(0, 2);
        app.delete_group(0);

        assert_eq!(app.groups.len(), 0);
        assert_eq!(app.display_order.len(), 3);
        // Members inserted where group was, then remaining window
        assert!(matches!(app.display_order[0], DisplaySlot::Window(1)));
        assert!(matches!(app.display_order[1], DisplaySlot::Window(2)));
        assert!(matches!(app.display_order[2], DisplaySlot::Window(3)));
    }

    #[test]
    fn rename_group_inline() {
        let mut app = make_app();
        add_item(&mut app, 1, "A");
        app.create_group(1);
        assert_eq!(app.groups[0].name, "Group 1");

        // Start rename — populates RenameState with current name
        app.start_rename(0);
        assert!(app.rename.is_some());
        assert_eq!(app.rename.as_ref().unwrap().text, "Group 1");

        // Simulate typing: clear and type new name
        if let Some(ref mut rs) = app.rename {
            rs.text = "My Project".to_string();
            rs.cursor = rs.text.len();
        }

        // Commit
        app.commit_rename();
        assert!(app.rename.is_none());
        assert_eq!(app.groups[0].name, "My Project");
    }

    #[test]
    fn rename_cancel_preserves_old_name() {
        let mut app = make_app();
        add_item(&mut app, 1, "A");
        app.create_group(1);

        app.start_rename(0);
        if let Some(ref mut rs) = app.rename {
            rs.text = "New Name".to_string();
        }
        app.cancel_rename();
        assert_eq!(app.groups[0].name, "Group 1");
    }

    #[test]
    fn rename_empty_string_preserves_old_name() {
        let mut app = make_app();
        add_item(&mut app, 1, "A");
        app.create_group(1);

        app.start_rename(0);
        if let Some(ref mut rs) = app.rename {
            rs.text = "   ".to_string();
        }
        app.commit_rename();
        assert_eq!(app.groups[0].name, "Group 1"); // blank rejected
    }

    // ── Drag-and-drop ──

    #[test]
    fn is_gap_in_group_true_between_members() {
        let mut app = make_app();
        add_item(&mut app, 1, "A");
        add_item(&mut app, 2, "B");
        add_item(&mut app, 3, "C");
        app.create_group(1);
        app.add_to_group(0, 2);
        // display_rows: [Header(0), Window(1,g0), Window(2,g0), Window(3,None)]

        assert!(app.is_gap_in_group(1, 0)); // between header and first member
        assert!(app.is_gap_in_group(2, 0)); // between two members
        assert!(app.is_gap_in_group(3, 0)); // after last member
        assert!(!app.is_gap_in_group(0, 0)); // before header
    }

    #[test]
    fn reorder_ungrouped_windows() {
        let mut app = make_app();
        add_item(&mut app, 1, "A");
        add_item(&mut app, 2, "B");
        add_item(&mut app, 3, "C");
        app.build_display_rows();

        // Drag window at row 0 to gap 2 (between B and C)
        let y_gap2 = app.row_y(2); // just above row 2's midpoint
        app.handle_drop(0, y_gap2);

        // A moved from position 0 to after B
        assert!(matches!(app.display_order[0], DisplaySlot::Window(2)));
        assert!(matches!(app.display_order[1], DisplaySlot::Window(1)));
        assert!(matches!(app.display_order[2], DisplaySlot::Window(3)));
    }

    #[test]
    fn drag_window_onto_group_header_adds_to_group() {
        let mut app = make_app();
        add_item(&mut app, 1, "A");
        add_item(&mut app, 2, "B");
        app.create_group(1);
        // display_rows: [Header(0), Window(1,g0), Window(2,None)]

        // Drag window 2 (row 2) onto group header (row 0)
        let header_y = app.row_y(0) + ITEM_H as i16 / 2;
        app.handle_drop(2, header_y);

        assert_eq!(app.groups[0].member_wids, vec![1, 2]);
        assert_eq!(app.display_order.len(), 1); // only group remains
    }

    #[test]
    fn drag_grouped_window_outside_ungroups() {
        let mut app = make_app();
        add_item(&mut app, 1, "A");
        add_item(&mut app, 2, "B");
        add_item(&mut app, 3, "C");
        app.create_group(1);
        app.add_to_group(0, 2);
        // display_rows: [Header(0), Window(1,g0), Window(2,g0), Window(3,None)]

        // Drag window 1 (row 1) past end of list (well below everything)
        app.handle_drop(1, 9999);

        assert_eq!(app.groups[0].member_wids, vec![2]); // only window 2 remains
        // Window 1 should now be ungrouped in display_order
        let ungrouped_wids: Vec<u32> = app
            .display_order
            .iter()
            .filter_map(|s| if let DisplaySlot::Window(w) = s { Some(*w) } else { None })
            .collect();
        assert!(ungrouped_wids.contains(&1));
    }

    // ── Menu entries ──

    #[test]
    fn menu_for_ungrouped_window_has_new_group() {
        let mut app = make_app();
        add_item(&mut app, 1, "A");
        app.build_display_rows();

        let entries = build_menu_entries(&app, 0);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].label, "Rename Tab");
        assert_eq!(entries[1].label, "New Group");
    }

    #[test]
    fn menu_for_ungrouped_shows_existing_groups() {
        let mut app = make_app();
        add_item(&mut app, 1, "A");
        add_item(&mut app, 2, "B");
        app.create_group(1);
        // display_rows: [Header(0), Window(1,g0), Window(2,None)]

        let entries = build_menu_entries(&app, 2); // right-click on ungrouped window 2
        assert_eq!(entries.len(), 3); // "Rename Tab" + "New Group" + "Add to Group 1"
        assert!(matches!(entries[2].action, MenuAction::AddToGroup(0)));
    }

    #[test]
    fn menu_for_grouped_window_has_remove() {
        let mut app = make_app();
        add_item(&mut app, 1, "A");
        app.create_group(1);
        // display_rows: [Header(0), Window(1,g0)]

        let entries = build_menu_entries(&app, 1);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].label, "Rename Tab");
        assert_eq!(entries[1].label, "Remove from Group");
    }

    #[test]
    fn menu_for_group_header_has_rename_and_delete() {
        let mut app = make_app();
        add_item(&mut app, 1, "A");
        app.create_group(1);

        let entries = build_menu_entries(&app, 0); // group header
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].label, "Rename Group");
        assert_eq!(entries[1].label, "Delete Group");
    }

    // ── Tab rename ──

    #[test]
    fn display_label_without_prefix() {
        let mut app = make_app();
        add_item(&mut app, 1, "Firefox");
        assert_eq!(app.items[0].display_label(), "Firefox");
    }

    #[test]
    fn display_label_with_prefix() {
        let mut app = make_app();
        add_item(&mut app, 1, "steve@bambam: ~/dev");
        app.items[0].custom_prefix = "Dev Terminal".to_string();
        assert_eq!(
            app.items[0].display_label(),
            "Dev Terminal: steve@bambam: ~/dev"
        );
    }

    #[test]
    fn menu_for_ungrouped_window_has_rename_tab() {
        let mut app = make_app();
        add_item(&mut app, 1, "A");
        app.build_display_rows();

        let entries = build_menu_entries(&app, 0);
        assert!(entries.iter().any(|e| e.label == "Rename Tab"));
    }

    #[test]
    fn menu_for_grouped_window_has_rename_tab() {
        let mut app = make_app();
        add_item(&mut app, 1, "A");
        app.create_group(1);

        let entries = build_menu_entries(&app, 1); // window row inside group
        assert!(entries.iter().any(|e| e.label == "Rename Tab"));
    }

    #[test]
    fn start_tab_rename_initializes_with_prefix() {
        let mut app = make_app();
        add_item(&mut app, 1, "Firefox");
        app.items[0].custom_prefix = "Browser".to_string();
        app.build_display_rows();

        app.start_tab_rename(1);
        assert!(app.rename.is_some());
        let rs = app.rename.as_ref().unwrap();
        assert_eq!(rs.text, "Browser");
    }

    #[test]
    fn start_tab_rename_empty_prefix() {
        let mut app = make_app();
        add_item(&mut app, 1, "Firefox");
        app.build_display_rows();

        app.start_tab_rename(1);
        assert!(app.rename.is_some());
        let rs = app.rename.as_ref().unwrap();
        assert_eq!(rs.text, "");
    }

    #[test]
    fn commit_tab_rename_sets_prefix() {
        let mut app = make_app();
        add_item(&mut app, 1, "Firefox");
        app.build_display_rows();

        app.start_tab_rename(1);
        if let Some(ref mut rs) = app.rename {
            rs.text = "Browser".to_string();
            rs.cursor = rs.text.len();
        }
        app.commit_rename();
        assert_eq!(app.items[0].custom_prefix, "Browser");
    }

    #[test]
    fn commit_tab_rename_empty_clears_prefix() {
        let mut app = make_app();
        add_item(&mut app, 1, "Firefox");
        app.items[0].custom_prefix = "Browser".to_string();
        app.build_display_rows();

        app.start_tab_rename(1);
        if let Some(ref mut rs) = app.rename {
            rs.text = "   ".to_string(); // whitespace only
        }
        app.commit_rename();
        assert_eq!(app.items[0].custom_prefix, ""); // cleared
    }

    #[test]
    fn cancel_tab_rename_preserves_prefix() {
        let mut app = make_app();
        add_item(&mut app, 1, "Firefox");
        app.items[0].custom_prefix = "Browser".to_string();
        app.build_display_rows();

        app.start_tab_rename(1);
        if let Some(ref mut rs) = app.rename {
            rs.text = "Something else".to_string();
        }
        app.cancel_rename();
        assert_eq!(app.items[0].custom_prefix, "Browser"); // unchanged
    }

    // ── find_item ──

    #[test]
    fn find_item_by_wid() {
        let mut app = make_app();
        add_item(&mut app, 42, "Test Window");
        assert!(app.find_item(42).is_some());
        assert_eq!(app.find_item(42).unwrap().label, "Test Window");
        assert!(app.find_item(99).is_none());
    }

    // ── display_row_to_slot_position ──

    #[test]
    fn slot_position_maps_correctly() {
        let mut app = make_app();
        add_item(&mut app, 1, "A");
        add_item(&mut app, 2, "B");
        add_item(&mut app, 3, "C");
        app.create_group(1);
        app.add_to_group(0, 2);
        // display_order: [Group(0), Window(3)]
        // display_rows: [Header(0), Window(1,g0), Window(2,g0), Window(3,None)]

        assert_eq!(app.display_row_to_slot_position(0), 0); // before header → slot 0
        assert_eq!(app.display_row_to_slot_position(3), 1); // before Window(3) → slot 1
        assert_eq!(app.display_row_to_slot_position(4), 2); // past end → slot 2
    }

    // ── geometry persistence ──

    #[test]
    fn geometry_round_trip() {
        let dir = std::env::temp_dir().join("ptm_test_geom_rt");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = geometry_path_in(&dir);

        save_geometry_to(&path, 100, 200, 300, 400);
        let loaded = load_geometry_from(&path);
        assert_eq!(loaded, Some((100, 200, 300, 400)));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn geometry_negative_coords() {
        let dir = std::env::temp_dir().join("ptm_test_geom_neg");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = geometry_path_in(&dir);

        save_geometry_to(&path, -50, -100, 250, 600);
        let loaded = load_geometry_from(&path);
        assert_eq!(loaded, Some((-50, -100, 250, 600)));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn geometry_missing_file_returns_none() {
        let dir = std::env::temp_dir().join("ptm_test_geom_missing");
        let _ = std::fs::remove_dir_all(&dir);
        let path = geometry_path_in(&dir);

        assert_eq!(load_geometry_from(&path), None);
    }

    #[test]
    fn geometry_malformed_file_returns_none() {
        let dir = std::env::temp_dir().join("ptm_test_geom_bad");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = geometry_path_in(&dir);

        std::fs::write(&path, "not valid geometry\n").unwrap();
        assert_eq!(load_geometry_from(&path), None);

        std::fs::write(&path, "1 2 3\n").unwrap();
        assert_eq!(load_geometry_from(&path), None);

        std::fs::write(&path, "1 2 3 4 5\n").unwrap();
        assert_eq!(load_geometry_from(&path), None);

        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── Group persistence ──

    fn groups_path_in(dir: &std::path::Path) -> std::path::PathBuf {
        dir.join("groups")
    }

    fn add_item_with_class(app: &mut App, wid: u32, label: &str, wm_class: &str) {
        app.items.push(Item {
            wid,
            label: label.to_string(),
            wm_class: wm_class.to_string(),
            accent_pixel: 0,
            custom_prefix: String::new(),
            session: None,
        });
        app.display_order.push(DisplaySlot::Window(wid));
    }

    #[test]
    fn groups_save_load_round_trip() {
        let dir = std::env::temp_dir().join("ptm_test_groups_rt");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = groups_path_in(&dir);

        let groups = vec![
            SavedGroup {
                name: "Browsers".to_string(),
                collapsed: true,
                members: vec![
                    SavedMember {
                        label: "Firefox".to_string(),
                        wm_class: "Navigator".to_string(),
                        custom_prefix: "FF".to_string(),
                    },
                    SavedMember {
                        label: "Chrome".to_string(),
                        wm_class: "google-chrome".to_string(),
                        custom_prefix: String::new(),
                    },
                ],
            },
            SavedGroup {
                name: "Terminals".to_string(),
                collapsed: false,
                members: vec![SavedMember {
                    label: "Terminal".to_string(),
                    wm_class: "gnome-terminal-server".to_string(),
                    custom_prefix: "Dev".to_string(),
                }],
            },
        ];

        save_groups_to(&path, &groups);
        let loaded = load_groups_from(&path).unwrap();

        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].name, "Browsers");
        assert!(loaded[0].collapsed);
        assert_eq!(loaded[0].members.len(), 2);
        assert_eq!(loaded[0].members[0].label, "Firefox");
        assert_eq!(loaded[0].members[0].wm_class, "Navigator");
        assert_eq!(loaded[0].members[0].custom_prefix, "FF");
        assert_eq!(loaded[0].members[1].label, "Chrome");
        assert_eq!(loaded[0].members[1].custom_prefix, "");
        assert_eq!(loaded[1].name, "Terminals");
        assert!(!loaded[1].collapsed);
        assert_eq!(loaded[1].members.len(), 1);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn groups_load_missing_file_returns_none() {
        let dir = std::env::temp_dir().join("ptm_test_groups_missing");
        let _ = std::fs::remove_dir_all(&dir);
        let path = groups_path_in(&dir);
        assert!(load_groups_from(&path).is_none());
    }

    #[test]
    fn groups_load_malformed_returns_none() {
        let dir = std::env::temp_dir().join("ptm_test_groups_bad");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = groups_path_in(&dir);

        // Wrong version
        std::fs::write(&path, "v2\nGROUP\tFoo\t0\n").unwrap();
        assert!(load_groups_from(&path).is_none());

        // Bad collapsed value
        std::fs::write(&path, "v1\nGROUP\tFoo\t2\n").unwrap();
        assert!(load_groups_from(&path).is_none());

        // MEMBER before any GROUP
        std::fs::write(&path, "v1\nMEMBER\tFoo\tbar\t\n").unwrap();
        assert!(load_groups_from(&path).is_none());

        // Wrong number of fields in GROUP
        std::fs::write(&path, "v1\nGROUP\tFoo\n").unwrap();
        assert!(load_groups_from(&path).is_none());

        // Wrong number of fields in MEMBER
        std::fs::write(&path, "v1\nGROUP\tFoo\t0\nMEMBER\tBar\n").unwrap();
        assert!(load_groups_from(&path).is_none());

        // Unknown line type
        std::fs::write(&path, "v1\nGROUP\tFoo\t0\nBADLINE\n").unwrap();
        assert!(load_groups_from(&path).is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn extract_saved_state_from_app() {
        let mut app = make_app();
        add_item_with_class(&mut app, 1, "Firefox", "Navigator");
        add_item_with_class(&mut app, 2, "Terminal", "gnome-terminal");
        add_item_with_class(&mut app, 3, "Code", "code");
        app.items[0].custom_prefix = "FF".to_string();
        app.create_group(1);
        app.add_to_group(0, 2);
        // Group 0 has windows 1, 2; window 3 is ungrouped

        let saved = extract_saved_state(&app);
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].name, "Group 1");
        assert!(!saved[0].collapsed);
        assert_eq!(saved[0].members.len(), 2);
        assert_eq!(saved[0].members[0].label, "Firefox");
        assert_eq!(saved[0].members[0].wm_class, "Navigator");
        assert_eq!(saved[0].members[0].custom_prefix, "FF");
        assert_eq!(saved[0].members[1].label, "Terminal");
        assert_eq!(saved[0].members[1].wm_class, "gnome-terminal");
    }

    #[test]
    fn restore_groups_basic() {
        let mut app = make_app();
        add_item_with_class(&mut app, 10, "Firefox", "Navigator");
        add_item_with_class(&mut app, 20, "Terminal", "gnome-terminal");
        add_item_with_class(&mut app, 30, "Code", "code");

        let saved = vec![SavedGroup {
            name: "Browsers".to_string(),
            collapsed: true,
            members: vec![SavedMember {
                label: "Firefox".to_string(),
                wm_class: "Navigator".to_string(),
                custom_prefix: String::new(),
            }],
        }];

        restore_groups(&mut app, &saved);

        assert_eq!(app.groups.len(), 1);
        assert_eq!(app.groups[0].name, "Browsers");
        assert!(app.groups[0].collapsed);
        assert_eq!(app.groups[0].member_wids, vec![10]);
        // display_order: Group first, then ungrouped windows
        assert!(matches!(app.display_order[0], DisplaySlot::Group(0)));
        assert!(matches!(app.display_order[1], DisplaySlot::Window(20)));
        assert!(matches!(app.display_order[2], DisplaySlot::Window(30)));
    }

    #[test]
    fn restore_groups_partial_match() {
        let mut app = make_app();
        add_item_with_class(&mut app, 10, "Firefox", "Navigator");
        add_item_with_class(&mut app, 20, "Code", "code");

        let saved = vec![SavedGroup {
            name: "Dev".to_string(),
            collapsed: false,
            members: vec![
                SavedMember {
                    label: "Firefox".to_string(),
                    wm_class: "Navigator".to_string(),
                    custom_prefix: String::new(),
                },
                SavedMember {
                    label: "Terminal".to_string(),
                    wm_class: "gnome-terminal".to_string(),
                    custom_prefix: String::new(),
                },
                SavedMember {
                    label: "Code".to_string(),
                    wm_class: "code".to_string(),
                    custom_prefix: String::new(),
                },
            ],
        }];

        restore_groups(&mut app, &saved);

        assert_eq!(app.groups.len(), 1);
        assert_eq!(app.groups[0].member_wids, vec![10, 20]); // Terminal not found
    }

    #[test]
    fn restore_groups_no_match_skips_group() {
        let mut app = make_app();
        add_item_with_class(&mut app, 10, "Firefox", "Navigator");

        let saved = vec![SavedGroup {
            name: "Gone".to_string(),
            collapsed: false,
            members: vec![SavedMember {
                label: "Terminal".to_string(),
                wm_class: "gnome-terminal".to_string(),
                custom_prefix: String::new(),
            }],
        }];

        restore_groups(&mut app, &saved);

        assert_eq!(app.groups.len(), 0);
        assert!(matches!(app.display_order[0], DisplaySlot::Window(10)));
    }

    #[test]
    fn restore_groups_duplicate_titles_different_class() {
        let mut app = make_app();
        add_item_with_class(&mut app, 10, "Terminal", "gnome-terminal");
        add_item_with_class(&mut app, 20, "Terminal", "xterm");

        let saved = vec![SavedGroup {
            name: "Terms".to_string(),
            collapsed: false,
            members: vec![SavedMember {
                label: "Terminal".to_string(),
                wm_class: "xterm".to_string(),
                custom_prefix: String::new(),
            }],
        }];

        restore_groups(&mut app, &saved);

        assert_eq!(app.groups.len(), 1);
        assert_eq!(app.groups[0].member_wids, vec![20]); // matched xterm, not gnome-terminal
    }

    #[test]
    fn restore_groups_custom_prefix() {
        let mut app = make_app();
        add_item_with_class(&mut app, 10, "Firefox", "Navigator");
        add_item_with_class(&mut app, 20, "Terminal", "gnome-terminal");

        let saved = vec![SavedGroup {
            name: "Dev".to_string(),
            collapsed: false,
            members: vec![
                SavedMember {
                    label: "Firefox".to_string(),
                    wm_class: "Navigator".to_string(),
                    custom_prefix: "Browser".to_string(),
                },
                SavedMember {
                    label: "Terminal".to_string(),
                    wm_class: "gnome-terminal".to_string(),
                    custom_prefix: String::new(),
                },
            ],
        }];

        restore_groups(&mut app, &saved);

        assert_eq!(app.items[0].custom_prefix, "Browser");
        assert_eq!(app.items[1].custom_prefix, ""); // empty prefix not overwritten
    }

    // ── Property-change classification ──

    #[test]
    fn classify_property_root_client_list() {
        assert_eq!(
            classify_property_event(100, true, 100, 101, 102),
            PropertyAction::RefreshClientList
        );
    }

    #[test]
    fn classify_property_root_active_window() {
        assert_eq!(
            classify_property_event(101, true, 100, 101, 102),
            PropertyAction::UpdateActiveWindow
        );
    }

    #[test]
    fn classify_property_window_net_wm_name() {
        assert_eq!(
            classify_property_event(102, false, 100, 101, 102),
            PropertyAction::UpdateWindowTitle
        );
    }

    #[test]
    fn classify_property_window_legacy_wm_name() {
        assert_eq!(
            classify_property_event(u32::from(AtomEnum::WM_NAME), false, 100, 101, 102),
            PropertyAction::UpdateWindowTitle
        );
    }

    #[test]
    fn classify_property_ignores_unknown_root_atom() {
        assert_eq!(
            classify_property_event(999, true, 100, 101, 102),
            PropertyAction::Ignore
        );
    }

    #[test]
    fn classify_property_ignores_unknown_window_atom() {
        assert_eq!(
            classify_property_event(999, false, 100, 101, 102),
            PropertyAction::Ignore
        );
    }

    #[test]
    fn classify_property_wm_name_on_root_is_ignored() {
        // _NET_WM_NAME on the root window is not the taskbar signal we care about
        assert_eq!(
            classify_property_event(102, true, 100, 101, 102),
            PropertyAction::Ignore
        );
    }

    // ── tmux detection (pure helpers) ──

    #[test]
    fn parse_tmux_list_clients_empty() {
        assert!(parse_tmux_list_clients("").is_empty());
    }

    #[test]
    fn parse_tmux_list_clients_basic() {
        let input = "1234 main\n5678 dev-work\n";
        let m = parse_tmux_list_clients(input);
        assert_eq!(m.len(), 2);
        assert_eq!(m.get(&1234).map(String::as_str), Some("main"));
        assert_eq!(m.get(&5678).map(String::as_str), Some("dev-work"));
    }

    #[test]
    fn parse_tmux_list_clients_session_with_spaces() {
        // tmux allows session names with spaces; split on first whitespace only
        let input = "1234 my cool session\n";
        let m = parse_tmux_list_clients(input);
        assert_eq!(m.get(&1234).map(String::as_str), Some("my cool session"));
    }

    #[test]
    fn parse_tmux_list_clients_skips_blank_lines() {
        let input = "\n1234 main\n\n\n5678 dev\n";
        let m = parse_tmux_list_clients(input);
        assert_eq!(m.len(), 2);
    }

    #[test]
    fn parse_tmux_list_clients_skips_malformed() {
        // Missing session name, non-numeric PID, etc. — drop silently.
        let input = "notanum main\n1234\n5678 ok\n";
        let m = parse_tmux_list_clients(input);
        assert_eq!(m.len(), 1);
        assert_eq!(m.get(&5678).map(String::as_str), Some("ok"));
    }

    // ── tmux list-sessions parsing ──

    #[test]
    fn parse_tmux_list_sessions_empty() {
        assert!(parse_tmux_list_sessions("").is_empty());
    }

    #[test]
    fn parse_tmux_list_sessions_single_attached() {
        let input = "demo 1\n";
        let v = parse_tmux_list_sessions(input);
        assert_eq!(v, vec![("demo".to_string(), true)]);
    }

    #[test]
    fn parse_tmux_list_sessions_single_orphan() {
        let input = "demo 0\n";
        let v = parse_tmux_list_sessions(input);
        assert_eq!(v, vec![("demo".to_string(), false)]);
    }

    #[test]
    fn parse_tmux_list_sessions_mixed() {
        let input = "work 2\norphan 0\ndev 1\n";
        let v = parse_tmux_list_sessions(input);
        assert_eq!(
            v,
            vec![
                ("work".to_string(), true),
                ("orphan".to_string(), false),
                ("dev".to_string(), true),
            ]
        );
    }

    #[test]
    fn parse_tmux_list_sessions_preserves_spaces_in_name() {
        // Session names with spaces — split on the LAST whitespace so the
        // trailing attached-count stays separable.
        let input = "my cool session 0\n";
        let v = parse_tmux_list_sessions(input);
        assert_eq!(v, vec![("my cool session".to_string(), false)]);
    }

    #[test]
    fn parse_tmux_list_sessions_skips_malformed() {
        // Trailing token isn't a number → drop.
        let input = "bad notanumber\ngood 1\n";
        let v = parse_tmux_list_sessions(input);
        assert_eq!(v, vec![("good".to_string(), true)]);
    }

    #[test]
    fn parse_tmux_list_sessions_skips_blank_lines() {
        let input = "\n\na 0\n\nb 1\n\n";
        let v = parse_tmux_list_sessions(input);
        assert_eq!(
            v,
            vec![("a".to_string(), false), ("b".to_string(), true)]
        );
    }

    // ── /proc/<pid>/status parsing ──

    #[test]
    fn parse_proc_status_ppid_basic() {
        let s = "Name:\ttmux: client\nState:\tS (sleeping)\nTgid:\t300\nPid:\t300\nPPid:\t483411\nUid:\t1000\n";
        assert_eq!(parse_proc_status_ppid(s), Some(483411));
    }

    #[test]
    fn parse_proc_status_ppid_missing() {
        assert_eq!(parse_proc_status_ppid("Name:\tfoo\nState:\tR\n"), None);
        assert_eq!(parse_proc_status_ppid(""), None);
    }

    #[test]
    fn parse_proc_status_ppid_malformed() {
        assert_eq!(parse_proc_status_ppid("PPid:\tnotanumber\n"), None);
    }

    // ── Ancestor walk from tmux client up to its owning window ──

    fn ppid_reader(map: HashMap<u32, u32>) -> impl FnMut(u32) -> Option<u32> {
        move |p| map.get(&p).copied()
    }

    fn mk_pid_map(pairs: &[(u32, u32)]) -> HashMap<u32, Vec<u32>> {
        let mut m: HashMap<u32, Vec<u32>> = HashMap::new();
        for &(pid, wid) in pairs {
            m.entry(pid).or_default().push(wid);
        }
        m
    }

    #[test]
    fn walk_to_window_owner_start_pid_is_window() {
        // tmux client is itself running directly under xterm, and xterm's
        // pid was already harvested as a window pid.
        let pid_to_wid = mk_pid_map(&[(100, 42)]);
        let read = ppid_reader(HashMap::new());
        assert_eq!(walk_to_window_owner(100, &pid_to_wid, read, 10), Some(42));
    }

    #[test]
    fn walk_to_window_owner_walks_up_chain() {
        // client 300 -> shell 200 -> xterm 100 (window wid=42)
        let pid_to_wid = mk_pid_map(&[(100, 42)]);
        let read = ppid_reader([(300, 200), (200, 100), (100, 1)].iter().cloned().collect());
        assert_eq!(walk_to_window_owner(300, &pid_to_wid, read, 10), Some(42));
    }

    #[test]
    fn walk_to_window_owner_stops_at_closest_match() {
        // Both 200 and the grand-ancestor 100 are window pids (e.g. cinnamon
        // is also reported as a window pid). The client is under 200, so we
        // must stop at 200 and not keep walking to 100.
        let pid_to_wid = mk_pid_map(&[(200, 42), (100, 99)]);
        let read = ppid_reader([(300, 200), (200, 100), (100, 1)].iter().cloned().collect());
        assert_eq!(walk_to_window_owner(300, &pid_to_wid, read, 10), Some(42));
    }

    #[test]
    fn walk_to_window_owner_reaches_init_without_match() {
        let pid_to_wid = mk_pid_map(&[(100, 42)]);
        let read = ppid_reader([(300, 1)].iter().cloned().collect());
        assert_eq!(walk_to_window_owner(300, &pid_to_wid, read, 10), None);
    }

    #[test]
    fn walk_to_window_owner_process_disappears() {
        // read_ppid returns None (e.g. the process was reaped mid-walk).
        let pid_to_wid = mk_pid_map(&[(100, 42)]);
        let read = ppid_reader(HashMap::new());
        assert_eq!(walk_to_window_owner(300, &pid_to_wid, read, 10), None);
    }

    #[test]
    fn walk_to_window_owner_respects_depth_limit() {
        let pid_to_wid = mk_pid_map(&[(100, 42)]);
        let read = ppid_reader(
            [(500, 400), (400, 300), (300, 200), (200, 100)].iter().cloned().collect(),
        );
        // depth 3 stops before reaching 100
        assert_eq!(walk_to_window_owner(500, &pid_to_wid, read, 3), None);
        // depth 5 reaches it
        let read2 = ppid_reader(
            [(500, 400), (400, 300), (300, 200), (200, 100)].iter().cloned().collect(),
        );
        assert_eq!(walk_to_window_owner(500, &pid_to_wid, read2, 5), Some(42));
    }

    #[test]
    fn walk_to_window_owner_empty_window_set() {
        let pid_to_wid: HashMap<u32, Vec<u32>> = HashMap::new();
        let read = ppid_reader([(300, 200), (200, 100)].iter().cloned().collect());
        assert_eq!(walk_to_window_owner(300, &pid_to_wid, read, 10), None);
    }

    #[test]
    fn walk_to_window_owner_returns_none_on_collision() {
        // gnome-terminal-server pid 2380 hosts two tracked windows. The walk
        // lands here and must refuse to pick one — wrong attribution is worse
        // than no attribution.
        let pid_to_wid = mk_pid_map(&[(2380, 42), (2380, 99)]);
        let read = ppid_reader([(605774, 2380), (2380, 1)].iter().cloned().collect());
        assert_eq!(walk_to_window_owner(605774, &pid_to_wid, read, 10), None);
    }

    #[test]
    fn walk_to_window_owner_collision_does_not_fall_through_to_ancestor() {
        // If the first hit is ambiguous, the walk gives up rather than
        // climbing further and matching an unrelated ancestor (e.g. systemd,
        // if it were a tracked window pid for some reason).
        let pid_to_wid = mk_pid_map(&[(2380, 42), (2380, 99), (1179, 7)]);
        let read = ppid_reader([(605774, 2380), (2380, 1179)].iter().cloned().collect());
        assert_eq!(walk_to_window_owner(605774, &pid_to_wid, read, 10), None);
    }

    // ── Pending-attach claim ──

    fn mk_item(wid: u32) -> Item {
        Item {
            wid,
            label: format!("w{}", wid),
            wm_class: String::new(),
            accent_pixel: 0,
            custom_prefix: String::new(),
            session: None,
        }
    }

    #[test]
    fn pending_attach_claims_sole_new_wid() {
        let now = std::time::Instant::now();
        let mut pending = Some(("demo".to_string(), now));
        let prior: HashSet<u32> = [1, 2].iter().copied().collect();
        let mut items = vec![mk_item(1), mk_item(2), mk_item(3)];
        let pre = claim_pending_attach(
            &mut pending,
            &prior,
            &mut items,
            std::time::Duration::from_secs(5),
            now,
        );
        assert_eq!(items[2].session.as_deref(), Some("demo"));
        assert!(items[0].session.is_none());
        assert!(items[1].session.is_none());
        assert!(pending.is_none(), "claim should clear pending");
        assert!(pre.contains("demo"));
    }

    #[test]
    fn pending_attach_defers_when_no_new_wid() {
        let now = std::time::Instant::now();
        let mut pending = Some(("demo".to_string(), now));
        let prior: HashSet<u32> = [1, 2].iter().copied().collect();
        let mut items = vec![mk_item(1), mk_item(2)];
        let pre = claim_pending_attach(
            &mut pending,
            &prior,
            &mut items,
            std::time::Duration::from_secs(5),
            now,
        );
        assert!(pre.is_empty());
        assert!(pending.is_some(), "pending stays until window appears or timeout");
    }

    #[test]
    fn pending_attach_defers_when_multiple_new_wids() {
        // User clicked attach but also happened to open an unrelated window
        // in the same refresh window. We can't tell which is which — wait.
        let now = std::time::Instant::now();
        let mut pending = Some(("demo".to_string(), now));
        let prior: HashSet<u32> = [1].iter().copied().collect();
        let mut items = vec![mk_item(1), mk_item(2), mk_item(3)];
        let pre = claim_pending_attach(
            &mut pending,
            &prior,
            &mut items,
            std::time::Duration::from_secs(5),
            now,
        );
        assert!(pre.is_empty());
        assert!(items[1].session.is_none());
        assert!(items[2].session.is_none());
        assert!(pending.is_some());
    }

    #[test]
    fn pending_attach_times_out() {
        let spawn = std::time::Instant::now();
        let later = spawn + std::time::Duration::from_secs(10);
        let mut pending = Some(("demo".to_string(), spawn));
        let prior: HashSet<u32> = [1].iter().copied().collect();
        let mut items = vec![mk_item(1), mk_item(2)];
        let pre = claim_pending_attach(
            &mut pending,
            &prior,
            &mut items,
            std::time::Duration::from_secs(5),
            later,
        );
        assert!(pre.is_empty());
        assert!(items[1].session.is_none());
        assert!(pending.is_none(), "timed-out pending should be cleared");
    }

    #[test]
    fn pending_attach_none_is_noop() {
        let now = std::time::Instant::now();
        let mut pending: Option<(String, std::time::Instant)> = None;
        let prior: HashSet<u32> = HashSet::new();
        let mut items = vec![mk_item(1)];
        let pre = claim_pending_attach(
            &mut pending,
            &prior,
            &mut items,
            std::time::Duration::from_secs(5),
            now,
        );
        assert!(pre.is_empty());
        assert!(items[0].session.is_none());
        assert!(pending.is_none());
    }

    // ── Terminal detection ──

    #[test]
    fn detect_terminal_prefers_env_terminal() {
        let argv = detect_terminal_command(Some("urxvt"), |_| true);
        assert_eq!(argv, vec!["urxvt".to_string()]);
    }

    #[test]
    fn detect_terminal_env_terminal_splits_whitespace() {
        // Some users set TERMINAL with args.
        let argv = detect_terminal_command(Some("alacritty -T MyTerm"), |_| true);
        assert_eq!(argv, vec!["alacritty", "-T", "MyTerm"]);
    }

    #[test]
    fn detect_terminal_empty_env_falls_through() {
        // Empty string isn't a valid override.
        let argv = detect_terminal_command(Some(""), |name| name == "x-terminal-emulator");
        assert_eq!(argv, vec!["x-terminal-emulator".to_string()]);
    }

    #[test]
    fn detect_terminal_uses_x_terminal_emulator_when_env_unset() {
        let argv = detect_terminal_command(None, |name| name == "x-terminal-emulator");
        assert_eq!(argv, vec!["x-terminal-emulator".to_string()]);
    }

    #[test]
    fn detect_terminal_falls_back_to_xdg_terminal_exec() {
        let argv = detect_terminal_command(None, |name| name == "xdg-terminal-exec");
        assert_eq!(argv, vec!["xdg-terminal-exec".to_string()]);
    }

    #[test]
    fn detect_terminal_falls_back_to_xterm() {
        // No env, no optional binaries on PATH.
        let argv = detect_terminal_command(None, |_| false);
        assert_eq!(argv, vec!["xterm".to_string()]);
    }

    #[test]
    fn detect_terminal_prefers_x_terminal_emulator_over_xdg() {
        let argv = detect_terminal_command(None, |_| true);
        assert_eq!(argv, vec!["x-terminal-emulator".to_string()]);
    }

    // ── Attach argv builder ──

    #[test]
    fn terminal_argv_for_attach_xterm_uses_dash_e() {
        let term = vec!["xterm".to_string()];
        let argv = terminal_argv_for_attach(&term, "demo");
        assert_eq!(
            argv,
            vec!["xterm", "-e", "tmux", "attach-session", "-t", "demo"]
        );
    }

    #[test]
    fn terminal_argv_for_attach_gnome_terminal_uses_double_dash() {
        let term = vec!["gnome-terminal".to_string()];
        let argv = terminal_argv_for_attach(&term, "demo");
        assert_eq!(
            argv,
            vec!["gnome-terminal", "--", "tmux", "attach-session", "-t", "demo"]
        );
    }

    #[test]
    fn terminal_argv_for_attach_ptyxis_uses_double_dash() {
        let term = vec!["ptyxis".to_string()];
        let argv = terminal_argv_for_attach(&term, "demo");
        assert_eq!(
            argv,
            vec!["ptyxis", "--", "tmux", "attach-session", "-t", "demo"]
        );
    }

    #[test]
    fn terminal_argv_for_attach_unknown_falls_back_to_dash_e() {
        let term = vec!["some-obscure-terminal".to_string()];
        let argv = terminal_argv_for_attach(&term, "dev");
        assert_eq!(
            argv,
            vec![
                "some-obscure-terminal",
                "-e",
                "tmux",
                "attach-session",
                "-t",
                "dev"
            ]
        );
    }

    #[test]
    fn terminal_argv_for_attach_preserves_leading_args() {
        // $TERMINAL="alacritty -T MyTerm" case.
        let term = vec![
            "alacritty".to_string(),
            "-T".to_string(),
            "MyTerm".to_string(),
        ];
        let argv = terminal_argv_for_attach(&term, "demo");
        assert_eq!(
            argv,
            vec![
                "alacritty",
                "-T",
                "MyTerm",
                "-e",
                "tmux",
                "attach-session",
                "-t",
                "demo"
            ]
        );
    }

    #[test]
    fn terminal_argv_for_attach_uses_basename_for_match() {
        // Absolute path still recognised as gnome-terminal.
        let term = vec!["/usr/bin/gnome-terminal".to_string()];
        let argv = terminal_argv_for_attach(&term, "demo");
        assert_eq!(
            argv,
            vec![
                "/usr/bin/gnome-terminal",
                "--",
                "tmux",
                "attach-session",
                "-t",
                "demo"
            ]
        );
    }

    #[test]
    fn terminal_argv_for_attach_empty_term_returns_empty() {
        let argv = terminal_argv_for_attach(&[], "demo");
        assert!(argv.is_empty());
    }

    // ── Session context menu + inline rename ──

    fn push_session(app: &mut App, name: &str) {
        app.display_order
            .push(DisplaySlot::Session(name.to_string()));
        app.build_display_rows();
    }

    #[test]
    fn menu_for_session_has_attach_rename_kill() {
        let mut app = make_app();
        push_session(&mut app, "demo");
        let entries = build_menu_entries(&app, 0);
        assert_eq!(entries.len(), 3);
        assert!(matches!(entries[0].action, MenuAction::AttachSession));
        assert!(matches!(entries[1].action, MenuAction::RenameSession));
        assert!(matches!(entries[2].action, MenuAction::KillSession));
    }

    #[test]
    fn start_session_rename_initializes_with_current_name() {
        let mut app = make_app();
        app.start_session_rename("my-session");
        let rs = app.rename.as_ref().expect("rename state should be set");
        assert_eq!(rs.text, "my-session");
        assert_eq!(rs.cursor, "my-session".len());
        assert!(matches!(&rs.target, RenameTarget::Session(n) if n == "my-session"));
    }

    #[test]
    fn cancel_session_rename_preserves_display_order() {
        let mut app = make_app();
        push_session(&mut app, "demo");
        app.start_session_rename("demo");
        // User types then presses escape.
        if let Some(ref mut rs) = app.rename {
            rs.text = "renamed".to_string();
        }
        app.cancel_rename();
        assert_eq!(app.display_order.len(), 1);
        assert!(matches!(&app.display_order[0], DisplaySlot::Session(n) if n == "demo"));
    }

    // ── Header button hit-test ──

    #[test]
    fn header_button_hit_test_top_region() {
        let app = make_app();
        assert!(app.hit_test_header_button(0));
        assert!(app.hit_test_header_button((HEADER_H - 1) as i16));
    }

    #[test]
    fn header_button_hit_test_below_header_is_false() {
        let app = make_app();
        assert!(!app.hit_test_header_button(HEADER_H as i16));
        assert!(!app.hit_test_header_button(HEADER_H as i16 + 10));
    }

    #[test]
    fn header_button_hit_test_negative_is_false() {
        let app = make_app();
        assert!(!app.hit_test_header_button(-1));
    }
}
