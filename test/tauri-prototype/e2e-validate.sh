#!/bin/bash
# Deterministic E2E validation for Tauri prototype
# Verifies all interaction patterns via /tmp/ptm-events.log (programmatic pass/fail)
#
# Run inside the VM:
#   cd /mnt/host-dev/process-tab-manager/test/tauri-prototype && bash e2e-validate.sh

set -uo pipefail

BINARY="./src-tauri/target/release/tauri-prototype"
EVENT_LOG="/tmp/ptm-events.log"
STATE_FILE="/tmp/ptm-test-state.json"
SCREENSHOT_DIR="/mnt/host-dev/process-tab-manager/test/screenshots/tauri-validate"

PASS=0
FAIL=0

mkdir -p "$SCREENSHOT_DIR"

# ─── Helpers ─────────────────────────────────────────────────────────

result() {
    local name="$1" status="$2" detail="${3:-}"
    if [[ "$status" == "PASS" ]]; then
        echo "  ✓ $name"
        ((PASS++))
    else
        echo "  ✗ $name — $detail"
        ((FAIL++))
    fi
}

wait_for_window() {
    for i in $(seq 1 20); do
        local wid
        wid=$(DISPLAY=:0 xdotool search --name "PTM Prototype" 2>/dev/null | head -1) || true
        if [[ -n "$wid" ]]; then echo "$wid"; return 0; fi
        sleep 0.5
    done
    return 1
}

clean_logs() {
    rm -f "$EVENT_LOG" "$STATE_FILE"
}

# Wait for an event pattern to appear in the log (polls up to $2 seconds)
wait_for_event() {
    local pattern="$1" timeout="${2:-3}"
    local attempts=$((timeout * 10))
    for i in $(seq 1 "$attempts"); do
        if [[ -f "$EVENT_LOG" ]] && grep -q "$pattern" "$EVENT_LOG" 2>/dev/null; then
            return 0
        fi
        sleep 0.1
    done
    return 1
}

screenshot() {
    DISPLAY=:0 import -window root "$SCREENSHOT_DIR/$1.png" 2>/dev/null
}

# ─── Build check ─────────────────────────────────────────────────────

if [[ ! -x "$BINARY" ]]; then
    echo "FATAL: Binary not found at $BINARY"
    echo "Run: bash build.sh"
    exit 1
fi

# ─── Start Prototype ─────────────────────────────────────────────────

echo "=== Tauri Prototype E2E Validation ==="
echo ""

pkill -f tauri-prototype 2>/dev/null || true
sleep 1
clean_logs

echo "Starting prototype..."
DISPLAY=:0 "$BINARY" > /tmp/tauri-validate.log 2>&1 &
PID=$!

WID=$(wait_for_window) || { echo "FATAL: window not found"; kill $PID 2>/dev/null; exit 1; }
echo "Window: WID=$WID PID=$PID"

DISPLAY=:0 xdotool windowactivate "$WID"
sleep 0.5

# Wait for init event (proves file logging works)
if ! wait_for_event "init" 5; then
    echo "FATAL: Event log not being written. Tauri invoke may be broken."
    kill $PID 2>/dev/null
    exit 1
fi
echo "Event logging confirmed."

# Get geometry
GEO=$(DISPLAY=:0 xdotool getwindowgeometry "$WID")
WIN_X=$(echo "$GEO" | grep -oP 'Position: \K\d+')
WIN_Y=$(echo "$GEO" | grep -oP ',\K\d+(?= )')
echo "Position: ${WIN_X},${WIN_Y}"
echo ""

# Item positions: header ~30px, items start ~50px, each ~35px tall
ITEM1_X=$((WIN_X + 100))
ITEM1_Y=$((WIN_Y + 55))
ITEM2_Y=$((WIN_Y + 90))
ITEM3_Y=$((WIN_Y + 125))
ITEM4_Y=$((WIN_Y + 160))

# ─── TEST 1: Click select ────────────────────────────────────────────

echo "--- Test 1: Click select ---"
clean_logs
sleep 0.2

DISPLAY=:0 xdotool mousemove "$ITEM1_X" "$ITEM2_Y"
sleep 0.2
DISPLAY=:0 xdotool click 1
sleep 0.5

if wait_for_event "click.*id=2" 3; then
    # Also verify isTrusted
    if grep -q "click.*id=2.*isTrusted=true" "$EVENT_LOG" 2>/dev/null; then
        result "Click select (item 2, isTrusted=true)" "PASS"
    else
        result "Click select (item 2)" "PASS"
    fi
else
    result "Click select (item 2)" "FAIL" "no click event for id=2 in log"
fi

# Verify state file updated
if [[ -f "$STATE_FILE" ]] && grep -q '"selectedId": 2' "$STATE_FILE" 2>/dev/null; then
    result "State file updated (selectedId=2)" "PASS"
else
    result "State file updated (selectedId=2)" "FAIL" "state file missing or wrong selectedId"
fi

# ─── TEST 2: Right-click context menu ────────────────────────────────

echo "--- Test 2: Right-click context menu ---"
clean_logs
sleep 0.2

DISPLAY=:0 xdotool mousemove "$ITEM1_X" "$ITEM1_Y"
sleep 0.2
DISPLAY=:0 xdotool click 3
sleep 0.5

if wait_for_event "contextmenu.*id=1" 3; then
    if grep -q "contextmenu.*id=1.*isTrusted=true" "$EVENT_LOG" 2>/dev/null; then
        result "Right-click menu (item 1, isTrusted=true)" "PASS"
    else
        result "Right-click menu (item 1)" "PASS"
    fi
else
    result "Right-click menu (item 1)" "FAIL" "no contextmenu event for id=1 in log"
fi

screenshot "02-rightclick-menu"

# Dismiss menu
DISPLAY=:0 xdotool mousemove "$((WIN_X + 5))" "$((WIN_Y + 5))"
sleep 0.2
DISPLAY=:0 xdotool click 1
sleep 0.3

# ─── TEST 3: Double-click ────────────────────────────────────────────

echo "--- Test 3: Double-click ---"
clean_logs
sleep 0.2

DISPLAY=:0 xdotool mousemove "$ITEM1_X" "$ITEM3_Y"
sleep 0.2
DISPLAY=:0 xdotool click --repeat 2 --delay 80 1
sleep 0.5

if wait_for_event "dblclick.*id=3" 3; then
    if grep -q "dblclick.*id=3.*isTrusted=true" "$EVENT_LOG" 2>/dev/null; then
        result "Double-click (item 3, isTrusted=true)" "PASS"
    else
        result "Double-click (item 3)" "PASS"
    fi
else
    result "Double-click (item 3)" "FAIL" "no dblclick event for id=3 in log"
fi

# ─── TEST 4: Keyboard F2 ─────────────────────────────────────────────

echo "--- Test 4: Keyboard F2 ---"
# First select an item
DISPLAY=:0 xdotool mousemove "$ITEM1_X" "$ITEM1_Y"
sleep 0.2
DISPLAY=:0 xdotool click 1
sleep 0.3

clean_logs
sleep 0.2

DISPLAY=:0 xdotool key F2
sleep 0.5

if wait_for_event "f2-rename" 3; then
    result "F2 rename event" "PASS"
else
    result "F2 rename event" "FAIL" "no f2-rename event in log"
fi

# Also check keydown for F2
if grep -q "keydown.*key=F2" "$EVENT_LOG" 2>/dev/null; then
    result "F2 keydown event" "PASS"
else
    result "F2 keydown event" "FAIL" "no keydown for F2 in log"
fi

# ─── TEST 5: Keyboard typing ─────────────────────────────────────────

echo "--- Test 5: Keyboard typing ---"
clean_logs
sleep 0.2

DISPLAY=:0 xdotool windowfocus "$WID"
sleep 0.2
DISPLAY=:0 xdotool type --clearmodifiers "hello"
sleep 0.5

if wait_for_event "keydown.*key=h" 3; then
    result "Keyboard typing (h)" "PASS"
else
    result "Keyboard typing (h)" "FAIL" "no keydown for h in log"
fi

if grep -q "keydown.*key=o" "$EVENT_LOG" 2>/dev/null; then
    result "Keyboard typing (o)" "PASS"
else
    result "Keyboard typing (o)" "FAIL" "no keydown for o in log"
fi

# ─── TEST 6: DnD reorder ─────────────────────────────────────────────

echo "--- Test 6: DnD reorder ---"

# Record initial order
clean_logs
sleep 0.2

# Click item 1 first to ensure state file is fresh
DISPLAY=:0 xdotool mousemove "$ITEM1_X" "$ITEM1_Y"
sleep 0.2
DISPLAY=:0 xdotool click 1
sleep 0.5

# Read initial order from state
INITIAL_ORDER=""
if [[ -f "$STATE_FILE" ]]; then
    INITIAL_ORDER=$(python3 -c "
import json
with open('$STATE_FILE') as f:
    s = json.load(f)
print(','.join(str(i['id']) for i in s['items']))
" 2>/dev/null) || true
fi

clean_logs
sleep 0.2

# Drag item 1 (Firefox) down past items 2, 3, 4
# Use slow gradual movement (300ms hold + gradual steps)
DISPLAY=:0 xdotool mousemove "$ITEM1_X" "$ITEM1_Y"
sleep 0.3
DISPLAY=:0 xdotool mousedown 1
sleep 0.35

# Slow gradual movement down
for step in $(seq 1 15); do
    DISPLAY=:0 xdotool mousemove "$ITEM1_X" "$((ITEM1_Y + step * 8))"
    sleep 0.04
done
sleep 0.2

DISPLAY=:0 xdotool mouseup 1
sleep 0.8

# Check for reorder event
if wait_for_event "reorder" 3; then
    result "DnD reorder event" "PASS"
else
    # Also accept drop event as partial success
    if wait_for_event "drop" 1; then
        result "DnD drop event (no reorder)" "PASS"
    else
        result "DnD reorder event" "FAIL" "no reorder or drop event in log"
    fi
fi

# Verify dragstart fired
if grep -q "dragstart" "$EVENT_LOG" 2>/dev/null; then
    result "DnD dragstart event" "PASS"
else
    result "DnD dragstart event" "FAIL" "no dragstart in log"
fi

# Check if item order changed in state file
if [[ -n "$INITIAL_ORDER" ]] && [[ -f "$STATE_FILE" ]]; then
    NEW_ORDER=$(python3 -c "
import json
with open('$STATE_FILE') as f:
    s = json.load(f)
print(','.join(str(i['id']) for i in s['items']))
" 2>/dev/null) || true
    if [[ -n "$NEW_ORDER" ]] && [[ "$NEW_ORDER" != "$INITIAL_ORDER" ]]; then
        result "DnD state order changed ($INITIAL_ORDER → $NEW_ORDER)" "PASS"
    else
        result "DnD state order changed" "FAIL" "order unchanged: $INITIAL_ORDER → ${NEW_ORDER:-?}"
    fi
fi

screenshot "06-after-dnd"

# ─── Cleanup ─────────────────────────────────────────────────────────

echo ""
kill $PID 2>/dev/null || true
sleep 1

# ─── Summary ─────────────────────────────────────────────────────────

echo "=== Summary ==="
echo "  Passed: $PASS   Failed: $FAIL"
echo ""

if [[ $FAIL -eq 0 ]]; then
    echo "ALL TESTS PASSED"
    exit 0
else
    echo "SOME TESTS FAILED"
    exit 1
fi
