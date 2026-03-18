use std::sync::{Arc, Mutex, mpsc};

use tauri::{AppHandle, Emitter};
use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::rust_connection::RustConnection;

use ptm_core::bridge::{self, AtomIds, PtmEvent};
use ptm_core::filter::Filter;
use ptm_core::state::AppState;
use ptm_core::x11::connection::{self as x11conn, AtomCache};

use crate::{build_sidebar_items, refresh_state};

#[derive(Clone, serde::Serialize)]
struct ClickEvent {
    x: f64,
    y: f64,
    button: u32,
    root_x: f64,
    root_y: f64,
    event_wid: u32,
    registered_wid: u32,
}

#[derive(Clone, serde::Serialize)]
struct DragEvent {
    x: f64,
    y: f64,
}

/// Find PTM's own window ID by matching PID against _NET_CLIENT_LIST.
/// Retries with delay since the Tauri window may not exist yet at startup.
fn find_ptm_window(conn: &RustConnection, root: u32, atoms: &AtomCache) -> Option<u32> {
    let own_pid = std::process::id();
    for _ in 0..20 {
        if let Ok(Some(wid)) = x11conn::find_window_by_pid(conn, root, atoms, own_pid) {
            return Some(wid);
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    log::warn!("Could not find PTM window after retries");
    None
}

/// Subscribe to XI2 ButtonPress, ButtonRelease, and Motion events on the given window.
/// XI2 allows multiple clients to listen simultaneously, so this doesn't
/// conflict with GTK's own event handling.
fn subscribe_xi2_events(conn: &RustConnection, wid: u32) -> Result<(), Box<dyn std::error::Error>> {
    use x11rb::protocol::xinput::{self, ConnectionExt as _};

    // Negotiate XI2 version
    let ver = conn.xinput_xi_query_version(2, 0)?.reply()?;
    log::info!("XI2 version {}.{}", ver.major_version, ver.minor_version);

    // Build event mask: ButtonPress + ButtonRelease + Motion on all master devices
    let event_mask = xinput::EventMask {
        deviceid: 1, // XIAllMasterDevices
        mask: vec![xinput::XIEventMask::BUTTON_PRESS
                 | xinput::XIEventMask::BUTTON_RELEASE
                 | xinput::XIEventMask::MOTION],
    };

    // Subscribe — .check() is mandatory to catch BadAccess errors
    conn.xinput_xi_select_events(wid, &[event_mask])?.check()?;

    log::info!("Subscribed to XI2 ButtonPress/ButtonRelease/Motion on PTM window 0x{:08x}", wid);
    Ok(())
}

/// Start the background X11 event monitor thread.
///
/// Blocks on X11 events and emits Tauri events to the frontend.
pub fn start(
    conn: Arc<RustConnection>,
    atoms: Arc<AtomCache>,
    bridge_atoms: AtomIds,
    root: u32,
    state: Arc<Mutex<AppState>>,
    filter: Filter,
    save_tx: mpsc::Sender<()>,
    app: AppHandle,
) {
    std::thread::spawn(move || {
        // Find and subscribe to XI2 events on PTM's own window
        let ptm_wid = find_ptm_window(&conn, root, &atoms);
        if let Some(wid) = ptm_wid {
            if let Err(e) = subscribe_xi2_events(&conn, wid) {
                log::error!("Failed to subscribe XI2 events on PTM window: {}", e);
            }
        }

        let registered_wid = ptm_wid.unwrap_or(0);

        // XI2 drag tracking: only emit motion events while button 1 is held
        let mut xi2_dragging = false;

        // Safety timer: track last known window count
        let mut last_count = 0usize;
        let mut last_check = std::time::Instant::now();

        loop {
            // Use poll with timeout for safety timer behavior
            match conn.poll_for_event() {
                Ok(Some(event)) => {
                    // Check for XI2 events on PTM window
                    match &event {
                        Event::XinputButtonPress(bp) => {
                            let click = ClickEvent {
                                x: (bp.event_x as f64 / 65536.0).round(),
                                y: (bp.event_y as f64 / 65536.0).round(),
                                button: bp.detail,
                                root_x: (bp.root_x as f64 / 65536.0).round(),
                                root_y: (bp.root_y as f64 / 65536.0).round(),
                                event_wid: bp.event.into(),
                                registered_wid,
                            };
                            log::debug!("XI2 ButtonPress: event=({},{}) root=({},{}) event_wid=0x{:08x} registered=0x{:08x} btn={}",
                                click.x, click.y, click.root_x, click.root_y,
                                click.event_wid, click.registered_wid, click.button);
                            if bp.detail == 1 {
                                xi2_dragging = true;
                            }
                            let _ = app.emit("x11-click", click);
                        }
                        Event::XinputButtonRelease(br) if br.detail == 1 => {
                            xi2_dragging = false;
                            let drag_end = DragEvent {
                                x: (br.event_x as f64 / 65536.0).round(),
                                y: (br.event_y as f64 / 65536.0).round(),
                            };
                            log::debug!("XI2 ButtonRelease: ({},{})", drag_end.x, drag_end.y);
                            let _ = app.emit("x11-drag-end", drag_end);
                        }
                        Event::XinputMotion(motion) if xi2_dragging => {
                            let drag_move = DragEvent {
                                x: (motion.event_x as f64 / 65536.0).round(),
                                y: (motion.event_y as f64 / 65536.0).round(),
                            };
                            let _ = app.emit("x11-drag-move", drag_move);
                        }
                        _ => {}
                    }

                    if let Some(ev) = bridge::translate_event(&event, &bridge_atoms, root) {
                        log::debug!("PtmEvent: {:?}", ev);
                        handle_event(&ev, &conn, root, &atoms, &filter, &state, &save_tx, &app);
                    }

                    // Drain remaining queued events
                    while let Ok(Some(event)) = conn.poll_for_event() {
                        match &event {
                            Event::XinputButtonPress(bp) => {
                                let click = ClickEvent {
                                    x: (bp.event_x as f64 / 65536.0).round(),
                                    y: (bp.event_y as f64 / 65536.0).round(),
                                    button: bp.detail,
                                    root_x: (bp.root_x as f64 / 65536.0).round(),
                                    root_y: (bp.root_y as f64 / 65536.0).round(),
                                    event_wid: bp.event.into(),
                                    registered_wid,
                                };
                                if bp.detail == 1 {
                                    xi2_dragging = true;
                                }
                                let _ = app.emit("x11-click", click);
                            }
                            Event::XinputButtonRelease(br) if br.detail == 1 => {
                                xi2_dragging = false;
                                let drag_end = DragEvent {
                                    x: (br.event_x as f64 / 65536.0).round(),
                                    y: (br.event_y as f64 / 65536.0).round(),
                                };
                                let _ = app.emit("x11-drag-end", drag_end);
                            }
                            Event::XinputMotion(motion) if xi2_dragging => {
                                let drag_move = DragEvent {
                                    x: (motion.event_x as f64 / 65536.0).round(),
                                    y: (motion.event_y as f64 / 65536.0).round(),
                                };
                                let _ = app.emit("x11-drag-move", drag_move);
                            }
                            _ => {}
                        }

                        if let Some(ev) = bridge::translate_event(&event, &bridge_atoms, root) {
                            handle_event(&ev, &conn, root, &atoms, &filter, &state, &save_tx, &app);
                        }
                    }

                    // After processing events, emit sidebar update
                    emit_sidebar_update(&state, &filter, &app);
                }
                Ok(None) => {
                    // No event available — do safety check if 1s elapsed
                    if last_check.elapsed() >= std::time::Duration::from_secs(1) {
                        last_check = std::time::Instant::now();
                        if let Ok(ids) = x11conn::get_client_list(&conn, root, &atoms) {
                            if ids.len() != last_count {
                                log::debug!("Safety timer: window count {} → {}", last_count, ids.len());
                                last_count = ids.len();
                                refresh_state(&conn, root, &atoms, &filter, &state);
                                emit_sidebar_update(&state, &filter, &app);
                            }
                        }
                    }

                    // Block briefly to avoid busy-spinning
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                Err(e) => {
                    log::error!("X11 connection error: {}", e);
                    break;
                }
            }
        }
    });
}

fn handle_event(
    ev: &PtmEvent,
    conn: &RustConnection,
    root: u32,
    atoms: &AtomCache,
    filter: &Filter,
    state: &Arc<Mutex<AppState>>,
    _save_tx: &mpsc::Sender<()>,
    _app: &AppHandle,
) {
    match ev {
        PtmEvent::WindowListChanged | PtmEvent::WindowDestroyed(_) => {
            refresh_state(conn, root, atoms, filter, state);
        }
        PtmEvent::ActiveWindowChanged => {
            if let Ok(Some(active)) = x11conn::get_active_window(conn, root, atoms) {
                state.lock().unwrap().set_active(Some(active));
            }
        }
        PtmEvent::WindowTitleChanged(wid) => {
            if let Ok(info) = x11conn::get_window_info(conn, *wid, atoms) {
                state.lock().unwrap().update_title(*wid, &info.title);
            }
        }
        PtmEvent::WindowStateChanged(wid) => {
            if let Ok(info) = x11conn::get_window_info(conn, *wid, atoms) {
                state
                    .lock()
                    .unwrap()
                    .update_state(*wid, info.is_minimized, info.is_urgent);
            }
        }
        PtmEvent::DesktopChanged => {
            log::debug!("Desktop changed");
        }
    }
}

fn emit_sidebar_update(state: &Arc<Mutex<AppState>>, filter: &Filter, app: &AppHandle) {
    let s = state.lock().unwrap();
    let items = build_sidebar_items(&s, filter);
    let _ = app.emit("sidebar-update", items);
}
