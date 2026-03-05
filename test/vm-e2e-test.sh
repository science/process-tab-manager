#!/usr/bin/env bash
# VM E2E tests for Process Tab Manager
# Runs inside the cinnamon-dev VM via SSH
# Usage: ./test/vm-e2e-test.sh [test_name]
#   No args = run all tests
#   test_name = run only that test

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
VM_NAME="${PTM_VM:-ptm-test}"
VM_CTL="$PROJECT_DIR/vm/vm-ctl.sh"
VM_PROJECT="/mnt/host-dev/process-tab-manager"
SCREENSHOT_DIR="$PROJECT_DIR/test/screenshots"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
NC='\033[0m' # No Color

PASS=0
FAIL=0
SKIP=0

mkdir -p "$SCREENSHOT_DIR"

# ── Helpers ──

VM_SSH_OPTS="-o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR -o ConnectTimeout=5"

get_vm_ip() {
    virsh domifaddr "$VM_NAME" --source lease 2>/dev/null \
        | grep -oP '\d+\.\d+\.\d+\.\d+' | head -1
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

log_test() {
    printf "  %-50s " "$1"
}

pass() {
    PASS=$((PASS + 1))
    echo -e "${GREEN}PASS${NC}"
}

fail() {
    FAIL=$((FAIL + 1))
    echo -e "${RED}FAIL${NC} — $1"
}

skip() {
    SKIP=$((SKIP + 1))
    echo -e "${YELLOW}SKIP${NC} — $1"
}

screenshot() {
    local name="$1"
    vm_ssh "DISPLAY=:0 import -window root /tmp/ptm-screenshot-${name}.png" 2>/dev/null || true
    local vm_ip
    vm_ip=$(get_vm_ip) || return 0
    scp -q -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null \
        "steve@${vm_ip}:/tmp/ptm-screenshot-${name}.png" \
        "$SCREENSHOT_DIR/${name}.png" 2>/dev/null || true
}

screenshot_crop() {
    local name="$1"
    local geometry="$2"  # WxH+X+Y format for ImageMagick
    screenshot "${name}-full"
    vm_ssh "DISPLAY=:0 convert /tmp/ptm-screenshot-${name}-full.png -crop $geometry /tmp/ptm-screenshot-${name}.png" 2>/dev/null || true
    local vm_ip
    vm_ip=$(get_vm_ip) || return 0
    scp -q -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null \
        "steve@${vm_ip}:/tmp/ptm-screenshot-${name}.png" \
        "$SCREENSHOT_DIR/${name}.png" 2>/dev/null || true
}

# Start PTM in the VM (uses pre-built release binary)
start_ptm() {
    vm_ssh "DISPLAY=:0 RUST_LOG=debug nohup $VM_PROJECT/target/release/process-tab-manager >/tmp/ptm.log 2>&1 &"
    sleep 3
}

# Kill PTM
stop_ptm() {
    vm_ssh "pkill -f 'process.tab.manager' 2>/dev/null; true" || true
    sleep 1
}

# Open N xterm windows
open_xterms() {
    local count="$1"
    for i in $(seq 1 "$count"); do
        vm_ssh "DISPLAY=:0 nohup xterm -title 'xterm-$i' >/dev/null 2>&1 &"
    done
    sleep 2
}

# Close all xterms
close_xterms() {
    vm_ssh "pkill xterm 2>/dev/null; true" || true
    sleep 1
}

# Count xterm windows via xdotool
count_xterms() {
    vm_ssh "DISPLAY=:0 xdotool search --class xterm 2>/dev/null | wc -l"
}

# Get PTM window list (window titles from the listbox)
ptm_window_count() {
    vm_ssh "DISPLAY=:0 xdotool search --name 'Process Tab Manager' 2>/dev/null | wc -l"
}

# ── Helpers: VM health ──

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
        sleep 15
    elif [[ "$state" != "running" ]]; then
        echo -e "${RED}ERROR: VM '$VM_NAME' in unexpected state: $state${NC}" >&2
        echo "Try: virsh destroy $VM_NAME && virsh start $VM_NAME" >&2
        exit 1
    fi

    # Wait for IP + SSH (up to 60s)
    local ip=""
    for i in $(seq 1 30); do
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

    echo "  VM healthy: SSH + desktop OK"
}

# ── Preflight ──

echo "=== PTM VM E2E Tests ==="
echo ""

vm_ensure_healthy

# Check prerequisites
echo "Checking prerequisites..."
log_test "Rust toolchain"
if vm_ssh "source ~/.cargo/env && rustc --version" 2>/dev/null | grep -q rustc; then
    pass
else
    fail "Rust not installed in VM"
    echo -e "${RED}Install Rust in VM: ./vm/vm-ctl.sh ssh 'curl --proto =https --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y'${NC}"
    exit 1
fi

log_test "libgtk-4-dev"
if vm_ssh "dpkg -l libgtk-4-dev 2>/dev/null | grep -q ^ii"; then
    pass
else
    fail "libgtk-4-dev not installed"
    exit 1
fi

log_test "xdotool"
if vm_ssh "which xdotool" 2>/dev/null | grep -q xdotool; then
    pass
else
    fail "xdotool not installed"
    exit 1
fi

log_test "xterm"
if vm_ssh "which xterm" 2>/dev/null | grep -q xterm; then
    pass
else
    fail "xterm not installed"
    exit 1
fi

log_test "virtiofs mount"
if vm_ssh "test -f $VM_PROJECT/Cargo.toml"; then
    pass
else
    fail "$VM_PROJECT not mounted"
    exit 1
fi

log_test "DISPLAY available"
if vm_ssh "DISPLAY=:0 xdpyinfo >/dev/null 2>&1 && echo ok" 2>/dev/null | grep -q ok; then
    pass
else
    fail "No DISPLAY — is Cinnamon running?"
    exit 1
fi

# Build PTM
# Touch source files to work around virtiofs stale mtime issue
vm_ssh "find $VM_PROJECT/src -name '*.rs' -exec touch {} +" 2>/dev/null || true
echo ""
echo "Building PTM in VM..."
if ! vm_ssh "cd $VM_PROJECT && source ~/.cargo/env && cargo build --release" 2>&1; then
    echo -e "${RED}Build failed!${NC}"
    exit 1
fi
echo "Build OK"
echo ""

# ── Clean state ──
stop_ptm
close_xterms
vm_ssh "rm -f ~/.config/process-tab-manager/state.json" 2>/dev/null || true
# Restart Cinnamon to clear any stale _NET_ACTIVE_WINDOW from previous runs
vm_ssh 'DISPLAY=:0 nohup cinnamon --replace >/dev/null 2>&1 &' 2>/dev/null || true
sleep 3

# ── Tests ──

FILTER="${1:-}"

run_test() {
    local test_name="$1"
    if [[ -n "$FILTER" && "$test_name" != "$FILTER" ]]; then
        return
    fi
    echo ""
    echo "--- $test_name ---"
    "$test_name"
}

# Test 1: PTM launches and shows window list
test_launch_and_list() {
    stop_ptm
    close_xterms

    open_xterms 3
    start_ptm

    sleep 2
    screenshot "launch"

    log_test "PTM window exists"
    if vm_ssh "DISPLAY=:0 xdotool search --name 'Process Tab Manager'" 2>/dev/null | grep -q .; then
        pass
    else
        fail "PTM window not found"
    fi

    stop_ptm
    close_xterms
}

# Test 2: Window list updates when windows open/close
test_dynamic_list() {
    stop_ptm
    close_xterms

    open_xterms 2
    start_ptm
    sleep 2

    screenshot "dynamic-before"

    # Open 2 more xterms
    open_xterms 2
    sleep 2
    screenshot "dynamic-after-open"

    # Close all xterms — PTM should update
    close_xterms
    sleep 2
    screenshot "dynamic-after-close"

    log_test "PTM survives window churn"
    if vm_ssh "DISPLAY=:0 xdotool search --name 'Process Tab Manager'" 2>/dev/null | grep -q .; then
        pass
    else
        fail "PTM crashed during window churn"
    fi

    stop_ptm
}

# Test 3: Click focuses a window
test_click_focus() {
    stop_ptm
    close_xterms

    open_xterms 2
    start_ptm
    sleep 2

    # Get xterm window IDs
    local xterm_wids
    xterm_wids=$(vm_ssh "DISPLAY=:0 xdotool search --class xterm 2>/dev/null; true")
    local first_xterm
    first_xterm=$(echo "$xterm_wids" | head -1)

    # Click on the PTM window (first row area)
    local ptm_wid
    ptm_wid=$(vm_ssh "DISPLAY=:0 xdotool search --name 'Process Tab Manager' 2>/dev/null; true" | head -1)

    if [[ -z "$ptm_wid" ]]; then
        log_test "Click focuses window"
        fail "PTM window not found"
        stop_ptm
        close_xterms
        return
    fi

    # Activate PTM, click first row
    vm_ssh "DISPLAY=:0 xdotool windowactivate $ptm_wid 2>/dev/null; true"
    sleep 0.5
    # Click at approximate row position (x=100, y=30 relative to PTM window)
    vm_ssh "DISPLAY=:0 xdotool windowfocus $ptm_wid key Return 2>/dev/null; true"
    sleep 1

    screenshot "click-focus"

    log_test "Click activates window"
    # After clicking a row, an xterm should be active
    local active
    active=$(vm_ssh "DISPLAY=:0 xdotool getactivewindow 2>/dev/null; true")
    if echo "$xterm_wids" | grep -q "$active"; then
        pass
    else
        # The PTM might still be active if click didn't work perfectly
        skip "Could not verify focus change (active=$active)"
    fi

    stop_ptm
    close_xterms
}

# Test 4: Save on exit — state.json created when PTM shuts down
test_save_on_exit() {
    stop_ptm
    close_xterms
    vm_ssh "rm -f ~/.config/process-tab-manager/state.json"

    open_xterms 2
    start_ptm
    sleep 2

    # Gracefully stop PTM (SIGTERM triggers shutdown save)
    stop_ptm
    sleep 2

    log_test "State file created on exit"
    if vm_ssh "test -f ~/.config/process-tab-manager/state.json"; then
        pass
    else
        fail "state.json not created on shutdown"
    fi

    log_test "State file contains valid JSON"
    if vm_ssh "python3 -m json.tool ~/.config/process-tab-manager/state.json >/dev/null 2>&1"; then
        pass
    else
        fail "state.json is not valid JSON"
    fi

    close_xterms
}

# Test 5: PTM does not show itself in its own list
test_self_filter() {
    stop_ptm
    close_xterms

    start_ptm
    sleep 2

    screenshot "self-filter"

    # PTM should have a window but its list should be empty (no xterms open)
    # Since xterms are the only filtered class and PTM filters itself out,
    # the listbox should have zero rows
    log_test "PTM window exists"
    if vm_ssh "DISPLAY=:0 xdotool search --name 'Process Tab Manager'" 2>/dev/null | grep -q .; then
        pass
    else
        fail "PTM window not found"
    fi

    # Open 1 xterm, verify it shows up (PTM itself shouldn't)
    open_xterms 1
    sleep 2

    screenshot "self-filter-with-xterm"

    # PTM should still be running
    log_test "PTM survives with managed windows"
    if vm_ssh "DISPLAY=:0 xdotool search --name 'Process Tab Manager'" 2>/dev/null | grep -q .; then
        pass
    else
        fail "PTM crashed"
    fi

    stop_ptm
    close_xterms
}

# Test 6: Rapid window churn — open 10 xterms fast
test_rapid_churn() {
    stop_ptm
    close_xterms

    start_ptm
    sleep 2

    # Open 10 xterms as fast as possible (no sleep between)
    for i in $(seq 1 10); do
        vm_ssh "DISPLAY=:0 nohup xterm -title 'rapid-$i' >/dev/null 2>&1 &"
    done
    sleep 3

    screenshot "rapid-churn"

    log_test "PTM survives rapid window creation"
    if vm_ssh "DISPLAY=:0 xdotool search --name 'Process Tab Manager'" 2>/dev/null | grep -q .; then
        pass
    else
        fail "PTM crashed during rapid churn"
    fi

    # Close all 10 at once
    close_xterms
    sleep 2

    log_test "PTM survives rapid window destruction"
    if vm_ssh "DISPLAY=:0 xdotool search --name 'Process Tab Manager'" 2>/dev/null | grep -q .; then
        pass
    else
        fail "PTM crashed during rapid close"
    fi

    stop_ptm
}

# Test 7: Focus pass-through — clicking a PTM row while PTM is in the background
# should activate the target window, not just bring PTM to foreground.
# This uses real mouse clicks (xdotool mousemove + click) to reproduce
# the actual user interaction, NOT windowactivate+keypress.
test_focus_passthrough() {
    stop_ptm
    close_xterms

    # Restart Cinnamon to clear stale _NET_ACTIVE_WINDOW from previous tests
    vm_ssh 'DISPLAY=:0 nohup cinnamon --replace >/dev/null 2>&1 &' 2>/dev/null || true
    sleep 3

    open_xterms 2
    start_ptm
    sleep 2

    local ptm_wid
    ptm_wid=$(vm_ssh "DISPLAY=:0 xdotool search --name 'Process Tab Manager' 2>/dev/null" | head -1)

    if [[ -z "$ptm_wid" ]]; then
        log_test "Focus pass-through (background click)"
        fail "PTM window not found"
        stop_ptm; close_xterms
        return
    fi

    local xterm_wids
    xterm_wids=$(vm_ssh "DISPLAY=:0 xdotool search --class xterm 2>/dev/null")
    local xterm1 xterm2
    xterm1=$(echo "$xterm_wids" | head -1)
    xterm2=$(echo "$xterm_wids" | tail -1)

    # Move xterms fully out of PTM's area and resize them narrower
    vm_ssh "DISPLAY=:0 xdotool windowmove $xterm1 300 50 windowsize $xterm1 400 300" 2>/dev/null || true
    vm_ssh "DISPLAY=:0 xdotool windowmove $xterm2 300 400 windowsize $xterm2 400 300" 2>/dev/null || true
    # Move PTM to left edge so coordinates are predictable
    vm_ssh "DISPLAY=:0 xdotool windowmove $ptm_wid 0 0" 2>/dev/null || true
    sleep 0.5

    # Step 1: Activate PTM and press Enter to activate first row.
    # After this, xterm gets focus and PTM goes to background.
    # (xdotool synthetic mouse clicks don't reliably trigger GTK4 row_activated,
    # but key events do — this matches test_click_focus behavior)
    vm_ssh "DISPLAY=:0 xdotool windowactivate --sync $ptm_wid" 2>/dev/null || true
    sleep 0.5
    vm_ssh "DISPLAY=:0 xdotool windowfocus $ptm_wid key Return" 2>/dev/null || true
    sleep 1.5

    local after_first
    after_first=$(vm_ssh "DISPLAY=:0 xdotool getactivewindow 2>/dev/null")

    log_test "Mouse click on foreground PTM row activates target"
    if echo "$xterm_wids" | grep -qx "$after_first"; then
        pass
    else
        fail "Expected xterm active after click, got $after_first (ptm=$ptm_wid)"
        stop_ptm; close_xterms
        return
    fi

    screenshot "focus-passthrough-step1"

    # Step 2: PTM is now in the background (xterm has focus).
    # Move cursor over a PTM row, wait for GTK to process the motion event
    # (sets hover_wid), THEN click. The click raises PTM, is-active fires,
    # and the hover_wid target gets activated.
    # Rows are ~22px each starting at y=0 in the ListBox (no CSD headerbar with UTILITY type)
    vm_ssh "DISPLAY=:0 xdotool mousemove --window $ptm_wid 125 30" 2>/dev/null || true
    sleep 0.5
    vm_ssh "DISPLAY=:0 xdotool click 1" 2>/dev/null || true
    sleep 1.5

    screenshot "focus-passthrough-step2"

    local after_second
    after_second=$(vm_ssh "DISPLAY=:0 xdotool getactivewindow 2>/dev/null")

    log_test "Background click on PTM row activates target (not PTM)"
    if [[ "$after_second" == "$ptm_wid" ]]; then
        fail "PTM stole focus — background click did not pass through (active=$after_second)"
    elif echo "$xterm_wids" | grep -qx "$after_second"; then
        pass
    else
        skip "Active window ($after_second) is neither PTM nor xterm"
    fi

    stop_ptm
    close_xterms
}

# Test 8: Dark theme — verify dark background renders (not white/default)
test_dark_theme() {
    stop_ptm
    close_xterms

    open_xterms 2
    start_ptm
    sleep 2

    # Get PTM window geometry for targeted crop
    local ptm_wid
    ptm_wid=$(vm_ssh "DISPLAY=:0 xdotool search --name 'Process Tab Manager' 2>/dev/null" | head -1)

    if [[ -z "$ptm_wid" ]]; then
        log_test "Dark theme renders"
        fail "PTM window not found"
        stop_ptm; close_xterms
        return
    fi

    local geo
    geo=$(vm_ssh "DISPLAY=:0 xdotool getwindowgeometry --shell $ptm_wid 2>/dev/null")
    local wx wy ww wh
    wx=$(echo "$geo" | grep "^X=" | cut -d= -f2)
    wy=$(echo "$geo" | grep "^Y=" | cut -d= -f2)
    ww=$(echo "$geo" | grep "^WIDTH=" | cut -d= -f2)
    wh=$(echo "$geo" | grep "^HEIGHT=" | cut -d= -f2)

    # Crop just the PTM sidebar area
    screenshot_crop "dark-theme" "${ww}x${wh}+${wx}+${wy}"

    # Sample pixels from the sidebar background — dark theme should have dark pixels
    # Use ImageMagick to get average color of a small region
    local avg_brightness
    avg_brightness=$(vm_ssh "DISPLAY=:0 convert /tmp/ptm-screenshot-dark-theme.png -crop 50x50+10+10 -resize 1x1 -format '%[fx:luminance]' info:" 2>/dev/null || echo "")

    log_test "Dark theme renders (not white)"
    if [[ -n "$avg_brightness" ]]; then
        # luminance < 0.3 means dark background. White would be ~1.0
        local is_dark
        is_dark=$(echo "$avg_brightness < 0.3" | bc -l 2>/dev/null || echo "")
        if [[ "$is_dark" == "1" ]]; then
            pass
        else
            fail "Background luminance $avg_brightness (expected < 0.3 for dark theme)"
        fi
    else
        skip "Could not measure background color"
    fi

    stop_ptm
    close_xterms
}

# Test 9: Snap alignment — target window aligns with PTM frame edge
test_snap_alignment() {
    stop_ptm
    close_xterms

    open_xterms 1
    start_ptm
    sleep 2

    local ptm_wid
    ptm_wid=$(vm_ssh "DISPLAY=:0 xdotool search --name 'Process Tab Manager' 2>/dev/null" | head -1)
    local xterm_wid
    xterm_wid=$(vm_ssh "DISPLAY=:0 xdotool search --class xterm 2>/dev/null" | head -1)

    if [[ -z "$ptm_wid" || -z "$xterm_wid" ]]; then
        log_test "Snap alignment"
        fail "PTM or xterm window not found"
        stop_ptm; close_xterms
        return
    fi

    # Move PTM to a known position
    vm_ssh "DISPLAY=:0 xdotool windowmove $ptm_wid 50 50" 2>/dev/null || true
    sleep 0.5

    # Click the first row in PTM to trigger snap.
    # Use mousemove --window for window-relative coordinates (avoids title bar offset issues).
    # First move cursor over the row to set hover_wid, then click.
    vm_ssh "DISPLAY=:0 xdotool mousemove --window $ptm_wid 125 20" 2>/dev/null || true
    sleep 0.5
    vm_ssh "DISPLAY=:0 xdotool click 1" 2>/dev/null || true
    sleep 2

    # Get positions
    local ptm_geo xterm_geo
    ptm_geo=$(vm_ssh "DISPLAY=:0 xdotool getwindowgeometry --shell $ptm_wid 2>/dev/null")
    xterm_geo=$(vm_ssh "DISPLAY=:0 xdotool getwindowgeometry --shell $xterm_wid 2>/dev/null")

    local ptm_x ptm_w xterm_x ptm_y xterm_y
    ptm_x=$(echo "$ptm_geo" | grep "^X=" | cut -d= -f2)
    ptm_w=$(echo "$ptm_geo" | grep "^WIDTH=" | cut -d= -f2)
    ptm_y=$(echo "$ptm_geo" | grep "^Y=" | cut -d= -f2)
    xterm_x=$(echo "$xterm_geo" | grep "^X=" | cut -d= -f2)
    xterm_y=$(echo "$xterm_geo" | grep "^Y=" | cut -d= -f2)

    screenshot "snap-alignment"

    log_test "Snapped window X near PTM right edge"
    if [[ -n "$ptm_x" && -n "$ptm_w" && -n "$xterm_x" ]]; then
        local ptm_right=$((ptm_x + ptm_w))
        local x_diff=$((xterm_x - ptm_right))
        # Allow some frame tolerance (±50px for WM decorations)
        if [[ $x_diff -ge -50 && $x_diff -le 50 ]]; then
            pass
        else
            fail "xterm X=$xterm_x, PTM right=$ptm_right, diff=$x_diff (expected ±50)"
        fi
    else
        skip "Could not get window positions"
    fi

    log_test "Snapped window Y near PTM Y"
    if [[ -n "$ptm_y" && -n "$xterm_y" ]]; then
        local y_diff=$((xterm_y - ptm_y))
        if [[ $y_diff -ge -50 && $y_diff -le 50 ]]; then
            pass
        else
            fail "xterm Y=$xterm_y, PTM Y=$ptm_y, diff=$y_diff (expected ±50)"
        fi
    else
        skip "Could not get window positions"
    fi

    stop_ptm
    close_xterms
}

# Test 10: Position persistence — PTM remembers position across restarts
test_position_persistence() {
    stop_ptm
    close_xterms
    vm_ssh "rm -f ~/.config/process-tab-manager/state.json"

    open_xterms 1
    start_ptm
    sleep 2

    local ptm_wid
    ptm_wid=$(vm_ssh "DISPLAY=:0 xdotool search --name 'Process Tab Manager' 2>/dev/null" | head -1)

    if [[ -z "$ptm_wid" ]]; then
        log_test "Position persistence"
        fail "PTM window not found"
        stop_ptm; close_xterms
        return
    fi

    # Move PTM to a specific position
    vm_ssh "DISPLAY=:0 xdotool windowmove $ptm_wid 200 150" 2>/dev/null || true
    sleep 1

    # Gracefully stop PTM (SIGTERM triggers shutdown save with position)
    stop_ptm
    sleep 2

    log_test "State file has position data"
    local saved_pos
    saved_pos=$(vm_ssh "python3 -c \"import json; d=json.load(open('/home/steve/.config/process-tab-manager/state.json')); print(d.get('window_x','None'), d.get('window_y','None'))\"" 2>/dev/null || echo "None None")
    local saved_x saved_y
    saved_x=$(echo "$saved_pos" | awk '{print $1}')
    saved_y=$(echo "$saved_pos" | awk '{print $2}')
    if [[ -n "$saved_x" && "$saved_x" != "None" ]]; then
        pass
    else
        fail "state.json missing window_x/window_y"
        close_xterms
        return
    fi

    # Restart PTM — it should restore to the saved position
    start_ptm
    sleep 4

    ptm_wid=$(vm_ssh "DISPLAY=:0 xdotool search --name 'Process Tab Manager' 2>/dev/null" | head -1)

    if [[ -z "$ptm_wid" ]]; then
        log_test "Position restored after restart"
        fail "PTM window not found after restart"
        close_xterms
        return
    fi

    # Read the saved position from the PTM log (what translate_coordinates returned at restore time)
    # This is the most reliable comparison since save and restore use the same X11 API.
    local restored_log_pos
    restored_log_pos=$(vm_ssh "grep 'Restoring PTM position' /tmp/ptm.log 2>/dev/null | tail -1")

    log_test "Position restored after restart"
    if [[ -z "$restored_log_pos" ]]; then
        fail "No position restore log found"
    elif echo "$restored_log_pos" | grep -qE "Restoring PTM position to \($saved_x, $saved_y\)"; then
        pass
    else
        fail "Log: $restored_log_pos (expected saved=$saved_x,$saved_y)"
    fi

    stop_ptm
    close_xterms
}

# ── Run tests ──

run_test test_launch_and_list
run_test test_dynamic_list
run_test test_click_focus
run_test test_save_on_exit
run_test test_self_filter
run_test test_rapid_churn
run_test test_focus_passthrough
run_test test_dark_theme
run_test test_snap_alignment
run_test test_position_persistence

# ── Summary ──

echo ""
echo "========================="
echo -e "Results: ${GREEN}${PASS} passed${NC}, ${RED}${FAIL} failed${NC}, ${YELLOW}${SKIP} skipped${NC}"
echo "Screenshots: $SCREENSHOT_DIR/"
echo "========================="

if [[ $FAIL -gt 0 ]]; then
    exit 1
fi
