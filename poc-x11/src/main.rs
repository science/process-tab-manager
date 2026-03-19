use std::collections::{HashSet, VecDeque};
use std::time::SystemTime;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::*;
use x11rb::protocol::Event;
use x11rb::wrapper::ConnectionExt as _;
use x11rb::COPY_DEPTH_FROM_PARENT;

// Layout constants
const WIN_W: u16 = 250;
const WIN_H: u16 = 600;
const ITEM_X: i16 = 10;
const ITEM_W: u16 = 230;
const ITEM_H: u16 = 30;
const ITEM_SPACING: i16 = 4;
const ITEM_Y_START: i16 = 10;
const SEPARATOR_Y_OFFSET: i16 = 10;
const LOG_LINE_H: i16 = 16;
const DRAG_THRESHOLD: i16 = 5;
const MAX_LOG: usize = 100;
const GROUP_INDENT: i16 = 16;
const MENU_ITEM_H: u16 = 22;
const MENU_PADDING: i16 = 4;
const MENU_MIN_W: u16 = 180;

// Colors (RGB)
const BG_COLOR: u32 = 0x1c1b22;
const SEPARATOR_COLOR: u32 = 0x444444;
const TEXT_COLOR: u32 = 0xd4d4d4;
const LOG_TEXT_COLOR: u32 = 0x999999;
const INDICATOR_COLOR: u32 = 0xffffff;
const GHOST_COLOR: u32 = 0x666666;
const ITEM_COLOR: u32 = 0x2d2d3d;
const ITEM_ACTIVE_COLOR: u32 = 0x3d3d5c;
const MENU_BG_COLOR: u32 = 0x2d2d3d;
const MENU_BORDER_COLOR: u32 = 0x555555;
const MENU_HOVER_COLOR: u32 = 0x3d3d5c;
const GROUP_HEADER_COLOR: u32 = 0x252535;

// Cycle of accent colors for left-edge stripe
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
    log: VecDeque<String>,
    log_scroll: i16,
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
            log: VecDeque::new(),
            log_scroll: 0,
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

    fn separator_y(&self) -> i16 {
        ITEM_Y_START
            + (self.display_rows.len() as i16) * (ITEM_H as i16 + ITEM_SPACING)
            + SEPARATOR_Y_OFFSET
    }

    fn log_area_top(&self) -> i16 {
        self.separator_y() + 10
    }

    fn max_visible_log_lines(&self) -> i16 {
        ((self.height as i16 - self.log_area_top()) / LOG_LINE_H).max(0)
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

    #[allow(dead_code)]
    fn row_group_id(&self, idx: usize) -> Option<u32> {
        match &self.display_rows[idx] {
            DisplayRow::GroupHeader { group_id } => Some(*group_id),
            DisplayRow::Window { group_id, .. } => *group_id,
        }
    }

    fn add_log(&mut self, msg: String) {
        self.log.push_back(msg);
        if self.log.len() > MAX_LOG {
            self.log.pop_front();
        }
        let total = self.log.len() as i16;
        let visible = self.max_visible_log_lines();
        if total > visible {
            self.log_scroll = total - visible;
        }
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
        // Replace Window(wid) slot with Group(gid) in display_order
        for slot in &mut self.display_order {
            if matches!(slot, DisplaySlot::Window(w) if *w == wid) {
                *slot = DisplaySlot::Group(gid);
                break;
            }
        }
        self.build_display_rows();
    }

    fn add_to_group(&mut self, gid: u32, wid: u32) {
        // Remove from display_order (if ungrouped)
        self.display_order
            .retain(|s| !matches!(s, DisplaySlot::Window(w) if *w == wid));
        // Remove from any other group
        for group in &mut self.groups {
            group.member_wids.retain(|w| *w != wid);
        }
        // Add to target group
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
        // Find last member row after header
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
                    row_count += 1; // header
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

        // Check if cursor is directly on a group header
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
            DisplayRow::Window { wid, group_id: src_gid } => {
                if let Some(target_gid) = on_header_gid {
                    if src_gid == Some(target_gid) {
                        return; // already in this group
                    }
                    // Remove from current location
                    if let Some(gid) = src_gid {
                        self.remove_wid_from_group(gid, wid);
                    } else {
                        self.display_order
                            .retain(|s| !matches!(s, DisplaySlot::Window(w) if *w == wid));
                    }
                    // Add to target group
                    if let Some(g) = self.groups.iter_mut().find(|g| g.id == target_gid) {
                        g.member_wids.push(wid);
                    }
                } else if let Some(src_gid) = src_gid {
                    if self.is_gap_in_group(drop_gap, src_gid) {
                        self.reorder_within_group(src_gid, wid, drop_gap);
                    } else {
                        // Ungroup: remove from group, insert at top level
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
        .get_property(false, root, atoms.net_workarea, AtomEnum::CARDINAL, 0, 1024)?
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

    app.items = new_items;
    app.build_display_rows();
    Ok(())
}

// ── Helpers ──

fn timestamp() -> String {
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap();
    let total_secs = now.as_secs();
    let millis = now.subsec_millis();
    let secs = total_secs % 60;
    let mins = (total_secs / 60) % 60;
    let hours = (total_secs / 3600) % 24;
    format!("{:02}:{:02}:{:02}.{:03}", hours, mins, secs, millis)
}

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
    sep_pixel: u32,
    text_pixel: u32,
    log_text_pixel: u32,
    indicator_pixel: u32,
    ghost_pixel: u32,
    item_pixel: u32,
    #[allow(dead_code)]
    item_active_pixel: u32,
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
        let sep_pixel = alloc_color(conn, colormap, SEPARATOR_COLOR)?;
        let text_pixel = alloc_color(conn, colormap, TEXT_COLOR)?;
        let log_text_pixel = alloc_color(conn, colormap, LOG_TEXT_COLOR)?;
        let indicator_pixel = alloc_color(conn, colormap, INDICATOR_COLOR)?;
        let ghost_pixel = alloc_color(conn, colormap, GHOST_COLOR)?;
        let item_pixel = alloc_color(conn, colormap, ITEM_COLOR)?;
        let item_active_pixel = alloc_color(conn, colormap, ITEM_ACTIVE_COLOR)?;
        let menu_bg_pixel = alloc_color(conn, colormap, MENU_BG_COLOR)?;
        let menu_border_pixel = alloc_color(conn, colormap, MENU_BORDER_COLOR)?;
        let menu_hover_pixel = alloc_color(conn, colormap, MENU_HOVER_COLOR)?;
        let group_header_pixel = alloc_color(conn, colormap, GROUP_HEADER_COLOR)?;

        let mut group_color_pixels = Vec::new();
        for &c in GROUP_COLORS {
            group_color_pixels.push(alloc_color(conn, colormap, c)?);
        }

        let font = conn.generate_id()?;
        conn.open_font(font, b"fixed")?;

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
            sep_pixel,
            text_pixel,
            log_text_pixel,
            indicator_pixel,
            ghost_pixel,
            item_pixel,
            item_active_pixel,
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
            match row {
                DisplayRow::GroupHeader { group_id } => {
                    self.draw_group_header(conn, pix, app, *group_id, y)?;
                }
                DisplayRow::Window { wid, group_id } => {
                    if let Some(item) = app.find_item(*wid) {
                        let (x, w) = if group_id.is_some() {
                            (ITEM_X + GROUP_INDENT, ITEM_W - GROUP_INDENT as u16)
                        } else {
                            (ITEM_X, ITEM_W)
                        };
                        self.draw_item(conn, pix, x, y, w, ITEM_H as u16, item, false)?;
                    }
                }
            }
        }

        // Draw separator
        conn.change_gc(self.gc, &ChangeGCAux::new().foreground(self.sep_pixel))?;
        conn.poly_fill_rectangle(
            pix,
            self.gc,
            &[Rectangle {
                x: ITEM_X,
                y: app.separator_y(),
                width: ITEM_W,
                height: 1,
            }],
        )?;

        // Draw log
        let log_top = app.log_area_top();
        let max_lines = app.max_visible_log_lines();
        conn.change_gc(
            self.gc,
            &ChangeGCAux::new().foreground(self.log_text_pixel),
        )?;

        let start = app.log_scroll.max(0) as usize;
        for (i, line_idx) in (start..app.log.len()).enumerate() {
            if i as i16 >= max_lines {
                break;
            }
            let text_y = log_top + (i as i16) * LOG_LINE_H + 12;
            if let Some(text) = app.log.get(line_idx) {
                conn.image_text8(pix, self.gc, ITEM_X, text_y, text.as_bytes())?;
            }
        }

        // Draw drag visuals
        if let Some(drag) = &app.drag {
            if drag.started {
                // Drop indicator line
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
                                    conn,
                                    pix,
                                    x,
                                    ghost_y,
                                    w,
                                    ITEM_H as u16,
                                    item,
                                    true,
                                )?;
                            }
                        }
                    }
                }
            }
        }

        // Copy to window
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
    ) -> Result<(), Box<dyn std::error::Error>> {
        let bg = if ghost {
            self.ghost_pixel
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

        // Left accent stripe
        conn.change_gc(
            self.gc,
            &ChangeGCAux::new().foreground(item.accent_pixel),
        )?;
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
        let max_chars = ((w as i16 - 12) / 6).max(0) as usize;
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
    ) -> Result<(), Box<dyn std::error::Error>> {
        let group = match app.groups.iter().find(|g| g.id == group_id) {
            Some(g) => g,
            None => return Ok(()),
        };

        // Background
        conn.change_gc(
            self.gc,
            &ChangeGCAux::new().foreground(self.group_header_pixel),
        )?;
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

        // Left accent stripe (color based on group id)
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

        // Text: arrow + name + count if collapsed
        let arrow = if group.collapsed { ">" } else { "v" };
        let text = if group.collapsed {
            format!("{} {} ({})", arrow, group.name, group.member_wids.len())
        } else {
            format!("{} {}", arrow, group.name)
        };
        conn.change_gc(self.gc, &ChangeGCAux::new().foreground(self.text_pixel))?;
        let text_x = ITEM_X + 8;
        let text_y = y + (ITEM_H as i16 / 2) + 4;
        let max_chars = ((ITEM_W as i16 - 12) / 6).max(0) as usize;
        let display: String = text.chars().take(max_chars).collect();
        conn.image_text8(drawable, self.gc, text_x, text_y, display.as_bytes())?;
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
    // Close existing menu
    if app.context_menu.is_some() {
        close_context_menu(conn, app)?;
    }

    let entries = build_menu_entries(app, target_row);
    if entries.is_empty() {
        return Ok(());
    }

    let height = (entries.len() as u16) * MENU_ITEM_H + (MENU_PADDING as u16 * 2);
    let width = MENU_MIN_W;

    // Clamp to screen edges
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

    let grab_reply = conn
        .grab_pointer(
            false,
            win,
            EventMask::BUTTON_PRESS | EventMask::BUTTON_RELEASE | EventMask::POINTER_MOTION,
            GrabMode::ASYNC,
            GrabMode::ASYNC,
            0u32, // confine_to: none
            0u32, // cursor: none
            0u32, // time: current
        )?
        .reply()?;
    if grab_reply.status != GrabStatus::SUCCESS {
        app.add_log(format!("grab failed: {:?}", grab_reply.status));
    }

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
                let wid = *wid;
                let label = app
                    .find_item(wid)
                    .map(|i| i.label.clone())
                    .unwrap_or_default();
                app.create_group(wid);
                app.add_log(format!("new group <- {} {}", label, timestamp()));
            }
        }
        MenuAction::AddToGroup(gid) => {
            if let DisplayRow::Window { wid, .. } = &app.display_rows[target_row] {
                let wid = *wid;
                app.add_to_group(gid, wid);
                app.add_log(format!("add to group {}", timestamp()));
            }
        }
        MenuAction::RemoveFromGroup => {
            if let DisplayRow::Window { wid, .. } = &app.display_rows[target_row] {
                let wid = *wid;
                app.remove_from_group(wid);
                app.add_log(format!("ungrouped {}", timestamp()));
            }
        }
        MenuAction::RenameGroup => {
            if let DisplayRow::GroupHeader { group_id } = &app.display_rows[target_row] {
                let gid = *group_id;
                app.rename_group(gid);
                let name = app
                    .groups
                    .iter()
                    .find(|g| g.id == gid)
                    .map(|g| g.name.clone())
                    .unwrap_or_default();
                app.add_log(format!("renamed -> {} {}", name, timestamp()));
            }
        }
        MenuAction::DeleteGroup => {
            if let DisplayRow::GroupHeader { group_id } = &app.display_rows[target_row] {
                let gid = *group_id;
                app.delete_group(gid);
                app.add_log(format!("deleted group {}", timestamp()));
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
        | EventMask::STRUCTURE_NOTIFY;

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
    app.add_log(format!(
        "Loaded {} windows {}",
        app.items.len(),
        timestamp()
    ));

    let mut event_count: u32 = 0;

    loop {
        let event = conn.wait_for_event()?;
        event_count = event_count.wrapping_add(1);

        // Refresh periodically (skip during drag or menu)
        if event_count % 50 == 0 && app.drag.is_none() && app.context_menu.is_none() {
            let old_count = app.items.len();
            refresh_items(&conn, root, &atoms, &mut app, colormap)?;
            if app.items.len() != old_count {
                app.add_log(format!(
                    "Refresh: {} windows {}",
                    app.items.len(),
                    timestamp()
                ));
            }
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
                let y = ev.event_y;
                match ev.detail {
                    4 => {
                        if y >= app.log_area_top() {
                            app.log_scroll = (app.log_scroll - 3).max(0);
                        }
                    }
                    5 => {
                        if y >= app.log_area_top() {
                            let max_scroll =
                                (app.log.len() as i16 - app.max_visible_log_lines()).max(0);
                            app.log_scroll = (app.log_scroll + 3).min(max_scroll);
                        }
                    }
                    1 => {
                        if let Some(row) = app.hit_test_row(y) {
                            app.drag = Some(DragState {
                                source_row: row,
                                start_y: y,
                                current_y: y,
                                started: false,
                            });
                        }
                    }
                    3 => {
                        // Right-click: open context menu
                        if app.drag.is_none() {
                            if let Some(row) = app.hit_test_row(y) {
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
                        let row = drag.source_row;
                        if row < app.display_rows.len() {
                            let label = match &app.display_rows[row] {
                                DisplayRow::GroupHeader { group_id } => app
                                    .groups
                                    .iter()
                                    .find(|g| g.id == *group_id)
                                    .map(|g| g.name.clone())
                                    .unwrap_or_default(),
                                DisplayRow::Window { wid, .. } => app
                                    .find_item(*wid)
                                    .map(|i| i.label.clone())
                                    .unwrap_or_default(),
                            };
                            app.add_log(format!("drag {} {}", label, timestamp()));
                        }
                    }
                    needs_redraw = true;
                }
                // Drain queued motion events
                while let Some(queued) = conn.poll_for_event()? {
                    if let Event::MotionNotify(mn) = queued {
                        if let Some(ref mut drag) = app.drag {
                            drag.current_y = mn.event_y;
                            needs_redraw = true;
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
                if needs_redraw {
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
            // Drag-reorder (group-aware)
            app.handle_drop(drag.source_row, drag.current_y);
            app.add_log(format!("drop {}", timestamp()));
        } else {
            // Click (no drag)
            if drag.source_row < app.display_rows.len() {
                let row = app.display_rows[drag.source_row].clone();
                match row {
                    DisplayRow::GroupHeader { group_id } => {
                        app.toggle_collapse(group_id);
                        let collapsed = app
                            .groups
                            .iter()
                            .find(|g| g.id == group_id)
                            .map(|g| g.collapsed)
                            .unwrap_or(false);
                        app.add_log(format!(
                            "{} {}",
                            if collapsed { "collapsed" } else { "expanded" },
                            timestamp()
                        ));
                    }
                    DisplayRow::Window { wid, .. } => {
                        let label = app
                            .find_item(wid)
                            .map(|i| i.label.clone())
                            .unwrap_or_default();
                        if let Err(e) = activate_window(conn, root, wid, atoms) {
                            app.add_log(format!("activate err: {} {}", e, timestamp()));
                        } else {
                            app.add_log(format!("activate {} {}", label, timestamp()));
                            std::thread::sleep(std::time::Duration::from_millis(50));
                            if let Err(e) =
                                snap_to_sidebar(conn, root, app.our_wid, wid, atoms)
                            {
                                app.add_log(format!("snap err: {} {}", e, timestamp()));
                            } else {
                                app.add_log(format!("snapped {}", timestamp()));
                            }
                        }
                    }
                }
            }
        }
    }
}
