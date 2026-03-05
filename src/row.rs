use std::cell::RefCell;
use std::rc::Rc;

use gtk4 as gtk;
use gtk::prelude::*;
use gtk::{Entry, Label, ListBoxRow};

use crate::state::AppState;

pub type OnChangeCb = Rc<RefCell<Option<Box<dyn Fn()>>>>;

/// Build a minimal ListBoxRow: [icon 16x16] [title label].
/// No reorder buttons — reorder is via DnD and Alt+Up/Down.
/// Includes a DragSource for drag-to-reorder.
pub fn build_row(
    display_text: &str,
    tooltip: &str,
    wid: u32,
    is_active: bool,
    icon: gtk::Image,
) -> ListBoxRow {
    icon.set_margin_start(6);
    icon.set_margin_end(6);

    let label = Label::new(Some(display_text));
    label.set_halign(gtk::Align::Start);
    label.set_hexpand(true);
    label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    label.set_tooltip_text(Some(tooltip));

    let hbox = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    hbox.append(&icon);
    hbox.append(&label);

    let row = ListBoxRow::new();
    row.set_child(Some(&hbox));
    row.set_widget_name(&format!("wid-{}", wid));

    if is_active {
        row.add_css_class("ptm-active");
    }

    // DragSource for reordering
    let drag_source = gtk::DragSource::new();
    drag_source.set_actions(gdk4::DragAction::MOVE);
    let wid_str = wid.to_string();
    drag_source.connect_prepare(move |_source, _x, _y| {
        let value = glib::Value::from(&wid_str);
        Some(gdk4::ContentProvider::for_value(&value))
    });
    row.add_controller(drag_source);

    row
}

/// Parse a window ID from a row's widget name (e.g., "wid-12345" → 12345).
pub fn parse_wid_from_name(name: &str) -> Option<u32> {
    name.strip_prefix("wid-")?.parse().ok()
}

/// Get the label widget from a row (navigates HBox → second child (after icon) → Label).
pub fn get_row_label(row: &ListBoxRow) -> Option<Label> {
    row.child()
        .and_then(|c| c.downcast::<gtk::Box>().ok())
        .and_then(|b| {
            // Label is the second child (after the icon image)
            b.first_child()
                .and_then(|first| first.next_sibling())
                .and_then(|c| c.downcast::<Label>().ok())
        })
}

/// Replace the label in a row with an Entry for inline editing.
pub fn start_inline_rename(
    row: &ListBoxRow,
    wid: u32,
    state: &Rc<RefCell<AppState>>,
    on_change: &OnChangeCb,
) {
    let current_text = get_row_label(row)
        .map(|l| l.text().to_string())
        .unwrap_or_default();

    let entry = Entry::new();
    entry.set_text(&current_text);
    entry.set_has_frame(false);
    entry.set_margin_start(8);
    entry.set_margin_end(8);
    row.set_child(Some(&entry));
    entry.grab_focus();
    entry.select_region(0, -1); // Select all text

    let state_for_activate = Rc::clone(state);
    let row_for_activate = row.clone();
    let on_change_activate = Rc::clone(on_change);
    entry.connect_activate(move |entry| {
        let new_name = entry.text().to_string();
        commit_rename(&row_for_activate, wid, &new_name, &state_for_activate, &on_change_activate);
    });

    // Also commit on focus-out
    let state_for_focus = Rc::clone(state);
    let row_for_focus = row.clone();
    let entry_clone = entry.clone();
    let on_change_focus = Rc::clone(on_change);
    let focus_controller = gtk::EventControllerFocus::new();
    focus_controller.connect_leave(move |_| {
        let new_name = entry_clone.text().to_string();
        commit_rename(&row_for_focus, wid, &new_name, &state_for_focus, &on_change_focus);
    });
    entry.add_controller(focus_controller);
}

/// Commit the rename and swap the Entry back to a Label (inside an HBox).
pub fn commit_rename(
    row: &ListBoxRow,
    wid: u32,
    new_name: &str,
    state: &Rc<RefCell<AppState>>,
    on_change: &OnChangeCb,
) {
    // Already committed (might fire twice from activate + focus-out)
    if row.child().and_then(|c| c.downcast::<Entry>().ok()).is_none() {
        return;
    }

    let trimmed = new_name.trim();
    if !trimmed.is_empty() {
        state.borrow_mut().rename_window(wid, trimmed);
    }

    // Get display text
    let state_ref = state.borrow();
    let display = state_ref.display_name(wid);
    let native = state_ref.native_title(wid);

    let label = Label::new(Some(display));
    label.set_halign(gtk::Align::Start);
    label.set_hexpand(true);
    label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    label.set_tooltip_text(Some(native));

    // Restore HBox with label (icon lost during rename — will be restored on next rebuild)
    let hbox = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    hbox.append(&label);

    row.set_child(Some(&hbox));

    // Notify that state changed (triggers debounced save)
    if let Some(ref f) = *on_change.borrow() {
        f();
    }
}
