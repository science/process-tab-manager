# Process Tab Manager

Vertical sidebar for managing application windows on Linux/X11.

## Tech Stack

The project has two implementations:

- **poc-x11/** — Active development. Pure X11 sidebar using **x11rb** (v0.13). Single-file Rust binary (~1800 LOC) with no framework — talks directly to the X server. OneDark color palette, Nimbus Mono L font.
- **ptm-core/ + src-tauri/ + frontend/** — Legacy Tauri v2 implementation (retained for reference). Not actively developed.
- Cargo workspace: root `Cargo.toml` with members `ptm-core`, `src-tauri`, `poc-x11`

## Build & Test

```bash
source "$HOME/.cargo/env"
CARGO_TARGET_DIR=/tmp/ptm-target cargo test -p poc-x11     # Unit tests (25 tests, no display needed)
CARGO_TARGET_DIR=/tmp/ptm-target cargo build -p poc-x11 --release  # Build binary
DISPLAY=:0 /tmp/ptm-target/release/poc-x11                 # Run (needs X11 desktop)
```

Legacy Tauri commands (not actively used):
```bash
cargo test -p ptm-core                              # ptm-core unit tests
cargo build -p process-tab-manager --release        # Tauri binary
cd test/e2e && npx wdio run wdio.conf.js            # E2E tests
```

## Project Structure

```
Cargo.toml                  # Workspace root
poc-x11/                    # Active: pure X11 sidebar
  Cargo.toml                # Depends on x11rb 0.13
  src/
    main.rs                 # Everything: data model, EWMH, rendering, event loop, tests
ptm-core/                   # Legacy: pure Rust library crate
  src/                      # state.rs, config.rs, geometry.rs, filter.rs, bridge.rs, x11/*
  tests/                    # Unit tests
src-tauri/                  # Legacy: Tauri app crate
  src/                      # lib.rs, x11_monitor.rs, icon_resolver.rs
frontend/                   # Legacy: web frontend
  main.js, style.css, index.html
test/e2e/                   # Legacy: WebdriverIO E2E tests
run.sh                      # Legacy: build + launch Tauri app
```

## poc-x11 Architecture

Single-file binary with clean separation of concerns:

- **Data model**: `Item`, `Group`, `DisplaySlot`, `DisplayRow`, `App` — all state management, group operations, drag-and-drop resolution
- **EWMH helpers**: `get_client_list`, `get_active_window`, `get_window_title`, `activate_window`, `snap_to_sidebar` — thin wrappers over X11 properties
- **Renderer**: double-buffered drawing to pixmap, copies to window. Handles items, group headers, ghost drag, drop indicators
- **Context menu**: override-redirect popup with pointer grab. `build_menu_entries` / `open_context_menu` / `draw_context_menu`
- **Event loop**: single `wait_for_event` loop, context menu mode vs normal mode

### What's testable without X11

All state logic on `App` struct: `build_display_rows`, `hit_test_row`, `drop_index_from_y`, group operations (`create_group`, `add_to_group`, `remove_from_group`, `delete_group`, `rename_group`, `toggle_collapse`), drag resolution (`handle_drop`, `is_gap_in_group`, `reorder_within_group`), and `build_menu_entries`. Currently 25 unit tests.

### What requires X11

Rendering, EWMH property queries, window activation/snapping, context menu popup, color allocation.

## x11rb API Notes

- `AtomEnum` variants (e.g. `AtomEnum::WM_CLASS`) can be passed directly to `get_property` — do NOT call `.into()` on them (causes ambiguous type inference)
- Font: Nimbus Mono L 13px (`-urw-nimbus mono l-regular-r-normal--13-*-*-*-*-*-iso8859-1`) with fallback to fixed 13px
- `image_text8` is Latin-1 only — no Unicode arrows, use ASCII (`+`/`-` for collapse)

## Development Environment

Development happens directly on the X11 desktop (Cinnamon on Debian/Ubuntu). The project directory is at `~/dev/process-tab-manager`.

### Running locally

```bash
source "$HOME/.cargo/env"
CARGO_TARGET_DIR=/tmp/ptm-target cargo build -p poc-x11 --release && DISPLAY=:0 /tmp/ptm-target/release/poc-x11
CARGO_TARGET_DIR=/tmp/ptm-target cargo test -p poc-x11    # Unit tests
```

### Desktop recovery

If Cinnamon crashes or becomes unresponsive:

1. Restart Cinnamon: `DISPLAY=:0 nohup cinnamon --replace &`
2. Restart LightDM: `sudo systemctl restart lightdm` (from SSH only — see warning below)
3. Reboot: `sudo reboot` (from SSH only — see warning below)

**NEVER run `systemctl restart lightdm` or `sudo reboot` from within the desktop session.** These commands kill the X11 session, which destroys ALL terminals — including Claude Code itself. Only use these from an external SSH session. For WM issues when working locally, use option 1 (Cinnamon restart) which is safe.

### Environment notes

- **DISPLAY=:0** — all GUI commands need this when running from a non-GUI terminal (SSH, tmux)
- **CARGO_TARGET_DIR=/tmp/ptm-target** — required for builds; virtiofs mount doesn't support exec
- **sudo** uses fingerprint GUI popup (works from Claude Code shell)
- **git/gh** auth is configured for account `science` (the origin account)

## Git Workflow

- **Local commits are encouraged.** Make intermediate commits freely to checkpoint progress — they're cheap and reversible.
- **Never push to origin without asking the user first.** Pushing affects the shared remote and should always be explicitly confirmed.

## TDD Workflow

Every behavior change starts with a failing test (RED), then implementation (GREEN), then verification.

### Verification Tiers

**Tier 1 — Unit tests (preferred).** `cargo test -p poc-x11` for all state/group/DnD logic. No display needed.

**Tier 2 — Manual visual review.** For rendering, colors, layout — launch the app on the X11 desktop and verify interactively.

### TDD Steps

1. Write or update a test in the `#[cfg(test)] mod tests` block at the bottom of `poc-x11/src/main.rs`
2. Run the test — confirm it fails (RED)
3. Implement the change
4. Run the test — confirm it passes (GREEN)
5. Run the full test suite — confirm no regressions
6. Commit test and implementation together
