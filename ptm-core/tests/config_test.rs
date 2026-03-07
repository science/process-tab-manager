use std::fs;
use ptm_core::config::Config;

#[test]
fn default_config_has_terminal_classes() {
    let config = Config::default();
    let classes = config.wm_classes();
    // Must include common terminal emulators
    assert!(classes.iter().any(|c| c == "Gnome-terminal"));
    assert!(classes.iter().any(|c| c == "kitty"));
    assert!(classes.iter().any(|c| c == "Alacritty"));
    assert!(classes.iter().any(|c| c == "xterm"));
    assert!(classes.iter().any(|c| c == "XTerm"));
    assert!(classes.iter().any(|c| c == "Tilix"));
    assert!(classes.iter().any(|c| c == "Konsole"));
    assert!(classes.iter().any(|c| c == "Ghostty"));
    assert!(classes.iter().any(|c| c == "Terminator"));
}

#[test]
fn default_config_excludes_ptm() {
    let config = Config::default();
    let classes = config.wm_classes();
    // Our own window class must not be in the default filter
    assert!(!classes.iter().any(|c| c == "process-tab-manager"));
}

#[test]
fn round_trip_json() {
    let config = Config::default();
    let json = config.to_json().expect("serialize");
    let loaded = Config::from_json(&json).expect("deserialize");
    assert_eq!(config.wm_classes(), loaded.wm_classes());
}

#[test]
fn custom_config_from_json() {
    let json = r#"{"wm_classes": ["Firefox", "Slack"]}"#;
    let config = Config::from_json(json).expect("parse");
    assert_eq!(config.wm_classes(), &["Firefox", "Slack"]);
}

#[test]
fn merge_adds_user_classes_to_defaults() {
    let base = Config::default();
    let overlay_json = r#"{"wm_classes": ["Firefox", "Slack"]}"#;
    let overlay = Config::from_json(overlay_json).expect("parse");
    let merged = base.merge(&overlay);
    // Should have both defaults and user additions
    assert!(merged.wm_classes().iter().any(|c| c == "Gnome-terminal"));
    assert!(merged.wm_classes().iter().any(|c| c == "Firefox"));
    assert!(merged.wm_classes().iter().any(|c| c == "Slack"));
}

#[test]
fn merge_deduplicates() {
    let base = Config::default();
    // Gnome-terminal is already in defaults
    let overlay_json = r#"{"wm_classes": ["Gnome-terminal", "Firefox"]}"#;
    let overlay = Config::from_json(overlay_json).expect("parse");
    let merged = base.merge(&overlay);
    let count = merged
        .wm_classes()
        .iter()
        .filter(|c| c.as_str() == "Gnome-terminal")
        .count();
    assert_eq!(count, 1, "Gnome-terminal should appear exactly once");
}

#[test]
fn empty_json_gives_empty_config() {
    let json = r#"{"wm_classes": []}"#;
    let config = Config::from_json(json).expect("parse");
    assert!(config.wm_classes().is_empty());
}

#[test]
fn invalid_json_returns_error() {
    let result = Config::from_json("not json");
    assert!(result.is_err());
}

// -- Persistence tests (Phase 1.8) --

#[test]
fn save_and_load_config_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.json");

    let config = Config::default();
    config.save_to_file(&path).expect("save");

    let loaded = Config::load_from_file(&path).expect("load");
    assert_eq!(config.wm_classes(), loaded.wm_classes());
}

#[test]
fn load_from_nonexistent_file_returns_none() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nonexistent.json");
    let result = Config::load_from_file(&path);
    assert!(result.is_none());
}

#[test]
fn load_from_invalid_file_returns_none() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bad.json");
    fs::write(&path, "not valid json").unwrap();
    let result = Config::load_from_file(&path);
    assert!(result.is_none());
}

#[test]
fn save_creates_parent_directories() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nested").join("dir").join("config.json");

    let config = Config::default();
    config.save_to_file(&path).expect("save should create dirs");
    assert!(path.exists());
}
