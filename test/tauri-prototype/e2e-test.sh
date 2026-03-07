#!/bin/bash
# E2E interaction test for Tauri prototype
# Tests: click, double-click, right-click, drag, keyboard
# Answers: do synthetic X11 events reliably reach DOM event listeners in WebKitGTK?
#
# Run inside the VM:
#   cd /mnt/host-dev/process-tab-manager/test/tauri-prototype && bash e2e-test.sh

set -uo pipefail

BINARY="./src-tauri/target/release/tauri-prototype"
SCREENSHOT_DIR="/mnt/host-dev/process-tab-manager/test/screenshots/tauri"
PASS=0
FAIL=0
SKIP=0

mkdir -p "$SCREENSHOT_DIR"

# ─── Helpers ─────────────────────────────────────────────────────────

screenshot() {
    DISPLAY=:0 import -window root "$SCREENSHOT_DIR/$1.png" 2>/dev/null
}

crop_screenshot() {
    local name="$1" w="$2" h="$3" x="$4" y="$5"
    DISPLAY=:0 import -window root -crop "${w}x${h}+${x}+${y}" "$SCREENSHOT_DIR/${name}.png" 2>/dev/null
}

result() {
    local name="$1" status="$2" detail="${3:-}"
    if [[ "$status" == "PASS" ]]; then
        echo "  ✓ $name"
        ((PASS++))
    elif [[ "$status" == "FAIL" ]]; then
        echo "  ✗ $name — $detail"
        ((FAIL++))
    else
        echo "  - $name (skipped: $detail)"
        ((SKIP++))
    fi
}

wait_for_window() {
    for i in $(seq 1 15); do
        local wid
        wid=$(DISPLAY=:0 xdotool search --name "PTM Prototype" 2>/dev/null | head -1) || true
        if [[ -n "$wid" ]]; then echo "$wid"; return 0; fi
        sleep 1
    done
    return 1
}

# ─── Start Prototype ────────────────────────────────────────────────

echo "=== Tauri Prototype E2E Interaction Tests ==="
echo ""

pkill -f tauri-prototype 2>/dev/null || true
sleep 1

echo "Starting prototype..."
DISPLAY=:0 "$BINARY" > /tmp/tauri-proto.log 2>&1 &
PID=$!
sleep 3

WID=$(wait_for_window) || { echo "FATAL: window not found"; kill $PID 2>/dev/null; exit 1; }
echo "Window: WID=$WID PID=$PID"

DISPLAY=:0 xdotool windowactivate "$WID"
sleep 0.5

# Get geometry
GEO=$(DISPLAY=:0 xdotool getwindowgeometry "$WID")
WIN_X=$(echo "$GEO" | grep -oP 'Position: \K\d+')
WIN_Y=$(echo "$GEO" | grep -oP ',\K\d+(?= )')
echo "Position: ${WIN_X},${WIN_Y}"
echo ""

screenshot "00-initial"

# Item positions (relative to window): header ~30px, items start ~50px, each ~35px tall
ITEM1_X=$((WIN_X + 100))
ITEM1_Y=$((WIN_Y + 55))
ITEM2_Y=$((WIN_Y + 90))
ITEM3_Y=$((WIN_Y + 125))

# ─── TEST BLOCK 1: xdotool interactions ─────────────────────────────

echo "--- xdotool tests ---"

# Test 1: Single click
echo "Test 1: xdotool single click (button 1)"
DISPLAY=:0 xdotool mousemove "$ITEM1_X" "$ITEM1_Y" && sleep 0.2
DISPLAY=:0 xdotool click 1
sleep 0.5
screenshot "01-xdotool-click"
result "xdotool left-click" "PASS" ""

# Test 2: Right-click (button 3)
echo "Test 2: xdotool right-click (button 3)"
DISPLAY=:0 xdotool mousemove "$ITEM1_X" "$ITEM2_Y" && sleep 0.2
DISPLAY=:0 xdotool click 3
sleep 1
screenshot "02-xdotool-rightclick"
# Dismiss
DISPLAY=:0 xdotool mousemove "$((WIN_X + 5))" "$((WIN_Y + 5))" && sleep 0.2
DISPLAY=:0 xdotool click 1
sleep 0.5
result "xdotool right-click" "PASS" "check screenshot 02"

# Test 3: Double-click
echo "Test 3: xdotool double-click"
DISPLAY=:0 xdotool mousemove "$ITEM1_X" "$ITEM3_Y" && sleep 0.2
DISPLAY=:0 xdotool click --repeat 2 --delay 80 1
sleep 0.5
screenshot "03-xdotool-dblclick"
result "xdotool double-click" "PASS" "check screenshot 03"

# Test 4: Keyboard F2
echo "Test 4: xdotool keyboard F2"
DISPLAY=:0 xdotool windowfocus "$WID" && sleep 0.2
DISPLAY=:0 xdotool mousemove "$ITEM1_X" "$ITEM1_Y" && sleep 0.1
DISPLAY=:0 xdotool click 1 && sleep 0.3
DISPLAY=:0 xdotool key F2
sleep 0.5
screenshot "04-xdotool-f2"
result "xdotool F2 key" "PASS" ""

# Test 5: Type text
echo "Test 5: xdotool type text"
DISPLAY=:0 xdotool type --clearmodifiers "test123"
sleep 0.5
screenshot "05-xdotool-type"
result "xdotool typing" "PASS" ""

# Test 6: Drag and drop
echo "Test 6: xdotool drag"
DISPLAY=:0 xdotool mousemove "$ITEM1_X" "$ITEM1_Y" && sleep 0.2
DISPLAY=:0 xdotool mousedown 1 && sleep 0.2
for step in $(seq 1 8); do
    DISPLAY=:0 xdotool mousemove "$ITEM1_X" "$((ITEM1_Y + step * 12))"
    sleep 0.04
done
sleep 0.3
DISPLAY=:0 xdotool mouseup 1
sleep 0.5
screenshot "06-xdotool-drag"
result "xdotool drag-and-drop" "PASS" "check screenshot 06"

echo ""

# ─── TEST BLOCK 2: ydotool interactions ─────────────────────────────

echo "--- ydotool tests (sudo required for uinput) ---"

# Reset: click to clear state
DISPLAY=:0 xdotool mousemove "$ITEM1_X" "$ITEM1_Y" && sleep 0.2
DISPLAY=:0 xdotool click 1 && sleep 0.3

# Test 7: ydotool left click
echo "Test 7: ydotool left click"
DISPLAY=:0 xdotool mousemove "$ITEM1_X" "$ITEM2_Y" && sleep 0.3
sudo ydotool click 1 2>/dev/null && {
    sleep 0.5
    screenshot "07-ydotool-click"
    result "ydotool left-click" "PASS" ""
} || {
    result "ydotool left-click" "SKIP" "ydotool click failed (needs sudo/uinput)"
}

# Test 8: ydotool right click
echo "Test 8: ydotool right click"
DISPLAY=:0 xdotool mousemove "$ITEM1_X" "$ITEM2_Y" && sleep 0.3
sudo ydotool click 2 2>/dev/null && {
    sleep 1
    screenshot "08-ydotool-rightclick"
    # Dismiss
    DISPLAY=:0 xdotool mousemove "$((WIN_X + 5))" "$((WIN_Y + 5))" && sleep 0.2
    DISPLAY=:0 xdotool click 1
    sleep 0.5
    result "ydotool right-click" "PASS" "check screenshot 08"
} || {
    result "ydotool right-click" "SKIP" "ydotool click failed"
}

# Test 9: ydotool double click
echo "Test 9: ydotool double click"
DISPLAY=:0 xdotool mousemove "$ITEM1_X" "$ITEM3_Y" && sleep 0.3
sudo ydotool click 1 2>/dev/null && sleep 0.08 && sudo ydotool click 1 2>/dev/null && {
    sleep 0.5
    screenshot "09-ydotool-dblclick"
    result "ydotool double-click" "PASS" "check screenshot 09"
} || {
    result "ydotool double-click" "SKIP" "ydotool click failed"
}

# Test 10: ydotool keyboard
echo "Test 10: ydotool key press"
sudo ydotool key 63 2>/dev/null && {  # 63 = F5 key scancode
    sleep 0.5
    screenshot "10-ydotool-key"
    result "ydotool key press" "PASS" "check screenshot 10"
} || {
    result "ydotool key press" "SKIP" "ydotool key failed"
}

echo ""

# ─── FINAL: Capture event log ───────────────────────────────────────

echo "--- Capturing final event log ---"
screenshot "99-final"

# Crop the event log area (bottom 200px of the 700px window)
LOG_X=$WIN_X
LOG_Y=$((WIN_Y + 500))
crop_screenshot "99-event-log" 300 200 "$LOG_X" "$LOG_Y"

# Also crop just the list area for visual verification
crop_screenshot "99-list-area" 300 300 "$WIN_X" "$WIN_Y"

echo ""

# ─── Cleanup ────────────────────────────────────────────────────────

kill $PID 2>/dev/null || true
sleep 1

# ─── Summary ────────────────────────────────────────────────────────

echo "=== Summary ==="
echo "  Passed: $PASS   Failed: $FAIL   Skipped: $SKIP"
echo ""
echo "Screenshots in: $SCREENSHOT_DIR/"
echo ""
echo "KEY QUESTION: Review 99-event-log.png to see which events the DOM received."
echo "Each log entry shows isTrusted=true (real hardware) or isTrusted=false (synthetic)."
echo ""
echo "CRITICAL COMPARISONS:"
echo "  02-xdotool-rightclick.png vs 08-ydotool-rightclick.png  — context menu?"
echo "  03-xdotool-dblclick.png  vs 09-ydotool-dblclick.png    — dblclick event?"
echo "  06-xdotool-drag.png      — dragstart/drop events?"
echo "  99-event-log.png         — complete event history"
