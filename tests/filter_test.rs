use process_tab_manager::filter::Filter;

#[test]
fn exact_match() {
    let filter = Filter::new(vec!["Gnome-terminal".to_string()]);
    assert!(filter.matches("Gnome-terminal"));
}

#[test]
fn case_insensitive_match() {
    let filter = Filter::new(vec!["Gnome-terminal".to_string()]);
    assert!(filter.matches("gnome-terminal"));
    assert!(filter.matches("GNOME-TERMINAL"));
}

#[test]
fn rejects_non_matching() {
    let filter = Filter::new(vec!["Gnome-terminal".to_string()]);
    assert!(!filter.matches("firefox"));
    assert!(!filter.matches(""));
    assert!(!filter.matches("Gnome-terminal-extra"));
}

#[test]
fn multiple_classes() {
    let filter = Filter::new(vec![
        "Gnome-terminal".to_string(),
        "kitty".to_string(),
        "Alacritty".to_string(),
    ]);
    assert!(filter.matches("kitty"));
    assert!(filter.matches("Alacritty"));
    assert!(filter.matches("alacritty"));
    assert!(!filter.matches("firefox"));
}

#[test]
fn empty_filter_matches_nothing() {
    let filter = Filter::new(vec![]);
    assert!(!filter.matches("anything"));
}

#[test]
fn whitespace_in_class_is_exact() {
    let filter = Filter::new(vec!["Gnome-terminal".to_string()]);
    assert!(!filter.matches(" Gnome-terminal"));
    assert!(!filter.matches("Gnome-terminal "));
}
