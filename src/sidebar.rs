use std::cell::RefCell;
use std::rc::Rc;

use gtk4 as gtk;
use gtk::prelude::*;
use gtk::{Button, Entry, GestureClick, Label, ListBox, ListBoxRow};
use x11rb::rust_connection::RustConnection;

use crate::filter::Filter;
use crate::geometry;
use crate::state::AppState;
use crate::x11::actions;
use crate::x11::connection::AtomCache;

/// Sidebar manages the GTK ListBox displaying tracked windows.
#[derive(Clone)]
pub struct Sidebar {
    listbox: ListBox,
    state: Rc<RefCell<Option<(Rc<RefCell<AppState>>, Filter)>>>,
    on_change: Rc<RefCell<Option<Box<dyn Fn()>>>>,
}

impl Sidebar {
    pub fn new() -> Self {
        let listbox = ListBox::new();
        listbox.set_selection_mode(gtk::SelectionMode::Single);
        Self {
            listbox,
            state: Rc::new(RefCell::new(None)),
            on_change: Rc::new(RefCell::new(None)),
        }
    }

    /// Set a callback to be invoked when user changes state (rename, reorder).
    pub fn set_on_change(&self, f: impl Fn() + 'static) {
        *self.on_change.borrow_mut() = Some(Box::new(f));
    }

    fn notify_change(&self) {
        if let Some(ref f) = *self.on_change.borrow() {
            f();
        }
    }

    pub fn widget(&self) -> &ListBox {
        &self.listbox
    }

    /// Set up click handler for focusing/snapping windows.
    pub fn connect_click(
        &self,
        conn: Rc<RustConnection>,
        atoms: Rc<AtomCache>,
        root: u32,
        window: gtk::ApplicationWindow,
    ) {
        self.listbox.connect_row_activated(move |_listbox, row| {
            let wid = parse_wid_from_name(&row.widget_name());
            let Some(wid) = wid else { return };

            // Check for Ctrl modifier
            let ctrl_pressed = gdk4::Display::default()
                .and_then(|d| d.default_seat())
                .and_then(|seat| seat.keyboard())
                .map(|kb| {
                    let modifiers = kb.modifier_state();
                    modifiers.contains(gdk4::ModifierType::CONTROL_MASK)
                })
                .unwrap_or(false);

            // Activate the window
            if let Err(e) = actions::activate_window(&conn, root, wid, &atoms) {
                log::error!("Failed to activate window 0x{:08x}: {}", wid, e);
                return;
            }

            if !ctrl_pressed {
                // Get sidebar geometry for snap calculation
                let sidebar_rect = geometry::Rect {
                    x: 0, // GTK4 doesn't expose toplevel position easily; default to 0
                    y: 0,
                    width: window.width() as u32,
                    height: window.height() as u32,
                };

                let workarea = get_workarea_estimate(&window);
                let pos = geometry::snap_position(&sidebar_rect, &workarea);

                if let Err(e) = actions::move_window(&conn, wid, pos.x, pos.y) {
                    log::error!("Failed to move window 0x{:08x}: {}", wid, e);
                }
            }
        });
    }

    /// Connect double-click handler for inline rename.
    pub fn connect_rename(&self, state: Rc<RefCell<AppState>>) {
        let state_for_rename = state;
        let on_change = Rc::clone(&self.on_change);

        // Double-click on a row triggers rename
        let gesture = GestureClick::new();
        gesture.set_button(gdk4::BUTTON_PRIMARY);

        let listbox = self.listbox.clone();
        gesture.connect_pressed(move |gesture, n_press, _x, y| {
            if n_press != 2 {
                return; // Only handle double-click
            }

            // Find which row was double-clicked by y coordinate
            let mut idx = 0;
            while let Some(row) = listbox.row_at_index(idx) {
                let (_, row_y, _, row_h) = (
                    row.allocation().x(),
                    row.allocation().y(),
                    row.allocation().width(),
                    row.allocation().height(),
                );

                // Check if click is within this row
                if y >= row_y as f64 && y < (row_y + row_h) as f64 {
                    let wid = parse_wid_from_name(&row.widget_name());
                    if let Some(wid) = wid {
                        start_inline_rename(&row, wid, &state_for_rename, &on_change);
                        gesture.set_state(gtk::EventSequenceState::Claimed);
                    }
                    return;
                }
                idx += 1;
            }
        });

        self.listbox.add_controller(gesture);
    }

    /// Store state+filter references so reorder buttons can trigger rebuilds.
    pub fn set_reorder_state(&self, state: Rc<RefCell<AppState>>, filter: Filter) {
        *self.state.borrow_mut() = Some((state, filter));
    }

    /// Fully rebuild the ListBox from current state + filter.
    pub fn rebuild(&self, state: &AppState, filter: &Filter) {
        while let Some(child) = self.listbox.first_child() {
            self.listbox.remove(&child);
        }

        let active = state.active_window();
        let entries: Vec<_> = state.filtered_windows(filter).collect();
        let count = entries.len();

        for (idx, entry) in entries.into_iter().enumerate() {
            let display = state.display_name(entry.id);
            let display_text = if display.is_empty() {
                format!("{} (0x{:08x})", entry.wm_class, entry.id)
            } else if state.has_rename(entry.id) {
                display.to_string()
            } else {
                format!("{}: {}", entry.wm_class, display)
            };

            let label = Label::new(Some(&display_text));
            label.set_halign(gtk::Align::Start);
            label.set_margin_start(8);
            label.set_hexpand(true);
            label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
            label.set_tooltip_text(Some(&entry.title));

            let hbox = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            hbox.append(&label);

            // Up button (disabled for first row)
            let up_btn = Button::with_label("\u{25B2}");
            up_btn.add_css_class("ptm-reorder-btn");
            up_btn.set_sensitive(idx > 0);
            let sidebar_state = Rc::clone(&self.state);
            let sidebar = self.clone();
            let from_idx = idx;
            up_btn.connect_clicked(move |_| {
                if let Some((ref app_state, ref filt)) = *sidebar_state.borrow() {
                    app_state.borrow_mut().reorder(from_idx, from_idx - 1);
                    sidebar.rebuild(&app_state.borrow(), filt);
                    sidebar.notify_change();
                }
            });
            hbox.append(&up_btn);

            // Down button (disabled for last row)
            let down_btn = Button::with_label("\u{25BC}");
            down_btn.add_css_class("ptm-reorder-btn");
            down_btn.set_sensitive(idx < count - 1);
            let sidebar_state = Rc::clone(&self.state);
            let sidebar = self.clone();
            let from_idx = idx;
            down_btn.connect_clicked(move |_| {
                if let Some((ref app_state, ref filt)) = *sidebar_state.borrow() {
                    app_state.borrow_mut().reorder(from_idx, from_idx + 1);
                    sidebar.rebuild(&app_state.borrow(), filt);
                    sidebar.notify_change();
                }
            });
            hbox.append(&down_btn);

            let row = ListBoxRow::new();
            row.set_child(Some(&hbox));
            row.set_widget_name(&format!("wid-{}", entry.id));

            if Some(entry.id) == active {
                row.add_css_class("ptm-active");
            }

            self.listbox.append(&row);
        }
    }

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

    pub fn update_title(&self, wid: u32, title: &str) {
        let target_name = format!("wid-{}", wid);
        let mut idx = 0;
        while let Some(row) = self.listbox.row_at_index(idx) {
            if row.widget_name() == target_name {
                // Label is inside an HBox inside the row
                let label = row
                    .child()
                    .and_then(|c| c.downcast::<gtk::Box>().ok())
                    .and_then(|b| b.first_child())
                    .and_then(|c| c.downcast::<Label>().ok());
                if let Some(label) = label {
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

fn parse_wid_from_name(name: &str) -> Option<u32> {
    name.strip_prefix("wid-")?.parse().ok()
}

type OnChangeCb = Rc<RefCell<Option<Box<dyn Fn()>>>>;

/// Replace the label in a row with an Entry for inline editing.
fn start_inline_rename(
    row: &ListBoxRow,
    wid: u32,
    state: &Rc<RefCell<AppState>>,
    on_change: &OnChangeCb,
) {
    let current_text = row
        .child()
        .and_then(|c| c.downcast::<gtk::Box>().ok())
        .and_then(|b| b.first_child())
        .and_then(|c| c.downcast::<Label>().ok())
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
fn commit_rename(
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
    label.set_margin_start(8);
    label.set_hexpand(true);
    label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    label.set_tooltip_text(Some(native));

    // Restore HBox with label (buttons will be re-added on next rebuild)
    let hbox = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    hbox.append(&label);

    row.set_child(Some(&hbox));

    // Notify that state changed (triggers debounced save)
    if let Some(ref f) = *on_change.borrow() {
        f();
    }
}

fn get_workarea_estimate(window: &gtk::ApplicationWindow) -> geometry::Rect {
    let display = gtk::prelude::WidgetExt::display(window);
    if let Some(surface) = window.surface() {
        if let Some(monitor) = display.monitor_at_surface(&surface) {
            let geo = monitor.geometry();
            return geometry::Rect {
                x: geo.x(),
                y: geo.y(),
                width: geo.width() as u32,
                height: geo.height() as u32,
            };
        }
    }

    geometry::Rect {
        x: 0,
        y: 0,
        width: 1920,
        height: 1080,
    }
}
