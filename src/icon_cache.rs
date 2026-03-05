use std::collections::HashMap;

use gtk4 as gtk;
use gtk::prelude::*;

/// Caches resolved icons per wm_class to avoid repeated lookups.
pub struct IconCache {
    cache: HashMap<String, Option<gio::Icon>>,
}

impl IconCache {
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
        }
    }

    /// Look up an icon for a WM_CLASS. Returns a GIcon if found, None otherwise.
    /// Results are cached per wm_class.
    pub fn lookup(&mut self, wm_class: &str) -> Option<&gio::Icon> {
        self.cache
            .entry(wm_class.to_string())
            .or_insert_with(|| resolve_icon(wm_class))
            .as_ref()
    }

    /// Create a GTK Image widget for the given wm_class.
    /// Returns a themed icon image if found, or a generic fallback.
    pub fn icon_image(&mut self, wm_class: &str) -> gtk::Image {
        if let Some(gicon) = self.lookup(wm_class) {
            let image = gtk::Image::from_gicon(gicon);
            image.set_pixel_size(16);
            image
        } else {
            let image = gtk::Image::from_icon_name("application-x-executable");
            image.set_pixel_size(16);
            image
        }
    }
}

/// Try to resolve an icon via desktop app info.
fn resolve_icon(wm_class: &str) -> Option<gio::Icon> {
    // Try common desktop ID patterns
    let candidates = [
        wm_class.to_lowercase(),
        wm_class.to_string(),
        // Handle "Gnome-terminal" → "org.gnome.Terminal"
        format!(
            "org.gnome.{}",
            wm_class
                .replace("Gnome-", "")
                .replace("gnome-", "")
        ),
    ];

    for candidate in &candidates {
        let desktop_id = format!("{}.desktop", candidate);
        if let Some(app_info) = gio::DesktopAppInfo::new(&desktop_id) {
            if let Some(icon) = app_info.icon() {
                return Some(icon);
            }
        }
    }

    // Try the icon theme directly with the lowercase class name
    let theme = gtk::IconTheme::for_display(&gdk4::Display::default()?);
    let lower = wm_class.to_lowercase();
    if theme.has_icon(&lower) {
        return Some(gio::Icon::from(gio::ThemedIcon::new(&lower)));
    }

    None
}
