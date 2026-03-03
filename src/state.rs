use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

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

    /// Move a window from one position to another in the list.
    /// Out-of-bounds indices are silently ignored.
    pub fn reorder(&mut self, from: usize, to: usize) {
        let len = self.windows.len();
        if from >= len || to >= len || from == to {
            return;
        }
        let entry = self.windows.remove(from);
        self.windows.insert(to, entry);
    }

    /// Capture current state for persistence.
    pub fn to_saved(&self) -> SavedState {
        let window_order: Vec<String> = self
            .windows
            .iter()
            .map(|w| format!("{}:{}", w.wm_class, w.wm_instance))
            .collect();
        let window_ids: Vec<u32> = self.windows.iter().map(|w| w.id).collect();
        SavedState {
            window_order,
            window_ids,
            renames: self.renames.clone(),
        }
    }

    /// Restore renames and ordering from a saved state.
    /// Maps saved window positions/renames to current live windows by class:instance key.
    pub fn restore_from(&mut self, saved: &SavedState) {
        // Track which saved positions have been claimed (for duplicate class:instance)
        let mut claimed: Vec<bool> = vec![false; saved.window_order.len()];

        // Assign each current window a sort key based on its position in saved order
        let mut sort_keys: Vec<(usize, usize)> = Vec::new();
        for (orig_idx, w) in self.windows.iter().enumerate() {
            let key = format!("{}:{}", w.wm_class, w.wm_instance);
            let sort_key = saved
                .window_order
                .iter()
                .enumerate()
                .find(|(i, k)| k.as_str() == key && !claimed[*i])
                .map(|(i, _)| {
                    claimed[i] = true;
                    i
                })
                .unwrap_or(saved.window_order.len() + orig_idx);
            sort_keys.push((orig_idx, sort_key));
        }
        sort_keys.sort_by_key(|&(_, sk)| sk);

        let old_windows = self.windows.clone();
        self.windows.clear();
        for &(orig_idx, _) in &sort_keys {
            self.windows.push(old_windows[orig_idx].clone());
        }

        // Restore renames: map old wid → saved position → current window at that position
        self.renames.clear();
        for (old_wid, name) in &saved.renames {
            // Find position of old_wid in saved window_ids
            if let Some(saved_pos) = saved.window_ids.iter().position(|id| id == old_wid) {
                // Find which current window claimed that saved position
                // by looking at sort_keys: find the entry whose sort_key == saved_pos
                for &(orig_idx, sort_key) in &sort_keys {
                    if sort_key == saved_pos {
                        let new_wid = old_windows[orig_idx].id;
                        self.renames.insert(new_wid, name.clone());
                        break;
                    }
                }
            }
        }
    }
}

/// Serializable snapshot of app state for persistence across restarts.
/// Uses class:instance keys (not window IDs, which change across restarts).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedState {
    /// Ordered list of class:instance keys representing the user's preferred window order.
    pub window_order: Vec<String>,
    /// Window IDs at save time (parallel to window_order). Used to map renames.
    pub window_ids: Vec<u32>,
    /// User-assigned renames, keyed by window ID at save time.
    pub renames: HashMap<u32, String>,
}

impl SavedState {
    /// Save state to a JSON file, creating parent directories if needed.
    pub fn save_to_file(&self, path: &Path) -> Result<(), std::io::Error> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        std::fs::write(path, json)
    }

    /// Load state from a JSON file. Returns None if file doesn't exist or is invalid.
    pub fn load_from_file(path: &Path) -> Option<Self> {
        let data = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&data).ok()
    }
}
