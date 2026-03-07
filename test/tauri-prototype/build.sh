#!/bin/bash
# Build the Tauri prototype for testing
# Run this inside the VM: cd /mnt/host-dev/process-tab-manager/test/tauri-prototype && bash build.sh

set -euo pipefail

cd "$(dirname "$0")"

# Create dist directory (frontend files served by Tauri)
rm -rf dist
mkdir -p dist/src
cp index.html dist/
cp src/main.js dist/src/

# The JS uses ES module import from @tauri-apps/api — for the prototype
# without a bundler, we need to inline the invoke function or use
# the built-in __TAURI__ global instead.
# Patch main.js to use the global __TAURI__ API instead of npm import
sed -i "s|import { invoke } from '@tauri-apps/api/core';|// Using __TAURI__ global (no bundler)|" dist/src/main.js

# Build Rust backend
cd src-tauri
source "$HOME/.cargo/env"
cargo build --release 2>&1 | tail -5

echo ""
echo "Build complete!"
echo "Binary: $(pwd)/target/release/tauri-prototype"
echo ""
echo "Run with: DISPLAY=:0 $(pwd)/target/release/tauri-prototype"
