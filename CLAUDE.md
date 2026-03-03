# Process Tab Manager

Vertical sidebar for managing application windows on Linux/X11. See PLAN.md for full architecture.

## Tech Stack

- **Rust** with **GTK4-rs** (v0.10) for UI and **x11rb** (v0.13) for X11 window management
- Single-threaded: x11rb FD registered with GLib main loop via `glib::unix_fd_source_new`

## Build & Test

```bash
source "$HOME/.cargo/env"
cargo build                    # Build main binary
cargo test                     # Run pure module unit tests (no display needed)
cargo run --bin spike-x11      # Phase 0 spike: X11 window discovery
cargo run --bin spike-gtk      # Phase 0 spike: GTK4 ListBox
cargo run --bin spike-bridge   # Phase 0 spike: event loop integration
```

## Module Separation Rule

**Pure modules** (`state.rs`, `config.rs`, `geometry.rs`, `filter.rs`) — zero GTK/X11 imports. Tested with `cargo test`.

**Impure modules** (`app.rs`, `sidebar.rs`, `row.rs`, `bridge.rs`, `x11/*`) — depend on GTK4/x11rb. Tested via VM E2E.

## TDD Workflow

Every module gets tests first, then implementation. Pure modules use `cargo test`. Impure modules use VM E2E tests.

## Project Structure

See PLAN.md § Project Structure for the full layout.
