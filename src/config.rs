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
