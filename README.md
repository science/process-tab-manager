# Process Tab Manager

A vertical sidebar for managing application windows on Linux/X11. Think "Firefox vertical tabs" but for your desktop — terminals, browsers, editors, whatever you configure.

## Current State: Alpha (Phase 2.1 complete)

Working features:

- **Live window list** — automatically discovers and displays windows matching configured WM_CLASS values (terminals by default: xterm, Gnome-terminal, kitty, Alacritty, Ghostty, Tilix, Konsole, Terminator)
- **Click to focus + snap** — click a row to activate that window and snap it beside the sidebar
- **Ctrl+click** — focus without snapping (keeps the window where it is)
- **Cross-workspace click** — clicking a window on another workspace switches to that workspace first, then activates (no snap)
- **Active window highlight** — blue left border on the active window (Firefox-style)
- **Live title updates** — window titles update in real-time as they change
- **Application icons** — themed icons resolved from desktop entries, with fallback
- **Drag-and-drop reorder** — drag rows to reorder the list
- **Keyboard navigation** — F2 to rename, Delete to remove, Alt+Up/Down to reorder
- **Double-click rename** — double-click any row to give it a custom name
- **Right-click context menu** — Rename, Close Window, Remove from List
- **Window state indicators** — minimized windows shown italic/dimmed, urgent windows highlighted
- **Persistence** — renames and ordering saved to `~/.config/process-tab-manager/state.json`
- **Self-filtering** — PTM doesn't show itself in its own window list
- **Firefox-inspired dark theme** — clean minimal rows, embedded CSS

## Try It

Requires the `ptm-test` VM (libvirt/KVM with virtiofs mount). One command does everything:

```bash
./run.sh
```

The script handles VM startup, prerequisites, building, launching PTM + 3 xterms, and opening the SPICE viewer.

### Things to try

1. **Click rows** — windows activate and snap beside PTM
2. **Ctrl+click** — focus without snapping
3. **Drag rows** to reorder the list
4. **Double-click** to rename a row
5. **Right-click** for context menu (Rename, Close, Remove)
6. **Alt+Up/Down** to reorder via keyboard
7. **F2** to rename, **Delete** to remove from list
8. **Kill PTM** (`pkill process-tab`) and relaunch — renames and order persist

## Config

Optional. Create `~/.config/process-tab-manager/config.json` to override the default WM_CLASS filter:

```json
{
  "wm_classes": ["Gnome-terminal", "kitty", "Firefox", "Nemo"]
}
```

## Build & Test

```bash
source ~/.cargo/env
cargo test              # 82 unit tests (no display needed)
cargo build --release   # Release binary

# E2E tests (requires ptm-test VM running)
PTM_VM=ptm-test bash test/vm-e2e-test.sh
```

## Tech Stack

Rust + GTK4-rs + x11rb. Single-threaded — x11rb FD registered with GLib main loop. See [PLAN.md](PLAN.md) for full architecture.
