use gtk4 as gtk;
use gtk::prelude::*;
use gtk::{Label, ListBox, ListBoxRow};

use crate::filter::Filter;
use crate::state::AppState;

/// Sidebar manages the GTK ListBox displaying tracked windows.
#[derive(Clone)]
pub struct Sidebar {
    listbox: ListBox,
}

impl Sidebar {
    pub fn new() -> Self {
        let listbox = ListBox::new();
        listbox.set_selection_mode(gtk::SelectionMode::Single);
        Self { listbox }
    }

    pub fn widget(&self) -> &ListBox {
        &self.listbox
    }

    /// Fully rebuild the ListBox from current state + filter.
    pub fn rebuild(&self, state: &AppState, filter: &Filter) {
        // Remove all existing rows
        while let Some(child) = self.listbox.first_child() {
            self.listbox.remove(&child);
        }

        let active = state.active_window();

        for entry in state.filtered_windows(filter) {
            let display_text = if entry.title.is_empty() {
                format!("{} (0x{:08x})", entry.wm_class, entry.id)
            } else {
                format!("{}: {}", entry.wm_class, entry.title)
            };

            let label = Label::new(Some(&display_text));
            label.set_halign(gtk::Align::Start);
            label.set_margin_start(8);
            label.set_margin_end(8);
            label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
            label.set_tooltip_text(Some(&entry.title));

            let row = ListBoxRow::new();
            row.set_child(Some(&label));

            // Store window ID as widget name for lookup
            row.set_widget_name(&format!("wid-{}", entry.id));

            if Some(entry.id) == active {
                row.add_css_class("ptm-active");
            }

            self.listbox.append(&row);
        }
    }

    /// Highlight the active window row.
    pub fn set_active(&self, wid: u32) {
        let target_name = format!("wid-{}", wid);
        let mut idx = 0;
        while let Some(row) = self.listbox.row_at_index(idx) {
            if row.widget_name() == target_name {
                row.add_css_class("ptm-active");
                self.listbox.select_row(Some(&row));
            } else {
                row.remove_css_class("ptm-active");
            }
            idx += 1;
        }
    }

    /// Update a single window's title in the list (avoids full rebuild).
    pub fn update_title(&self, wid: u32, title: &str) {
        let target_name = format!("wid-{}", wid);
        let mut idx = 0;
        while let Some(row) = self.listbox.row_at_index(idx) {
            if row.widget_name() == target_name {
                if let Some(label) = row.child().and_then(|c| c.downcast::<Label>().ok()) {
                    // Keep the class prefix in the display text
                    let current = label.text();
                    if let Some(colon_pos) = current.find(": ") {
                        let class_prefix = &current[..colon_pos];
                        label.set_text(&format!("{}: {}", class_prefix, title));
                    } else {
                        label.set_text(title);
                    }
                    label.set_tooltip_text(Some(title));
                }
                return;
            }
            idx += 1;
        }
    }
}
