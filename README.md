# Process Tab Manager

A vertical sidebar for managing application windows on Linux/X11. Think "Firefox vertical tabs" but for your desktop — terminals, browsers, editors, whatever you configure.

![Dark theme sidebar](assets/ptm.svg)

## Why?

Linux window managers give you a taskbar, but taskbars are horizontal, icon-based, and treat all windows equally. If you work with many terminals (or any group of similar windows), they blur together — tiny icons, truncated titles, no custom names, no grouping.

PTM is a persistent vertical sidebar that sits at the screen edge and shows only the windows you care about, filtered by WM_CLASS. You can rename windows, organize them into collapsible groups, reorder them, and click to focus+snap. It's the window management equivalent of Firefox's vertical tab sidebar: always visible, always organized, keyboard-navigable.

The core idea: **your window list should be as manageable as your browser tabs.**

## Features

- **Live window list** — automatically discovers and displays windows matching configured WM_CLASS values (terminals by default: xterm, Gnome-terminal, kitty, Alacritty, Ghostty, Tilix, Konsole, Terminator)
- **Click to focus + snap** — click a row to activate that window and snap it beside the sidebar
- **Ctrl+click** — focus without snapping (keeps the window where it is)
- **Cross-workspace** — clicking a window on another workspace switches to that workspace first, then activates
- **Groups** — create named groups, drag windows between them, collapse/expand, rename groups
- **Drag-and-drop reorder** — drag rows to reorder within or between groups
- **Keyboard navigation** — Arrow keys to navigate, Enter to activate, F2 to rename, Delete to hide, Ctrl+Shift+Up/Down to reorder
- **Double-click rename** — double-click any row to give it a custom name
- **Right-click context menu** — Rename, Close Window, Remove from List, Create Group, Add to Group
- **Window state indicators** — minimized windows shown italic/dimmed, urgent windows highlighted
- **Application icons** — themed icons resolved from `.desktop` entries, with fallback
- **Active window highlight** — blue left border on the active window (Firefox-style)
- **Persistence** — renames, groups, and ordering saved to `~/.config/process-tab-manager/state.json`
- **Self-filtering** — PTM doesn't show itself in its own window list (DOCK window type)
- **Firefox-inspired dark theme** — clean minimal rows, `#1c1b22` background

## Requirements

- Linux with X11 (tested on Cinnamon/Muffin)
- Rust toolchain (`rustup`)
- System packages: `libwebkit2gtk-4.1-dev`, `xterm` (for test windows)

## Quick Start

```bash
./run.sh
```

This builds PTM, launches it, and opens 3 test xterm windows. Things to try:

1. **Click rows** — windows activate and snap beside PTM
2. **Ctrl+click** — focus without snapping
3. **Drag rows** to reorder the list
4. **Double-click** to rename a row
5. **Right-click** for context menu (Rename, Close, Create Group)
6. **Ctrl+Shift+Up/Down** to reorder via keyboard
7. **F2** to rename, **Delete** to remove from list
8. Kill PTM and relaunch — renames, groups, and order persist

## Install / Uninstall

```bash
cargo build -p process-tab-manager --release
./install.sh       # Installs to ~/.local/bin, adds .desktop entry
./uninstall.sh     # Removes installed files
```

## Configuration

Optional. Create `~/.config/process-tab-manager/config.json` to override the default WM_CLASS filter:

```json
{
  "wm_classes": ["Gnome-terminal", "kitty", "Firefox", "Nemo"]
}
```

## Build & Test

```bash
# Unit tests (111 tests, no display needed)
cargo test -p ptm-core

# Build release binary
cargo build -p process-tab-manager --release

# E2E tests (requires X11 desktop)
cd test/e2e && npx wdio run wdio.conf.js

# Single E2E spec
cd test/e2e && npx wdio run wdio.conf.js --spec specs/rename.e2e.js
```

## Tech Stack

- **Tauri v2** — desktop app framework with embedded webview
- **ptm-core** — pure Rust library: window state, config, geometry, filtering, X11 via [x11rb](https://crates.io/crates/x11rb)
- **src-tauri** — Tauri app crate: Tauri commands, background X11 polling thread, icon resolution
- **frontend** — vanilla HTML/CSS/JS (no framework), served by Tauri webview
- **WebdriverIO** — E2E test suite (9 spec files) via `tauri-driver`

## Project Structure

```
Cargo.toml                  # Workspace root (members: ptm-core, src-tauri)
ptm-core/                   # Pure Rust library crate
  src/
    lib.rs                  # Re-exports
    state.rs                # Window/group state, renames, display ordering
    config.rs               # WM_CLASS filtering config
    geometry.rs             # Monitor geometry, window snapping
    filter.rs               # Window class filtering
    bridge.rs               # X11 event translation
    x11/                    # X11 connection, EWMH, window actions, monitors
src-tauri/                  # Tauri app crate
  src/
    lib.rs                  # Tauri commands, app setup, event wiring
    x11_monitor.rs          # Background thread: polls X11, emits sidebar-update
    icon_resolver.rs        # .desktop file icon lookup
frontend/                   # Web frontend
  index.html
  main.js                   # Event listeners, rendering, interactions
  style.css                 # Firefox dark theme
test/e2e/                   # WebdriverIO E2E test suite
  specs/                    # Test specs (window-list, rename, snap, etc.)
  pageobjects/              # Page Object Model (sidebar, context-menu, rename)
  helpers/                  # DOM queries, state reader, xterm fixtures
assets/                     # Icon (ptm.svg) and .desktop file
run.sh                      # One-command build + launch
install.sh / uninstall.sh   # Local install/uninstall
```
