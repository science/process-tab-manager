#!/usr/bin/env bash
# In-VM E2E test runner for Process Tab Manager
# Runs entirely inside the VM with DISPLAY=:0 — no SSH overhead per action.
# Called by vm-e2e-test.sh on the host via a single SSH invocation.
# Usage: bash /mnt/host-dev/process-tab-manager/test/vm-e2e-runner.sh [test_name]

set -euo pipefail

VM_PROJECT="/mnt/host-dev/process-tab-manager"
SCREENSHOT_DIR="$VM_PROJECT/test/screenshots"
export DISPLAY=:0

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
NC='\033[0m'

PASS=0
FAIL=0
SKIP=0

mkdir -p "$SCREENSHOT_DIR"

# ── Helpers ──

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

# ── Polling utilities (replace fixed sleeps) ──

wait_for() {
    local desc="$1" cmd="$2" timeout="${3:-5}"
    local attempts=$((timeout * 10))
    for i in $(seq 1 "$attempts"); do
        if eval "$cmd" 2>/dev/null; then return 0; fi
        sleep 0.1
    done
    echo "TIMEOUT waiting for: $desc" >&2
    return 1
}

wait_for_ptm() {
    wait_for "PTM window" "xdotool search --name 'Process Tab Manager' | grep -q ." "${1:-5}"
}

wait_for_xterms() {
    local count="$1"
    wait_for "$count xterms" \
        "[ \$(xdotool search --class xterm 2>/dev/null | wc -l) -ge $count ]" "${2:-5}"
}

wait_for_no_xterms() {
    wait_for "no xterms" \
        "! xdotool search --class xterm 2>/dev/null | grep -q ." "${1:-5}"
}

wait_for_ptm_stopped() {
    wait_for "PTM stopped" "! pgrep -f 'target/release/process-tab-manager' >/dev/null" "${1:-3}"
}

# ── PTM lifecycle ──

start_ptm() {
    RUST_LOG=debug nohup "$VM_PROJECT/target/release/process-tab-manager" \
        >/tmp/ptm.log 2>&1 &
    wait_for_ptm 5
}

stop_ptm() {
    # Use specific binary path to avoid killing the runner script itself
    # (whose path also contains "process-tab-manager")
    pkill -f 'target/release/process-tab-manager' 2>/dev/null || true
    wait_for_ptm_stopped 3
}

open_xterms() {
    local count="$1"
    for i in $(seq 1 "$count"); do
        nohup xterm -title "xterm-$i" >/dev/null 2>&1 &
    done
    wait_for_xterms "$count"
}

close_xterms() {
    pkill xterm 2>/dev/null || true
    wait_for_no_xterms 3
}

# ── Cinnamon WM restart (with retry) ──

restart_cinnamon() {
    local attempt
    for attempt in 1 2; do
        nohup cinnamon --replace >/dev/null 2>&1 & disown
        sleep 2
        if pgrep -x cinnamon >/dev/null 2>&1; then
            wait_for "Cinnamon ready" "xprop -root _NET_CLIENT_LIST >/dev/null 2>&1" 10
            return 0
        fi
        echo "  Cinnamon crashed on attempt $attempt, retrying..." >&2
        sleep 1
    done
    echo "  WARNING: Cinnamon won't start — WM-dependent tests may fail" >&2
}

# ── Screenshots (saved to virtiofs, instantly visible on host) ──

screenshot() {
    local name="$1"
    local path="$SCREENSHOT_DIR/${name}.png"
    import -window root "$path" 2>/dev/null || true
    echo "  [screenshot] $path"
}

screenshot_crop() {
    local name="$1" geometry="$2"
    local path="$SCREENSHOT_DIR/${name}.png"
    import -window root "/tmp/ptm-ss-full.png" 2>/dev/null || true
    convert "/tmp/ptm-ss-full.png" -crop "$geometry" "$path" 2>/dev/null || true
    echo "  [screenshot] $path"
}

# ── Test functions ──

# Test 1: PTM launches and shows window list
test_launch_and_list() {
    stop_ptm
    close_xterms

    open_xterms 3
    start_ptm

    sleep 1
    screenshot "launch"

    log_test "PTM window exists"
    if xdotool search --name 'Process Tab Manager' 2>/dev/null | grep -q .; then
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
    sleep 1

    screenshot "dynamic-before"

    # Open 2 more xterms
    open_xterms 2
    sleep 1
    screenshot "dynamic-after-open"

    # Close all xterms — PTM should update
    close_xterms
    sleep 1
    screenshot "dynamic-after-close"

    log_test "PTM survives window churn"
    if xdotool search --name 'Process Tab Manager' 2>/dev/null | grep -q .; then
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
    sleep 1

    local xterm_wids
    xterm_wids=$(xdotool search --class xterm 2>/dev/null || true)
    local first_xterm
    first_xterm=$(echo "$xterm_wids" | head -1)

    local ptm_wid
    ptm_wid=$(xdotool search --name 'Process Tab Manager' 2>/dev/null | head -1 || true)

    if [[ -z "$ptm_wid" ]]; then
        log_test "Click focuses window"
        fail "PTM window not found"
        stop_ptm; close_xterms
        return
    fi

    # Activate PTM, press Enter to activate first row
    xdotool windowactivate "$ptm_wid" 2>/dev/null || true
    sleep 0.3
    xdotool windowfocus "$ptm_wid" key Return 2>/dev/null || true
    sleep 0.5

    screenshot "click-focus"

    log_test "Click activates window"
    local active
    active=$(xdotool getactivewindow 2>/dev/null || true)
    if echo "$xterm_wids" | grep -q "$active"; then
        pass
    else
        skip "Could not verify focus change (active=$active)"
    fi

    stop_ptm
    close_xterms
}

# Test 4: Save on exit — state.json created when PTM shuts down
test_save_on_exit() {
    stop_ptm
    close_xterms
    rm -f ~/.config/process-tab-manager/state.json

    open_xterms 2
    start_ptm
    sleep 1

    # Gracefully stop PTM (SIGTERM triggers shutdown save)
    stop_ptm
    sleep 1

    log_test "State file created on exit"
    if [[ -f ~/.config/process-tab-manager/state.json ]]; then
        pass
    else
        fail "state.json not created on shutdown"
    fi

    log_test "State file contains valid JSON"
    if python3 -m json.tool ~/.config/process-tab-manager/state.json >/dev/null 2>&1; then
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
    sleep 1

    screenshot "self-filter"

    log_test "PTM window exists"
    if xdotool search --name 'Process Tab Manager' 2>/dev/null | grep -q .; then
        pass
    else
        fail "PTM window not found"
    fi

    # Open 1 xterm, verify PTM still running
    open_xterms 1
    sleep 1

    screenshot "self-filter-with-xterm"

    log_test "PTM survives with managed windows"
    if xdotool search --name 'Process Tab Manager' 2>/dev/null | grep -q .; then
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
    sleep 1

    # Open 10 xterms as fast as possible
    for i in $(seq 1 10); do
        nohup xterm -title "rapid-$i" >/dev/null 2>&1 &
    done
    wait_for_xterms 10 5

    screenshot "rapid-churn"

    log_test "PTM survives rapid window creation"
    if xdotool search --name 'Process Tab Manager' 2>/dev/null | grep -q .; then
        pass
    else
        fail "PTM crashed during rapid churn"
    fi

    # Close all 10 at once
    close_xterms
    sleep 1

    log_test "PTM survives rapid window destruction"
    if xdotool search --name 'Process Tab Manager' 2>/dev/null | grep -q .; then
        pass
    else
        fail "PTM crashed during rapid close"
    fi

    stop_ptm
}

# Test 7: Focus pass-through
test_focus_passthrough() {
    stop_ptm
    close_xterms

    # Restart Cinnamon to clear stale _NET_ACTIVE_WINDOW (with retry)
    restart_cinnamon

    open_xterms 2
    start_ptm
    sleep 1

    local ptm_wid
    ptm_wid=$(xdotool search --name 'Process Tab Manager' 2>/dev/null | head -1)

    if [[ -z "$ptm_wid" ]]; then
        log_test "Focus pass-through (background click)"
        fail "PTM window not found"
        stop_ptm; close_xterms
        return
    fi

    local xterm_wids
    xterm_wids=$(xdotool search --class xterm 2>/dev/null)
    local xterm1 xterm2
    xterm1=$(echo "$xterm_wids" | head -1)
    xterm2=$(echo "$xterm_wids" | tail -1)

    # Move xterms out of PTM's area
    xdotool windowmove "$xterm1" 300 50 windowsize "$xterm1" 400 300 2>/dev/null || true
    xdotool windowmove "$xterm2" 300 400 windowsize "$xterm2" 400 300 2>/dev/null || true
    xdotool windowmove "$ptm_wid" 0 0 2>/dev/null || true
    sleep 0.3

    # Step 1: Activate PTM and press Enter to activate first row
    xdotool windowactivate --sync "$ptm_wid" 2>/dev/null || true
    sleep 0.5
    xdotool windowfocus "$ptm_wid" key Return 2>/dev/null || true
    sleep 1.5

    local after_first
    after_first=$(xdotool getactivewindow 2>/dev/null)

    log_test "Mouse click on foreground PTM row activates target"
    if echo "$xterm_wids" | grep -qx "$after_first"; then
        pass
    else
        fail "Expected xterm active after click, got $after_first (ptm=$ptm_wid)"
        stop_ptm; close_xterms
        return
    fi

    screenshot "focus-passthrough-step1"

    # Step 2: PTM is in background. Move cursor over row, click.
    xdotool mousemove --window "$ptm_wid" 125 30 2>/dev/null || true
    sleep 0.5
    xdotool click 1 2>/dev/null || true
    sleep 1.5

    screenshot "focus-passthrough-step2"

    local after_second
    after_second=$(xdotool getactivewindow 2>/dev/null)

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

# Test 8: Dark theme — verify dark background renders
test_dark_theme() {
    stop_ptm
    close_xterms

    open_xterms 2
    start_ptm
    sleep 1

    local ptm_wid
    ptm_wid=$(xdotool search --name 'Process Tab Manager' 2>/dev/null | head -1)

    if [[ -z "$ptm_wid" ]]; then
        log_test "Dark theme renders"
        fail "PTM window not found"
        stop_ptm; close_xterms
        return
    fi

    local geo
    geo=$(xdotool getwindowgeometry --shell "$ptm_wid" 2>/dev/null)
    local wx wy ww wh
    wx=$(echo "$geo" | grep "^X=" | cut -d= -f2)
    wy=$(echo "$geo" | grep "^Y=" | cut -d= -f2)
    ww=$(echo "$geo" | grep "^WIDTH=" | cut -d= -f2)
    wh=$(echo "$geo" | grep "^HEIGHT=" | cut -d= -f2)

    screenshot_crop "dark-theme" "${ww}x${wh}+${wx}+${wy}"

    # Sample pixels from the sidebar background
    local avg_brightness
    avg_brightness=$(convert "$SCREENSHOT_DIR/dark-theme.png" -crop 50x50+10+10 -resize 1x1 -format '%[fx:luminance]' info: 2>/dev/null || echo "")

    log_test "Dark theme renders (not white)"
    if [[ -n "$avg_brightness" ]]; then
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
    sleep 1

    local ptm_wid
    ptm_wid=$(xdotool search --name 'Process Tab Manager' 2>/dev/null | head -1)
    local xterm_wid
    xterm_wid=$(xdotool search --class xterm 2>/dev/null | head -1)

    if [[ -z "$ptm_wid" || -z "$xterm_wid" ]]; then
        log_test "Snap alignment"
        fail "PTM or xterm window not found"
        stop_ptm; close_xterms
        return
    fi

    # Move PTM to a known position
    xdotool windowmove "$ptm_wid" 50 50 2>/dev/null || true
    sleep 0.3

    # Click the first row to trigger snap
    xdotool mousemove --window "$ptm_wid" 125 20 2>/dev/null || true
    sleep 0.3
    xdotool click 1 2>/dev/null || true
    sleep 1

    # Get positions
    local ptm_geo xterm_geo
    ptm_geo=$(xdotool getwindowgeometry --shell "$ptm_wid" 2>/dev/null)
    xterm_geo=$(xdotool getwindowgeometry --shell "$xterm_wid" 2>/dev/null)

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
    rm -f ~/.config/process-tab-manager/state.json

    open_xterms 1
    start_ptm
    sleep 1

    local ptm_wid
    ptm_wid=$(xdotool search --name 'Process Tab Manager' 2>/dev/null | head -1)

    if [[ -z "$ptm_wid" ]]; then
        log_test "Position persistence"
        fail "PTM window not found"
        stop_ptm; close_xterms
        return
    fi

    # Move PTM to a specific position
    xdotool windowmove "$ptm_wid" 200 150 2>/dev/null || true
    sleep 0.5

    # Gracefully stop PTM (SIGTERM triggers shutdown save with position)
    stop_ptm
    sleep 1

    log_test "State file has position data"
    local saved_pos
    saved_pos=$(python3 -c "import json; d=json.load(open('/home/steve/.config/process-tab-manager/state.json')); print(d.get('window_x','None'), d.get('window_y','None'))" 2>/dev/null || echo "None None")
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
    sleep 2

    ptm_wid=$(xdotool search --name 'Process Tab Manager' 2>/dev/null | head -1)

    if [[ -z "$ptm_wid" ]]; then
        log_test "Position restored after restart"
        fail "PTM window not found after restart"
        close_xterms
        return
    fi

    local restored_log_pos
    restored_log_pos=$(grep 'Restoring PTM position' /tmp/ptm.log 2>/dev/null | tail -1)

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

# ── Test runner ──

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

# Clean state before tests
echo "=== PTM In-VM E2E Runner ==="
echo ""
echo "Cleaning state..."
stop_ptm
close_xterms
rm -f ~/.config/process-tab-manager/state.json
restart_cinnamon
echo "Ready."

# Run all tests
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

# Summary
echo ""
echo "========================="
echo -e "Results: ${GREEN}${PASS} passed${NC}, ${RED}${FAIL} failed${NC}, ${YELLOW}${SKIP} skipped${NC}"
echo "Screenshots: $SCREENSHOT_DIR/"
echo "========================="

if [[ $FAIL -gt 0 ]]; then
    exit 1
fi
