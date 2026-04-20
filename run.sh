#!/bin/bash
set -e
cd "$(dirname "$0")"
source "$HOME/.cargo/env"
cargo build --release
DISPLAY=:0 /tmp/ptm-target/release/ptm "$@"
