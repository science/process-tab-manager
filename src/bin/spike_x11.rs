//! Spike A: x11rb window discovery (no GTK)
//!
//! Connects to X11, reads _NET_CLIENT_LIST, prints window class + title for each.
//! Proves x11rb works, atom interning works, property parsing works.

use x11rb::connection::Connection;
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt};
use x11rb::rust_connection::RustConnection;

fn main() -> anyhow::Result<()> {
    env_logger::init();

    let (conn, screen_num) = RustConnection::connect(None)?;
    let screen = &conn.setup().roots[screen_num];
    let root = screen.root;

    println!("Connected to X11 display, screen {screen_num}");
    println!("Root window: 0x{root:08x}");
    println!();

    // Intern atoms we need
    let net_client_list = conn.intern_atom(false, b"_NET_CLIENT_LIST")?.reply()?.atom;
    let net_wm_name = conn.intern_atom(false, b"_NET_WM_NAME")?.reply()?.atom;
    let utf8_string = conn.intern_atom(false, b"UTF8_STRING")?.reply()?.atom;

    // Read _NET_CLIENT_LIST from root window
    let reply = conn
        .get_property(false, root, net_client_list, AtomEnum::WINDOW, 0, 1024)?
        .reply()?;

    let window_ids: Vec<u32> = reply
        .value32()
        .map(|iter| iter.collect())
        .unwrap_or_default();

    println!("Found {} windows:", window_ids.len());
    println!("{:-<70}", "");

    for wid in &window_ids {
        // Get WM_CLASS (ICCCM — null-separated instance\0class\0)
        let class_reply = conn
            .get_property(false, *wid, AtomEnum::WM_CLASS.into(), AtomEnum::STRING, 0, 256)?
            .reply()?;
        let class_str = parse_wm_class(&class_reply.value);

        // Get _NET_WM_NAME (EWMH — UTF-8 title)
        let title_reply = conn
            .get_property(false, *wid, net_wm_name, utf8_string, 0, 1024)?
            .reply()?;
        let title = String::from_utf8_lossy(&title_reply.value);

        // Fallback to WM_NAME if _NET_WM_NAME is empty
        let title = if title.is_empty() {
            let wm_name_reply = conn
                .get_property(false, *wid, AtomEnum::WM_NAME.into(), AtomEnum::STRING, 0, 1024)?
                .reply()?;
            String::from_utf8_lossy(&wm_name_reply.value).into_owned()
        } else {
            title.into_owned()
        };

        println!("  0x{wid:08x}  {class_str:<30} {title}");
    }

    println!();
    println!("Spike A complete: x11rb connection, atom interning, and property parsing all work.");

    Ok(())
}

/// Parse WM_CLASS bytes: "instance\0class\0" → "class (instance)"
fn parse_wm_class(data: &[u8]) -> String {
    // WM_CLASS is null-separated: instance\0class\0
    let parts: Vec<&str> = std::str::from_utf8(data)
        .unwrap_or("")
        .split('\0')
        .filter(|s| !s.is_empty())
        .collect();

    match parts.len() {
        0 => "(unknown)".to_string(),
        1 => parts[0].to_string(),
        _ => format!("{} ({})", parts[1], parts[0]),
    }
}
