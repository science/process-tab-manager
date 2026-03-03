use process_tab_manager::state::{AppState, SavedState, WindowEntry};

fn make_entry(id: u32, class: &str, title: &str) -> WindowEntry {
    WindowEntry {
        id,
        wm_class: class.to_string(),
        wm_instance: class.to_lowercase(),
        title: title.to_string(),
        desktop: Some(0),
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
    use process_tab_manager::filter::Filter;

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
