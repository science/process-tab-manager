use std::sync::{Arc, Mutex, mpsc};

use tauri::{AppHandle, Emitter};
use x11rb::connection::Connection;
use x11rb::rust_connection::RustConnection;

use ptm_core::bridge::{self, AtomIds, PtmEvent};
use ptm_core::filter::Filter;
use ptm_core::state::AppState;
use ptm_core::x11::connection::{self as x11conn, AtomCache};

use crate::{build_sidebar_items, refresh_state};

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
        // Safety timer: track last known window count
        let mut last_count = 0usize;
        let mut last_check = std::time::Instant::now();

        loop {
            // Use poll with timeout for safety timer behavior
            match conn.poll_for_event() {
                Ok(Some(event)) => {
                    if let Some(ev) = bridge::translate_event(&event, &bridge_atoms, root) {
                        log::debug!("PtmEvent: {:?}", ev);
                        handle_event(&ev, &conn, root, &atoms, &filter, &state, &save_tx, &app);
                    }

                    // Drain remaining queued events
                    while let Ok(Some(event)) = conn.poll_for_event() {
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
