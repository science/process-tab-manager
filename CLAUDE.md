# Process Tab Manager

Vertical sidebar for managing application windows on Linux/X11. Pure X11 via x11rb, single binary.

## Build & Test

```bash
source "$HOME/.cargo/env"
CARGO_TARGET_DIR=/tmp/ptm-dev cargo test                       # 270+ unit + 2 e2e (Xvfb)
CARGO_TARGET_DIR=/tmp/ptm-dev cargo test --lib                 # unit tests only (~50ms)
CARGO_TARGET_DIR=/tmp/ptm-dev cargo test --test e2e_kill_session  # only the e2e suite (~6s)
CARGO_TARGET_DIR=/tmp/ptm-dev cargo build --release             # Build dev binary
DISPLAY=:0 /tmp/ptm-dev/release/ptm                             # Run (needs X11 desktop)
PTM_BIN=/tmp/ptm-dev/release/ptm tests/e2e/<name>.sh           # Run a single e2e script standalone
```

**Dev vs production**: Dev builds use `/tmp/ptm-dev`. The installed launcher (`install.sh`) uses `/tmp/ptm-target`. This keeps `cargo build` during development from overwriting the production binary.

**System dependencies**: building needs Rust + X11 dev headers. Running needs an X11 display. The e2e tests (`tests/e2e/*.sh` driven by `tests/e2e_kill_session.rs`) additionally need `xvfb`, `xdotool`, `tmux`, `scrot`, `xdpyinfo` (`sudo apt install xvfb xdotool tmux scrot x11-utils`). They spin up an isolated Xvfb display on `:99` so they don't touch the desktop session, and use a fresh `HOME` so saved groups state can't perturb row layout. Tests share the user's tmux server (default socket) and serialize themselves via a `Mutex` in the test wrapper.

## Project Structure

```
Cargo.toml              # Package: process-tab-manager, binary: ptm
src/
  main.rs               # Everything: data model, EWMH, rendering, event loop, tests
LICENSE
README.md
```

## Architecture

Single-file binary (~7200 LOC) with clean separation of concerns within `main.rs`:

- **Data model** (top): `Item`, `Group`, `DisplaySlot`, `DisplayRow`, `App` — all state management, group operations, drag-and-drop resolution
- **EWMH helpers**: `get_client_list`, `get_active_window`, `get_window_title`, `activate_window`, `snap_to_sidebar` — thin wrappers over X11 properties
- **Renderer**: double-buffered drawing to pixmap, copies to window. Items, group headers, ghost drag, drop indicators, hover, active highlight
- **Context menu**: override-redirect popup with pointer grab. `build_menu_entries` / `open_context_menu` / `draw_context_menu`
- **Event loop**: single `wait_for_event` loop with two modes: context menu (grab active) and normal
- **Tests**: `#[cfg(test)] mod tests` at bottom — 220+ tests covering all pure state logic

### What's testable without X11

All `App` methods: `build_display_rows`, `hit_test_row`, `drop_index_from_y`, group operations (`create_group`, `add_to_group`, `remove_from_group`, `delete_group`, `rename_group`, `toggle_collapse`), drag resolution (`handle_drop`, `is_gap_in_group`, `reorder_within_group`), and `build_menu_entries`.

### What requires X11

Rendering, EWMH property queries, window activation/snapping, context menu popup, color allocation.

## x11rb API Notes

- `AtomEnum` variants (e.g. `AtomEnum::WM_CLASS`) can be passed directly to `get_property` — do NOT call `.into()` on them (causes ambiguous type inference)
- Font: Nimbus Mono L 13px with fallback to fixed 13px
- `image_text8` is Latin-1 only — no Unicode, use ASCII (`+`/`-` for collapse)
- Override-redirect windows need explicit event masks + pointer grab for menus
- `grab_pointer(owner_events: false)` routes all pointer events to grab window with relative coords

## Development Environment

Development happens directly on the X11 desktop (Cinnamon on Debian/Ubuntu). The project directory is at `~/dev/process-tab-manager`.

### Desktop recovery

If Cinnamon crashes or becomes unresponsive:

1. Restart Cinnamon: `DISPLAY=:0 nohup cinnamon --replace &`
2. Restart LightDM: `sudo systemctl restart lightdm` (from SSH only — see warning below)
3. Reboot: `sudo reboot` (from SSH only — see warning below)

**NEVER run `systemctl restart lightdm` or `sudo reboot` from within the desktop session.** These commands kill the X11 session, which destroys ALL terminals — including Claude Code itself.

### Environment notes

- **DISPLAY=:0** — all GUI commands need this when running from a non-GUI terminal (SSH, tmux)
- **CARGO_TARGET_DIR=/tmp/ptm-dev** — dev builds; `/tmp/ptm-target` is reserved for the installed production binary via `install.sh`. Both needed because virtiofs mount doesn't support exec
- **sudo** uses fingerprint GUI popup (works from Claude Code shell)
- **git/gh** auth is configured for account `science` (the origin account)

## Git Workflow

- **Local commits are encouraged.** Make intermediate commits freely to checkpoint progress.
- **Never push to origin without asking the user first.**

## TDD Workflow

Every behavior change starts with a failing test (RED), then implementation (GREEN), then verification.

**Tier 1 — Unit tests (preferred).** `cargo test` for all state/group/DnD logic. No display needed.

**Tier 2 — Manual visual review.** For rendering, colors, layout — launch the app and verify interactively.

### TDD Steps

1. Write or update a test in the `#[cfg(test)] mod tests` block at the bottom of `src/main.rs`
2. Run the test — confirm it fails (RED)
3. Implement the change
4. Run the test — confirm it passes (GREEN)
5. Run the full test suite — confirm no regressions
6. Commit test and implementation together
