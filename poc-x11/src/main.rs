use std::collections::HashSet;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::*;
use x11rb::protocol::Event;
use x11rb::wrapper::ConnectionExt as _;
use x11rb::COPY_DEPTH_FROM_PARENT;

// Layout constants
const WIN_W: u16 = 250;
const WIN_H: u16 = 600;
const ITEM_X: i16 = 8;
const ITEM_W: u16 = 234;
const ITEM_H: u16 = 28;
const ITEM_SPACING: i16 = 2;
const ITEM_Y_START: i16 = 8;
const DRAG_THRESHOLD: i16 = 5;
const GROUP_INDENT: i16 = 16;
const MENU_ITEM_H: u16 = 24;
const MENU_PADDING: i16 = 4;
const MENU_MIN_W: u16 = 180;
const CHAR_WIDTH: i16 = 8; // approximate for Nimbus Mono L 13px

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

// Accent colors for left-edge stripe (OneDark)
const ACCENT_COLORS: &[u32] = &[0xe06c75, 0x98c379, 0x61afef, 0xc678dd, 0xe5c07b, 0x56b6c2];
const GROUP_COLORS: &[u32] = &[0x61afef, 0xe06c75, 0x98c379, 0xc678dd, 0xe5c07b, 0x56b6c2];

// Preset names for group rename cycling
const GROUP_PRESETS: &[&str] = &["Alpha", "Beta", "Gamma", "Delta", "Epsilon", "Zeta"];

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
        Ok(Self {
            net_client_list: c0.reply()?.atom,
            net_active_window: c1.reply()?.atom,
            net_wm_name: c2.reply()?.atom,
            net_wm_desktop: c3.reply()?.atom,
            net_current_desktop: c4.reply()?.atom,
            net_frame_extents: c5.reply()?.atom,
            net_workarea: c6.reply()?.atom,
            utf8_string: c7.reply()?.atom,
        })
    }
}

// ── Data model ──

struct Item {
    wid: u32,
    label: String,
    #[allow(dead_code)]
    wm_class: String,
    accent_pixel: u32,
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
}

#[derive(Clone, Debug)]
enum DisplayRow {
    GroupHeader { group_id: u32 },
    Window { wid: u32, group_id: Option<u32> },
}

#[derive(Clone, Debug)]
enum MenuAction {
    CreateGroup,
    AddToGroup(u32),
    RemoveFromGroup,
    RenameGroup,
    DeleteGroup,
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

// ── App ──

struct App {
    items: Vec<Item>,
    groups: Vec<Group>,
    display_order: Vec<DisplaySlot>,
    display_rows: Vec<DisplayRow>,
    next_group_id: u32,
    context_menu: Option<ContextMenu>,
    active_wid: Option<u32>,
    hover_row: Option<usize>,
    drag: Option<DragState>,
    width: u16,
    height: u16,
    our_wid: u32,
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
            active_wid: None,
            hover_row: None,
            drag: None,
            width: WIN_W,
            height: WIN_H,
            our_wid,
        }
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
            }
        }
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

    fn rename_group(&mut self, gid: u32) {
        if let Some(group) = self.groups.iter_mut().find(|g| g.id == gid) {
            let current = GROUP_PRESETS.iter().position(|n| *n == group.name);
            let next = match current {
                Some(i) => (i + 1) % GROUP_PRESETS.len(),
                None => 0,
            };
            group.name = GROUP_PRESETS[next].to_string();
        }
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

    let mut live_wids = HashSet::new();
    let mut new_items = Vec::new();
    let mut color_idx = 0usize;

    for wid in wids {
        if wid == app.our_wid {
            continue;
        }
        match get_window_desktop(conn, wid, atoms) {
            Ok(Some(d)) if d != current_desktop => continue,
            _ => {}
        }

        live_wids.insert(wid);

        let title = get_window_title(conn, wid, atoms).unwrap_or_default();
        let (_instance, class) = get_wm_class(conn, wid).unwrap_or_default();
        let display = if title.len() > 30 {
            format!("{}...", &title[..27])
        } else if title.is_empty() {
            class.clone()
        } else {
            title
        };

        let accent = ACCENT_COLORS[color_idx % ACCENT_COLORS.len()];
        let accent_pixel = alloc_color(conn, colormap, accent)?;
        color_idx += 1;

        new_items.push(Item {
            wid,
            label: display,
            wm_class: class,
            accent_pixel,
        });
    }

    // Remove dead wids from groups
    for group in &mut app.groups {
        group.member_wids.retain(|w| live_wids.contains(w));
    }

    // Remove dead entries from display_order
    app.display_order.retain(|slot| match slot {
        DisplaySlot::Window(wid) => live_wids.contains(wid),
        DisplaySlot::Group(gid) => app.groups.iter().any(|g| g.id == *gid),
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

    // Update active window
    app.active_wid = get_active_window(conn, root, atoms).unwrap_or(None);

    app.items = new_items;
    app.build_display_rows();
    Ok(())
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
                        let (x, w) = if group_id.is_some() {
                            (ITEM_X + GROUP_INDENT, ITEM_W - GROUP_INDENT as u16)
                        } else {
                            (ITEM_X, ITEM_W)
                        };
                        self.draw_item(conn, pix, x, y, w, ITEM_H as u16, item, false, hovered, is_active)?;
                    }
                }
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
                        x: ITEM_X,
                        y: indicator_y,
                        width: ITEM_W,
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
                                let (x, w) = if group_id.is_some() {
                                    (ITEM_X + GROUP_INDENT, ITEM_W - GROUP_INDENT as u16)
                                } else {
                                    (ITEM_X, ITEM_W)
                                };
                                self.draw_item(
                                    conn, pix, x, ghost_y, w, ITEM_H as u16, item, true, false, false,
                                )?;
                            }
                        }
                    }
                }
            }
        }

        conn.copy_area(pix, self.window, self.gc, 0, 0, 0, 0, app.width, app.height)?;
        conn.flush()?;
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

        // Label
        conn.change_gc(self.gc, &ChangeGCAux::new().foreground(self.text_pixel))?;
        let text_x = x + 8;
        let text_y = y + (h as i16 / 2) + 4;
        let max_chars = ((w as i16 - 12) / CHAR_WIDTH).max(0) as usize;
        let display: String = item.label.chars().take(max_chars).collect();
        if !display.is_empty() {
            conn.image_text8(drawable, self.gc, text_x, text_y, display.as_bytes())?;
        }
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
                x: ITEM_X,
                y,
                width: ITEM_W,
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
                x: ITEM_X,
                y,
                width: ITEM_W,
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
                x: ITEM_X,
                y,
                width: 3,
                height: ITEM_H as u16,
            }],
        )?;

        // Arrow + name
        let arrow = if group.collapsed { "+" } else { "-" };
        let name_text = format!("{} {}", arrow, group.name);

        conn.change_gc(self.gc, &ChangeGCAux::new().foreground(self.text_pixel))?;
        let text_x = ITEM_X + 8;
        let text_y = y + (ITEM_H as i16 / 2) + 4;
        let max_chars = ((ITEM_W as i16 - 12) / CHAR_WIDTH).max(0) as usize;
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
        conn.change_gc(self.gc, &ChangeGCAux::new().foreground(self.ghost_pixel))?;
        conn.poly_fill_rectangle(
            drawable,
            self.gc,
            &[Rectangle {
                x: ITEM_X,
                y,
                width: ITEM_W,
                height: ITEM_H as u16,
            }],
        )?;
        if let Some(group) = app.groups.iter().find(|g| g.id == group_id) {
            let text = format!("{} (+{})", group.name, group.member_wids.len());
            conn.change_gc(self.gc, &ChangeGCAux::new().foreground(self.text_pixel))?;
            conn.image_text8(
                drawable,
                self.gc,
                ITEM_X + 8,
                y + (ITEM_H as i16 / 2) + 4,
                text.as_bytes(),
            )?;
        }
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
            vec![MenuEntry {
                label: "Remove from Group".to_string(),
                action: MenuAction::RemoveFromGroup,
            }]
        }
        DisplayRow::Window {
            group_id: None, ..
        } => {
            let mut entries = vec![MenuEntry {
                label: "New Group".to_string(),
                action: MenuAction::CreateGroup,
            }];
            for group in &app.groups {
                entries.push(MenuEntry {
                    label: format!("Add to {}", group.name),
                    action: MenuAction::AddToGroup(group.id),
                });
            }
            entries
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
                app.rename_group(*group_id);
            }
        }
        MenuAction::DeleteGroup => {
            if let DisplayRow::GroupHeader { group_id } = &app.display_rows[target_row] {
                app.delete_group(*group_id);
            }
        }
    }
}

// ── Main ──

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (conn, screen_num) = x11rb::connect(None)?;
    let screen = &conn.setup().roots[screen_num];
    let root = screen.root;
    let colormap = screen.default_colormap;

    let atoms = Atoms::new(&conn)?;

    let window: Window = conn.generate_id()?;
    let event_mask = EventMask::BUTTON_PRESS
        | EventMask::BUTTON_RELEASE
        | EventMask::POINTER_MOTION
        | EventMask::EXPOSURE
        | EventMask::STRUCTURE_NOTIFY
        | EventMask::LEAVE_WINDOW;

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
        b"poc-x11",
    )?;

    let mut app = App::new(window);
    let mut renderer = Renderer::new(&conn, screen, window)?;

    conn.map_window(window)?;
    conn.flush()?;

    refresh_items(&conn, root, &atoms, &mut app, colormap)?;

    let mut event_count: u32 = 0;

    loop {
        let event = conn.wait_for_event()?;
        event_count = event_count.wrapping_add(1);

        // Refresh periodically (skip during drag or menu)
        if event_count % 50 == 0 && app.drag.is_none() && app.context_menu.is_none() {
            refresh_items(&conn, root, &atoms, &mut app, colormap)?;
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

        // ── Normal mode ──
        match event {
            Event::Expose(ev) if ev.count == 0 && ev.window == window => {
                renderer.redraw(&conn, &app)?;
            }
            Event::ConfigureNotify(ev) if ev.window == window => {
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
                        if let Some(row) = app.hit_test_row(ev.event_y) {
                            app.drag = Some(DragState {
                                source_row: row,
                                start_y: ev.event_y,
                                current_y: ev.event_y,
                                started: false,
                            });
                        }
                    }
                    3 => {
                        if app.drag.is_none() {
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
                    if new_hover != app.hover_row {
                        app.hover_row = new_hover;
                        needs_redraw = true;
                    }
                    // Drain queued motion for hover too
                    while let Some(queued) = conn.poll_for_event()? {
                        if let Event::MotionNotify(mn) = queued {
                            let h = app.hit_test_row(mn.event_y);
                            if h != app.hover_row {
                                app.hover_row = h;
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
                if app.hover_row.is_some() {
                    app.hover_row = None;
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

    // ── Hit testing ──

    #[test]
    fn row_y_is_sequential() {
        let app = make_app();
        let y0 = app.row_y(0);
        let y1 = app.row_y(1);
        assert_eq!(y0, ITEM_Y_START);
        assert_eq!(y1, ITEM_Y_START + ITEM_H as i16 + ITEM_SPACING);
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
    fn rename_group_cycles_presets() {
        let mut app = make_app();
        add_item(&mut app, 1, "A");
        app.create_group(1);

        // Initial name is "Group 1", not in presets
        app.rename_group(0);
        assert_eq!(app.groups[0].name, "Alpha");
        app.rename_group(0);
        assert_eq!(app.groups[0].name, "Beta");
        app.rename_group(0);
        assert_eq!(app.groups[0].name, "Gamma");
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
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].label, "New Group");
    }

    #[test]
    fn menu_for_ungrouped_shows_existing_groups() {
        let mut app = make_app();
        add_item(&mut app, 1, "A");
        add_item(&mut app, 2, "B");
        app.create_group(1);
        // display_rows: [Header(0), Window(1,g0), Window(2,None)]

        let entries = build_menu_entries(&app, 2); // right-click on ungrouped window 2
        assert_eq!(entries.len(), 2); // "New Group" + "Add to Group 1"
        assert!(matches!(entries[1].action, MenuAction::AddToGroup(0)));
    }

    #[test]
    fn menu_for_grouped_window_has_remove() {
        let mut app = make_app();
        add_item(&mut app, 1, "A");
        app.create_group(1);
        // display_rows: [Header(0), Window(1,g0)]

        let entries = build_menu_entries(&app, 1);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].label, "Remove from Group");
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
}
