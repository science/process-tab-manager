use std::cell::RefCell;
use std::os::fd::AsRawFd;
use std::path::PathBuf;
use std::rc::Rc;

use glib::IOCondition;
use gtk4 as gtk;
use gtk::prelude::*;
use gtk::{Application, ApplicationWindow, CssProvider, ScrolledWindow};
use x11rb::connection::Connection;
use x11rb::rust_connection::RustConnection;

use crate::bridge::{self, AtomIds, PtmEvent};
use crate::config::Config;
use crate::filter::Filter;
use crate::sidebar::Sidebar;
use crate::state::{AppState, SavedState};
use crate::x11::actions;
use crate::x11::connection::{self as x11conn, AtomCache};
use crate::x11::monitor;

const APP_ID: &str = "com.github.science.ptm";

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

/// Schedule a debounced save of the current state.
fn schedule_save(state: &Rc<RefCell<AppState>>, save_pending: &Rc<RefCell<bool>>) {
    if *save_pending.borrow() {
        return; // Already scheduled
    }
    *save_pending.borrow_mut() = true;

    let state = Rc::clone(state);
    let save_pending = Rc::clone(save_pending);
    glib::timeout_add_local_once(std::time::Duration::from_secs(2), move || {
        *save_pending.borrow_mut() = false;
        let s = state.borrow();
        let saved = s.to_saved();
        if let Err(e) = saved.save_to_file(&state_path()) {
            log::error!("Failed to save state: {}", e);
        } else {
            log::debug!("State saved");
        }
    });
}

pub fn run() -> glib::ExitCode {
    env_logger::init();

    let app = Application::builder().application_id(APP_ID).build();
    app.connect_activate(activate);
    app.run()
}

fn activate(app: &Application) {
    // Enable dark theme before loading CSS — Adwaita's light theme overrides
    // custom background colors at PRIORITY_APPLICATION regardless of selectors.
    let display = gdk4::Display::default().expect("Could not get default display");
    let settings = gtk::Settings::for_display(&display);
    settings.set_gtk_application_prefer_dark_theme(true);

    // Load CSS at PRIORITY_USER (800) to override Adwaita-dark defaults
    let css_provider = CssProvider::new();
    css_provider.connect_parsing_error(|_provider, section, error| {
        let loc = section.start_location();
        log::warn!(
            "CSS parse error at line {}:{} — {}",
            loc.lines() + 1,
            loc.chars(),
            error
        );
    });
    css_provider.load_from_data(include_str!("../style.css"));
    gtk::style_context_add_provider_for_display(
        &display,
        &css_provider,
        gtk::STYLE_PROVIDER_PRIORITY_USER,
    );

    // Connect to X11
    let (conn, screen_num) = RustConnection::connect(None).expect("Failed to connect to X11");
    let screen = &conn.setup().roots[screen_num];
    let root = screen.root;

    let atoms = AtomCache::new(&conn).expect("Failed to intern atoms");

    // Subscribe to root window events
    monitor::subscribe_root_events(&conn, root).expect("Failed to subscribe to root events");

    // Load config (file overrides merged with defaults)
    let config = if let Some(user_config) = Config::load_from_file(&config_path()) {
        Config::default().merge(&user_config)
    } else {
        Config::default()
    };
    let filter = Filter::new(config.wm_classes().to_vec());
    let state = Rc::new(RefCell::new(AppState::new()));
    let save_pending = Rc::new(RefCell::new(false));

    // Build sidebar
    let sidebar = Sidebar::new();

    // Connect double-click rename handler
    sidebar.connect_rename(Rc::clone(&state));

    // Connect keyboard handler (F2, Delete, Alt+Up/Down)
    sidebar.connect_keyboard(Rc::clone(&state));

    // Store state+filter for keyboard/DnD reorder
    sidebar.set_reorder_state(Rc::clone(&state), filter.clone());

    // Connect drag-and-drop reorder
    sidebar.connect_dnd();

    // Set up save-on-change callback
    let state_for_save = Rc::clone(&state);
    let save_pending_for_cb = Rc::clone(&save_pending);
    sidebar.set_on_change(move || {
        schedule_save(&state_for_save, &save_pending_for_cb);
    });

    // Initial population
    refresh_state(&conn, root, &atoms, &filter, &state, &sidebar);

    // Restore saved state (renames and ordering)
    let saved_position: Rc<RefCell<Option<(i32, i32)>>> = Rc::new(RefCell::new(None));
    if let Some(saved) = SavedState::load_from_file(&state_path()) {
        if let (Some(x), Some(y)) = (saved.window_x, saved.window_y) {
            *saved_position.borrow_mut() = Some((x, y));
        }
        state.borrow_mut().restore_from(&saved);
        sidebar.rebuild(&state.borrow(), &filter);
    }

    // Bridge atom IDs
    let bridge_atoms = AtomIds {
        net_client_list: atoms.net_client_list,
        net_active_window: atoms.net_active_window,
        net_wm_name: atoms.net_wm_name,
        net_current_desktop: atoms.net_current_desktop,
        net_wm_state: atoms.net_wm_state,
    };

    // Register x11rb FD with GLib main loop
    let fd = conn.stream().as_raw_fd();
    let conn = Rc::new(conn);
    let atoms = Rc::new(atoms);

    let conn_for_fd = Rc::clone(&conn);
    let atoms_for_fd = Rc::clone(&atoms);
    let state_for_fd = Rc::clone(&state);
    let sidebar_for_fd = sidebar.clone();
    let filter_for_fd = filter.clone();

    glib::source::unix_fd_add_local(fd, IOCondition::IN, move |_fd, _cond| {
        while let Ok(Some(event)) = conn_for_fd.poll_for_event() {
            let ptm_event = bridge::translate_event(&event, &bridge_atoms, root);
            if let Some(ev) = ptm_event {
                log::debug!("PtmEvent: {:?}", ev);
                match ev {
                    PtmEvent::WindowListChanged | PtmEvent::WindowDestroyed(_) => {
                        refresh_state(
                            &conn_for_fd,
                            root,
                            &atoms_for_fd,
                            &filter_for_fd,
                            &state_for_fd,
                            &sidebar_for_fd,
                        );
                    }
                    PtmEvent::ActiveWindowChanged => {
                        if let Ok(Some(active)) =
                            x11conn::get_active_window(&conn_for_fd, root, &atoms_for_fd)
                        {
                            state_for_fd.borrow_mut().set_active(Some(active));
                            sidebar_for_fd.set_active(active);
                        }
                    }
                    PtmEvent::WindowTitleChanged(wid) => {
                        if let Ok(info) =
                            x11conn::get_window_info(&conn_for_fd, wid, &atoms_for_fd)
                        {
                            state_for_fd.borrow_mut().update_title(wid, &info.title);
                            sidebar_for_fd.update_title(wid, &info.title);
                        }
                    }
                    PtmEvent::WindowStateChanged(wid) => {
                        if let Ok(info) =
                            x11conn::get_window_info(&conn_for_fd, wid, &atoms_for_fd)
                        {
                            state_for_fd
                                .borrow_mut()
                                .update_state(wid, info.is_minimized, info.is_urgent);
                            sidebar_for_fd.update_state(wid, info.is_minimized, info.is_urgent);
                        }
                    }
                    PtmEvent::DesktopChanged => {
                        // For now, just log it. Cross-workspace behavior is Phase 1.5.
                        log::debug!("Desktop changed");
                    }
                }
            }
        }
        glib::ControlFlow::Continue
    });

    // Safety timer: catch any missed events (1 second)
    let conn_for_timer = Rc::clone(&conn);
    let atoms_for_timer = Rc::clone(&atoms);
    let state_for_timer = Rc::clone(&state);
    let sidebar_for_timer = sidebar.clone();
    let filter_for_timer = filter.clone();
    let last_count = Rc::new(RefCell::new(0usize));

    glib::timeout_add_seconds_local(1, move || {
        if let Ok(ids) = x11conn::get_client_list(&conn_for_timer, root, &atoms_for_timer) {
            let current = ids.len();
            let mut prev = last_count.borrow_mut();
            if current != *prev {
                log::debug!("Safety timer: window count {} → {}", *prev, current);
                refresh_state(
                    &conn_for_timer,
                    root,
                    &atoms_for_timer,
                    &filter_for_timer,
                    &state_for_timer,
                    &sidebar_for_timer,
                );
                *prev = current;
            }
        }
        glib::ControlFlow::Continue
    });

    // Scrolled window
    let scrolled = ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .child(sidebar.widget())
        .vexpand(true)
        .build();

    let vbox = gtk::Box::new(gtk::Orientation::Vertical, 0);
    vbox.append(&scrolled);

    let window = ApplicationWindow::builder()
        .application(app)
        .title("Process Tab Manager")
        .default_width(250)
        .default_height(600)
        .child(&vbox)
        .build();

    // Prevent PTM from stealing focus when the user clicks a row —
    // click events still reach child widgets, but the window won't grab_focus()
    window.set_focus_on_click(false);

    // Shared cell for PTM's own X11 window ID (discovered after window is mapped)
    let ptm_wid: Rc<RefCell<Option<u32>>> = Rc::new(RefCell::new(None));

    // Connect click handler for focus + snap (with cross-workspace support)
    sidebar.connect_click(Rc::clone(&conn), Rc::clone(&atoms), root, window.clone(), Rc::clone(&state), Rc::clone(&ptm_wid));

    // Connect focus pass-through: when PTM gains focus from a background click,
    // activate the row under the cursor (the WM eats the click on raise)
    sidebar.connect_focus_passthrough(
        Rc::clone(&conn), Rc::clone(&atoms), root, &window, Rc::clone(&state), window.clone(), Rc::clone(&ptm_wid),
    );

    // Connect right-click context menu
    sidebar.connect_context_menu(Rc::clone(&conn), Rc::clone(&atoms), root, Rc::clone(&state));

    // Shared cell to store PTM position captured before quit (window still alive).
    // connect_shutdown fires after windows are destroyed, so we can't query X11 there.
    let last_position: Rc<RefCell<Option<(i32, i32)>>> = Rc::new(RefCell::new(None));

    // Handle SIGTERM gracefully — capture position while window still exists, then quit
    let app_for_signal = app.downgrade();
    let conn_for_signal = Rc::clone(&conn);
    let ptm_wid_for_signal = Rc::clone(&ptm_wid);
    let last_pos_for_signal = Rc::clone(&last_position);
    glib::unix_signal_add_local(15 /* SIGTERM */, move || {
        // Capture client area position before app.quit() destroys the window.
        if let Some(pw) = *ptm_wid_for_signal.borrow() {
            if let Ok((x, y)) = x11conn::get_window_position(&conn_for_signal, pw, root) {
                *last_pos_for_signal.borrow_mut() = Some((x, y));
                log::debug!("Captured PTM position ({}, {}) before shutdown", x, y);
            }
        }
        if let Some(app) = app_for_signal.upgrade() {
            app.quit();
        }
        glib::ControlFlow::Break
    });

    // Save state on shutdown (uses position captured by SIGTERM handler)
    let state_for_shutdown = Rc::clone(&state);
    let last_pos_for_shutdown = Rc::clone(&last_position);
    app.connect_shutdown(move |_| {
        let s = state_for_shutdown.borrow();
        let mut saved = s.to_saved();

        if let Some((x, y)) = *last_pos_for_shutdown.borrow() {
            saved.window_x = Some(x);
            saved.window_y = Some(y);
        }

        if let Err(e) = saved.save_to_file(&state_path()) {
            log::error!("Failed to save state on shutdown: {}", e);
        } else {
            log::debug!("State saved on shutdown");
        }
    });

    window.present();

    // Discover PTM's own X11 window by PID. Uses find_window_by_pid which searches
    // root's children via query_tree (PTM may not be in _NET_CLIENT_LIST if the WM
    // doesn't fully manage it). Retries every 500ms until found.
    let conn_for_type = Rc::clone(&conn);
    let atoms_for_type = Rc::clone(&atoms);
    let ptm_wid_for_type = Rc::clone(&ptm_wid);
    let conn_for_restore = Rc::clone(&conn);
    glib::timeout_add_local(std::time::Duration::from_millis(500), move || {
        if ptm_wid_for_type.borrow().is_some() {
            return glib::ControlFlow::Break;
        }

        let own_pid = std::process::id();
        match x11conn::find_window_by_pid(&conn_for_type, root, &atoms_for_type, own_pid) {
            Ok(Some(wid)) => {
                log::debug!("Discovered PTM window 0x{:08x} (pid={})", wid, own_pid);
                *ptm_wid_for_type.borrow_mut() = Some(wid);

                if let Err(e) = actions::set_window_type_utility(&conn_for_type, wid, &atoms_for_type) {
                    log::error!("Failed to set window type: {}", e);
                }

                // Restore position after a delay to let WM process the type change
                let saved_pos = *saved_position.borrow();
                let conn_r = Rc::clone(&conn_for_restore);
                if let Some((x, y)) = saved_pos {
                    glib::timeout_add_local_once(
                        std::time::Duration::from_millis(200),
                        move || {
                            log::debug!("Restoring PTM position to ({}, {})", x, y);
                            if let Err(e) = actions::move_window(&conn_r, wid, x, y) {
                                log::error!("Failed to restore position: {}", e);
                            }
                        },
                    );
                }

                return glib::ControlFlow::Break;
            }
            Ok(None) => {
                log::debug!("PTM WID discovery: not found yet (pid={})", own_pid);
            }
            Err(e) => {
                log::warn!("PTM WID discovery error: {}", e);
            }
        }
        glib::ControlFlow::Continue
    });
}

fn refresh_state(
    conn: &RustConnection,
    root: u32,
    atoms: &AtomCache,
    filter: &Filter,
    state: &Rc<RefCell<AppState>>,
    sidebar: &Sidebar,
) {
    let client_list = match x11conn::get_client_list(conn, root, atoms) {
        Ok(ids) => ids,
        Err(e) => {
            log::error!("Failed to get client list: {}", e);
            return;
        }
    };

    // Get info for each window, subscribe to its events
    let mut entries = Vec::new();
    for wid in &client_list {
        match x11conn::get_window_info(conn, *wid, atoms) {
            Ok(info) => {
                // Subscribe to title change events for this window
                let _ = monitor::subscribe_window_events(conn, *wid);
                entries.push(crate::state::WindowEntry {
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

    // Update state
    let mut s = state.borrow_mut();
    s.update_windows(entries);

    // Get active window
    if let Ok(Some(active)) = x11conn::get_active_window(conn, root, atoms) {
        s.set_active(Some(active));
    }

    // Rebuild sidebar from filtered state
    sidebar.rebuild(&s, filter);
}
