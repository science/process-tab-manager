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

| Item | Why | Install command |
|---|---|---|
| `xdotool` | Synthetic key/mouse input for live UAT of rename UX | `sudo apt-get install -y xdotool` |
| `scrot` | Screenshot capture for visual verification of rename selection rendering | `sudo apt-get install -y scrot` |
| `wmctrl` | Listing windows by class/PID; complements xdotool | `sudo apt-get install -y wmctrl` |

**Apt note:** the VM's apt-cacher proxy at `10.70.144.1:3142` was returning
500/503 errors. Worked around with `sudo apt-get -o Acquire::http::Proxy=false install ...`. If a future bootstrap script hits the same issue, add the same flag.

## Build verification

- `CARGO_TARGET_DIR=/tmp/ptm-dev cargo test` — 110 tests pass at session start; 166 after Cluster 1 work.
- `CARGO_TARGET_DIR=/tmp/ptm-dev cargo build --release` — builds clean.
- Runtime smoke: see session notes below.

## Bootstrap-script suggestions

Suggested updates to `install.sh` so a fresh dev VM works out of the box:

1. Ensure `libx11-dev`, `libxcb-render0-dev`, `libxcb-screensaver0-dev` are
   listed (they were already present on dev-2 — confirm install.sh covers
   them for fresh installs).
2. For dev/UAT specifically (NOT for end-user install), document the
   xdotool/scrot/wmctrl trio as optional dev-time tools.
3. Detect and warn if `Acquire::http::Proxy` is set to a broken cache; fall
   back to direct fetch with the override flag above.

