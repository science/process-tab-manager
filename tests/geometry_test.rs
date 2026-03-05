use process_tab_manager::geometry::{snap_position, snap_position_with_frames, FrameExtents, Rect};

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

// ── Frame-aware snap tests ──

#[test]
fn snap_with_frames_aligned_titlebars() {
    // PTM client area at (5, 30) with frame: left=5, right=5, top=30, bottom=5
    // Target frame: left=5, right=5, top=30, bottom=5
    let sidebar = Rect { x: 5, y: 30, width: 250, height: 600 };
    let sidebar_frame = FrameExtents { left: 5, right: 5, top: 30, bottom: 5 };
    let target_frame = FrameExtents { left: 5, right: 5, top: 30, bottom: 5 };
    let workarea = Rect { x: 0, y: 0, width: 1920, height: 1080 };

    let pos = snap_position_with_frames(&sidebar, &sidebar_frame, &target_frame, &workarea);

    // PTM frame right edge = client_x + client_width + frame_right = 5 + 250 + 5 = 260
    // Target client_x = ptm_frame_right + target_frame_left = 260 + 5 = 265
    assert_eq!(pos.x, 265);
    // PTM frame top = client_y - frame_top = 30 - 30 = 0
    // Target client_y = ptm_frame_top + target_frame_top = 0 + 30 = 30
    assert_eq!(pos.y, 30);
}

#[test]
fn snap_with_zero_frames_matches_original() {
    // Zero frames should degrade to same as snap_position
    let sidebar = Rect { x: 0, y: 0, width: 250, height: 600 };
    let zero_frame = FrameExtents { left: 0, right: 0, top: 0, bottom: 0 };
    let workarea = Rect { x: 0, y: 0, width: 1920, height: 1080 };

    let frame_pos = snap_position_with_frames(&sidebar, &zero_frame, &zero_frame, &workarea);
    let orig_pos = snap_position(&sidebar, &workarea);

    assert_eq!(frame_pos.x, orig_pos.x);
    assert_eq!(frame_pos.y, orig_pos.y);
}

#[test]
fn snap_with_frames_workarea_offset() {
    // Panel at top: workarea starts at y=40
    // PTM client at (5, 70), frame top=30 → PTM frame top = 70-30 = 40 (at workarea top)
    let sidebar = Rect { x: 5, y: 70, width: 250, height: 560 };
    let sidebar_frame = FrameExtents { left: 5, right: 5, top: 30, bottom: 5 };
    let target_frame = FrameExtents { left: 5, right: 5, top: 30, bottom: 5 };
    let workarea = Rect { x: 0, y: 40, width: 1920, height: 1040 };

    let pos = snap_position_with_frames(&sidebar, &sidebar_frame, &target_frame, &workarea);

    // PTM frame right = 5 + 250 + 5 = 260
    // Target client x = 260 + 5 = 265
    assert_eq!(pos.x, 265);
    // PTM frame top = 70 - 30 = 40. Target client y = 40 + 30 = 70
    assert_eq!(pos.y, 70);
}

#[test]
fn snap_with_frames_clamps_to_workarea() {
    // Sidebar near right edge — snap would go off-screen
    let sidebar = Rect { x: 1800, y: 30, width: 250, height: 600 };
    let sidebar_frame = FrameExtents { left: 5, right: 5, top: 30, bottom: 5 };
    let target_frame = FrameExtents { left: 5, right: 5, top: 30, bottom: 5 };
    let workarea = Rect { x: 0, y: 0, width: 1920, height: 1080 };

    let pos = snap_position_with_frames(&sidebar, &sidebar_frame, &target_frame, &workarea);

    // x should be clamped so it doesn't exceed workarea right edge
    assert!(pos.x <= workarea.x + workarea.width as i32);
}

#[test]
fn snap_with_asymmetric_frames() {
    // PTM has no frame (CSD), target has WM frame with 30px titlebar
    let sidebar = Rect { x: 0, y: 0, width: 250, height: 600 };
    let sidebar_frame = FrameExtents { left: 0, right: 0, top: 0, bottom: 0 };
    let target_frame = FrameExtents { left: 5, right: 5, top: 30, bottom: 5 };
    let workarea = Rect { x: 0, y: 0, width: 1920, height: 1080 };

    let pos = snap_position_with_frames(&sidebar, &sidebar_frame, &target_frame, &workarea);

    // PTM frame right = 0 + 250 + 0 = 250
    // Target client x = 250 + 5 = 255
    assert_eq!(pos.x, 255);
    // PTM frame top = 0 - 0 = 0. Target client y = 0 + 30 = 30
    assert_eq!(pos.y, 30);
}
