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
}

impl AppState {
    pub fn new() -> Self {
        Self {
            windows: Vec::new(),
            active: None,
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
    pub fn update_windows(&mut self, entries: Vec<WindowEntry>) {
        // Build lookup for updating existing entries
        let mut update_map: std::collections::HashMap<u32, &WindowEntry> =
            entries.iter().map(|e| (e.id, e)).collect();

        // Update existing windows in place (preserve order), remove dead ones
        self.windows.retain_mut(|w| {
            if let Some(updated) = update_map.remove(&w.id) {
                w.title = updated.title.clone();
                w.desktop = updated.desktop;
                true
            } else {
                false
            }
        });

        // Existing IDs (after retain)
        let existing_ids: std::collections::HashSet<u32> =
            self.windows.iter().map(|w| w.id).collect();

        // Append new windows in their input order
        for entry in entries {
            if !existing_ids.contains(&entry.id) {
                self.windows.push(entry);
            }
        }
    }

    /// Update a single window's title without a full refresh.
    pub fn update_title(&mut self, wid: u32, title: &str) {
        if let Some(w) = self.windows.iter_mut().find(|w| w.id == wid) {
            w.title = title.to_string();
        }
    }

    pub fn set_active(&mut self, wid: Option<u32>) {
        self.active = wid;
    }

    pub fn filtered_windows<'a>(&'a self, filter: &'a Filter) -> impl Iterator<Item = &'a WindowEntry> {
        self.windows.iter().filter(move |w| filter.matches(&w.wm_class))
    }
}
