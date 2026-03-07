use anyhow::Result;
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt};
use x11rb::rust_connection::RustConnection;

use super::ewmh;

/// Cached atom IDs to avoid repeated intern_atom round-trips.
pub struct AtomCache {
    pub net_client_list: u32,
    pub net_active_window: u32,
    pub net_wm_name: u32,
    pub net_wm_desktop: u32,
    pub net_current_desktop: u32,
    pub net_wm_pid: u32,
    pub net_wm_state: u32,
    pub net_wm_state_hidden: u32,
    pub net_wm_state_demands_attention: u32,
    pub net_close_window: u32,
    pub net_wm_window_type: u32,
    pub net_wm_window_type_utility: u32,
    pub net_wm_window_type_dock: u32,
    pub net_workarea: u32,
    pub net_frame_extents: u32,
    pub utf8_string: u32,
}

/// Info about a single window discovered via EWMH.
#[derive(Debug, Clone)]
pub struct WindowInfo {
    pub id: u32,
    pub wm_instance: String,
    pub wm_class: String,
    pub title: String,
    pub desktop: Option<u32>,
    pub pid: Option<u32>,
    pub is_minimized: bool,
    pub is_urgent: bool,
}

impl AtomCache {
    pub fn new(conn: &RustConnection) -> Result<Self> {
        // Pipeline all intern_atom requests before calling reply
        let c1 = conn.intern_atom(false, b"_NET_CLIENT_LIST")?;
        let c2 = conn.intern_atom(false, b"_NET_ACTIVE_WINDOW")?;
        let c3 = conn.intern_atom(false, b"_NET_WM_NAME")?;
        let c4 = conn.intern_atom(false, b"_NET_WM_DESKTOP")?;
        let c5 = conn.intern_atom(false, b"_NET_CURRENT_DESKTOP")?;
        let c6 = conn.intern_atom(false, b"_NET_WM_PID")?;
        let c7 = conn.intern_atom(false, b"UTF8_STRING")?;
        let c8 = conn.intern_atom(false, b"_NET_WM_STATE")?;
        let c9 = conn.intern_atom(false, b"_NET_WM_STATE_HIDDEN")?;
        let c10 = conn.intern_atom(false, b"_NET_WM_STATE_DEMANDS_ATTENTION")?;
        let c11 = conn.intern_atom(false, b"_NET_CLOSE_WINDOW")?;
        let c12 = conn.intern_atom(false, b"_NET_WM_WINDOW_TYPE")?;
        let c13 = conn.intern_atom(false, b"_NET_WM_WINDOW_TYPE_UTILITY")?;
        let c14 = conn.intern_atom(false, b"_NET_WORKAREA")?;
        let c15 = conn.intern_atom(false, b"_NET_FRAME_EXTENTS")?;
        let c16 = conn.intern_atom(false, b"_NET_WM_WINDOW_TYPE_DOCK")?;

        Ok(Self {
            net_client_list: c1.reply()?.atom,
            net_active_window: c2.reply()?.atom,
            net_wm_name: c3.reply()?.atom,
            net_wm_desktop: c4.reply()?.atom,
            net_current_desktop: c5.reply()?.atom,
            net_wm_pid: c6.reply()?.atom,
            utf8_string: c7.reply()?.atom,
            net_wm_state: c8.reply()?.atom,
            net_wm_state_hidden: c9.reply()?.atom,
            net_wm_state_demands_attention: c10.reply()?.atom,
            net_close_window: c11.reply()?.atom,
            net_wm_window_type: c12.reply()?.atom,
            net_wm_window_type_utility: c13.reply()?.atom,
            net_wm_window_type_dock: c16.reply()?.atom,
            net_workarea: c14.reply()?.atom,
            net_frame_extents: c15.reply()?.atom,
        })
    }
}

/// Get the list of managed window IDs from the window manager.
pub fn get_client_list(conn: &RustConnection, root: u32, atoms: &AtomCache) -> Result<Vec<u32>> {
    let reply = conn
        .get_property(false, root, atoms.net_client_list, AtomEnum::WINDOW, 0, 4096)?
        .reply()?;
    Ok(ewmh::parse_window_ids(&reply.value))
}

/// Get the currently active window ID.
pub fn get_active_window(conn: &RustConnection, root: u32, atoms: &AtomCache) -> Result<Option<u32>> {
    let reply = conn
        .get_property(false, root, atoms.net_active_window, AtomEnum::WINDOW, 0, 1)?
        .reply()?;
    Ok(ewmh::parse_window_id(&reply.value))
}

/// Get full info about a single window. Pipelines all requests for efficiency.
pub fn get_window_info(conn: &RustConnection, wid: u32, atoms: &AtomCache) -> Result<WindowInfo> {
    // Pipeline: send all requests before reading any replies
    let class_cookie = conn.get_property(false, wid, AtomEnum::WM_CLASS, AtomEnum::STRING, 0, 256)?;
    let name_cookie = conn.get_property(false, wid, atoms.net_wm_name, atoms.utf8_string, 0, 1024)?;
    let desktop_cookie = conn.get_property(false, wid, atoms.net_wm_desktop, AtomEnum::CARDINAL, 0, 1)?;
    let pid_cookie = conn.get_property(false, wid, atoms.net_wm_pid, AtomEnum::CARDINAL, 0, 1)?;
    let state_cookie = conn.get_property(false, wid, atoms.net_wm_state, AtomEnum::ATOM, 0, 64)?;

    let class_reply = class_cookie.reply()?;
    let (wm_instance, wm_class) = ewmh::parse_wm_class(&class_reply.value);

    let name_reply = name_cookie.reply()?;
    let mut title = ewmh::parse_wm_name(&name_reply.value);

    // Fallback to WM_NAME if _NET_WM_NAME is empty
    if title.is_empty() {
        let fallback = conn
            .get_property(false, wid, AtomEnum::WM_NAME, AtomEnum::STRING, 0, 1024)?
            .reply()?;
        title = ewmh::parse_wm_name(&fallback.value);
    }

    let desktop_reply = desktop_cookie.reply()?;
    let desktop = ewmh::parse_window_id(&desktop_reply.value);

    let pid_reply = pid_cookie.reply()?;
    let pid = ewmh::parse_window_id(&pid_reply.value);

    let state_reply = state_cookie.reply()?;
    let wm_state = ewmh::parse_wm_state_flags(
        &state_reply.value,
        atoms.net_wm_state_hidden,
        atoms.net_wm_state_demands_attention,
    );

    Ok(WindowInfo {
        id: wid,
        wm_instance,
        wm_class,
        title,
        desktop,
        pid,
        is_minimized: wm_state.is_hidden,
        is_urgent: wm_state.demands_attention,
    })
}

/// Get a window's position (client area origin) relative to the root window.
pub fn get_window_position(conn: &RustConnection, wid: u32, root: u32) -> Result<(i32, i32)> {
    let reply = conn.translate_coordinates(wid, root, 0, 0)?.reply()?;
    Ok((reply.dst_x as i32, reply.dst_y as i32))
}

/// Get a window's frame extents (left, right, top, bottom) from _NET_FRAME_EXTENTS.
/// Returns default (0,0,0,0) if the property is not set.
pub fn get_frame_extents(conn: &RustConnection, wid: u32, atoms: &AtomCache) -> Result<(u32, u32, u32, u32)> {
    let reply = conn
        .get_property(false, wid, atoms.net_frame_extents, AtomEnum::CARDINAL, 0, 4)?
        .reply()?;
    let vals = ewmh::parse_window_ids(&reply.value);
    if vals.len() >= 4 {
        Ok((vals[0], vals[1], vals[2], vals[3]))
    } else {
        Ok((0, 0, 0, 0))
    }
}

/// Get the workarea for a given desktop from _NET_WORKAREA.
/// Returns (x, y, width, height) or None if not available.
pub fn get_workarea(conn: &RustConnection, root: u32, atoms: &AtomCache, desktop: u32) -> Result<Option<(i32, i32, u32, u32)>> {
    let reply = conn
        .get_property(false, root, atoms.net_workarea, AtomEnum::CARDINAL, 0, 256)?
        .reply()?;
    let vals = ewmh::parse_window_ids(&reply.value);
    let base = (desktop as usize) * 4;
    if vals.len() >= base + 4 {
        Ok(Some((vals[base] as i32, vals[base + 1] as i32, vals[base + 2], vals[base + 3])))
    } else {
        Ok(None)
    }
}

/// Find the visible application window by PID among direct children of root.
/// GTK4 creates multiple X11 windows under root for a single app (some 1x1 and unmapped).
/// We identify the real window by checking _NET_WM_PID + mapped + non-trivial size.
/// If no direct child matches, checks one level of WM reparenting (child-of-child).
pub fn find_window_by_pid(conn: &RustConnection, root: u32, atoms: &AtomCache, target_pid: u32) -> Result<Option<u32>> {
    let tree = conn.query_tree(root)?.reply()?;
    let mut best: Option<(u32, u64)> = None; // (wid, area)

    for wid in tree.children {
        // Check PID on direct child first
        if let Some((found, area)) = check_window_pid_and_area(conn, wid, atoms, target_pid)? {
            if best.map_or(true, |(_, best_area)| area > best_area) {
                best = Some((found, area));
            }
        }

        // Check if WM reparented the real window under a frame window.
        // Muffin wraps managed windows: root -> frame -> client.
        // The frame won't have _NET_WM_PID, but its child (the real window) will.
        if let Ok(subtree) = conn.query_tree(wid) {
            if let Ok(sub_reply) = subtree.reply() {
                for child in sub_reply.children {
                    if let Some((found, area)) = check_window_pid_and_area(conn, child, atoms, target_pid)? {
                        if best.map_or(true, |(_, best_area)| area > best_area) {
                            best = Some((found, area));
                        }
                    }
                }
            }
        }
    }
    Ok(best.map(|(wid, _)| wid))
}

/// Check if a single window has the target PID and non-trivial geometry.
/// Returns (wid, area) so the caller can pick the largest window.
fn check_window_pid_and_area(conn: &RustConnection, wid: u32, atoms: &AtomCache, target_pid: u32) -> Result<Option<(u32, u64)>> {
    let pid_reply = match conn.get_property(false, wid, atoms.net_wm_pid, AtomEnum::CARDINAL, 0, 1) {
        Ok(cookie) => match cookie.reply() {
            Ok(r) => r,
            Err(_) => return Ok(None),
        },
        Err(_) => return Ok(None),
    };

    let pid = match ewmh::parse_window_id(&pid_reply.value) {
        Some(p) => p,
        None => return Ok(None),
    };

    if pid != target_pid {
        return Ok(None);
    }

    let geo = match conn.get_geometry(wid) {
        Ok(cookie) => match cookie.reply() {
            Ok(g) => g,
            Err(_) => return Ok(None),
        },
        Err(_) => return Ok(None),
    };

    let area = geo.width as u64 * geo.height as u64;
    if geo.width > 1 && geo.height > 1 {
        log::debug!("find_window_by_pid: candidate 0x{:08x} ({}x{}, area={}, pid={})", wid, geo.width, geo.height, area, target_pid);
        Ok(Some((wid, area)))
    } else {
        log::debug!("find_window_by_pid: skipped 0x{:08x} ({}x{}, pid={})", wid, geo.width, geo.height, target_pid);
        Ok(None)
    }
}

/// Get current desktop number.
pub fn get_current_desktop(conn: &RustConnection, root: u32, atoms: &AtomCache) -> Result<Option<u32>> {
    let reply = conn
        .get_property(false, root, atoms.net_current_desktop, AtomEnum::CARDINAL, 0, 1)?
        .reply()?;
    Ok(ewmh::parse_window_id(&reply.value))
}
