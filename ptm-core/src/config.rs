use std::path::Path;
use serde::{Deserialize, Serialize};

const DEFAULT_WM_CLASSES: &[&str] = &[
    "Gnome-terminal",
    "Tilix",
    "xterm",
    "XTerm",
    "Konsole",
    "kitty",
    "Ghostty",
    "Terminator",
    "Alacritty",
];

/// Application configuration — which WM_CLASS values to manage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    wm_classes: Vec<String>,
}

impl Config {
    pub fn wm_classes(&self) -> &[String] {
        &self.wm_classes
    }

    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Save config to a JSON file, creating parent directories if needed.
    pub fn save_to_file(&self, path: &Path) -> Result<(), std::io::Error> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        std::fs::write(path, json)
    }

    /// Load config from a JSON file. Returns None if file doesn't exist or is invalid.
    pub fn load_from_file(path: &Path) -> Option<Self> {
        let data = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&data).ok()
    }

    /// Merge overlay classes into self, deduplicating (case-sensitive).
    pub fn merge(&self, overlay: &Config) -> Config {
        let mut classes = self.wm_classes.clone();
        for c in &overlay.wm_classes {
            if !classes.contains(c) {
                classes.push(c.clone());
            }
        }
        Config { wm_classes: classes }
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            wm_classes: DEFAULT_WM_CLASSES.iter().map(|s| s.to_string()).collect(),
        }
    }
}
