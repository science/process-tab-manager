use anyhow::Result;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{
    ClientMessageData, ClientMessageEvent, ConfigureWindowAux, ConnectionExt, EventMask,
};
use x11rb::rust_connection::RustConnection;

use super::connection::AtomCache;

/// Activate (focus + raise) a window. Uses source=2 (pager) to bypass focus-stealing prevention.
pub fn activate_window(conn: &RustConnection, root: u32, wid: u32, atoms: &AtomCache) -> Result<()> {
    let data = ClientMessageData::from([
        2u32, // source indication: 2 = pager/taskbar
        0,    // timestamp (0 = current)
        0,    // currently active window (0 = none)
        0, 0,
    ]);

    let event = ClientMessageEvent {
        response_type: 33, // ClientMessage
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

/// Move a window to a specific position. Does NOT resize — user's window size is preserved.
pub fn move_window(conn: &RustConnection, wid: u32, x: i32, y: i32) -> Result<()> {
    conn.configure_window(wid, &ConfigureWindowAux::new().x(x).y(y))?;
    conn.flush()?;
    Ok(())
}

/// Switch to a different virtual desktop.
pub fn switch_desktop(conn: &RustConnection, root: u32, desktop: u32, atoms: &AtomCache) -> Result<()> {
    let data = ClientMessageData::from([
        desktop,
        0u32, // timestamp
        0, 0, 0,
    ]);

    let event = ClientMessageEvent {
        response_type: 33,
        format: 32,
        sequence: 0,
        window: root,
        type_: atoms.net_current_desktop,
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
