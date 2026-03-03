use process_tab_manager::state::{AppState, WindowEntry};

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
