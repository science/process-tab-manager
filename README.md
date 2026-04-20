# Process Tab Manager

A vertical sidebar for managing application windows on Linux/X11. Think "Firefox vertical tabs" but for your desktop — terminals, browsers, editors, whatever you have open.

## Why?

Linux taskbars are horizontal, icon-based, and treat all windows equally. If you work with many terminals (or any group of similar windows), they blur together. PTM is a persistent vertical sidebar that shows your windows in an organized, manageable list — like browser tabs for your desktop.

## Features

- **Live window list** — automatically discovers all windows on the current desktop via EWMH
- **Click to focus + snap** — click a row to activate that window and snap it beside the sidebar
- **Tab grouping** — right-click to create groups, add/remove windows, collapse/expand
- **Drag-and-drop reorder** — drag rows to reorder, drag between groups, drag out to ungroup
- **Right-click context menu** — New Group, Add to Group, Remove from Group, Rename, Delete
- **Active window highlight** — blue accent stripe and tinted background on the focused window
- **Hover feedback** — subtle highlight on mouse-over
- **OneDark color scheme** — dark background, warm gray text, colorful accent stripes

## Building

Requires Rust and X11 development headers. `tmux` is an optional runtime dependency — install it to get the session-marker dot next to tmux-backed windows; PTM runs fine without it.

```bash
cargo build --release
```

The binary is at `target/release/ptm`.

To install the binary, icon, and desktop entry into `~/.local/` (and install tmux if missing):

```bash
./install.sh
```

## Running

```bash
DISPLAY=:0 ./target/release/ptm
```

Or if using a virtiofs mount (no exec):
```bash
CARGO_TARGET_DIR=/tmp/ptm-target cargo build --release
DISPLAY=:0 /tmp/ptm-target/release/ptm
```

## Testing

```bash
cargo test     # 25 unit tests, no X11 display needed
```

Tests cover state management, group operations, drag-and-drop resolution, hit testing, and context menu entry generation — all pure logic that runs without a display server.

## Architecture

Single Rust binary (~1800 LOC) talking directly to X11 via [x11rb](https://github.com/psychon/x11rb). No toolkit, no framework, no webview — just rectangles, text, and event handling.

- **One event loop** — `wait_for_event` dispatches to context menu mode or normal mode
- **Double-buffered rendering** — draw to pixmap, copy to window
- **Override-redirect popup** — context menu is a borderless window with pointer grab
- **EWMH protocol** — `_NET_CLIENT_LIST`, `_NET_ACTIVE_WINDOW`, `_NET_WM_NAME` for window discovery

## License

MIT
