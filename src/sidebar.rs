use std::cell::RefCell;
use std::rc::Rc;

use gtk4 as gtk;
use gtk::prelude::*;
use gtk::{GestureClick, ListBox};
use x11rb::rust_connection::RustConnection;

use crate::filter::Filter;
use crate::geometry::{self, FrameExtents};
use crate::icon_cache::IconCache;
use crate::row;
use crate::state::AppState;
use crate::x11::actions;
use crate::x11::connection::{self as x11conn, AtomCache};

/// Sidebar manages the GTK ListBox displaying tracked windows.
#[derive(Clone)]
pub struct Sidebar {
    listbox: ListBox,
    state: Rc<RefCell<Option<(Rc<RefCell<AppState>>, Filter)>>>,
    on_change: Rc<RefCell<Option<Box<dyn Fn()>>>>,
    icon_cache: Rc<RefCell<IconCache>>,
    /// WID of the row currently under the mouse cursor (tracked via EventControllerMotion).
    /// Used for focus pass-through: when PTM gains focus from a background click,
    /// the hovered row's target is activated immediately.
    hover_wid: Rc<RefCell<Option<u32>>>,
}

impl Sidebar {
    pub fn new() -> Self {
        let listbox = ListBox::new();
        listbox.set_selection_mode(gtk::SelectionMode::Single);
        // Track which row the cursor hovers over (for focus pass-through)
        let hover_wid: Rc<RefCell<Option<u32>>> = Rc::new(RefCell::new(None));

        let hover_for_motion = Rc::clone(&hover_wid);
        let listbox_for_motion = listbox.clone();
        let motion = gtk::EventControllerMotion::new();
        motion.connect_motion(move |_, _x, y| {
            *hover_for_motion.borrow_mut() = find_row_wid_at_y(&listbox_for_motion, y);
        });

        let hover_for_enter = Rc::clone(&hover_wid);
        let listbox_for_enter = listbox.clone();
        motion.connect_enter(move |_, _x, y| {
            *hover_for_enter.borrow_mut() = find_row_wid_at_y(&listbox_for_enter, y);
        });

        let hover_for_leave = Rc::clone(&hover_wid);
        motion.connect_leave(move |_| {
            *hover_for_leave.borrow_mut() = None;
        });

        listbox.add_controller(motion);

        Self {
            listbox,
            state: Rc::new(RefCell::new(None)),
            on_change: Rc::new(RefCell::new(None)),
            icon_cache: Rc::new(RefCell::new(IconCache::new())),
            hover_wid,
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
    /// Uses row_activated which fires on click/Enter when PTM is in the foreground.
    /// Background clicks are handled separately by connect_focus_passthrough().
    pub fn connect_click(
        &self,
        conn: Rc<RustConnection>,
        atoms: Rc<AtomCache>,
        root: u32,
        window: gtk::ApplicationWindow,
        state: Rc<RefCell<AppState>>,
        ptm_wid: Rc<RefCell<Option<u32>>>,
    ) {
        self.listbox.connect_row_activated(move |_listbox, row| {
            let wid = row::parse_wid_from_name(&row.widget_name());
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

            // Detect cross-workspace: compare target desktop to current desktop
            let target_desktop = state.borrow().window_desktop(wid);
            let current_desktop = x11conn::get_current_desktop(&conn, root, &atoms)
                .ok()
                .flatten();
            let cross_workspace = match (target_desktop, current_desktop) {
                (Some(td), Some(cd)) => td != cd,
                _ => false,
            };

            // Switch desktop first if cross-workspace
            if cross_workspace {
                if let Some(td) = target_desktop {
                    if let Err(e) = actions::switch_desktop(&conn, root, td, &atoms) {
                        log::error!("Failed to switch desktop to {}: {}", td, e);
                    }
                }
            }

            // Activate the window
            if let Err(e) = actions::activate_window(&conn, root, wid, &atoms) {
                log::error!("Failed to activate window 0x{:08x}: {}", wid, e);
                return;
            }

            // Snap to sidebar only on same workspace + no Ctrl
            if !ctrl_pressed && !cross_workspace {
                let pos = compute_snap_position(&conn, &atoms, root, &ptm_wid, wid, &window);
                if let Err(e) = actions::move_window(&conn, wid, pos.x, pos.y) {
                    log::error!("Failed to move window 0x{:08x}: {}", wid, e);
                }
            }
        });
    }

    /// Connect right-click context menu (Rename, Close Window, Remove from List).
    pub fn connect_context_menu(
        &self,
        conn: Rc<RustConnection>,
        atoms: Rc<AtomCache>,
        root: u32,
        state: Rc<RefCell<AppState>>,
    ) {
        let gesture = GestureClick::new();
        gesture.set_button(gdk4::BUTTON_SECONDARY);

        let listbox = self.listbox.clone();
        let on_change = Rc::clone(&self.on_change);
        let sidebar = self.clone();

        gesture.connect_pressed(move |gesture, _n_press, x, y| {
            // Find which row was right-clicked
            let mut idx = 0;
            let mut target_row = None;
            let mut target_wid = None;
            while let Some(row_widget) = listbox.row_at_index(idx) {
                let row_y = row_widget.allocation().y();
                let row_h = row_widget.allocation().height();
                if y >= row_y as f64 && y < (row_y + row_h) as f64 {
                    if let Some(wid) = row::parse_wid_from_name(&row_widget.widget_name()) {
                        target_row = Some(row_widget);
                        target_wid = Some(wid);
                    }
                    break;
                }
                idx += 1;
            }

            let Some(row_widget) = target_row else { return };
            let Some(wid) = target_wid else { return };

            gesture.set_state(gtk::EventSequenceState::Claimed);

            // Build menu model
            let menu = gio::Menu::new();
            menu.append(Some("Rename (F2)"), Some("ptm.rename"));
            menu.append(Some("Close Window"), Some("ptm.close-window"));
            menu.append(Some("Remove from List"), Some("ptm.remove-from-list"));

            // Create action group
            let actions = gio::SimpleActionGroup::new();

            // Rename action
            let rename_state = Rc::clone(&state);
            let rename_on_change = Rc::clone(&on_change);
            let rename_row = row_widget.clone();
            let rename_action = gio::SimpleAction::new("rename", None);
            rename_action.connect_activate(move |_, _| {
                row::start_inline_rename(&rename_row, wid, &rename_state, &rename_on_change);
            });
            actions.add_action(&rename_action);

            // Close Window action
            let close_conn = Rc::clone(&conn);
            let close_atoms = Rc::clone(&atoms);
            let close_action = gio::SimpleAction::new("close-window", None);
            close_action.connect_activate(move |_, _| {
                if let Err(e) = actions::close_window(&close_conn, root, wid, &close_atoms) {
                    log::error!("Failed to close window 0x{:08x}: {}", wid, e);
                }
            });
            actions.add_action(&close_action);

            // Remove from List action
            let remove_state = Rc::clone(&state);
            let remove_sidebar = sidebar.clone();
            let remove_action = gio::SimpleAction::new("remove-from-list", None);
            remove_action.connect_activate(move |_, _| {
                remove_state.borrow_mut().hide_window(wid);
                if let Some((ref app_state, ref filt)) = *remove_sidebar.state.borrow() {
                    remove_sidebar.rebuild(&app_state.borrow(), filt);
                }
                remove_sidebar.notify_change();
            });
            actions.add_action(&remove_action);

            listbox.insert_action_group("ptm", Some(&actions));

            // Create and show popover
            let popover = gtk::PopoverMenu::from_model(Some(&menu));
            popover.set_parent(&row_widget);
            popover.set_pointing_to(Some(&gdk4::Rectangle::new(x as i32, (y - row_widget.allocation().y() as f64) as i32, 1, 1)));
            popover.popup();
        });

        self.listbox.add_controller(gesture);
    }

    /// Connect double-click handler for inline rename.
    pub fn connect_rename(&self, state: Rc<RefCell<AppState>>) {
        let state_for_rename = state;
        let on_change = Rc::clone(&self.on_change);

        let gesture = GestureClick::new();
        gesture.set_button(gdk4::BUTTON_PRIMARY);

        let listbox = self.listbox.clone();
        gesture.connect_pressed(move |gesture, n_press, _x, y| {
            if n_press != 2 {
                return;
            }

            let mut idx = 0;
            while let Some(row_widget) = listbox.row_at_index(idx) {
                let row_y = row_widget.allocation().y();
                let row_h = row_widget.allocation().height();

                if y >= row_y as f64 && y < (row_y + row_h) as f64 {
                    let wid = row::parse_wid_from_name(&row_widget.widget_name());
                    if let Some(wid) = wid {
                        row::start_inline_rename(&row_widget, wid, &state_for_rename, &on_change);
                        gesture.set_state(gtk::EventSequenceState::Claimed);
                    }
                    return;
                }
                idx += 1;
            }
        });

        self.listbox.add_controller(gesture);
    }

    /// Connect keyboard handler for navigation and actions.
    pub fn connect_keyboard(&self, state: Rc<RefCell<AppState>>) {
        let key_controller = gtk::EventControllerKey::new();
        let listbox = self.listbox.clone();
        let sidebar = self.clone();
        let on_change = Rc::clone(&self.on_change);
        let state_for_kb = Rc::clone(&state);

        key_controller.connect_key_pressed(move |_controller, keyval, _keycode, modifiers| {
            let Some(selected) = listbox.selected_row() else {
                return glib::Propagation::Proceed;
            };
            let Some(wid) = row::parse_wid_from_name(&selected.widget_name()) else {
                return glib::Propagation::Proceed;
            };

            let alt = modifiers.contains(gdk4::ModifierType::ALT_MASK);

            match keyval {
                gdk4::Key::F2 => {
                    row::start_inline_rename(&selected, wid, &state_for_kb, &on_change);
                    glib::Propagation::Stop
                }
                gdk4::Key::Delete => {
                    state_for_kb.borrow_mut().hide_window(wid);
                    if let Some((ref app_state, ref filt)) = *sidebar.state.borrow() {
                        sidebar.rebuild(&app_state.borrow(), filt);
                    }
                    sidebar.notify_change();
                    glib::Propagation::Stop
                }
                gdk4::Key::Up if alt => {
                    let idx = selected.index() as usize;
                    if idx > 0 {
                        if let Some((ref app_state, ref filt)) = *sidebar.state.borrow() {
                            app_state.borrow_mut().reorder(idx, idx - 1);
                            sidebar.rebuild(&app_state.borrow(), filt);
                            // Re-select the moved row
                            if let Some(new_row) = listbox.row_at_index((idx - 1) as i32) {
                                listbox.select_row(Some(&new_row));
                            }
                            sidebar.notify_change();
                        }
                    }
                    glib::Propagation::Stop
                }
                gdk4::Key::Down if alt => {
                    let idx = selected.index() as usize;
                    if let Some((ref app_state, ref filt)) = *sidebar.state.borrow() {
                        let count = app_state.borrow().filtered_windows(filt).count();
                        if idx < count - 1 {
                            app_state.borrow_mut().reorder(idx, idx + 1);
                            sidebar.rebuild(&app_state.borrow(), filt);
                            if let Some(new_row) = listbox.row_at_index((idx + 1) as i32) {
                                listbox.select_row(Some(&new_row));
                            }
                            sidebar.notify_change();
                        }
                    }
                    glib::Propagation::Stop
                }
                _ => glib::Propagation::Proceed,
            }
        });
        self.listbox.add_controller(key_controller);
    }

    /// Connect drag-and-drop reorder on the ListBox.
    pub fn connect_dnd(&self) {
        let drop_target = gtk::DropTarget::new(glib::Type::STRING, gdk4::DragAction::MOVE);
        let listbox = self.listbox.clone();
        let sidebar = self.clone();

        // Visual feedback: highlight the row under the cursor during drag
        let listbox_for_motion = self.listbox.clone();
        drop_target.connect_motion(move |_target, _x, y| {
            let mut idx = 0;
            while let Some(row_widget) = listbox_for_motion.row_at_index(idx) {
                let row_y = row_widget.allocation().y();
                let row_h = row_widget.allocation().height();
                if y >= row_y as f64 && y < (row_y + row_h) as f64 {
                    listbox_for_motion.drag_highlight_row(&row_widget);
                    break;
                }
                idx += 1;
            }
            gdk4::DragAction::MOVE
        });

        let listbox_for_leave = self.listbox.clone();
        drop_target.connect_leave(move |_target| {
            listbox_for_leave.drag_unhighlight_row();
        });

        drop_target.connect_drop(move |_target, value, _x, y| {
            let Ok(wid_str) = value.get::<String>() else {
                return false;
            };
            let Ok(source_wid) = wid_str.parse::<u32>() else {
                return false;
            };

            // Find source index by wid
            let mut source_idx = None;
            let mut target_idx = None;
            let mut idx = 0;
            while let Some(row_widget) = listbox.row_at_index(idx) {
                let name = row_widget.widget_name();
                if let Some(rwid) = row::parse_wid_from_name(&name) {
                    if rwid == source_wid {
                        source_idx = Some(idx as usize);
                    }
                }

                let row_y = row_widget.allocation().y();
                let row_h = row_widget.allocation().height();
                if y >= row_y as f64 && y < (row_y + row_h) as f64 {
                    target_idx = Some(idx as usize);
                }
                idx += 1;
            }

            // If dropped past all rows, target is last position
            if target_idx.is_none() && idx > 0 {
                target_idx = Some((idx - 1) as usize);
            }

            if let (Some(from), Some(to)) = (source_idx, target_idx) {
                if from != to {
                    if let Some((ref app_state, ref filt)) = *sidebar.state.borrow() {
                        app_state.borrow_mut().reorder(from, to);
                        sidebar.rebuild(&app_state.borrow(), filt);
                        // Select the moved row
                        if let Some(new_row) = listbox.row_at_index(to as i32) {
                            listbox.select_row(Some(&new_row));
                        }
                        sidebar.notify_change();
                    }
                }
            }

            listbox.drag_unhighlight_row();
            true
        });

        self.listbox.add_controller(drop_target);
    }

    /// Store state+filter references so keyboard/DnD reorder can trigger rebuilds.
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

        let mut icon_cache = self.icon_cache.borrow_mut();

        for entry in entries.into_iter() {
            let display = state.display_name(entry.id);
            let display_text = if display.is_empty() {
                format!("{} (0x{:08x})", entry.wm_class, entry.id)
            } else if state.has_rename(entry.id) {
                display.to_string()
            } else {
                format!("{}: {}", entry.wm_class, display)
            };

            let icon = icon_cache.icon_image(&entry.wm_class);

            let row_widget = row::build_row(
                &display_text,
                &entry.title,
                entry.id,
                Some(entry.id) == active,
                icon,
            );

            if entry.is_minimized {
                row_widget.add_css_class("ptm-minimized");
            }
            if entry.is_urgent {
                row_widget.add_css_class("ptm-urgent");
            }

            self.listbox.append(&row_widget);
        }
    }

    pub fn set_active(&self, wid: u32) {
        let target_name = format!("wid-{}", wid);
        let mut idx = 0;
        while let Some(row_widget) = self.listbox.row_at_index(idx) {
            if row_widget.widget_name() == target_name {
                row_widget.add_css_class("ptm-active");
                self.listbox.select_row(Some(&row_widget));
            } else {
                row_widget.remove_css_class("ptm-active");
            }
            idx += 1;
        }
    }

    pub fn update_state(&self, wid: u32, is_minimized: bool, is_urgent: bool) {
        let target_name = format!("wid-{}", wid);
        let mut idx = 0;
        while let Some(row_widget) = self.listbox.row_at_index(idx) {
            if row_widget.widget_name() == target_name {
                if is_minimized {
                    row_widget.add_css_class("ptm-minimized");
                } else {
                    row_widget.remove_css_class("ptm-minimized");
                }
                if is_urgent {
                    row_widget.add_css_class("ptm-urgent");
                } else {
                    row_widget.remove_css_class("ptm-urgent");
                }
                return;
            }
            idx += 1;
        }
    }

    /// When PTM gains focus from a background click, activate the row under the cursor.
    /// The WM consumes the button press when raising a background window, so neither
    /// row_activated nor GestureClick fire. This workaround uses the hover-tracked WID
    /// (from EventControllerMotion) and the window's is-active notification.
    pub fn connect_focus_passthrough(
        &self,
        conn: Rc<RustConnection>,
        atoms: Rc<AtomCache>,
        root: u32,
        window: &gtk::ApplicationWindow,
        state: Rc<RefCell<AppState>>,
        app_window: gtk::ApplicationWindow,
        ptm_wid: Rc<RefCell<Option<u32>>>,
    ) {
        let hover_wid = Rc::clone(&self.hover_wid);

        window.connect_notify_local(Some("is-active"), move |win, _| {
            if !win.is_active() {
                return;
            }

            // Check if the cursor was hovering over a row when focus was gained
            let wid = match *hover_wid.borrow() {
                Some(wid) => wid,
                None => return,
            };

            log::debug!("Focus pass-through: activating 0x{:08x}", wid);

            // Cross-workspace detection
            let target_desktop = state.borrow().window_desktop(wid);
            let current_desktop = x11conn::get_current_desktop(&conn, root, &atoms)
                .ok()
                .flatten();
            let cross_workspace = match (target_desktop, current_desktop) {
                (Some(td), Some(cd)) => td != cd,
                _ => false,
            };

            if cross_workspace {
                if let Some(td) = target_desktop {
                    let _ = actions::switch_desktop(&conn, root, td, &atoms);
                }
            }

            if let Err(e) = actions::activate_window(&conn, root, wid, &atoms) {
                log::error!("Focus pass-through failed for 0x{:08x}: {}", wid, e);
                return;
            }

            // Snap
            if !cross_workspace {
                let pos = compute_snap_position(&conn, &atoms, root, &ptm_wid, wid, &app_window);
                let _ = actions::move_window(&conn, wid, pos.x, pos.y);
            }
        });
    }

    pub fn update_title(&self, wid: u32, title: &str) {
        let target_name = format!("wid-{}", wid);
        let mut idx = 0;
        while let Some(row_widget) = self.listbox.row_at_index(idx) {
            if row_widget.widget_name() == target_name {
                if let Some(label) = row::get_row_label(&row_widget) {
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

/// Find the WID of the ListBox row at the given y coordinate (relative to the ListBox).
fn find_row_wid_at_y(listbox: &ListBox, y: f64) -> Option<u32> {
    let mut idx = 0;
    while let Some(row_widget) = listbox.row_at_index(idx) {
        let alloc = row_widget.allocation();
        if y >= alloc.y() as f64 && y < (alloc.y() + alloc.height()) as f64 {
            return row::parse_wid_from_name(&row_widget.widget_name());
        }
        idx += 1;
    }
    None
}

/// Compute the snap position using real X11 geometry when available,
/// falling back to GTK monitor geometry estimate.
fn compute_snap_position(
    conn: &RustConnection,
    atoms: &AtomCache,
    root: u32,
    ptm_wid: &Rc<RefCell<Option<u32>>>,
    target_wid: u32,
    window: &gtk::ApplicationWindow,
) -> geometry::SnapPosition {
    let ptm_wid_val = *ptm_wid.borrow();

    if let Some(pw) = ptm_wid_val {
        // Try real X11 geometry
        if let Ok((px, py)) = x11conn::get_window_position(conn, pw, root) {
            let sidebar_rect = geometry::Rect {
                x: px,
                y: py,
                width: window.width() as u32,
                height: window.height() as u32,
            };

            let (sl, sr, st, sb) = x11conn::get_frame_extents(conn, pw, atoms)
                .unwrap_or((0, 0, 0, 0));
            let sidebar_frame = FrameExtents { left: sl, right: sr, top: st, bottom: sb };

            let (tl, tr, tt, tb) = x11conn::get_frame_extents(conn, target_wid, atoms)
                .unwrap_or((0, 0, 0, 0));
            let target_frame = FrameExtents { left: tl, right: tr, top: tt, bottom: tb };

            // Try real workarea from _NET_WORKAREA
            let desktop = x11conn::get_current_desktop(conn, root, atoms)
                .ok()
                .flatten()
                .unwrap_or(0);
            let workarea = x11conn::get_workarea(conn, root, atoms, desktop)
                .ok()
                .flatten()
                .map(|(x, y, w, h)| geometry::Rect { x, y, width: w, height: h })
                .unwrap_or_else(|| get_workarea_estimate(window));

            return geometry::snap_position_with_frames(
                &sidebar_rect,
                &sidebar_frame,
                &target_frame,
                &workarea,
            );
        }
    }

    // Fallback: use GTK-based estimate (original behavior)
    let sidebar_rect = geometry::Rect {
        x: 0,
        y: 0,
        width: window.width() as u32,
        height: window.height() as u32,
    };
    let workarea = get_workarea_estimate(window);
    geometry::snap_position(&sidebar_rect, &workarea)
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
