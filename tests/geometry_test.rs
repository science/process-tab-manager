use process_tab_manager::geometry::{snap_position, Rect};

#[test]
fn snap_right_of_sidebar() {
    let sidebar = Rect { x: 0, y: 0, width: 250, height: 600 };
    let workarea = Rect { x: 0, y: 0, width: 1920, height: 1080 };
    let pos = snap_position(&sidebar, &workarea);
    assert_eq!(pos.x, 250); // right edge of sidebar
    assert_eq!(pos.y, 0);   // top of workarea
}

#[test]
fn snap_with_sidebar_offset() {
    let sidebar = Rect { x: 100, y: 50, width: 250, height: 600 };
    let workarea = Rect { x: 0, y: 0, width: 1920, height: 1080 };
    let pos = snap_position(&sidebar, &workarea);
    assert_eq!(pos.x, 350); // 100 + 250
    assert_eq!(pos.y, 50);
}

#[test]
fn snap_clamps_to_workarea_right_edge() {
    // Sidebar near the right edge — snap position would be off-screen
    let sidebar = Rect { x: 1800, y: 0, width: 250, height: 600 };
    let workarea = Rect { x: 0, y: 0, width: 1920, height: 1080 };
    let pos = snap_position(&sidebar, &workarea);
    // Should clamp x so that at least the position is within workarea
    assert!(pos.x <= workarea.x + workarea.width as i32);
}

#[test]
fn snap_with_panel_offset_workarea() {
    // Workarea doesn't start at 0,0 (e.g. panel at top)
    let sidebar = Rect { x: 0, y: 40, width: 250, height: 560 };
    let workarea = Rect { x: 0, y: 40, width: 1920, height: 1040 };
    let pos = snap_position(&sidebar, &workarea);
    assert_eq!(pos.x, 250);
    assert_eq!(pos.y, 40);
}

#[test]
fn snap_position_struct() {
    let sidebar = Rect { x: 0, y: 0, width: 250, height: 600 };
    let workarea = Rect { x: 0, y: 0, width: 1920, height: 1080 };
    let pos = snap_position(&sidebar, &workarea);
    // SnapPosition should have x and y
    let _ = pos.x;
    let _ = pos.y;
}
