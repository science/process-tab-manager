# Process Tab Manager — Project Plan

A vertical sidebar app for managing application windows on Linux/X11. Think "Firefox vertical tabs" but for any application — terminals, browsers, image editors, whatever you configure.

## Core Concept

This is a **window management tool**, not a terminal emulator. It doesn't embed or control applications — it discovers running windows, displays them in a vertical sidebar, and manages focus/positioning when you click one. Your existing terminal (or any app) is fine as-is.

## Technology Decision: Rust + GTK4 + x11rb

Investigated Python+GTK3+Wnck, TypeScript+GJS, and Rust+GTK4. Rust wins for this project:

**Why not the original Python + GTK3 + Wnck plan:**
- Python Wnck bindings are broken on this system (typelib installed but `gi.require_version('Wnck', '3.0')` fails)
- Dynamically typed Python is suboptimal for AI-agent coding and TDD — bugs hide until runtime
- GTK3 is in maintenance mode; GTK4 is actively developed

**Why not TypeScript + GJS:**
- node-gtk is beta and untested with Wnck
- GJS has no type safety (plain JS, not TypeScript)
- The TypeScript→esbuild→GJS build pipeline is novel and unproven

**Why Rust + GTK4 + x11rb:**
- **Rust**: Maximum type safety. The compiler catches AI coding mistakes before runtime — the ultimate TDD guardrail. Good training data for AI agents.
- **GTK4-rs** (v0.10): Actively maintained Rust GTK bindings (updated March 2026). Custom CSS gives us a dark theme matching the environment without relying on Cinnamon's GTK3 theme.
- **x11rb** (v0.13): Pure Rust X11 library used by production window managers (Penrose, LeftWM). Replaces Wnck with ~150 lines of EWMH protocol code. No C dependency.
- **Compile times**: ~2-5 min first build (downloading deps), ~5-10s incremental. Fine for TDD on a small app.

**GTK4 theming note:** GTK4 doesn't use Cinnamon's GTK3 theme, but this doesn't matter. Linux desktops are already a theme patchwork (Slack, Firefox, Thunderbird all do their own thing). We embed our own dark CSS — matching the "sublime grey" background and font is straightforward. The sidebar is a utility with minimal UI; pixel-perfect Cinnamon theme matching is unnecessary.

**Original framework comparison (for reference):**

| Factor | GTK3 (Python) | GTK4 (Rust) | Qt6 |
|--------|---------------|-------------|------|
| Cinnamon theme | Native | Custom CSS (fine for utility app) | Approximate |
| Window management | Wnck (broken in Python) | x11rb (pure Rust, proven) | Manual X11 |
| Type safety | None (dynamic) | Maximum (Rust compiler) | C++ |
| Dependencies | Zero (pre-installed) | Rust toolchain + libgtk-4-dev | 28 packages |
| Drag-reorder | 1 line (`TreeView.set_reorderable`) | ~40 lines (DragSource/DropTarget) | 1 line |
| Long-term | Maintenance mode | Active development | Active development |
| AI coding quality | Medium | High (compiler feedback loop) | Medium |

## Architecture

```
┌─────────────────────────────────┐
│  process-tab-manager (sidebar)  │
│                                 │
│  ┌───────────────────────────┐  │     ┌──────────────────────┐
│  │ ▼ Terminals               │  │     │                      │
│  │   ● claude: dotfiles ←────│──│────→│  [focused terminal]  │
│  │   ● claude: web-api       │  │     │                      │
│  │   ● htop                  │  │     │  (positioned right   │
│  │ ▼ Browsers                │  │     │   of sidebar)        │
│  │   ● GitHub PR #42         │  │     │                      │
│  │   ● Stack Overflow        │  │     └──────────────────────┘
│  │ ▶ Image Editors (collapsed)│  │
│  └───────────────────────────┘  │
│           [+ Add App Filter]    │
└─────────────────────────────────┘
  narrow, resizable, always-on-top
```

**Tab Management UX Notes / High Level Concepts**
1. Note that the example in Architecture with "Terminals" grouped separately from "Browsers" is arbitrary and just an example. Just as easily multiple application tabs could be groups into a single project:
```
> Project 1
  * Terminal: claude
  * Terminal: tty
  * Terminal: npm run dev
  * Firefox: UAT
```
2. The design is that Tabs of various applications can be selected and grouped together to form a project. Grabbing a particular application instance/window for attaching to a project is the key idea, and UX for handling that will need to be designed. Mostly like a "color dropper" analogy would be effect. Click a "capture" button in the tab manager, and then click on a window, and the system will attach that window to the project.
3. All windows of a particular class (such a Terminal) may be declared to be always managed by the tab manager (that's what "Add App Filter" would be for)

**Data flow:**
1. x11rb monitors X11 events (PropertyNotify on root window) for window open/close/rename
2. We filter to windows matching configured WM_CLASS values
3. Display in GTK4 ListBox with user-assigned names (preserving native title as tooltip)
4. On click: x11rb activates/raises that window via `_NET_ACTIVE_WINDOW` ClientMessage, then repositions it adjacent to sidebar via `ConfigureWindow`

## Project Structure

```
process-tab-manager/
├── Cargo.toml
├── PLAN.md                    # This file
├── CLAUDE.md                  # AI assistant instructions
├── style.css                  # GTK4 dark theme CSS (embedded at compile time)
├── src/
│   ├── main.rs                # Entry point, CLI args
│   ├── app.rs                 # GtkApplication setup, CSS loading, window creation
│   ├── sidebar.rs             # GTK4 ListBox, row management, click/DnD handlers
│   ├── row.rs                 # Custom ListBoxRow widget (GObject subclass)
│   ├── bridge.rs              # GLib ↔ x11rb event loop integration (CRITICAL)
│   ├── x11/
│   │   ├── mod.rs             # Public API
│   │   ├── connection.rs      # x11rb connection, atom cache
│   │   ├── ewmh.rs            # EWMH property reads/writes (window discovery, title, workspace)
│   │   ├── monitor.rs         # X11 event subscription + dispatch
│   │   └── actions.rs         # activate_window, move_window, switch_desktop
│   ├── state.rs               # AppState: window list, renames, ordering (PURE — no GTK/X11)
│   ├── config.rs              # Config load/save/merge (PURE)
│   ├── geometry.rs            # Snap-to-sidebar math (PURE)
│   └── filter.rs              # WM_CLASS matching (PURE)
├── tests/
│   ├── geometry_test.rs       # Snap position, clamping, multi-monitor
│   ├── config_test.rs         # Load/save/merge, defaults, round-trip
│   ├── state_test.rs          # Add/remove/rename/reorder/serialize
│   ├── filter_test.rs         # WM_CLASS matching, case sensitivity
│   ├── ewmh_test.rs           # Parse X11 property reply bytes (no live connection)
│   └── bridge_test.rs         # Event loop integration tests
├── test/
│   ├── vm-e2e-test.sh         # VM E2E tests (bash + xdotool + screenshots)
│   └── screenshots/           # Captured test screenshots
├── vm -> ../cinnamon-multirow-windowlist/vm  # Symlink to shared VM infra
└── config/
    └── default-config.json    # Default WM_CLASS filters
```

### Module Separation Rule

**Pure modules** (`state.rs`, `config.rs`, `geometry.rs`, `filter.rs`) import ZERO GTK or X11 types. They are tested with `cargo test` on any system, no display needed.

**Impure modules** (`app.rs`, `sidebar.rs`, `row.rs`, `bridge.rs`, `x11/*`) depend on GTK4/x11rb and are tested via VM E2E.

## Key Dependencies (Cargo.toml)

```toml
[dependencies]
gtk4 = "0.10"
gdk4 = "0.10"
glib = "0.21"
gio = "0.21"
x11rb = { version = "0.13", features = ["extra-traits"] }
x11rb-protocol = "0.13"              # For testable protocol type parsing
serde = { version = "1", features = ["derive"] }
serde_json = "1"
log = "0.4"
env_logger = "0.11"
anyhow = "1"
thiserror = "2"

[dev-dependencies]
tempfile = "3"
```

## TDD RED GREEN

Work using TDD methods throughout the project. Every module gets tests first, then implementation.

**Pure modules** → unit tests via `cargo test` (no display needed, runs anywhere)
**Impure modules** → VM E2E tests via bash + xdotool + screenshots

The **event loop bridge** (`bridge.rs`) is the most architecturally critical module. Its event translation function is extracted as a pure function and thoroughly unit-tested. Integration (FD monitoring, GLib loop) is verified via VM E2E.

## VM Testing

In order to test this application, repeated E2E testing is necessary — use the existing cinnamon-dev VM for E2E testing. Reuses the shared VM infrastructure (libvirt/KVM, virtio-fs mount at `/mnt/host-dev/`) developed for the 3 Cinnamon taskbar applet projects in `~/dev/`. Same patterns: `vm-ctl.sh` for VM lifecycle, xdotool for interaction, screenshots for visual verification.

**Prerequisites in VM:** Rust toolchain, `libgtk-4-dev`, `xdotool`, `xterm`.

**Build in VM:** `cd /mnt/host-dev/process-tab-manager && cargo build`

**Build utility scripts to aid VM testing:** Scripts to uninstall everything, to run tests, to screenshot at key moments, to crop screenshots to correctly areas of screen, etc.
---

## Phase 0 — Prerequisites + Verification Spikes

### 0.1 — System setup
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
sudo apt install libgtk-4-dev
```

### 0.2 — Spike A: x11rb window discovery (no GTK)
Standalone binary that connects to X11, reads `_NET_CLIENT_LIST`, prints window class + title for each. Proves x11rb works, atom interning works, property parsing works.

### 0.3 — Spike B: GTK4 ListBox (no X11)
Standalone GTK4 window with hardcoded rows + dark CSS. Proves GTK4-rs compiles, CSS loads, ListBox renders.

### 0.4 — Spike C: Event loop integration (CRITICAL)
GTK4 window + x11rb together. Register x11rb's FD with `glib::unix_fd_source_new`. When windows open/close on the desktop, the GTK ListBox updates in real time. **This is the architectural proof point.** Pattern:

```rust
let fd = conn.stream().as_raw_fd();
glib::unix_fd_source_new(fd, IOCondition::IN, Priority::DEFAULT, move |_fd, _cond| {
    while let Ok(Some(event)) = conn.poll_for_event() {
        // dispatch to GTK widget updates (we're on the main thread)
    }
    ControlFlow::Continue
});
```

### 0.5 — Delete spikes, commit clean skeleton

---

## Phase 1 — MVP (the 80/20)

Goal: a working vertical sidebar that lists terminal windows and focuses/positions them on click. Persistence included so restarts don't lose your organization.

### 1.1 — Config + Filter (TDD)
1. **Test first:** `tests/config_test.rs` — default includes terminal classes, round-trips JSON, merges with defaults
2. **Test first:** `tests/filter_test.rs` — exact match, case-insensitive, rejects non-matching
3. **Implement:** `src/config.rs`, `src/filter.rs`
4. Default filter: common terminal classes (`Gnome-terminal`, `Tilix`, `xterm`, `XTerm`, `Konsole`, `kitty`, `Ghostty`, `Terminator`, `Alacritty`)

### 1.2 — X11 Window Discovery (TDD)
1. **Test first:** `tests/ewmh_test.rs` — parse `_NET_CLIENT_LIST` reply bytes, parse `WM_CLASS` bytes, parse `_NET_WM_NAME` UTF-8 (all using `x11rb-protocol` types, no live connection)
2. **Implement:** `src/x11/connection.rs` (AtomCache), `src/x11/ewmh.rs` (get_client_list, get_window_info)
3. **Implement:** `src/x11/monitor.rs` (subscribe to root PropertyNotify + SubstructureNotify)

### 1.3 — Event Loop Bridge (TDD)
1. **Test first:** `tests/bridge_test.rs` — event translation (construct x11rb protocol event structs, verify PtmEvent output), buffering safety
2. **Implement:** `src/bridge.rs` — FD source, event drain loop, PtmEvent translation

Domain event type (no x11rb types leak out):
```rust
pub enum PtmEvent {
    WindowListChanged,           // _NET_CLIENT_LIST changed on root
    ActiveWindowChanged(u32),    // _NET_ACTIVE_WINDOW changed
    WindowTitleChanged(u32),     // _NET_WM_NAME changed on a window
    DesktopChanged(u32),         // _NET_CURRENT_DESKTOP changed
    WindowDestroyed(u32),        // DestroyNotify received
}
```

### 1.4 — GTK4 Sidebar + List Display
1. **Test first:** `tests/state_test.rs` — update_window_list adds/removes, set_active, filtered_windows
2. **Implement:** `src/state.rs` — AppState, WindowInfo, update/filter/active logic
3. **Implement:** `src/app.rs` — GtkApplication, CSS, window creation (250x600, dark, resizable, normal window behavior)
4. **Implement:** `src/sidebar.rs` — ListBox from state, auto-sync on X11 events
5. **Implement:** `src/row.rs` — PtmRow GObject subclass (label, window_id)
6. Highlight currently focused window via CSS class + ListBox selection
7. Auto-update when windows open/close/change title (X11 events via bridge)
8. **VM E2E:** Open 5 xterms → PTM shows 5 rows. Close 2 → list shrinks. Focus changes → highlight moves.

### 1.5 — Click to Focus + Snap (TDD)
1. **Test first:** `tests/geometry_test.rs` — snap_position (right of sidebar, clamp to workarea, multi-monitor, edge cases)
2. **Implement:** `src/geometry.rs`
3. **Implement:** `src/x11/actions.rs`:
   - `activate_window`: `_NET_ACTIVE_WINDOW` ClientMessage with source=2 (pager — bypasses focus-stealing prevention)
   - `move_window`: `ConfigureWindow` with new x, y (never resize — user's window size is preserved)
   - `switch_desktop`: `_NET_CURRENT_DESKTOP` ClientMessage
4. **Implement:** `src/sidebar.rs` — GestureClick handler with modifier detection:
   - **Normal click (same workspace):** activate + snap to sidebar's right edge
   - **Ctrl+click:** focus only, no snap
   - **Cross-workspace:** switch desktop + activate, NO snap
5. Terminal/application is not resized — it is just moved to attach/snap to the tab mgr. User resizing is a choice and is honored/unaltered by this app.
6. **VM E2E:** Click row → xdotool verifies focus + position. Ctrl+click → position unchanged. Cross-workspace → workspace switches.

### 1.6 — Inline Rename (TDD)
1. **Test first:** `tests/state_test.rs` — rename_window, display_name returns user_name or native_title
2. **Implement:** State rename logic
3. **Implement:** `src/row.rs` — double-click triggers Label→Entry swap, Enter commits
4. Show native title as tooltip, so you don't lose it
5. If no user name set, display native title as primary
6. **VM E2E:** Double-click row, type name, verify change persists in list.

### 1.7 — Drag to Reorder (TDD)
1. **Test first:** `tests/state_test.rs` — reorder(from, to) for all index combos
2. **Implement:** State reorder
3. **Implement:** `src/row.rs` — DragSource + DropTarget controllers
4. **Fallback:** If GTK4 DnD proves too complex for MVP, use up/down buttons per row. DnD in Phase 2.
5. **VM E2E:** Drag row 3 to position 1, verify order change.

### 1.8 — Persistence (TDD)
1. **Test first:** `tests/config_test.rs` — load from disk, save to disk
2. **Test first:** `tests/state_test.rs` — serialize/deserialize, match saved state to live windows by WM_CLASS
3. **Implement:** Config loads from `~/.config/process-tab-manager/config.json`
4. **Implement:** State saves to `~/.config/process-tab-manager/state.json` on change (debounced via `glib::timeout_add`)
5. Load on startup, reload config on SIGHUP
6. **VM E2E:** Rename window, close PTM, reopen → name persists.

---

## Event Loop Bridge — TDD Strategy

The bridge (`src/bridge.rs`) is the most architecturally critical module. It integrates x11rb's X11 connection with GLib's main loop. Getting this wrong means missed events or frozen UI.

### Architecture

Single-threaded, FD-monitored (no threads, no synchronization bugs):
```
GLib Main Loop
├── GTK4 events (clicks, keys, draw)
├── x11rb FD source (PropertyNotify, DestroyNotify)
│   └── poll_for_event() drain loop
└── Debounce timers (state save)
```

### What to Test (tests/bridge_test.rs)

**1. Event translation (pure function, unit-testable):**
```rust
fn translate_event(event: &x11rb::protocol::Event, atoms: &AtomCache, root: u32) -> Option<PtmEvent>
```
- PropertyNotify on root with atom=_NET_CLIENT_LIST → `PtmEvent::WindowListChanged`
- PropertyNotify on root with atom=_NET_ACTIVE_WINDOW → `PtmEvent::ActiveWindowChanged(...)`
- PropertyNotify on window with atom=_NET_WM_NAME → `PtmEvent::WindowTitleChanged(window_id)`
- DestroyNotify → `PtmEvent::WindowDestroyed(window_id)`
- Unrelated events → `None`

Test by constructing `x11rb_protocol` event structs directly (no live connection needed).

**2. Buffering safety (VM E2E test):**
Rapidly open 10 xterms in quick succession. Verify all 10 appear in the sidebar within 2 seconds. No events lost. The `while poll_for_event()` drain loop + 1-second safety timer handles the buffering race.

---

## X11 Operations Reference

| Operation | EWMH Property/Message | x11rb Function |
|-----------|----------------------|----------------|
| List windows | `_NET_CLIENT_LIST` on root | `get_property()` → parse u32 array |
| Window class | `WM_CLASS` | `WmClass::get()` → class + instance strings |
| Window title | `_NET_WM_NAME` (UTF-8) or `WM_NAME` (fallback) | `get_property()` → UTF-8 bytes |
| Active window | `_NET_ACTIVE_WINDOW` on root | Read: `get_property()`. Set: `send_event()` ClientMessage (source=2) |
| Move window | — | `configure_window()` with x, y |
| Window workspace | `_NET_WM_DESKTOP` | `get_property()` → u32 |
| Current workspace | `_NET_CURRENT_DESKTOP` | Read: `get_property()`. Set: `send_event()` ClientMessage |
| Subscribe events | — | `change_window_attributes()` with PROPERTY_CHANGE + SUBSTRUCTURE_NOTIFY |

**Key detail — source=2 for activation:** EWMH specifies source indication: 2 = pager/taskbar. This tells Muffin/Cinnamon to bypass focus-stealing prevention.

**Key detail — request pipelining:** Send all `get_property` requests before calling `.reply()` to get wire-level pipelining. Critical when refreshing info for N windows.

---

## UX Questions and Answers

1. **What happens when you click a window that's on a different workspace?** Switch to that workspace (less surprising), but don't snap to tab manager (which remains in its designated workspace). This enables multiworkspace tab management in a basic way. Further UX may evolve.

2. **Auto-positioning behavior:** Normal click always repositions (snaps) the window next to the sidebar. Ctrl+click focuses without snapping. No snap for cross-workspace windows.

3. **Click behavior summary:**

| Action | Same Workspace | Different Workspace |
|--------|---------------|-------------------|
| Normal click | Activate + snap to sidebar | Switch workspace + activate, NO snap |
| Ctrl+click | Focus only, no snap | Switch workspace + focus only, no snap |

---

## Known Challenges

1. **PTM's own window in the list:** Filter by application ID (`com.github.science.ptm`) as WM_CLASS.
2. **Multi-monitor snap geometry:** Use GTK4's `gdk4::Display::monitors()` for per-monitor geometry, not the giant `_NET_WORKAREA`.
3. **GTK4 DnD complexity:** DragSource + DropTarget is more code than GTK3's `set_reorderable(true)`. Fallback: up/down arrow buttons for MVP.
4. **Window identity across restarts:** Match by `(WM_CLASS, WM_INSTANCE)` for Phase 1. Imperfect for multiple same-class windows but sufficient for rename persistence. Full identity matching is a Phase 2 problem.
5. **x11rb event buffering:** The `while poll_for_event()` drain loop + 1-second safety timer handles the race condition.

---

## Phase 2 — Grouping & Polish

### 2.1 — Tab groups
- GTK4 ListBox with nested sections (or TreeListModel for hierarchy)
- Groups are collapsible
- User can create named groups ("Terminals", "Browsers", etc.)
- Drag windows between groups
- Auto-assign to groups by WM_CLASS (configurable: "all Gnome-terminal windows go in Terminals group")

### 2.2 — Visual polish
- Right-click context menu on rows: rename, move to group, close window
- Keyboard navigation: arrow keys to move, Enter to focus, F2 to rename
- Visual indicator for windows that are minimized vs. visible
- Subtle animation or color change on the row when a window wants attention (X11 urgency hint via `_NET_WM_STATE`)
- App icon next to the window name (read from `_NET_WM_ICON` via x11rb)

### 2.3 — Multi-monitor awareness
- Detect which monitor the sidebar is on
- Snap focused windows to that monitor specifically
- Option to show only windows on the sidebar's monitor, or all monitors

### 2.4 — Window capture UX
- "Color dropper" style capture: click a "capture" button in the tab manager, then click on any window to attach it to a project group
- Uses X11 grab pointer to intercept the next click

---

## Phase 3 — Advanced (future ideas, not MVP)

### 3.1 — Session persistence / restore
- Save session: remember which app was running, with what working directory (for terminals, read /proc/PID/cwd), window title, group assignment
- On "restore session": launch the configured terminal app with the saved working directory
- Hard problem: we can launch a terminal in a directory, but we can't restore the command that was running (that's terminal emulator territory). Still useful for "I had 8 terminals in these 8 project dirs."
- Even harder for non-terminals (what does "restore a Firefox window" mean?)

### 3.2 — Wayland support
- x11rb is X11-only. Wayland has no equivalent window introspection API (by design — "security")
- If Cinnamon moves to Wayland, this tool breaks
- Would need compositor-specific protocols (wlr-foreign-toplevel-management for wlroots compositors)
- Muffin (Cinnamon's compositor) would need to expose something similar
- Cross that bridge when Cinnamon actually ships Wayland support

### 3.3 — Custom actions / hooks
- User-defined actions per window or per group (e.g., "when I focus this terminal, also focus this browser window")
- Shell hook on focus/group-switch for scripting
- Integration with tmux/zellij to also switch multiplexer sessions when focusing a terminal

---

## Research Questions

1. **Identity problem:** How do we re-identify a window across restarts? PID changes every launch. WM_CLASS + title is fragile (terminal titles change with `cd`). Phase 1 uses WM_CLASS + WM_INSTANCE for best-effort matching. Phase 2 may explore PID-based tracking during a session + fuzzy title matching for restore.

---

## Getting Started

```bash
# Install Rust toolchain
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
rustc --version  # Expect 1.83+

# Install GTK4 development headers
sudo apt install libgtk-4-dev
pkg-config --modversion gtk4  # Should show 4.x

# Verify x11rb can connect (after cargo init)
cargo run --bin spike_x11  # Phase 0 spike
```

Start with Phase 0: prove the three integration points (x11rb, GTK4, event loop bridge) work. Everything else builds from there.
