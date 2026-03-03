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
