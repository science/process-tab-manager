use anyhow::Result;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{ConnectionExt, EventMask};
use x11rb::rust_connection::RustConnection;

/// Subscribe to events on the root window so we get notified about
/// window list changes, active window changes, and desktop switches.
pub fn subscribe_root_events(conn: &RustConnection, root: u32) -> Result<()> {
    conn.change_window_attributes(
        root,
        &x11rb::protocol::xproto::ChangeWindowAttributesAux::new()
            .event_mask(EventMask::PROPERTY_CHANGE | EventMask::SUBSTRUCTURE_NOTIFY),
    )?;
    conn.flush()?;
    Ok(())
}

/// Subscribe to PropertyNotify on a specific window (for title changes).
pub fn subscribe_window_events(conn: &RustConnection, wid: u32) -> Result<()> {
    conn.change_window_attributes(
        wid,
        &x11rb::protocol::xproto::ChangeWindowAttributesAux::new()
            .event_mask(EventMask::PROPERTY_CHANGE | EventMask::STRUCTURE_NOTIFY),
    )?;
    Ok(())
}
