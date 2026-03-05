use gtk4 as gtk;
use gtk::prelude::*;
use gtk::{Application, ApplicationWindow, CssProvider, Label, ListBox, ListBoxRow, ScrolledWindow};

const APP_ID: &str = "com.github.science.css-probe";

fn main() -> glib::ExitCode {
    let app = Application::builder().application_id(APP_ID).build();
    app.connect_activate(activate);
    app.run()
}

fn activate(app: &Application) {
    // Load CSS from file (edit without recompile)
    let css_path = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .map(|d| d.join("../../style.css"))
        .unwrap_or_else(|| "style.css".into());

    // Also check for style.css next to the binary
    let css_path = if css_path.exists() {
        css_path
    } else {
        // Fallback: look relative to CWD
        std::path::PathBuf::from("style.css")
    };

    eprintln!("[css-probe] Loading CSS from: {}", css_path.display());

    let provider = CssProvider::new();

    // Hook parse errors BEFORE loading
    provider.connect_parsing_error(|_provider, section, error| {
        let loc = section.start_location();
        eprintln!(
            "[CSS ERROR] line {}:{} — {}",
            loc.lines() + 1,
            loc.chars(),
            error
        );
    });

    if css_path.exists() {
        let css_text = std::fs::read_to_string(&css_path).expect("Failed to read CSS file");
        eprintln!("[css-probe] CSS content ({} bytes):\n{}", css_text.len(), css_text);
        provider.load_from_data(&css_text);
    } else {
        eprintln!("[css-probe] WARNING: CSS file not found at {}", css_path.display());
        // Load minimal test CSS
        provider.load_from_data("window { background-color: red; }");
    }

    // Try multiple priorities
    let priority = std::env::var("CSS_PRIORITY")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
    eprintln!("[css-probe] Using CSS priority: {}", priority);

    gtk::style_context_add_provider_for_display(
        &gdk4::Display::default().expect("No display"),
        &provider,
        priority,
    );

    // Optionally prefer dark theme
    if std::env::var("PREFER_DARK").is_ok() {
        let settings = gtk::Settings::for_display(&gdk4::Display::default().unwrap());
        settings.set_gtk_application_prefer_dark_theme(true);
        eprintln!("[css-probe] Set prefer-dark-theme = true");
    }

    // Build UI matching PTM structure
    let listbox = ListBox::new();
    listbox.set_selection_mode(gtk::SelectionMode::Single);

    for (i, title) in ["Firefox: GitHub", "XTerm: bash", "Nemo: Documents"].iter().enumerate() {
        let label = Label::new(Some(title));
        label.set_halign(gtk::Align::Start);
        label.set_hexpand(true);

        let hbox = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        hbox.append(&label);

        let row = ListBoxRow::new();
        row.set_child(Some(&hbox));

        if i == 0 {
            row.add_css_class("ptm-active");
        }
        if i == 2 {
            row.add_css_class("ptm-minimized");
        }

        listbox.append(&row);
    }

    let scrolled = ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .child(&listbox)
        .vexpand(true)
        .build();

    let vbox = gtk::Box::new(gtk::Orientation::Vertical, 0);
    vbox.append(&scrolled);

    let window = ApplicationWindow::builder()
        .application(app)
        .title("CSS Probe")
        .default_width(250)
        .default_height(400)
        .child(&vbox)
        .build();

    window.present();
}
