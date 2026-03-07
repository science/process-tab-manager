mod icon_resolver;
mod x11_monitor;

use std::path::PathBuf;
use std::sync::{Arc, Mutex, mpsc};

use serde::Serialize;
use tauri::Manager;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::ConnectionExt as _;
use x11rb::rust_connection::RustConnection;

use ptm_core::bridge::AtomIds;
use ptm_core::config::Config;
use ptm_core::filter::Filter;
use ptm_core::state::{AppState, ItemKind, SavedState, WindowEntry};
use ptm_core::x11::actions;
use ptm_core::x11::connection::{self as x11conn, AtomCache};
use ptm_core::x11::monitor;

// ─── Shared state types ─────────────────────────────────────────────

struct PtmState {
    app_state: Arc<Mutex<AppState>>,
    conn: Arc<RustConnection>,
    atoms: Arc<AtomCache>,
    filter: Filter,
    root: u32,
    save_tx: mpsc::Sender<()>,
}

// ─── Types serialized to frontend ───────────────────────────────────

#[derive(Clone, Serialize)]
#[serde(tag = "kind")]
pub enum SidebarItem {
    Window {
        wid: u32,
        title: String,
        wm_class: String,
        icon_path: Option<String>,
        is_active: bool,
        is_minimized: bool,
        is_urgent: bool,
        is_renamed: bool,
    },
    GroupHeader {
        gid: u32,
        name: String,
        collapsed: bool,
        member_count: usize,
    },
    GroupedWindow {
        wid: u32,
        gid: u32,
        title: String,
        wm_class: String,
        icon_path: Option<String>,
        is_active: bool,
        is_minimized: bool,
        is_urgent: bool,
        is_renamed: bool,
    },
}

// ─── Config paths ───────────────────────────────────────────────────

fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("process-tab-manager")
}

fn config_path() -> PathBuf {
    config_dir().join("config.json")
}

fn state_path() -> PathBuf {
    config_dir().join("state.json")
}

// ─── State → SidebarItem conversion ────────────────────────────────

fn build_sidebar_items(state: &AppState, filter: &Filter) -> Vec<SidebarItem> {
    let active = state.active_window();
    state
        .display_items(filter)
        .into_iter()
        .map(|item| match item {
            ItemKind::UngroupedWindow(wid) => {
                let title = state.display_name(wid).to_string();
                let wm_class = state
                    .windows()
                    .iter()
                    .find(|w| w.id == wid)
                    .map(|w| w.wm_class.clone())
                    .unwrap_or_default();
                let w = state.windows().iter().find(|w| w.id == wid);
                SidebarItem::Window {
                    wid,
                    title,
                    wm_class: wm_class.clone(),
                    icon_path: icon_resolver::resolve_icon_path(&wm_class),
                    is_active: active == Some(wid),
                    is_minimized: w.map_or(false, |w| w.is_minimized),
                    is_urgent: w.map_or(false, |w| w.is_urgent),
                    is_renamed: state.has_rename(wid),
                }
            }
            ItemKind::GroupHeader(gid) => {
                let group = state.group(gid);
                SidebarItem::GroupHeader {
                    gid,
                    name: group.map_or_else(|| "Group".to_string(), |g| g.name.clone()),
                    collapsed: group.map_or(false, |g| g.collapsed),
                    member_count: group.map_or(0, |g| g.members.len()),
                }
            }
            ItemKind::GroupedWindow(wid, gid) => {
                let title = state.display_name(wid).to_string();
                let wm_class = state
                    .windows()
                    .iter()
                    .find(|w| w.id == wid)
                    .map(|w| w.wm_class.clone())
                    .unwrap_or_default();
                let w = state.windows().iter().find(|w| w.id == wid);
                SidebarItem::GroupedWindow {
                    wid,
                    gid,
                    title,
                    wm_class: wm_class.clone(),
                    icon_path: icon_resolver::resolve_icon_path(&wm_class),
                    is_active: active == Some(wid),
                    is_minimized: w.map_or(false, |w| w.is_minimized),
                    is_urgent: w.map_or(false, |w| w.is_urgent),
                    is_renamed: state.has_rename(wid),
                }
            }
        })
        .collect()
}

// ─── Tauri commands ─────────────────────────────────────────────────

#[tauri::command]
fn get_sidebar_items(ptm: tauri::State<'_, PtmState>) -> Vec<SidebarItem> {
    let state = ptm.app_state.lock().unwrap();
    build_sidebar_items(&state, &ptm.filter)
}

#[tauri::command]
fn activate_window(ptm: tauri::State<'_, PtmState>, wid: u32) -> Result<(), String> {
    // Cross-workspace: switch desktop if needed
    {
        let state = ptm.app_state.lock().unwrap();
        if let Some(target_desktop) = state.window_desktop(wid) {
            if let Ok(Some(current)) = x11conn::get_current_desktop(&ptm.conn, ptm.root, &ptm.atoms) {
                if target_desktop != current {
                    let _ = actions::switch_desktop(&ptm.conn, ptm.root, target_desktop, &ptm.atoms);
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
            }
        }
    }

    actions::activate_window(&ptm.conn, ptm.root, wid, &ptm.atoms)
        .map_err(|e| e.to_string())?;

    // Snap window to sidebar position
    if let Some(ptm_wid) = find_ptm_wid(&ptm.conn, ptm.root, &ptm.atoms) {
        log::info!("snap: ptm_wid=0x{:08x}, target_wid=0x{:08x}", ptm_wid, wid);
        let current_desktop = x11conn::get_current_desktop(&ptm.conn, ptm.root, &ptm.atoms).ok().flatten().unwrap_or(0);
        if let (Ok(sidebar_rect), Ok(Some(wa))) = (
            get_ptm_rect(&ptm.conn, ptm_wid, ptm.root),
            x11conn::get_workarea(&ptm.conn, ptm.root, &ptm.atoms, current_desktop),
        ) {
            let workarea = ptm_core::geometry::Rect {
                x: wa.0,
                y: wa.1,
                width: wa.2,
                height: wa.3,
            };
            let ptm_frame_t = x11conn::get_frame_extents(&ptm.conn, ptm_wid, &ptm.atoms).unwrap_or((0, 0, 0, 0));
            let target_frame_t = x11conn::get_frame_extents(&ptm.conn, wid, &ptm.atoms).unwrap_or((0, 0, 0, 0));
            log::info!("snap: sidebar=({},{} {}x{}), ptm_frame={:?}, target_frame={:?}, workarea=({},{} {}x{})",
                sidebar_rect.x, sidebar_rect.y, sidebar_rect.width, sidebar_rect.height,
                ptm_frame_t, target_frame_t,
                workarea.x, workarea.y, workarea.width, workarea.height);
            let ptm_frame = tuple_to_frame(ptm_frame_t);
            let target_frame = tuple_to_frame(target_frame_t);
            let pos = ptm_core::geometry::snap_position_with_frames(
                &sidebar_rect,
                &ptm_frame,
                &target_frame,
                &workarea,
            );
            log::info!("snap: moving target to ({}, {})", pos.x, pos.y);
            let _ = actions::move_window(&ptm.conn, wid, pos.x, pos.y);
        }
    } else {
        log::warn!("snap: could not find PTM's own X11 window");
    }

    Ok(())
}

#[tauri::command]
fn close_window(ptm: tauri::State<'_, PtmState>, wid: u32) -> Result<(), String> {
    actions::close_window(&ptm.conn, ptm.root, wid, &ptm.atoms)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn rename_window(ptm: tauri::State<'_, PtmState>, wid: u32, name: String) -> Result<(), String> {
    let mut state = ptm.app_state.lock().unwrap();
    state.rename_window(wid, &name);
    let _ = ptm.save_tx.send(());
    Ok(())
}

#[tauri::command]
fn clear_rename(ptm: tauri::State<'_, PtmState>, wid: u32) -> Result<(), String> {
    let mut state = ptm.app_state.lock().unwrap();
    state.clear_rename(wid);
    let _ = ptm.save_tx.send(());
    Ok(())
}

#[tauri::command]
fn hide_window(ptm: tauri::State<'_, PtmState>, wid: u32) -> Result<(), String> {
    let mut state = ptm.app_state.lock().unwrap();
    state.hide_window(wid);
    let _ = ptm.save_tx.send(());
    Ok(())
}

#[tauri::command]
fn reorder(ptm: tauri::State<'_, PtmState>, from: usize, to: usize) -> Result<(), String> {
    let mut state = ptm.app_state.lock().unwrap();
    state.reorder(from, to);
    let _ = ptm.save_tx.send(());
    Ok(())
}

#[tauri::command]
fn create_group(ptm: tauri::State<'_, PtmState>, name: String, wid: Option<u32>) -> u32 {
    let mut state = ptm.app_state.lock().unwrap();
    let gid = if let Some(wid) = wid {
        state.create_group_with_window(&name, wid)
    } else {
        state.create_group(&name)
    };
    let _ = ptm.save_tx.send(());
    gid
}

#[tauri::command]
fn delete_group(ptm: tauri::State<'_, PtmState>, gid: u32) {
    let mut state = ptm.app_state.lock().unwrap();
    state.delete_group(gid);
    let _ = ptm.save_tx.send(());
}

#[tauri::command]
fn rename_group(ptm: tauri::State<'_, PtmState>, gid: u32, name: String) {
    let mut state = ptm.app_state.lock().unwrap();
    state.rename_group(gid, &name);
    let _ = ptm.save_tx.send(());
}

#[tauri::command]
fn toggle_group(ptm: tauri::State<'_, PtmState>, gid: u32) {
    let mut state = ptm.app_state.lock().unwrap();
    state.toggle_group_collapsed(gid);
    let _ = ptm.save_tx.send(());
}

#[tauri::command]
fn add_to_group(ptm: tauri::State<'_, PtmState>, wid: u32, gid: u32) {
    let mut state = ptm.app_state.lock().unwrap();
    state.add_to_group(wid, gid);
    let _ = ptm.save_tx.send(());
}

#[tauri::command]
fn remove_from_group(ptm: tauri::State<'_, PtmState>, wid: u32) {
    let mut state = ptm.app_state.lock().unwrap();
    state.remove_from_group(wid);
    let _ = ptm.save_tx.send(());
}

#[tauri::command]
fn reorder_in_group(ptm: tauri::State<'_, PtmState>, gid: u32, from: usize, to: usize) {
    let mut state = ptm.app_state.lock().unwrap();
    state.reorder_in_group(gid, from, to);
    let _ = ptm.save_tx.send(());
}

#[derive(Clone, Serialize)]
struct WindowGeometry {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

fn get_window_geometry_inner(
    conn: &RustConnection,
    wid: u32,
    root: u32,
) -> Result<WindowGeometry, String> {
    let (x, y) = x11conn::get_window_position(conn, wid, root)
        .map_err(|e| e.to_string())?;
    let geo = conn
        .get_geometry(wid)
        .map_err(|e| e.to_string())?
        .reply()
        .map_err(|e| e.to_string())?;
    Ok(WindowGeometry {
        x,
        y,
        width: geo.width as u32,
        height: geo.height as u32,
    })
}

#[tauri::command]
fn get_window_geometry(ptm: tauri::State<'_, PtmState>, wid: u32) -> Result<WindowGeometry, String> {
    get_window_geometry_inner(&ptm.conn, wid, ptm.root)
}

#[tauri::command]
fn get_ptm_window_geometry(ptm: tauri::State<'_, PtmState>) -> Result<WindowGeometry, String> {
    let ptm_wid = find_ptm_wid(&ptm.conn, ptm.root, &ptm.atoms)
        .ok_or_else(|| "PTM window not found".to_string())?;
    get_window_geometry_inner(&ptm.conn, ptm_wid, ptm.root)
}

/// E2E test helper: write event log line to /tmp/ptm-events.log
#[tauri::command]
fn log_event(line: String) {
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("/tmp/ptm-events.log")
    {
        let _ = writeln!(f, "{}", line);
    }
}

/// E2E test helper: write test state snapshot
#[tauri::command]
fn write_test_state(json: String) {
    use std::io::Write;
    if let Ok(mut f) = std::fs::File::create("/tmp/ptm-test-state.json") {
        let _ = f.write_all(json.as_bytes());
    }
}

// ─── X11 helpers ────────────────────────────────────────────────────

fn find_ptm_wid(conn: &RustConnection, root: u32, atoms: &AtomCache) -> Option<u32> {
    let own_pid = std::process::id();
    x11conn::find_window_by_pid(conn, root, atoms, own_pid).ok().flatten()
}

fn get_ptm_rect(
    conn: &RustConnection,
    ptm_wid: u32,
    root: u32,
) -> anyhow::Result<ptm_core::geometry::Rect> {
    let (x, y) = x11conn::get_window_position(conn, ptm_wid, root)?;
    let geo = conn.get_geometry(ptm_wid)?.reply()?;
    Ok(ptm_core::geometry::Rect {
        x,
        y,
        width: geo.width as u32,
        height: geo.height as u32,
    })
}

fn tuple_to_frame(t: (u32, u32, u32, u32)) -> ptm_core::geometry::FrameExtents {
    ptm_core::geometry::FrameExtents {
        left: t.0,
        right: t.1,
        top: t.2,
        bottom: t.3,
    }
}

// ─── Refresh state from X11 ────────────────────────────────────────

pub fn refresh_state(
    conn: &RustConnection,
    root: u32,
    atoms: &AtomCache,
    _filter: &Filter,
    state: &Arc<Mutex<AppState>>,
) {
    let client_list = match x11conn::get_client_list(conn, root, atoms) {
        Ok(ids) => ids,
        Err(e) => {
            log::error!("Failed to get client list: {}", e);
            return;
        }
    };

    let mut entries = Vec::new();
    for wid in &client_list {
        match x11conn::get_window_info(conn, *wid, atoms) {
            Ok(info) => {
                let _ = monitor::subscribe_window_events(conn, *wid);
                entries.push(WindowEntry {
                    id: info.id,
                    wm_class: info.wm_class,
                    wm_instance: info.wm_instance,
                    title: info.title,
                    desktop: info.desktop,
                    pid: info.pid,
                    is_minimized: info.is_minimized,
                    is_urgent: info.is_urgent,
                });
            }
            Err(e) => {
                log::warn!("Failed to get window info for 0x{:08x}: {}", wid, e);
            }
        }
    }

    // Filter out PTM's own window
    let own_pid = std::process::id();
    entries.retain(|e| e.pid != Some(own_pid));

    let mut s = state.lock().unwrap();
    s.update_windows(entries);

    if let Ok(Some(active)) = x11conn::get_active_window(conn, root, atoms) {
        s.set_active(Some(active));
    }
}

// ─── Debounced save thread ─────────────────────────────────────────

fn start_save_thread(
    state: Arc<Mutex<AppState>>,
    rx: mpsc::Receiver<()>,
) {
    std::thread::spawn(move || {
        loop {
            // Wait for a save request
            if rx.recv().is_err() {
                break; // Channel closed
            }

            // Drain any additional queued requests
            while rx.try_recv().is_ok() {}

            // Debounce: wait 2 seconds
            std::thread::sleep(std::time::Duration::from_secs(2));

            // Drain again (more may have arrived during sleep)
            while rx.try_recv().is_ok() {}

            let s = state.lock().unwrap();
            let saved = s.to_saved();
            if let Err(e) = saved.save_to_file(&state_path()) {
                log::error!("Failed to save state: {}", e);
            } else {
                log::debug!("State saved");
            }
        }
    });
}

// ─── App entry point ────────────────────────────────────────────────

pub fn run() {
    env_logger::init();

    // Connect to X11
    let (conn, screen_num) = RustConnection::connect(None).expect("Failed to connect to X11");
    let screen = &conn.setup().roots[screen_num];
    let root = screen.root;

    let atoms = AtomCache::new(&conn).expect("Failed to intern atoms");
    monitor::subscribe_root_events(&conn, root).expect("Failed to subscribe to root events");

    // Load config
    let config = if let Some(user_config) = Config::load_from_file(&config_path()) {
        Config::default().merge(&user_config)
    } else {
        Config::default()
    };
    let filter = Filter::new(config.wm_classes().to_vec());

    let app_state = Arc::new(Mutex::new(AppState::new()));

    // Initial window population (must happen before restore so renames can map)
    let conn = Arc::new(conn);
    let atoms = Arc::new(atoms);
    refresh_state(&conn, root, &atoms, &filter, &app_state);

    // Restore saved state (renames, groups, display order)
    if let Some(saved) = SavedState::load_from_file(&state_path()) {
        app_state.lock().unwrap().restore_from(&saved);
    }

    // Save debounce channel
    let (save_tx, save_rx) = mpsc::channel();
    start_save_thread(Arc::clone(&app_state), save_rx);

    let ptm = PtmState {
        app_state: Arc::clone(&app_state),
        conn: Arc::clone(&conn),
        atoms: Arc::clone(&atoms),
        filter: filter.clone(),
        root,
        save_tx: save_tx.clone(),
    };

    // Bridge atom IDs for event translation
    let bridge_atoms = AtomIds {
        net_client_list: atoms.net_client_list,
        net_active_window: atoms.net_active_window,
        net_wm_name: atoms.net_wm_name,
        net_current_desktop: atoms.net_current_desktop,
        net_wm_state: atoms.net_wm_state,
    };

    tauri::Builder::default()
        .manage(ptm)
        .invoke_handler(tauri::generate_handler![
            get_sidebar_items,
            activate_window,
            close_window,
            rename_window,
            clear_rename,
            hide_window,
            reorder,
            create_group,
            delete_group,
            rename_group,
            toggle_group,
            add_to_group,
            remove_from_group,
            reorder_in_group,
            get_window_geometry,
            get_ptm_window_geometry,
            log_event,
            write_test_state,
        ])
        .setup(move |app| {
            let handle = app.handle().clone();

            // Set window type to DOCK so clicks pass through without focus-stealing.
            // Dock windows don't eat the first click and stay visible across desktops.
            // NOTE: Rapid create/destroy of DOCK windows triggers a Muffin bug that
            // corrupts _NET_CLIENT_LIST. Skip during E2E tests (PTM_NO_DOCK=1).
            if std::env::var("PTM_NO_DOCK").is_err() {
                let conn2 = Arc::clone(&conn);
                let atoms2 = Arc::clone(&atoms);
                let root2 = root;
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(500));
                    if let Some(ptm_wid) = find_ptm_wid(&conn2, root2, &atoms2) {
                        log::info!("Setting PTM window 0x{:08x} to DOCK type", ptm_wid);
                        let _ = actions::set_window_type_dock(&conn2, ptm_wid, &atoms2);
                    } else {
                        log::warn!("Could not find PTM window to set DOCK type");
                    }
                });
            } else {
                log::info!("PTM_NO_DOCK set, skipping DOCK window type");
            }

            // Start X11 monitor thread
            x11_monitor::start(
                Arc::clone(&conn),
                Arc::clone(&atoms),
                bridge_atoms,
                root,
                Arc::clone(&app_state),
                filter,
                save_tx,
                handle,
            );

            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { .. } = event {
                // Save state on close
                let ptm: tauri::State<'_, PtmState> = window.state();
                let s = ptm.app_state.lock().unwrap();
                let saved = s.to_saved();
                if let Err(e) = saved.save_to_file(&state_path()) {
                    log::error!("Failed to save state on close: {}", e);
                } else {
                    log::debug!("State saved on close");
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
