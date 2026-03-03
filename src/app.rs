use std::cell::RefCell;
use std::os::fd::AsRawFd;
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
use crate::state::AppState;
use crate::x11::connection::{self as x11conn, AtomCache};
use crate::x11::monitor;

const APP_ID: &str = "com.github.science.ptm";

pub fn run() -> glib::ExitCode {
    env_logger::init();

    let app = Application::builder().application_id(APP_ID).build();
    app.connect_activate(activate);
    app.run()
}

fn activate(app: &Application) {
    // Load CSS
    let css_provider = CssProvider::new();
    css_provider.load_from_data(include_str!("../style.css"));
    gtk::style_context_add_provider_for_display(
        &gdk4::Display::default().expect("Could not get default display"),
        &css_provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

    // Connect to X11
    let (conn, screen_num) = RustConnection::connect(None).expect("Failed to connect to X11");
    let screen = &conn.setup().roots[screen_num];
    let root = screen.root;

    let atoms = AtomCache::new(&conn).expect("Failed to intern atoms");

    // Subscribe to root window events
    monitor::subscribe_root_events(&conn, root).expect("Failed to subscribe to root events");

    // Set up shared state
    let config = Config::default();
    let filter = Filter::new(config.wm_classes().to_vec());
    let state = Rc::new(RefCell::new(AppState::new()));

    // Build sidebar
    let sidebar = Sidebar::new();

    // Connect double-click rename handler
    sidebar.connect_rename(Rc::clone(&state));

    // Initial population
    refresh_state(&conn, root, &atoms, &filter, &state, &sidebar);

    // Bridge atom IDs
    let bridge_atoms = AtomIds {
        net_client_list: atoms.net_client_list,
        net_active_window: atoms.net_active_window,
        net_wm_name: atoms.net_wm_name,
        net_current_desktop: atoms.net_current_desktop,
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

    // Connect click handler for focus + snap
    sidebar.connect_click(Rc::clone(&conn), Rc::clone(&atoms), root, window.clone());

    window.present();
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
                });
            }
            Err(e) => {
                log::warn!("Failed to get window info for 0x{:08x}: {}", wid, e);
            }
        }
    }

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
