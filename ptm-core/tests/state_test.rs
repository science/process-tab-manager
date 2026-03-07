use ptm_core::state::{AppState, ItemKind, SavedState, WindowEntry};

fn make_entry(id: u32, class: &str, title: &str) -> WindowEntry {
    WindowEntry {
        id,
        wm_class: class.to_string(),
        wm_instance: class.to_lowercase(),
        title: title.to_string(),
        desktop: Some(0),
        pid: None,
        is_minimized: false,
        is_urgent: false,
    }
}

#[test]
fn empty_state() {
    let state = AppState::new();
    assert!(state.windows().is_empty());
    assert_eq!(state.active_window(), None);
}

#[test]
fn update_adds_windows() {
    let mut state = AppState::new();
    let entries = vec![
        make_entry(1, "Gnome-terminal", "terminal 1"),
        make_entry(2, "Gnome-terminal", "terminal 2"),
    ];
    state.update_windows(entries);
    assert_eq!(state.windows().len(), 2);
}

#[test]
fn update_removes_closed_windows() {
    let mut state = AppState::new();
    state.update_windows(vec![
        make_entry(1, "Gnome-terminal", "t1"),
        make_entry(2, "Gnome-terminal", "t2"),
        make_entry(3, "Gnome-terminal", "t3"),
    ]);
    assert_eq!(state.windows().len(), 3);

    // Window 2 closed
    state.update_windows(vec![
        make_entry(1, "Gnome-terminal", "t1"),
        make_entry(3, "Gnome-terminal", "t3"),
    ]);
    assert_eq!(state.windows().len(), 2);
    assert!(state.windows().iter().all(|w| w.id != 2));
}

#[test]
fn update_preserves_order_of_existing_windows() {
    let mut state = AppState::new();
    state.update_windows(vec![
        make_entry(1, "Gnome-terminal", "t1"),
        make_entry(2, "Gnome-terminal", "t2"),
        make_entry(3, "Gnome-terminal", "t3"),
    ]);

    // X11 returns in different order, plus a new window
    state.update_windows(vec![
        make_entry(3, "Gnome-terminal", "t3"),
        make_entry(1, "Gnome-terminal", "t1"),
        make_entry(4, "Gnome-terminal", "t4"),
        make_entry(2, "Gnome-terminal", "t2"),
    ]);

    let ids: Vec<u32> = state.windows().iter().map(|w| w.id).collect();
    // Original order preserved: 1, 2, 3; new window appended: 4
    assert_eq!(ids, vec![1, 2, 3, 4]);
}

#[test]
fn update_refreshes_titles() {
    let mut state = AppState::new();
    state.update_windows(vec![make_entry(1, "Gnome-terminal", "old title")]);
    state.update_windows(vec![make_entry(1, "Gnome-terminal", "new title")]);
    assert_eq!(state.windows()[0].title, "new title");
}

#[test]
fn set_active_window() {
    let mut state = AppState::new();
    state.update_windows(vec![
        make_entry(1, "Gnome-terminal", "t1"),
        make_entry(2, "Gnome-terminal", "t2"),
    ]);
    state.set_active(Some(2));
    assert_eq!(state.active_window(), Some(2));
}

#[test]
fn set_active_to_unknown_window() {
    let mut state = AppState::new();
    state.update_windows(vec![make_entry(1, "Gnome-terminal", "t1")]);
    // Setting active to a window we don't track is fine — just store it
    state.set_active(Some(999));
    assert_eq!(state.active_window(), Some(999));
}

#[test]
fn set_active_none() {
    let mut state = AppState::new();
    state.set_active(Some(1));
    state.set_active(None);
    assert_eq!(state.active_window(), None);
}

#[test]
fn filtered_windows() {
    use ptm_core::filter::Filter;

    let mut state = AppState::new();
    state.update_windows(vec![
        make_entry(1, "Gnome-terminal", "t1"),
        make_entry(2, "firefox", "browser"),
        make_entry(3, "Gnome-terminal", "t2"),
    ]);

    let filter = Filter::new(vec!["Gnome-terminal".to_string()]);
    let filtered: Vec<&WindowEntry> = state.filtered_windows(&filter).collect();
    assert_eq!(filtered.len(), 2);
    assert!(filtered.iter().all(|w| w.wm_class == "Gnome-terminal"));
}

// -- Rename tests (Phase 1.6) --

#[test]
fn rename_window() {
    let mut state = AppState::new();
    state.update_windows(vec![make_entry(1, "Gnome-terminal", "original")]);
    state.rename_window(1, "my custom name");
    assert_eq!(state.display_name(1), "my custom name");
}

#[test]
fn display_name_returns_title_when_no_rename() {
    let mut state = AppState::new();
    state.update_windows(vec![make_entry(1, "Gnome-terminal", "native title")]);
    assert_eq!(state.display_name(1), "native title");
}

#[test]
fn rename_survives_title_update() {
    let mut state = AppState::new();
    state.update_windows(vec![make_entry(1, "Gnome-terminal", "original")]);
    state.rename_window(1, "my name");
    // Title updates (from X11) should not override user rename
    state.update_title(1, "new native title");
    assert_eq!(state.display_name(1), "my name");
    // But native title should still be accessible
    assert_eq!(state.native_title(1), "new native title");
}

#[test]
fn clear_rename_reverts_to_native() {
    let mut state = AppState::new();
    state.update_windows(vec![make_entry(1, "Gnome-terminal", "native")]);
    state.rename_window(1, "custom");
    assert_eq!(state.display_name(1), "custom");
    state.clear_rename(1);
    assert_eq!(state.display_name(1), "native");
}

#[test]
fn rename_unknown_window_is_noop() {
    let mut state = AppState::new();
    state.rename_window(999, "name");
    assert_eq!(state.display_name(999), "");
}

#[test]
fn rename_persists_across_window_list_updates() {
    let mut state = AppState::new();
    state.update_windows(vec![make_entry(1, "Gnome-terminal", "t1")]);
    state.rename_window(1, "my terminal");

    // Window list refresh (same window, new title)
    state.update_windows(vec![make_entry(1, "Gnome-terminal", "t1-updated")]);
    assert_eq!(state.display_name(1), "my terminal");
}

// -- Desktop lookup (Phase 1.9) --

#[test]
fn window_desktop_returns_desktop_number() {
    let mut state = AppState::new();
    state.update_windows(vec![
        WindowEntry {
            id: 1,
            wm_class: "Gnome-terminal".to_string(),
            wm_instance: "gnome-terminal".to_string(),
            title: "t1".to_string(),
            desktop: Some(2),
            pid: None,
            is_minimized: false,
            is_urgent: false,
        },
    ]);
    assert_eq!(state.window_desktop(1), Some(2));
    assert_eq!(state.window_desktop(999), None);
}

// -- Reorder tests (Phase 1.7) --

#[test]
fn reorder_move_down() {
    let mut state = AppState::new();
    state.update_windows(vec![
        make_entry(1, "Gnome-terminal", "t1"),
        make_entry(2, "Gnome-terminal", "t2"),
        make_entry(3, "Gnome-terminal", "t3"),
    ]);
    state.reorder(0, 2); // move first to third position
    let ids: Vec<u32> = state.windows().iter().map(|w| w.id).collect();
    assert_eq!(ids, vec![2, 3, 1]);
}

#[test]
fn reorder_move_up() {
    let mut state = AppState::new();
    state.update_windows(vec![
        make_entry(1, "Gnome-terminal", "t1"),
        make_entry(2, "Gnome-terminal", "t2"),
        make_entry(3, "Gnome-terminal", "t3"),
    ]);
    state.reorder(2, 0); // move third to first position
    let ids: Vec<u32> = state.windows().iter().map(|w| w.id).collect();
    assert_eq!(ids, vec![3, 1, 2]);
}

#[test]
fn reorder_same_position() {
    let mut state = AppState::new();
    state.update_windows(vec![
        make_entry(1, "Gnome-terminal", "t1"),
        make_entry(2, "Gnome-terminal", "t2"),
    ]);
    state.reorder(0, 0); // no-op
    let ids: Vec<u32> = state.windows().iter().map(|w| w.id).collect();
    assert_eq!(ids, vec![1, 2]);
}

#[test]
fn reorder_out_of_bounds_is_noop() {
    let mut state = AppState::new();
    state.update_windows(vec![
        make_entry(1, "Gnome-terminal", "t1"),
        make_entry(2, "Gnome-terminal", "t2"),
    ]);
    state.reorder(0, 10); // out of bounds
    let ids: Vec<u32> = state.windows().iter().map(|w| w.id).collect();
    assert_eq!(ids, vec![1, 2]);
}

#[test]
fn reorder_adjacent_swap() {
    let mut state = AppState::new();
    state.update_windows(vec![
        make_entry(1, "Gnome-terminal", "t1"),
        make_entry(2, "Gnome-terminal", "t2"),
        make_entry(3, "Gnome-terminal", "t3"),
    ]);
    state.reorder(0, 1); // swap first and second
    let ids: Vec<u32> = state.windows().iter().map(|w| w.id).collect();
    assert_eq!(ids, vec![2, 1, 3]);
}

// -- Persistence tests (Phase 1.8) --

#[test]
fn saved_state_captures_renames_and_order() {
    let mut state = AppState::new();
    state.update_windows(vec![
        make_entry(1, "Gnome-terminal", "t1"),
        make_entry(2, "Gnome-terminal", "t2"),
        make_entry(3, "Gnome-terminal", "t3"),
    ]);
    state.rename_window(1, "my terminal");
    state.reorder(0, 2); // move 1 to end: [2, 3, 1]

    let saved = state.to_saved();
    assert_eq!(saved.window_order.len(), 3);
    // Order should be wm_class:wm_instance keys reflecting current order
    assert_eq!(saved.renames.get(&1), Some(&"my terminal".to_string()));
}

#[test]
fn saved_state_round_trips_json() {
    let mut state = AppState::new();
    state.update_windows(vec![
        make_entry(1, "Gnome-terminal", "t1"),
        make_entry(2, "Gnome-terminal", "t2"),
    ]);
    state.rename_window(2, "custom name");

    let saved = state.to_saved();
    let json = serde_json::to_string_pretty(&saved).unwrap();
    let loaded: SavedState = serde_json::from_str(&json).unwrap();

    assert_eq!(saved.window_order, loaded.window_order);
    assert_eq!(saved.renames, loaded.renames);
}

#[test]
fn restore_renames_from_saved_state() {
    let mut state = AppState::new();
    state.update_windows(vec![
        make_entry(1, "Gnome-terminal", "t1"),
        make_entry(2, "Gnome-terminal", "t2"),
    ]);
    state.rename_window(1, "saved name");

    let saved = state.to_saved();

    // New session: different window IDs, same classes
    let mut new_state = AppState::new();
    new_state.update_windows(vec![
        make_entry(100, "Gnome-terminal", "t1-new"),
        make_entry(200, "Gnome-terminal", "t2-new"),
    ]);
    new_state.restore_from(&saved);

    // The rename should map to the first matching window
    // (window 100 matches position 0 which had the rename on window 1)
    assert_eq!(new_state.display_name(100), "saved name");
}

#[test]
fn save_and_load_state_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");

    let mut state = AppState::new();
    state.update_windows(vec![
        make_entry(1, "Gnome-terminal", "t1"),
        make_entry(2, "Gnome-terminal", "t2"),
    ]);
    state.rename_window(1, "my term");

    let saved = state.to_saved();
    saved.save_to_file(&path).expect("save");

    let loaded = SavedState::load_from_file(&path).expect("load");
    assert_eq!(saved.renames, loaded.renames);
}

#[test]
fn load_state_from_nonexistent_file_returns_none() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nonexistent.json");
    let result = SavedState::load_from_file(&path);
    assert!(result.is_none());
}

// -- Hide window tests (Phase 2.1.1) --

#[test]
fn hide_window_excludes_from_filtered() {
    use ptm_core::filter::Filter;

    let mut state = AppState::new();
    state.update_windows(vec![
        make_entry(1, "XTerm", "t1"),
        make_entry(2, "XTerm", "t2"),
        make_entry(3, "XTerm", "t3"),
    ]);

    let filter = Filter::new(vec!["XTerm".to_string()]);
    assert_eq!(state.filtered_windows(&filter).count(), 3);

    state.hide_window(2);
    assert_eq!(state.filtered_windows(&filter).count(), 2);
    assert!(state.filtered_windows(&filter).all(|w| w.id != 2));
}

#[test]
fn hide_window_cleared_on_window_list_update() {
    use ptm_core::filter::Filter;

    let mut state = AppState::new();
    state.update_windows(vec![
        make_entry(1, "XTerm", "t1"),
        make_entry(2, "XTerm", "t2"),
    ]);
    state.hide_window(2);

    let filter = Filter::new(vec!["XTerm".to_string()]);
    assert_eq!(state.filtered_windows(&filter).count(), 1);

    // Full refresh brings window back (user can re-hide if they want)
    state.update_windows(vec![
        make_entry(1, "XTerm", "t1"),
        make_entry(2, "XTerm", "t2"),
    ]);
    assert_eq!(state.filtered_windows(&filter).count(), 2);
}

#[test]
fn hide_unknown_window_is_noop() {
    let mut state = AppState::new();
    state.hide_window(999); // should not panic
}

#[test]
fn restore_order_from_saved_state() {
    let mut state = AppState::new();
    state.update_windows(vec![
        make_entry(1, "Gnome-terminal", "t1"),
        make_entry(2, "kitty", "k1"),
        make_entry(3, "Gnome-terminal", "t2"),
    ]);
    // Reorder: kitty first
    state.reorder(1, 0); // [kitty, Gnome-terminal, Gnome-terminal]

    let saved = state.to_saved();

    // New session with different IDs
    let mut new_state = AppState::new();
    new_state.update_windows(vec![
        make_entry(10, "Gnome-terminal", "new-t1"),
        make_entry(20, "kitty", "new-k1"),
        make_entry(30, "Gnome-terminal", "new-t2"),
    ]);
    new_state.restore_from(&saved);

    let ids: Vec<u32> = new_state.windows().iter().map(|w| w.id).collect();
    // kitty should be first (matching saved order), then Gnome-terminals
    assert_eq!(ids[0], 20, "kitty window should be first");
}

// -- Position persistence tests (Phase 2.2) --

#[test]
fn saved_state_with_position() {
    let mut state = AppState::new();
    state.update_windows(vec![make_entry(1, "XTerm", "t1")]);
    let mut saved = state.to_saved();
    saved.window_x = Some(100);
    saved.window_y = Some(200);

    let json = serde_json::to_string_pretty(&saved).unwrap();
    let loaded: SavedState = serde_json::from_str(&json).unwrap();
    assert_eq!(loaded.window_x, Some(100));
    assert_eq!(loaded.window_y, Some(200));
}

#[test]
fn saved_state_backward_compatible_no_position() {
    // Old state.json without window_x/window_y fields should deserialize fine
    let json = r#"{"window_order":["XTerm:xterm"],"window_ids":[1],"renames":{}}"#;
    let loaded: SavedState = serde_json::from_str(json).unwrap();
    assert_eq!(loaded.window_x, None);
    assert_eq!(loaded.window_y, None);
}

// ── Tab Groups (Phase 2.2) ──

#[test]
fn create_group() {
    let mut state = AppState::new();
    let gid = state.create_group("Work");
    assert_eq!(state.group(gid).unwrap().name, "Work");
    assert!(!state.group(gid).unwrap().collapsed);
    assert!(state.group(gid).unwrap().members.is_empty());
}

#[test]
fn create_multiple_groups_unique_ids() {
    let mut state = AppState::new();
    let g1 = state.create_group("A");
    let g2 = state.create_group("B");
    let g3 = state.create_group("C");
    assert_ne!(g1, g2);
    assert_ne!(g2, g3);
}

#[test]
fn delete_group() {
    let mut state = AppState::new();
    let gid = state.create_group("Temp");
    state.delete_group(gid);
    assert!(state.group(gid).is_none());
}

#[test]
fn delete_group_frees_members() {
    use ptm_core::filter::Filter;
    let mut state = AppState::new();
    state.update_windows(vec![
        make_entry(1, "XTerm", "t1"),
        make_entry(2, "XTerm", "t2"),
    ]);
    let gid = state.create_group_with_window("Work", 1);
    state.add_to_group(2, gid);
    // Both windows in group
    state.delete_group(gid);
    // After delete, both windows should appear as ungrouped in display_items
    let filter = Filter::new(vec!["XTerm".to_string()]);
    let items = state.display_items(&filter);
    assert_eq!(items.len(), 2);
    assert!(items.iter().all(|i| matches!(i, ItemKind::UngroupedWindow(_))));
}

#[test]
fn rename_group() {
    let mut state = AppState::new();
    let gid = state.create_group("Old");
    state.rename_group(gid, "New");
    assert_eq!(state.group(gid).unwrap().name, "New");
}

#[test]
fn toggle_collapsed() {
    let mut state = AppState::new();
    let gid = state.create_group("G");
    assert!(!state.group(gid).unwrap().collapsed);
    state.toggle_group_collapsed(gid);
    assert!(state.group(gid).unwrap().collapsed);
    state.toggle_group_collapsed(gid);
    assert!(!state.group(gid).unwrap().collapsed);
}

#[test]
fn add_window_to_group() {
    let mut state = AppState::new();
    state.update_windows(vec![make_entry(1, "XTerm", "t1")]);
    let gid = state.create_group("Work");
    state.add_to_group(1, gid);
    assert_eq!(state.group(gid).unwrap().members, vec![1]);
}

#[test]
fn add_to_nonexistent_group_is_noop() {
    let mut state = AppState::new();
    state.update_windows(vec![make_entry(1, "XTerm", "t1")]);
    state.add_to_group(1, 999);
    // No panic, no change
}

#[test]
fn remove_from_group() {
    let mut state = AppState::new();
    state.update_windows(vec![make_entry(1, "XTerm", "t1")]);
    let gid = state.create_group("Work");
    state.add_to_group(1, gid);
    assert_eq!(state.group(gid).unwrap().members.len(), 1);
    state.remove_from_group(1);
    assert!(state.group(gid).unwrap().members.is_empty());
}

#[test]
fn reassign_window_between_groups() {
    let mut state = AppState::new();
    state.update_windows(vec![make_entry(1, "XTerm", "t1")]);
    let g1 = state.create_group("A");
    let g2 = state.create_group("B");
    state.add_to_group(1, g1);
    assert_eq!(state.group(g1).unwrap().members, vec![1]);
    // Moving to g2 should remove from g1
    state.add_to_group(1, g2);
    assert!(state.group(g1).unwrap().members.is_empty());
    assert_eq!(state.group(g2).unwrap().members, vec![1]);
}

#[test]
fn create_group_with_window() {
    use ptm_core::filter::Filter;
    let mut state = AppState::new();
    state.update_windows(vec![
        make_entry(1, "XTerm", "t1"),
        make_entry(2, "XTerm", "t2"),
        make_entry(3, "XTerm", "t3"),
    ]);
    // Window 2 is at position 1. Group should replace it at the same position.
    let gid = state.create_group_with_window("Work", 2);
    assert_eq!(state.group(gid).unwrap().members, vec![2]);
    let filter = Filter::new(vec!["XTerm".to_string()]);
    let items = state.display_items(&filter);
    // Expected: UngroupedWindow(1), GroupHeader(gid), GroupedWindow(2, gid), UngroupedWindow(3)
    assert_eq!(items.len(), 4);
    assert!(matches!(items[0], ItemKind::UngroupedWindow(1)));
    assert!(matches!(items[1], ItemKind::GroupHeader(id) if id == gid));
    assert!(matches!(items[2], ItemKind::GroupedWindow(2, id) if id == gid));
    assert!(matches!(items[3], ItemKind::UngroupedWindow(3)));
}

#[test]
fn display_items_ungrouped_only() {
    use ptm_core::filter::Filter;
    let mut state = AppState::new();
    state.update_windows(vec![
        make_entry(1, "XTerm", "t1"),
        make_entry(2, "XTerm", "t2"),
    ]);
    let filter = Filter::new(vec!["XTerm".to_string()]);
    let items = state.display_items(&filter);
    assert_eq!(items.len(), 2);
    assert!(matches!(items[0], ItemKind::UngroupedWindow(1)));
    assert!(matches!(items[1], ItemKind::UngroupedWindow(2)));
}

#[test]
fn display_items_with_group() {
    use ptm_core::filter::Filter;
    let mut state = AppState::new();
    state.update_windows(vec![
        make_entry(1, "XTerm", "t1"),
        make_entry(2, "XTerm", "t2"),
        make_entry(3, "XTerm", "t3"),
    ]);
    let gid = state.create_group("Work");
    state.add_to_group(2, gid);
    state.add_to_group(3, gid);
    let filter = Filter::new(vec!["XTerm".to_string()]);
    let items = state.display_items(&filter);
    // Window 1 ungrouped, then group header, then windows 2 and 3
    assert_eq!(items.len(), 4);
    assert!(matches!(items[0], ItemKind::UngroupedWindow(1)));
    assert!(matches!(items[1], ItemKind::GroupHeader(id) if id == gid));
    assert!(matches!(items[2], ItemKind::GroupedWindow(2, id) if id == gid));
    assert!(matches!(items[3], ItemKind::GroupedWindow(3, id) if id == gid));
}

#[test]
fn display_items_collapsed_hides_children() {
    use ptm_core::filter::Filter;
    let mut state = AppState::new();
    state.update_windows(vec![
        make_entry(1, "XTerm", "t1"),
        make_entry(2, "XTerm", "t2"),
    ]);
    let gid = state.create_group("Work");
    state.add_to_group(1, gid);
    state.add_to_group(2, gid);
    state.toggle_group_collapsed(gid);
    let filter = Filter::new(vec!["XTerm".to_string()]);
    let items = state.display_items(&filter);
    // Only header, no children
    assert_eq!(items.len(), 1);
    assert!(matches!(items[0], ItemKind::GroupHeader(id) if id == gid));
}

#[test]
fn display_items_interspersed_ordering() {
    use ptm_core::filter::Filter;
    let mut state = AppState::new();
    state.update_windows(vec![
        make_entry(1, "XTerm", "t1"),
        make_entry(2, "XTerm", "t2"),
        make_entry(3, "XTerm", "t3"),
        make_entry(4, "XTerm", "t4"),
    ]);
    // Create group at position of window 2, add window 3 to it
    let gid = state.create_group_with_window("G", 2);
    state.add_to_group(3, gid);
    let filter = Filter::new(vec!["XTerm".to_string()]);
    let items = state.display_items(&filter);
    // Window 1, GroupHeader, Window 2 (grouped), Window 3 (grouped), Window 4
    assert_eq!(items.len(), 5);
    assert!(matches!(items[0], ItemKind::UngroupedWindow(1)));
    assert!(matches!(items[1], ItemKind::GroupHeader(_)));
    assert!(matches!(items[2], ItemKind::GroupedWindow(2, _)));
    assert!(matches!(items[3], ItemKind::GroupedWindow(3, _)));
    assert!(matches!(items[4], ItemKind::UngroupedWindow(4)));
}

#[test]
fn reorder_display_slots() {
    use ptm_core::filter::Filter;
    let mut state = AppState::new();
    state.update_windows(vec![
        make_entry(1, "XTerm", "t1"),
        make_entry(2, "XTerm", "t2"),
        make_entry(3, "XTerm", "t3"),
    ]);
    // Move window 3 to position 0
    state.reorder(2, 0);
    let filter = Filter::new(vec!["XTerm".to_string()]);
    let items = state.display_items(&filter);
    assert!(matches!(items[0], ItemKind::UngroupedWindow(3)));
    assert!(matches!(items[1], ItemKind::UngroupedWindow(1)));
    assert!(matches!(items[2], ItemKind::UngroupedWindow(2)));
}

#[test]
fn reorder_in_group() {
    let mut state = AppState::new();
    state.update_windows(vec![
        make_entry(1, "XTerm", "t1"),
        make_entry(2, "XTerm", "t2"),
        make_entry(3, "XTerm", "t3"),
    ]);
    let gid = state.create_group("G");
    state.add_to_group(1, gid);
    state.add_to_group(2, gid);
    state.add_to_group(3, gid);
    // Reorder within group: move index 0 to index 2
    state.reorder_in_group(gid, 0, 2);
    let members = &state.group(gid).unwrap().members;
    assert_eq!(members, &vec![2, 3, 1]);
}

#[test]
fn reorder_in_group_out_of_bounds() {
    let mut state = AppState::new();
    state.update_windows(vec![make_entry(1, "XTerm", "t1")]);
    let gid = state.create_group("G");
    state.add_to_group(1, gid);
    state.reorder_in_group(gid, 0, 10); // out of bounds — no-op
    assert_eq!(state.group(gid).unwrap().members, vec![1]);
}

#[test]
fn update_windows_cleans_group_members() {
    let mut state = AppState::new();
    state.update_windows(vec![
        make_entry(1, "XTerm", "t1"),
        make_entry(2, "XTerm", "t2"),
    ]);
    let gid = state.create_group("G");
    state.add_to_group(1, gid);
    state.add_to_group(2, gid);
    // Window 2 closed
    state.update_windows(vec![make_entry(1, "XTerm", "t1")]);
    assert_eq!(state.group(gid).unwrap().members, vec![1]);
}

#[test]
fn groups_persist_across_save_restore() {
    let mut state = AppState::new();
    state.update_windows(vec![
        make_entry(1, "XTerm", "t1"),
        make_entry(2, "XTerm", "t2"),
    ]);
    let gid = state.create_group_with_window("Work", 1);
    state.add_to_group(2, gid);
    state.rename_group(gid, "My Work");

    let saved = state.to_saved();
    let json = serde_json::to_string_pretty(&saved).unwrap();
    let loaded: SavedState = serde_json::from_str(&json).unwrap();

    // New session with different window IDs
    let mut new_state = AppState::new();
    new_state.update_windows(vec![
        make_entry(100, "XTerm", "t1-new"),
        make_entry(200, "XTerm", "t2-new"),
    ]);
    new_state.restore_from(&loaded);

    // Group should be restored with mapped members
    use ptm_core::filter::Filter;
    let filter = Filter::new(vec!["XTerm".to_string()]);
    let items = new_state.display_items(&filter);
    // Should have: GroupHeader, GroupedWindow(100), GroupedWindow(200)
    assert!(items.iter().any(|i| matches!(i, ItemKind::GroupHeader(_))));
    let grouped_count = items.iter().filter(|i| matches!(i, ItemKind::GroupedWindow(_, _))).count();
    assert_eq!(grouped_count, 2);
}

#[test]
fn add_to_group_initializes_display_order() {
    use ptm_core::filter::Filter;
    let mut state = AppState::new();
    state.update_windows(vec![
        make_entry(1, "XTerm", "t1"),
        make_entry(2, "XTerm", "t2"),
        make_entry(3, "XTerm", "t3"),
    ]);
    // Create an empty group (this initializes display_order via ensure_display_order)
    let gid = state.create_group("Work");
    // Now add_to_group should work even though display_order was originally empty
    state.add_to_group(1, gid);
    assert_eq!(state.group(gid).unwrap().members, vec![1]);

    let filter = Filter::new(vec!["XTerm".to_string()]);
    let items = state.display_items(&filter);
    // Should have: UngroupedWindow(2), UngroupedWindow(3), GroupHeader, GroupedWindow(1)
    assert_eq!(items.len(), 4);
    let grouped = items.iter().filter(|i| matches!(i, ItemKind::GroupedWindow(_, _))).count();
    assert_eq!(grouped, 1);
}

#[test]
fn backward_compatible_no_groups() {
    // Old state.json without group fields should deserialize fine
    let json = r#"{"window_order":["XTerm:xterm"],"window_ids":[1],"renames":{}}"#;
    let loaded: SavedState = serde_json::from_str(json).unwrap();
    assert!(loaded.groups.is_empty());
    assert!(loaded.display_order.is_empty());
}
