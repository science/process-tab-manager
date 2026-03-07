#!/usr/bin/env bash
# Host-side E2E test orchestrator for Tauri-based PTM
# Usage: PTM_VM=ptm-test bash test/tauri-e2e-test.sh [test_name]

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
VM_NAME="${PTM_VM:-ptm-test}"
VM_PROJECT="/mnt/host-dev/process-tab-manager"
SCREENSHOT_DIR="$PROJECT_DIR/test/screenshots"

RED='\033[0;31m'
GREEN='\033[0;32m'
NC='\033[0m'

mkdir -p "$SCREENSHOT_DIR"

VM_SSH_OPTS="-o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR -o ConnectTimeout=5"

VM_IP=""

get_vm_ip() {
    if [[ -n "$VM_IP" ]]; then echo "$VM_IP"; return; fi
    VM_IP=$(virsh domifaddr "$VM_NAME" --source lease 2>/dev/null \
        | grep -oP '\d+\.\d+\.\d+\.\d+' | head -1)
    echo "$VM_IP"
}

vm_ssh() {
    local ip; ip=$(get_vm_ip)
    if [[ -z "$ip" ]]; then echo "Error: Cannot get IP for VM '$VM_NAME'" >&2; return 1; fi
    ssh $VM_SSH_OPTS "steve@$ip" "$@"
}

sync_xauth() {
    vm_ssh 'COOKIE=$(sudo xauth -f /var/run/lightdm/root/:0 list 2>/dev/null | head -1 | awk "{print \$3}"); HOST=$(hostname); if [ -n "$COOKIE" ]; then xauth remove "$HOST/unix:0" 2>/dev/null; xauth add "$HOST/unix:0" MIT-MAGIC-COOKIE-1 "$COOKIE"; fi' 2>/dev/null || true
}

desktop_alive() {
    vm_ssh "DISPLAY=:0 xprop -root _NET_CLIENT_LIST >/dev/null 2>&1" 2>/dev/null
}

vm_ensure_healthy() {
    local state
    state=$(virsh domstate "$VM_NAME" 2>/dev/null || echo "unknown")

    if [[ "$state" == "shut off" ]]; then
        echo "  Starting VM '$VM_NAME'..."
        virsh start "$VM_NAME" >/dev/null
        VM_IP=""
        sleep 15
    elif [[ "$state" != "running" ]]; then
        echo -e "${RED}ERROR: VM '$VM_NAME' in unexpected state: $state${NC}" >&2
        exit 1
    fi

    VM_IP=""
    local ip=""
    for i in $(seq 1 30); do
        VM_IP=""; ip=$(get_vm_ip)
        [[ -n "$ip" ]] && break
        sleep 2
    done
    [[ -z "$ip" ]] && { echo -e "${RED}ERROR: No IP after 60s${NC}" >&2; exit 1; }

    for i in $(seq 1 15); do vm_ssh "true" 2>/dev/null && break; sleep 2; done
    if ! vm_ssh "true" 2>/dev/null; then
        echo -e "${RED}ERROR: SSH not responding at $ip${NC}" >&2; exit 1
    fi

    sync_xauth; sleep 2

    if ! desktop_alive; then
        echo "  Desktop not ready — restarting LightDM..."
        vm_ssh "sudo systemctl restart lightdm" 2>/dev/null || true
        sleep 10; sync_xauth
        desktop_alive || { echo -e "${RED}ERROR: Desktop won't start${NC}" >&2; exit 1; }
    fi

    echo "  VM healthy: SSH + desktop OK (IP: $VM_IP)"
}

# ── Main ──

FILTER="${1:-}"

echo "=== PTM Tauri E2E Tests ==="
echo ""

# 1. VM preflight
vm_ensure_healthy

# 2. Prerequisites
echo "Checking prerequisites..."
if ! vm_ssh "command -v xdotool >/dev/null && command -v xterm >/dev/null && test -f $VM_PROJECT/src-tauri/Cargo.toml && DISPLAY=:0 xdpyinfo >/dev/null 2>&1" 2>/dev/null; then
    echo -e "${RED}ERROR: Prerequisites missing${NC}" >&2
    exit 1
fi
echo "  Prerequisites OK"

# 3. Build in VM (Tauri needs WebKitGTK at compile time)
echo ""
echo "Building Tauri PTM in VM..."
# Touch source files to invalidate stale virtiofs mtimes
vm_ssh "find $VM_PROJECT/src-tauri/src $VM_PROJECT/ptm-core/src -name '*.rs' -exec touch {} + 2>/dev/null" || true
if ! vm_ssh "cd $VM_PROJECT/src-tauri && source ~/.cargo/env && cargo build --release 2>&1 | tail -5"; then
    echo -e "${RED}Build failed!${NC}"
    exit 1
fi
echo "Build OK"

# 4. Run tests in VM
echo ""
echo "Running tests in VM..."
echo ""
vm_ssh "bash $VM_PROJECT/test/tauri-e2e-runner.sh $FILTER"
