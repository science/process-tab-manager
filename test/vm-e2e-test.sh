#!/usr/bin/env bash
# Host-side E2E test orchestrator for Process Tab Manager
# Handles: VM preflight, host-side build, then delegates all test logic
# to vm-e2e-runner.sh running inside the VM (eliminates SSH-per-action overhead).
# Usage: PTM_VM=ptm-test bash test/vm-e2e-test.sh [test_name]

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
VM_NAME="${PTM_VM:-ptm-test}"
VM_PROJECT="/mnt/host-dev/process-tab-manager"
SCREENSHOT_DIR="$PROJECT_DIR/test/screenshots"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
NC='\033[0m'

mkdir -p "$SCREENSHOT_DIR"

# ── Helpers ──

VM_SSH_OPTS="-o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR -o ConnectTimeout=5"

# Resolve VM IP once and cache it
VM_IP=""

get_vm_ip() {
    if [[ -n "$VM_IP" ]]; then
        echo "$VM_IP"
        return
    fi
    VM_IP=$(virsh domifaddr "$VM_NAME" --source lease 2>/dev/null \
        | grep -oP '\d+\.\d+\.\d+\.\d+' | head -1)
    echo "$VM_IP"
}

vm_ssh() {
    local ip
    ip=$(get_vm_ip)
    if [[ -z "$ip" ]]; then
        echo "Error: Cannot get IP for VM '$VM_NAME'" >&2
        return 1
    fi
    ssh $VM_SSH_OPTS "steve@$ip" "$@"
}

# ── VM health check ──

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
        VM_IP=""  # Clear cached IP
        sleep 15
    elif [[ "$state" != "running" ]]; then
        echo -e "${RED}ERROR: VM '$VM_NAME' in unexpected state: $state${NC}" >&2
        echo "Try: virsh destroy $VM_NAME && virsh start $VM_NAME" >&2
        exit 1
    fi

    # Wait for IP + SSH (up to 60s)
    VM_IP=""  # Force re-resolve
    local ip=""
    for i in $(seq 1 30); do
        VM_IP=""
        ip=$(get_vm_ip)
        [[ -n "$ip" ]] && break
        sleep 2
    done
    if [[ -z "$ip" ]]; then
        echo -e "${RED}ERROR: No IP after 60s. Try: virsh destroy $VM_NAME && sudo bash vm/fix-clone-network.sh $VM_NAME && virsh start $VM_NAME${NC}" >&2
        exit 1
    fi

    for i in $(seq 1 15); do
        vm_ssh "true" 2>/dev/null && break
        sleep 2
    done
    if ! vm_ssh "true" 2>/dev/null; then
        echo -e "${RED}ERROR: SSH not responding at $ip${NC}" >&2
        exit 1
    fi

    # Sync Xauthority (cloned/reverted VMs can have stale cookies)
    sync_xauth
    sleep 2

    # Verify Cinnamon desktop is running
    if ! desktop_alive; then
        echo "  Desktop not ready — restarting LightDM..."
        vm_ssh "sudo systemctl restart lightdm" 2>/dev/null || true
        sleep 10
        sync_xauth

        if ! desktop_alive; then
            echo -e "${RED}ERROR: Cinnamon desktop won't start. VM may need re-creation:${NC}" >&2
            echo "  virsh destroy $VM_NAME; virsh undefine $VM_NAME --remove-all-storage" >&2
            echo "  sudo bash vm/clone-vm.sh $VM_NAME --ram 4096 --cpus 2 --mount /home/steve/dev:devmount" >&2
            exit 1
        fi
    fi

    echo "  VM healthy: SSH + desktop OK (IP: $VM_IP)"
}

# ── Main ──

FILTER="${1:-}"

echo "=== PTM VM E2E Tests ==="
echo ""

# 1. VM preflight
vm_ensure_healthy

# 2. Quick prerequisite check (single SSH call)
echo "Checking prerequisites..."
if ! vm_ssh "command -v xdotool >/dev/null && command -v xterm >/dev/null && test -f $VM_PROJECT/Cargo.toml && DISPLAY=:0 xdpyinfo >/dev/null 2>&1" 2>/dev/null; then
    echo -e "${RED}ERROR: Prerequisites missing (xdotool, xterm, virtiofs mount, or DISPLAY)${NC}" >&2
    echo "Run individual checks to diagnose:" >&2
    echo "  vm_ssh 'command -v xdotool && command -v xterm && test -f $VM_PROJECT/Cargo.toml && DISPLAY=:0 xdpyinfo'" >&2
    exit 1
fi
echo "  Prerequisites OK"

# 3. Build on HOST (much faster than in-VM via virtiofs)
echo ""
echo "Building PTM on host..."
if ! (source "$HOME/.cargo/env" && cd "$PROJECT_DIR" && cargo build --release) 2>&1; then
    echo -e "${RED}Build failed!${NC}"
    exit 1
fi
echo "Build OK"

# 4. Run all tests via single SSH call to in-VM runner
echo ""
echo "Running tests in VM..."
echo ""
vm_ssh "bash $VM_PROJECT/test/vm-e2e-runner.sh $FILTER"
