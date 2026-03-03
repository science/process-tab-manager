# Process Tab Manager

Vertical sidebar for managing application windows on Linux/X11. See PLAN.md for full architecture.

## Tech Stack

- **Rust 1.93+** with **GTK4-rs** (v0.10, glib v0.21) for UI and **x11rb** (v0.13) for X11 window management
- Single-threaded: x11rb FD registered with GLib main loop via `glib::unix_fd_add_local`
- System GTK4: 4.14.5 — use `load_from_data` for CSS (not `load_from_string` which requires GTK 4.16+)

## Build & Test

```bash
source "$HOME/.cargo/env"
cargo build                    # Build main binary
cargo test                     # Run pure module unit tests (no display needed)
cargo run                      # Run the app (requires X11 display)
```

## Module Separation Rule

**Pure modules** (`state.rs`, `config.rs`, `geometry.rs`, `filter.rs`) — zero GTK/X11 imports. Tested with `cargo test`.

**Impure modules** (`app.rs`, `sidebar.rs`, `row.rs`, `bridge.rs`, `x11/*`) — depend on GTK4/x11rb. Tested via VM E2E.

## x11rb API Notes

- `AtomEnum` variants (e.g. `AtomEnum::WM_CLASS`) can be passed directly to `get_property` — do NOT call `.into()` on them (causes ambiguous type inference)
- CSS: use `CssProvider::load_from_data(str)`, not `load_from_string` (unavailable in GTK 4.14)
- gtk4-rs 0.10 uses glib 0.21 and gio 0.21 (not 0.20 as originally planned)

## TDD Workflow

Every module gets tests first, then implementation. Pure modules use `cargo test`. Impure modules use VM E2E tests.

## Project Structure

See PLAN.md § Project Structure for the full layout.
