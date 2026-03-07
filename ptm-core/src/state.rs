use std::collections::{HashMap, HashSet};
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
    pub pid: Option<u32>,
    pub is_minimized: bool,
    pub is_urgent: bool,
}

/// A tab group containing zero or more windows.
#[derive(Debug, Clone)]
pub struct Group {
    pub id: u32,
    pub name: String,
    pub collapsed: bool,
    pub members: Vec<u32>,
}

/// A slot in the top-level display order.
#[derive(Debug, Clone, PartialEq)]
pub enum DisplaySlot {
    Window(u32),
    Group(u32),
}

/// An item to render in the sidebar.
#[derive(Debug, Clone, PartialEq)]
pub enum ItemKind {
    UngroupedWindow(u32),
    GroupHeader(u32),
    GroupedWindow(u32, u32), // (wid, group_id)
}

/// Application state — the list of tracked windows and which is active.
/// Pure: no GTK or X11 types.
pub struct AppState {
    windows: Vec<WindowEntry>,
    active: Option<u32>,
    /// User-assigned names, keyed by window ID.
    renames: HashMap<u32, String>,
    /// Windows hidden by the user for this session (not persisted).
    hidden: HashSet<u32>,
    /// Tab groups.
    groups: Vec<Group>,
    /// Top-level display ordering. Windows not in this list are appended at the end.
    display_order: Vec<DisplaySlot>,
    /// Next group ID to assign.
    next_group_id: u32,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            windows: Vec::new(),
            active: None,
            renames: HashMap::new(),
            hidden: HashSet::new(),
            groups: Vec::new(),
            display_order: Vec::new(),
            next_group_id: 1,
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

        // Clean up renames and hidden set for windows that no longer exist
        let live_ids: HashSet<u32> = self.windows.iter().map(|w| w.id).collect();
        self.renames.retain(|id, _| live_ids.contains(id));
        self.hidden.clear();

        // Clean up group members for windows that no longer exist
        for group in &mut self.groups {
            group.members.retain(|wid| live_ids.contains(wid));
        }

        // Clean up display_order: remove Window slots for closed windows
        self.display_order.retain(|slot| match slot {
            DisplaySlot::Window(wid) => live_ids.contains(wid),
            DisplaySlot::Group(gid) => self.groups.iter().any(|g| g.id == *gid),
        });
    }

    /// Update a single window's native title without a full refresh.
    pub fn update_title(&mut self, wid: u32, title: &str) {
        if let Some(w) = self.windows.iter_mut().find(|w| w.id == wid) {
            w.title = title.to_string();
        }
    }

    /// Update a window's minimized/urgent state without a full refresh.
    pub fn update_state(&mut self, wid: u32, is_minimized: bool, is_urgent: bool) {
        if let Some(w) = self.windows.iter_mut().find(|w| w.id == wid) {
            w.is_minimized = is_minimized;
            w.is_urgent = is_urgent;
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

    /// Hide a window from the sidebar for this session (not persisted).
    pub fn hide_window(&mut self, wid: u32) {
        self.hidden.insert(wid);
    }

    pub fn filtered_windows<'a>(&'a self, filter: &'a Filter) -> impl Iterator<Item = &'a WindowEntry> {
        self.windows.iter().filter(move |w| filter.matches(&w.wm_class) && !self.hidden.contains(&w.id))
    }

    /// Get the desktop number for a window.
    pub fn window_desktop(&self, wid: u32) -> Option<u32> {
        self.windows.iter().find(|w| w.id == wid).and_then(|w| w.desktop)
    }

    /// Check if a window has a user-assigned rename.
    pub fn has_rename(&self, wid: u32) -> bool {
        self.renames.contains_key(&wid)
    }

    /// Move a window from one position to another in the display order.
    /// Operates on the `display_order` list. Out-of-bounds indices are silently ignored.
    pub fn reorder(&mut self, from: usize, to: usize) {
        // If display_order is populated, reorder there. Otherwise fall back to windows list.
        if !self.display_order.is_empty() {
            let len = self.display_order.len();
            if from >= len || to >= len || from == to {
                return;
            }
            let slot = self.display_order.remove(from);
            self.display_order.insert(to, slot);
        } else {
            let len = self.windows.len();
            if from >= len || to >= len || from == to {
                return;
            }
            let entry = self.windows.remove(from);
            self.windows.insert(to, entry);
        }
    }

    // ── Group operations ──

    /// Look up a group by ID.
    pub fn group(&self, gid: u32) -> Option<&Group> {
        self.groups.iter().find(|g| g.id == gid)
    }

    /// Find the group ID a window belongs to, if any.
    pub fn window_group(&self, wid: u32) -> Option<u32> {
        self.groups.iter().find(|g| g.members.contains(&wid)).map(|g| g.id)
    }

    /// Look up a group mutably by ID.
    fn group_mut(&mut self, gid: u32) -> Option<&mut Group> {
        self.groups.iter_mut().find(|g| g.id == gid)
    }

    /// Create an empty group and append it to display_order.
    pub fn create_group(&mut self, name: &str) -> u32 {
        let gid = self.next_group_id;
        self.next_group_id += 1;
        self.groups.push(Group {
            id: gid,
            name: name.to_string(),
            collapsed: false,
            members: Vec::new(),
        });
        self.ensure_display_order();
        self.display_order.push(DisplaySlot::Group(gid));
        gid
    }

    /// Create a group containing `wid`, placed at the position of `wid` in display_order.
    pub fn create_group_with_window(&mut self, name: &str, wid: u32) -> u32 {
        let gid = self.next_group_id;
        self.next_group_id += 1;

        // Remove wid from any existing group
        for g in &mut self.groups {
            g.members.retain(|&w| w != wid);
        }

        self.groups.push(Group {
            id: gid,
            name: name.to_string(),
            collapsed: false,
            members: vec![wid],
        });

        self.ensure_display_order();

        // Replace the Window(wid) slot with Group(gid) at the same position
        if let Some(pos) = self.display_order.iter().position(|s| *s == DisplaySlot::Window(wid)) {
            self.display_order[pos] = DisplaySlot::Group(gid);
        } else {
            self.display_order.push(DisplaySlot::Group(gid));
        }

        gid
    }

    /// Delete a group. Its members become ungrouped windows re-inserted into display_order
    /// at the position where the group was.
    pub fn delete_group(&mut self, gid: u32) {
        let members = self.groups.iter()
            .find(|g| g.id == gid)
            .map(|g| g.members.clone())
            .unwrap_or_default();

        self.groups.retain(|g| g.id != gid);

        // Replace Group(gid) slot with Window slots for each member
        if let Some(pos) = self.display_order.iter().position(|s| *s == DisplaySlot::Group(gid)) {
            self.display_order.remove(pos);
            for (i, wid) in members.iter().enumerate() {
                self.display_order.insert(pos + i, DisplaySlot::Window(*wid));
            }
        }
    }

    pub fn rename_group(&mut self, gid: u32, name: &str) {
        if let Some(g) = self.group_mut(gid) {
            g.name = name.to_string();
        }
    }

    pub fn toggle_group_collapsed(&mut self, gid: u32) {
        if let Some(g) = self.group_mut(gid) {
            g.collapsed = !g.collapsed;
        }
    }

    /// Add a window to a group. Removes from any previous group first.
    /// Also removes the Window(wid) slot from display_order (the window is now inside the group).
    pub fn add_to_group(&mut self, wid: u32, gid: u32) {
        if !self.groups.iter().any(|g| g.id == gid) {
            return;
        }

        self.ensure_display_order();

        // Remove from any existing group
        for g in &mut self.groups {
            g.members.retain(|&w| w != wid);
        }

        // Add to target group
        if let Some(g) = self.group_mut(gid) {
            g.members.push(wid);
        }

        // Remove the ungrouped Window slot from display_order
        self.display_order.retain(|s| *s != DisplaySlot::Window(wid));
    }

    /// Remove a window from its group. The window becomes ungrouped and is
    /// re-inserted into display_order after its former group.
    pub fn remove_from_group(&mut self, wid: u32) {
        let former_gid = self.groups.iter()
            .find(|g| g.members.contains(&wid))
            .map(|g| g.id);

        for g in &mut self.groups {
            g.members.retain(|&w| w != wid);
        }

        // Insert Window(wid) after the group in display_order
        if let Some(gid) = former_gid {
            let pos = self.display_order.iter()
                .position(|s| *s == DisplaySlot::Group(gid))
                .map(|p| p + 1)
                .unwrap_or(self.display_order.len());
            self.display_order.insert(pos, DisplaySlot::Window(wid));
        }
    }

    /// Reorder windows within a group.
    pub fn reorder_in_group(&mut self, gid: u32, from: usize, to: usize) {
        if let Some(g) = self.group_mut(gid) {
            let len = g.members.len();
            if from >= len || to >= len || from == to {
                return;
            }
            let wid = g.members.remove(from);
            g.members.insert(to, wid);
        }
    }

    /// Produce the flat list of items for sidebar rendering, respecting
    /// display_order, groups, collapse state, and filter.
    pub fn display_items(&self, filter: &Filter) -> Vec<ItemKind> {
        let visible: HashSet<u32> = self.filtered_windows(filter)
            .map(|w| w.id)
            .collect();

        // Collect all window IDs that are inside groups
        let grouped_wids: HashSet<u32> = self.groups.iter()
            .flat_map(|g| g.members.iter().copied())
            .collect();

        // Build items from display_order
        let mut items = Vec::new();
        let mut seen_wids: HashSet<u32> = HashSet::new();
        let mut seen_gids: HashSet<u32> = HashSet::new();

        for slot in &self.display_order {
            match slot {
                DisplaySlot::Window(wid) => {
                    if visible.contains(wid) && !grouped_wids.contains(wid) && seen_wids.insert(*wid) {
                        items.push(ItemKind::UngroupedWindow(*wid));
                    }
                }
                DisplaySlot::Group(gid) => {
                    if let Some(group) = self.group(*gid) {
                        if seen_gids.insert(*gid) {
                            items.push(ItemKind::GroupHeader(*gid));
                            if !group.collapsed {
                                for &wid in &group.members {
                                    if visible.contains(&wid) && seen_wids.insert(wid) {
                                        items.push(ItemKind::GroupedWindow(wid, *gid));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // Append any visible windows not yet seen (new windows, or no display_order set)
        for w in &self.windows {
            if visible.contains(&w.id) && !grouped_wids.contains(&w.id) && seen_wids.insert(w.id) {
                items.push(ItemKind::UngroupedWindow(w.id));
            }
        }

        // Append any groups not yet seen
        for g in &self.groups {
            if seen_gids.insert(g.id) {
                items.push(ItemKind::GroupHeader(g.id));
                if !g.collapsed {
                    for &wid in &g.members {
                        if visible.contains(&wid) && seen_wids.insert(wid) {
                            items.push(ItemKind::GroupedWindow(wid, g.id));
                        }
                    }
                }
            }
        }

        items
    }

    /// Ensure display_order is populated from the current windows list.
    /// Called lazily when groups are first used.
    fn ensure_display_order(&mut self) {
        if !self.display_order.is_empty() {
            return;
        }
        // Initialize from windows list
        for w in &self.windows {
            self.display_order.push(DisplaySlot::Window(w.id));
        }
    }

    /// Capture current state for persistence.
    pub fn to_saved(&self) -> SavedState {
        let window_order: Vec<String> = self
            .windows
            .iter()
            .map(|w| format!("{}:{}", w.wm_class, w.wm_instance))
            .collect();
        let window_ids: Vec<u32> = self.windows.iter().map(|w| w.id).collect();

        let saved_groups: Vec<SavedGroup> = self.groups.iter().map(|g| {
            let member_keys: Vec<String> = g.members.iter().filter_map(|wid| {
                self.windows.iter().find(|w| w.id == *wid)
                    .map(|w| format!("{}:{}", w.wm_class, w.wm_instance))
            }).collect();
            SavedGroup {
                id: g.id,
                name: g.name.clone(),
                collapsed: g.collapsed,
                members: member_keys,
            }
        }).collect();

        let saved_display: Vec<SavedDisplaySlot> = self.display_order.iter().map(|slot| {
            match slot {
                DisplaySlot::Window(wid) => {
                    let key = self.windows.iter().find(|w| w.id == *wid)
                        .map(|w| format!("{}:{}", w.wm_class, w.wm_instance))
                        .unwrap_or_default();
                    SavedDisplaySlot::Window(key)
                }
                DisplaySlot::Group(gid) => SavedDisplaySlot::Group(*gid),
            }
        }).collect();

        SavedState {
            window_order,
            window_ids,
            renames: self.renames.clone(),
            window_x: None,
            window_y: None,
            groups: saved_groups,
            display_order: saved_display,
            next_group_id: self.next_group_id,
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

        // Build a mapping from class:instance key → current wid (first unclaimed)
        let mut key_to_wid: HashMap<String, Vec<u32>> = HashMap::new();
        for w in &self.windows {
            let key = format!("{}:{}", w.wm_class, w.wm_instance);
            key_to_wid.entry(key).or_default().push(w.id);
        }

        // Restore groups
        self.next_group_id = saved.next_group_id.max(1);
        self.groups.clear();
        for sg in &saved.groups {
            let mut members = Vec::new();
            for member_key in &sg.members {
                if let Some(wids) = key_to_wid.get_mut(member_key) {
                    if let Some(wid) = wids.first().copied() {
                        members.push(wid);
                        wids.remove(0);
                    }
                }
            }
            self.groups.push(Group {
                id: sg.id,
                name: sg.name.clone(),
                collapsed: sg.collapsed,
                members,
            });
        }

        // Restore display_order
        // Rebuild key_to_wid (consumed above)
        let mut key_to_wid2: HashMap<String, Vec<u32>> = HashMap::new();
        for w in &self.windows {
            let key = format!("{}:{}", w.wm_class, w.wm_instance);
            key_to_wid2.entry(key).or_default().push(w.id);
        }
        // Remove grouped wids from the pool
        let grouped: HashSet<u32> = self.groups.iter().flat_map(|g| g.members.iter().copied()).collect();
        for wids in key_to_wid2.values_mut() {
            wids.retain(|w| !grouped.contains(w));
        }

        self.display_order.clear();
        for sd in &saved.display_order {
            match sd {
                SavedDisplaySlot::Window(key) => {
                    if let Some(wids) = key_to_wid2.get_mut(key) {
                        if let Some(wid) = wids.first().copied() {
                            self.display_order.push(DisplaySlot::Window(wid));
                            wids.remove(0);
                        }
                    }
                }
                SavedDisplaySlot::Group(gid) => {
                    if self.groups.iter().any(|g| g.id == *gid) {
                        self.display_order.push(DisplaySlot::Group(*gid));
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
    /// Saved PTM window X position (None = use WM default placement).
    #[serde(default)]
    pub window_x: Option<i32>,
    /// Saved PTM window Y position (None = use WM default placement).
    #[serde(default)]
    pub window_y: Option<i32>,
    /// Saved tab groups.
    #[serde(default)]
    pub groups: Vec<SavedGroup>,
    /// Saved display order (top-level slots).
    #[serde(default)]
    pub display_order: Vec<SavedDisplaySlot>,
    /// Next group ID counter.
    #[serde(default = "default_next_group_id")]
    pub next_group_id: u32,
}

fn default_next_group_id() -> u32 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedGroup {
    pub id: u32,
    pub name: String,
    pub collapsed: bool,
    pub members: Vec<String>, // class:instance keys
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SavedDisplaySlot {
    Window(String), // class:instance key
    Group(u32),
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
