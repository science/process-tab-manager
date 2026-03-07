use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

static CACHE: std::sync::LazyLock<Mutex<HashMap<String, Option<String>>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

/// Resolve a WM_CLASS to an icon file path (PNG or SVG).
/// Results are cached per wm_class.
pub fn resolve_icon_path(wm_class: &str) -> Option<String> {
    let mut cache = CACHE.lock().unwrap();
    if let Some(cached) = cache.get(wm_class) {
        return cached.clone();
    }

    let result = find_icon(wm_class);
    cache.insert(wm_class.to_string(), result.clone());
    result
}

fn find_icon(wm_class: &str) -> Option<String> {
    // Try to find a .desktop file matching this wm_class
    let icon_name = find_icon_name_from_desktop(wm_class)?;
    find_icon_file(&icon_name)
}

/// Search /usr/share/applications/ for .desktop files matching wm_class.
/// Parse the Icon= line.
fn find_icon_name_from_desktop(wm_class: &str) -> Option<String> {
    let candidates = [
        wm_class.to_lowercase(),
        wm_class.to_string(),
        format!(
            "org.gnome.{}",
            wm_class
                .replace("Gnome-", "")
                .replace("gnome-", "")
        ),
    ];

    let app_dirs: Vec<PathBuf> = vec![
        PathBuf::from("/usr/share/applications"),
        PathBuf::from("/usr/local/share/applications"),
    ];

    for candidate in &candidates {
        let desktop_name = format!("{}.desktop", candidate);
        for dir in &app_dirs {
            let path = dir.join(&desktop_name);
            if let Some(icon) = parse_desktop_icon(&path) {
                return Some(icon);
            }
        }
    }

    // Fallback: use the lowercase class name as the icon name
    Some(wm_class.to_lowercase())
}

fn parse_desktop_icon(path: &Path) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(value) = trimmed.strip_prefix("Icon=") {
            let icon = value.trim();
            if !icon.is_empty() {
                return Some(icon.to_string());
            }
        }
    }
    None
}

/// Search icon theme directories for an icon by name.
fn find_icon_file(icon_name: &str) -> Option<String> {
    // If icon_name is already an absolute path, return it
    if icon_name.starts_with('/') && Path::new(icon_name).exists() {
        return Some(icon_name.to_string());
    }

    let sizes = ["48x48", "scalable", "256x256", "128x128", "64x64", "32x32", "24x24", "16x16"];
    let categories = ["apps", "categories", "mimetypes"];
    let extensions = ["png", "svg"];

    let themes = [
        "/usr/share/icons/hicolor",
        "/usr/share/icons/Adwaita",
        "/usr/share/pixmaps",
    ];

    // Check /usr/share/pixmaps first (simple flat directory)
    for ext in &extensions {
        let path = format!("/usr/share/pixmaps/{}.{}", icon_name, ext);
        if Path::new(&path).exists() {
            return Some(path);
        }
    }

    // Search themed icon directories
    for theme in &themes {
        for size in &sizes {
            for category in &categories {
                for ext in &extensions {
                    let path = format!("{}/{}/{}/{}.{}", theme, size, category, icon_name, ext);
                    if Path::new(&path).exists() {
                        return Some(path);
                    }
                }
            }
        }
    }

    None
}
