#!/bin/bash
# Launch PTM for interactive use in the ptm-test VM.
# Handles: VM boot, desktop recovery, prerequisite install, build, launch,
# and opens the SPICE desktop viewer.
#
# Usage: ./run.sh

set -eo pipefail

VM_NAME="ptm-test"
VM_PROJECT="/mnt/host-dev/process-tab-manager"
SSH_OPTS="-o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR -o ConnectTimeout=5"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
BOLD='\033[1m'
NC='\033[0m'

step() { echo -e "\n${BOLD}==> $1${NC}"; }
ok()   { echo -e "    ${GREEN}$1${NC}"; }
warn() { echo -e "    ${YELLOW}$1${NC}"; }

get_vm_ip() {
    virsh domifaddr "$VM_NAME" --source lease 2>/dev/null \
        | grep -oP '\d+\.\d+\.\d+\.\d+' | head -1
}

vm_ssh() {
    ssh $SSH_OPTS "steve@$VM_IP" "$@"
}

sync_xauth() {
    vm_ssh 'COOKIE=$(sudo xauth -f /var/run/lightdm/root/:0 list 2>/dev/null | head -1 | awk "{print \$3}"); HOST=$(hostname); if [ -n "$COOKIE" ]; then xauth remove "$HOST/unix:0" 2>/dev/null; xauth add "$HOST/unix:0" MIT-MAGIC-COOKIE-1 "$COOKIE"; fi' 2>/dev/null || true
}

# Check if Cinnamon window manager is running (not just Xorg/greeter)
desktop_alive() {
    vm_ssh "DISPLAY=:0 xprop -root _NET_CLIENT_LIST >/dev/null 2>&1" 2>/dev/null
}

# ── 1. Start VM if not running ──

step "Checking VM..."
state=$(virsh domstate "$VM_NAME" 2>/dev/null || echo "unknown")

if [[ "$state" == "running" ]]; then
    ok "VM is already running"
elif [[ "$state" == "shut off" ]]; then
    echo "    Starting VM..."
    virsh start "$VM_NAME" >/dev/null
    ok "VM started"
else
    echo -e "${RED}Error: VM '$VM_NAME' is in state '$state'. Create it first.${NC}" >&2
    exit 1
fi

# ── 2. Wait for network + SSH ──

step "Waiting for network..."
VM_IP=""
for i in $(seq 1 30); do
    VM_IP=$(get_vm_ip)
    [[ -n "$VM_IP" ]] && break
    sleep 2
done

if [[ -z "$VM_IP" ]]; then
    echo -e "${RED}Error: Could not get VM IP after 60s${NC}" >&2
    exit 1
fi
ok "VM IP: $VM_IP"

for i in $(seq 1 15); do
    vm_ssh "true" 2>/dev/null && break
    sleep 2
done

if ! vm_ssh "true" 2>/dev/null; then
    echo -e "${RED}Error: SSH not responding${NC}" >&2
    exit 1
fi
ok "SSH is up"

# ── 3. Ensure desktop session is running ──

step "Checking desktop..."
sync_xauth
sleep 3

if desktop_alive; then
    ok "Desktop is up"
else
    warn "Desktop not ready — restarting LightDM to trigger autologin..."
    vm_ssh "sudo systemctl restart lightdm" 2>/dev/null || true
    sleep 10
    sync_xauth

    if desktop_alive; then
        ok "Desktop recovered"
    else
        # Last resort: full reboot
        warn "Still no desktop — rebooting VM..."
        vm_ssh "sudo reboot" 2>/dev/null || true
        sleep 35
        VM_IP=$(get_vm_ip)
        for i in $(seq 1 15); do
            vm_ssh "true" 2>/dev/null && break
            sleep 3
        done
        sync_xauth
        sleep 10

        if desktop_alive; then
            ok "Desktop recovered after reboot"
        else
            echo -e "${RED}Error: Desktop won't start. Check VM manually: virt-viewer --attach $VM_NAME${NC}" >&2
            exit 1
        fi
    fi
fi

# ── 4. Disable screensaver/lock (prevents black screen in SPICE viewer) ──

vm_ssh "DISPLAY=:0 dconf write /org/cinnamon/desktop/screensaver/lock-enabled false" 2>/dev/null || true
vm_ssh "DISPLAY=:0 dconf write /org/cinnamon/desktop/screensaver/idle-activation-enabled false" 2>/dev/null || true
vm_ssh "DISPLAY=:0 dconf write /org/cinnamon/desktop/session/idle-delay 'uint32 0'" 2>/dev/null || true
vm_ssh "DISPLAY=:0 xset s off -dpms" 2>/dev/null || true

# ── 5. Install prerequisites if missing ──

step "Checking prerequisites..."

if ! vm_ssh "source ~/.cargo/env 2>/dev/null && rustc --version" 2>/dev/null | grep -q rustc; then
    warn "Rust not found — installing..."
    vm_ssh "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y" 2>&1
    ok "Rust installed"
else
    ok "Rust OK"
fi

for pkg in libgtk-4-dev xdotool xterm imagemagick; do
    if ! vm_ssh "dpkg -l $pkg 2>/dev/null | grep -q ^ii"; then
        warn "$pkg not found — installing..."
        vm_ssh "sudo apt-get install -y $pkg" 2>&1
    fi
done
ok "System packages OK"

if ! vm_ssh "test -f $VM_PROJECT/Cargo.toml"; then
    echo -e "${RED}Error: $VM_PROJECT not mounted (virtiofs)${NC}" >&2
    exit 1
fi
ok "Project mount OK"

# ── 6. Build ──

step "Building PTM..."
vm_ssh "cd $VM_PROJECT && source ~/.cargo/env && cargo build --release" 2>&1
ok "Build complete"

# ── 7. Kill any existing PTM/xterms, launch fresh ──

step "Launching PTM..."
vm_ssh "pkill -f 'process.tab.manager' 2>/dev/null; pkill xterm 2>/dev/null; true" 2>/dev/null || true
sleep 1
vm_ssh "DISPLAY=:0 nohup $VM_PROJECT/target/release/process-tab-manager >/tmp/ptm.log 2>&1 &"
sleep 2

if vm_ssh "DISPLAY=:0 xdotool search --name 'Process Tab Manager'" 2>/dev/null | grep -q .; then
    ok "PTM is running"
else
    warn "PTM window not detected (may still be starting)"
fi

# ── 8. Open some test windows ──

step "Opening test xterms..."
for i in 1 2 3; do
    vm_ssh "DISPLAY=:0 nohup xterm -title 'xterm-$i' >/dev/null 2>&1 &"
done
sleep 1
ok "3 xterms opened"

# ── 9. Launch desktop viewer ──

step "Opening desktop viewer..."
virt-viewer --attach "$VM_NAME" &
disown
ok "SPICE viewer launched"

echo ""
echo -e "${GREEN}${BOLD}Ready!${NC} PTM is running with 3 xterm windows."
echo ""
echo "Things to try:"
echo "  - Click a row to focus + snap that window"
echo "  - Ctrl+click to focus without snapping"
echo "  - Double-click a row to rename it"
echo "  - Use arrow buttons to reorder"
echo "  - Open/close xterms and watch the list update"
echo ""
echo "To stop: pkill -f process.tab.manager (via SSH)"
echo "VM logs: ssh steve@$VM_IP 'cat /tmp/ptm.log'"
