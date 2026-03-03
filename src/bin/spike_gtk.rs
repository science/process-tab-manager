//! Spike B: GTK4 ListBox with dark CSS (no X11)
//!
//! Standalone GTK4 window with hardcoded rows + dark CSS.
//! Proves GTK4-rs compiles, CSS loads, ListBox renders.

use gtk4 as gtk;
use gtk::prelude::*;
use gtk::{Application, ApplicationWindow, CssProvider, Label, ListBox, ScrolledWindow};

const APP_ID: &str = "com.github.science.ptm.spike-gtk";

fn main() -> glib::ExitCode {
    env_logger::init();

    let app = Application::builder().application_id(APP_ID).build();
    app.connect_activate(build_ui);
    app.run()
}

fn build_ui(app: &Application) {
    // Load CSS
    let css_provider = CssProvider::new();
    css_provider.load_from_string(include_str!("../../style.css"));
    gtk::style_context_add_provider_for_display(
        &gdk4::Display::default().expect("Could not get default display"),
        &css_provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

    // Create ListBox with hardcoded window entries
    let listbox = ListBox::new();
    listbox.set_selection_mode(gtk::SelectionMode::Single);

    let sample_windows = [
        ("Terminal", "claude: dotfiles"),
        ("Terminal", "claude: web-api"),
        ("Terminal", "htop"),
        ("Firefox", "GitHub PR #42"),
        ("Firefox", "Stack Overflow - Rust GTK4"),
        ("GIMP", "photo-edit.xcf"),
    ];

    for (class, title) in &sample_windows {
        let label = Label::new(Some(&format!("{class}: {title}")));
        label.set_halign(gtk::Align::Start);
        label.set_margin_start(8);
        label.set_margin_end(8);
        listbox.append(&label);
    }

    // Highlight second row as "active"
    if let Some(row) = listbox.row_at_index(1) {
        listbox.select_row(Some(&row));
    }

    // Click handler
    listbox.connect_row_activated(|_listbox, row| {
        println!("Clicked row index: {}", row.index());
    });

    // ScrolledWindow for the list
    let scrolled = ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .child(&listbox)
        .build();

    // Header label
    let header = Label::new(Some("Process Tab Manager"));
    header.add_css_class("ptm-header");

    // Vertical box layout
    let vbox = gtk::Box::new(gtk::Orientation::Vertical, 0);
    vbox.append(&header);
    vbox.append(&scrolled);

    // Main window
    let window = ApplicationWindow::builder()
        .application(app)
        .title("PTM Spike — GTK4 ListBox")
        .default_width(250)
        .default_height(400)
        .child(&vbox)
        .build();

    window.present();
    println!("Spike B: GTK4 window presented. Close the window to exit.");
}
