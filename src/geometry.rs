/// A rectangle (position + size).
#[derive(Debug, Clone, Copy)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// The position to move a window to when snapping it to the sidebar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SnapPosition {
    pub x: i32,
    pub y: i32,
}

/// Window manager frame extents (decoration borders around the client area).
#[derive(Debug, Clone, Copy, Default)]
pub struct FrameExtents {
    pub left: u32,
    pub right: u32,
    pub top: u32,
    pub bottom: u32,
}

/// Calculate where to place a window so it snaps to the right edge of the sidebar.
/// Clamps to keep within the workarea bounds.
pub fn snap_position(sidebar: &Rect, workarea: &Rect) -> SnapPosition {
    let x = sidebar.x + sidebar.width as i32;
    let y = sidebar.y;

    // Clamp x to not exceed the workarea right edge
    let max_x = workarea.x + workarea.width as i32;
    let x = x.min(max_x);

    // Clamp y to workarea
    let max_y = workarea.y + workarea.height as i32;
    let y = y.clamp(workarea.y, max_y);

    SnapPosition { x, y }
}

/// Calculate snap position accounting for window manager frame extents.
/// Aligns frame edges: target frame left touches sidebar frame right, title bars aligned.
/// Falls back to `snap_position` behavior when frames are zero.
pub fn snap_position_with_frames(
    sidebar: &Rect,
    sidebar_frame: &FrameExtents,
    target_frame: &FrameExtents,
    workarea: &Rect,
) -> SnapPosition {
    // PTM frame right edge = client_x + client_width + frame_right
    let ptm_frame_right = sidebar.x + sidebar.width as i32 + sidebar_frame.right as i32;

    // Target client_x = PTM frame right + target frame left
    let x = ptm_frame_right + target_frame.left as i32;

    // Align title bars: PTM frame top = client_y - frame_top
    let ptm_frame_top = sidebar.y - sidebar_frame.top as i32;
    // Target client_y = PTM frame top + target frame top
    let y = ptm_frame_top + target_frame.top as i32;

    // Clamp x to workarea right edge
    let max_x = workarea.x + workarea.width as i32;
    let x = x.min(max_x);

    // Clamp y to workarea
    let max_y = workarea.y + workarea.height as i32;
    let y = y.clamp(workarea.y + target_frame.top as i32, max_y);

    SnapPosition { x, y }
}
