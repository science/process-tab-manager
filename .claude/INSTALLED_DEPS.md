# Installed dependencies on dev-2 VM

Tracking what was already installed and what I needed to add to get PTM building/testable on this VM. Useful for updating `install.sh` and any future bootstrap scripts.

## Pre-existing on dev-2 (no action needed)

| Item | Version | Notes |
|---|---|---|
| Rust toolchain | rustc 1.94.1 / cargo 1.94.1 | Installed under `~/.cargo`. Need `source ~/.cargo/env` to expose in fresh shells. |
| `libx11-dev` | 2:1.8.7-1build1 | X11 client-side dev headers. |
| `libxcb-render0-dev` | 1.15-1ubuntu2 | XCB render dev. |
| `libxcb-screensaver0-dev` | 1.15-1ubuntu2 | (in same install set) |
| X11 server on `:0` | — | DISPLAY=:0 reachable from this shell. |

## Installed during this session

(none yet)

## Build verification

- `CARGO_TARGET_DIR=/tmp/ptm-dev cargo test` — 110 tests pass at session start.
- `CARGO_TARGET_DIR=/tmp/ptm-dev cargo build --release` — builds clean.
- Runtime smoke: see notes below.
