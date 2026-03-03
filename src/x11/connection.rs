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
}

impl AtomCache {
    pub fn new(conn: &RustConnection) -> Result<Self> {
        // Pipeline all intern_atom requests before calling reply
        let c1 = conn.intern_atom(false, b"_NET_CLIENT_LIST")?;
        let c2 = conn.intern_atom(false, b"_NET_ACTIVE_WINDOW")?;
        let c3 = conn.intern_atom(false, b"_NET_WM_NAME")?;
        let c4 = conn.intern_atom(false, b"_NET_WM_DESKTOP")?;
        let c5 = conn.intern_atom(false, b"_NET_CURRENT_DESKTOP")?;
        let c6 = conn.intern_atom(false, b"UTF8_STRING")?;

        Ok(Self {
            net_client_list: c1.reply()?.atom,
            net_active_window: c2.reply()?.atom,
            net_wm_name: c3.reply()?.atom,
            net_wm_desktop: c4.reply()?.atom,
            net_current_desktop: c5.reply()?.atom,
            utf8_string: c6.reply()?.atom,
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

    Ok(WindowInfo {
        id: wid,
        wm_instance,
        wm_class,
        title,
        desktop,
    })
}

/// Get current desktop number.
pub fn get_current_desktop(conn: &RustConnection, root: u32, atoms: &AtomCache) -> Result<Option<u32>> {
    let reply = conn
        .get_property(false, root, atoms.net_current_desktop, AtomEnum::CARDINAL, 0, 1)?
        .reply()?;
    Ok(ewmh::parse_window_id(&reply.value))
}
