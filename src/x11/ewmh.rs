/// Parse _NET_CLIENT_LIST or similar u32 array property bytes (little-endian).
pub fn parse_window_ids(data: &[u8]) -> Vec<u32> {
    data.chunks_exact(4)
        .map(|chunk| u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect()
}

/// Parse WM_CLASS bytes: "instance\0class\0" → (instance, class).
pub fn parse_wm_class(data: &[u8]) -> (String, String) {
    let s = std::str::from_utf8(data).unwrap_or("");
    let mut parts = s.split('\0').filter(|p| !p.is_empty());
    let instance = parts.next().unwrap_or("").to_string();
    let class = parts.next().unwrap_or("").to_string();
    (instance, class)
}

/// Parse _NET_WM_NAME or WM_NAME bytes (UTF-8 with lossy fallback).
pub fn parse_wm_name(data: &[u8]) -> String {
    String::from_utf8_lossy(data).into_owned()
}

/// Parse a single u32 from property bytes (e.g. _NET_ACTIVE_WINDOW, _NET_CURRENT_DESKTOP).
pub fn parse_window_id(data: &[u8]) -> Option<u32> {
    if data.len() < 4 {
        return None;
    }
    Some(u32::from_le_bytes([data[0], data[1], data[2], data[3]]))
}
