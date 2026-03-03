use process_tab_manager::x11::ewmh;

#[test]
fn parse_client_list_empty() {
    let data: &[u8] = &[];
    let ids = ewmh::parse_window_ids(data);
    assert!(ids.is_empty());
}

#[test]
fn parse_client_list_single() {
    // One u32 in little-endian: 0x01234567
    let data: &[u8] = &[0x67, 0x45, 0x23, 0x01];
    let ids = ewmh::parse_window_ids(data);
    assert_eq!(ids, vec![0x01234567]);
}

#[test]
fn parse_client_list_multiple() {
    // Three u32s in little-endian
    let data: &[u8] = &[
        0x01, 0x00, 0x00, 0x00, // 1
        0x02, 0x00, 0x00, 0x00, // 2
        0x03, 0x00, 0x00, 0x00, // 3
    ];
    let ids = ewmh::parse_window_ids(data);
    assert_eq!(ids, vec![1, 2, 3]);
}

#[test]
fn parse_client_list_truncated_bytes_ignored() {
    // 5 bytes — last byte doesn't form a complete u32, should be ignored
    let data: &[u8] = &[0x01, 0x00, 0x00, 0x00, 0xFF];
    let ids = ewmh::parse_window_ids(data);
    assert_eq!(ids, vec![1]);
}

#[test]
fn parse_wm_class_normal() {
    // WM_CLASS: "instance\0class\0"
    let data = b"gnome-terminal-server\0Gnome-terminal\0";
    let (instance, class) = ewmh::parse_wm_class(data);
    assert_eq!(instance, "gnome-terminal-server");
    assert_eq!(class, "Gnome-terminal");
}

#[test]
fn parse_wm_class_no_trailing_null() {
    let data = b"instance\0class";
    let (instance, class) = ewmh::parse_wm_class(data);
    assert_eq!(instance, "instance");
    assert_eq!(class, "class");
}

#[test]
fn parse_wm_class_single_part() {
    // Only instance, no class
    let data = b"firefox\0";
    let (instance, class) = ewmh::parse_wm_class(data);
    assert_eq!(instance, "firefox");
    assert_eq!(class, "");
}

#[test]
fn parse_wm_class_empty() {
    let data: &[u8] = &[];
    let (instance, class) = ewmh::parse_wm_class(data);
    assert_eq!(instance, "");
    assert_eq!(class, "");
}

#[test]
fn parse_net_wm_name_utf8() {
    let data = "claude: dotfiles 📁".as_bytes();
    let title = ewmh::parse_wm_name(data);
    assert_eq!(title, "claude: dotfiles 📁");
}

#[test]
fn parse_net_wm_name_empty() {
    let title = ewmh::parse_wm_name(&[]);
    assert_eq!(title, "");
}

#[test]
fn parse_net_wm_name_invalid_utf8() {
    let data: &[u8] = &[0xFF, 0xFE, 0x68, 0x69]; // invalid utf-8 prefix + "hi"
    let title = ewmh::parse_wm_name(data);
    // Should use lossy conversion, not panic
    assert!(title.contains("hi"));
}

#[test]
fn parse_active_window_id() {
    let data: &[u8] = &[0x42, 0x00, 0x60, 0x02]; // 0x02600042
    let wid = ewmh::parse_window_id(data);
    assert_eq!(wid, Some(0x02600042));
}

#[test]
fn parse_active_window_id_none() {
    // _NET_ACTIVE_WINDOW can be 0 (no active window) or missing
    let data: &[u8] = &[0x00, 0x00, 0x00, 0x00];
    let wid = ewmh::parse_window_id(data);
    assert_eq!(wid, Some(0)); // 0 is valid — callers decide if 0 means "none"
}

#[test]
fn parse_active_window_id_short_data() {
    let data: &[u8] = &[0x01, 0x02]; // too short
    let wid = ewmh::parse_window_id(data);
    assert_eq!(wid, None);
}

#[test]
fn parse_desktop_number() {
    let data: &[u8] = &[0x02, 0x00, 0x00, 0x00]; // desktop 2
    let desktop = ewmh::parse_window_id(data);
    assert_eq!(desktop, Some(2));
}
