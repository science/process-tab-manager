use std::collections::HashMap;

use crate::filter::Filter;

/// A window entry tracked by PTM.
#[derive(Debug, Clone)]
pub struct WindowEntry {
    pub id: u32,
    pub wm_class: String,
    pub wm_instance: String,
    pub title: String,
    pub desktop: Option<u32>,
}

/// Application state — the list of tracked windows and which is active.
/// Pure: no GTK or X11 types.
pub struct AppState {
    windows: Vec<WindowEntry>,
    active: Option<u32>,
    /// User-assigned names, keyed by window ID.
    renames: HashMap<u32, String>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            windows: Vec::new(),
            active: None,
            renames: HashMap::new(),
        }
    }

    pub fn windows(&self) -> &[WindowEntry] {
        &self.windows
    }

    pub fn active_window(&self) -> Option<u32> {
        self.active
    }

    /// Update window list from fresh X11 data.
    /// Preserves ordering of existing windows; appends new ones at the end (in input order).
    /// Removes windows no longer present. Updates titles of existing windows.
    /// Does NOT clear user renames.
    pub fn update_windows(&mut self, entries: Vec<WindowEntry>) {
        let mut update_map: HashMap<u32, &WindowEntry> =
            entries.iter().map(|e| (e.id, e)).collect();

        self.windows.retain_mut(|w| {
            if let Some(updated) = update_map.remove(&w.id) {
                w.title = updated.title.clone();
                w.desktop = updated.desktop;
                true
            } else {
                false
            }
        });

        let existing_ids: std::collections::HashSet<u32> =
            self.windows.iter().map(|w| w.id).collect();

        for entry in entries {
            if !existing_ids.contains(&entry.id) {
                self.windows.push(entry);
            }
        }

        // Clean up renames for windows that no longer exist
        let live_ids: std::collections::HashSet<u32> =
            self.windows.iter().map(|w| w.id).collect();
        self.renames.retain(|id, _| live_ids.contains(id));
    }

    /// Update a single window's native title without a full refresh.
    pub fn update_title(&mut self, wid: u32, title: &str) {
        if let Some(w) = self.windows.iter_mut().find(|w| w.id == wid) {
            w.title = title.to_string();
        }
    }

    pub fn set_active(&mut self, wid: Option<u32>) {
        self.active = wid;
    }

    /// Set a user-assigned name for a window.
    pub fn rename_window(&mut self, wid: u32, name: &str) {
        if self.windows.iter().any(|w| w.id == wid) {
            self.renames.insert(wid, name.to_string());
        }
    }

    /// Clear the user-assigned name, reverting to native title.
    pub fn clear_rename(&mut self, wid: u32) {
        self.renames.remove(&wid);
    }

    /// Get the display name: user rename if set, otherwise native title.
    pub fn display_name(&self, wid: u32) -> &str {
        if let Some(name) = self.renames.get(&wid) {
            return name;
        }
        self.windows
            .iter()
            .find(|w| w.id == wid)
            .map(|w| w.title.as_str())
            .unwrap_or("")
    }

    /// Get the native X11 title (regardless of rename).
    pub fn native_title(&self, wid: u32) -> &str {
        self.windows
            .iter()
            .find(|w| w.id == wid)
            .map(|w| w.title.as_str())
            .unwrap_or("")
    }

    pub fn filtered_windows<'a>(&'a self, filter: &'a Filter) -> impl Iterator<Item = &'a WindowEntry> {
        self.windows.iter().filter(move |w| filter.matches(&w.wm_class))
    }

    /// Check if a window has a user-assigned rename.
    pub fn has_rename(&self, wid: u32) -> bool {
        self.renames.contains_key(&wid)
    }
}
