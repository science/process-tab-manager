//! Spike C: Event loop integration (GTK4 + x11rb)
//!
//! GTK4 window + x11rb together. Registers x11rb's FD with glib::unix_fd_source_new.
//! When windows open/close on the desktop, the GTK ListBox updates in real time.
//! This is the architectural proof point.

use std::cell::RefCell;
use std::os::fd::AsRawFd;
use std::rc::Rc;

use glib::IOCondition;
use gtk4 as gtk;
use gtk::prelude::*;
use gtk::{Application, ApplicationWindow, CssProvider, Label, ListBox, ScrolledWindow};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt, EventMask};
use x11rb::protocol::Event;
use x11rb::rust_connection::RustConnection;

const APP_ID: &str = "com.github.science.ptm.spike-bridge";

struct X11State {
    conn: RustConnection,
    root: u32,
    net_client_list: u32,
    net_wm_name: u32,
    utf8_string: u32,
}

fn main() -> glib::ExitCode {
    env_logger::init();

    let app = Application::builder().application_id(APP_ID).build();
    app.connect_activate(activate);
    app.run()
}

fn activate(app: &Application) {
    // Load CSS
    let css_provider = CssProvider::new();
    css_provider.load_from_string(include_str!("../../style.css"));
    gtk::style_context_add_provider_for_display(
        &gdk4::Display::default().expect("Could not get default display"),
        &css_provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

    // Connect to X11
    let (conn, screen_num) = RustConnection::connect(None).expect("Failed to connect to X11");
    let screen = &conn.setup().roots[screen_num];
    let root = screen.root;

    // Intern atoms
    let net_client_list = conn
        .intern_atom(false, b"_NET_CLIENT_LIST")
        .unwrap()
        .reply()
        .unwrap()
        .atom;
    let net_wm_name = conn
        .intern_atom(false, b"_NET_WM_NAME")
        .unwrap()
        .reply()
        .unwrap()
        .atom;
    let utf8_string = conn
        .intern_atom(false, b"UTF8_STRING")
        .unwrap()
        .reply()
        .unwrap()
        .atom;

    // Subscribe to PropertyNotify on root window
    conn.change_window_attributes(
        root,
        &x11rb::protocol::xproto::ChangeWindowAttributesAux::new()
            .event_mask(EventMask::PROPERTY_CHANGE | EventMask::SUBSTRUCTURE_NOTIFY),
    )
    .unwrap();
    conn.flush().unwrap();

    let x11 = Rc::new(X11State {
        conn,
        root,
        net_client_list,
        net_wm_name,
        utf8_string,
    });

    // Create ListBox
    let listbox = ListBox::new();
    listbox.set_selection_mode(gtk::SelectionMode::Single);

    // Initial population
    refresh_window_list(&x11, &listbox);

    // Register x11rb FD with GLib main loop
    let fd = x11.conn.stream().as_raw_fd();
    let x11_for_source = Rc::clone(&x11);
    let listbox_for_source = listbox.clone();

    glib::source::unix_fd_add_local(fd, IOCondition::IN, move |_fd, _cond| {
        while let Ok(Some(event)) = x11_for_source.conn.poll_for_event() {
            match &event {
                Event::PropertyNotify(pn) if pn.atom == x11_for_source.net_client_list => {
                    println!("_NET_CLIENT_LIST changed — refreshing");
                    refresh_window_list(&x11_for_source, &listbox_for_source);
                }
                Event::PropertyNotify(pn) if pn.atom == x11_for_source.net_wm_name => {
                    println!(
                        "Window 0x{:08x} title changed — refreshing",
                        pn.window
                    );
                    refresh_window_list(&x11_for_source, &listbox_for_source);
                }
                Event::DestroyNotify(dn) => {
                    println!("Window 0x{:08x} destroyed — refreshing", dn.window);
                    refresh_window_list(&x11_for_source, &listbox_for_source);
                }
                _ => {}
            }
        }
        glib::ControlFlow::Continue
    });

    // Also add a 1-second safety timer to catch any missed events
    let x11_for_timer = Rc::clone(&x11);
    let listbox_for_timer = listbox.clone();
    let last_count = Rc::new(RefCell::new(0usize));
    glib::timeout_add_seconds_local(1, move || {
        let current = get_window_ids(&x11_for_timer).len();
        let mut prev = last_count.borrow_mut();
        if current != *prev {
            println!("Safety timer: window count changed {prev} → {current}");
            refresh_window_list(&x11_for_timer, &listbox_for_timer);
            *prev = current;
        }
        glib::ControlFlow::Continue
    });

    let scrolled = ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .child(&listbox)
        .build();

    let header = Label::new(Some("Live Window List"));
    header.add_css_class("ptm-header");

    let vbox = gtk::Box::new(gtk::Orientation::Vertical, 0);
    vbox.append(&header);
    vbox.append(&scrolled);

    let window = ApplicationWindow::builder()
        .application(app)
        .title("PTM Spike — Bridge (GTK4 + x11rb)")
        .default_width(300)
        .default_height(500)
        .child(&vbox)
        .build();

    window.present();
    println!("Spike C: Bridge window presented. Open/close windows to see live updates.");
}

fn get_window_ids(x11: &X11State) -> Vec<u32> {
    let reply = x11
        .conn
        .get_property(
            false,
            x11.root,
            x11.net_client_list,
            AtomEnum::WINDOW,
            0,
            1024,
        )
        .unwrap()
        .reply()
        .unwrap();

    reply.value32().map(|iter| iter.collect()).unwrap_or_default()
}

fn get_window_title(x11: &X11State, wid: u32) -> String {
    // Try _NET_WM_NAME first
    let reply = x11
        .conn
        .get_property(false, wid, x11.net_wm_name, x11.utf8_string, 0, 1024)
        .unwrap()
        .reply()
        .unwrap();
    let title = String::from_utf8_lossy(&reply.value).into_owned();

    if !title.is_empty() {
        return title;
    }

    // Fallback to WM_NAME
    let reply = x11
        .conn
        .get_property(false, wid, AtomEnum::WM_NAME.into(), AtomEnum::STRING, 0, 1024)
        .unwrap()
        .reply()
        .unwrap();
    String::from_utf8_lossy(&reply.value).into_owned()
}

fn get_wm_class(x11: &X11State, wid: u32) -> String {
    let reply = x11
        .conn
        .get_property(
            false,
            wid,
            AtomEnum::WM_CLASS.into(),
            AtomEnum::STRING,
            0,
            256,
        )
        .unwrap()
        .reply()
        .unwrap();

    let parts: Vec<&str> = std::str::from_utf8(&reply.value)
        .unwrap_or("")
        .split('\0')
        .filter(|s| !s.is_empty())
        .collect();

    match parts.len() {
        0 => "(unknown)".to_string(),
        1 => parts[0].to_string(),
        _ => parts[1].to_string(), // class name
    }
}

fn refresh_window_list(x11: &X11State, listbox: &ListBox) {
    // Remove all existing rows
    while let Some(child) = listbox.first_child() {
        listbox.remove(&child);
    }

    let window_ids = get_window_ids(x11);

    for wid in &window_ids {
        let class = get_wm_class(x11, *wid);
        let title = get_window_title(x11, *wid);

        let text = if title.is_empty() {
            format!("{class}  (0x{wid:08x})");
            class.clone()
        } else {
            format!("{class}: {title}")
        };

        let label = Label::new(Some(&text));
        label.set_halign(gtk::Align::Start);
        label.set_margin_start(8);
        label.set_margin_end(8);
        label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        listbox.append(&label);
    }

    println!("  Showing {} windows", window_ids.len());
}
