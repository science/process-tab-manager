#!/usr/bin/env bash
# E2E: clicking "+ New tmux" must stamp a persistent identity in BOTH
# places the matcher relies on, so the spawned tmux row is drift-immune:
#
#   (a) the tmux SESSION carries a `@ptm_id` user option, and
#   (b) the spawned XTERM window carries a `_PTM_ID` X11 property, and
#   (c) the two values are identical.
#
# This is the half of the persistent-identity scheme that pure-Rust unit
# tests can't reach: it needs a real X server (to read back the window
# property via xprop) and a real tmux server (to read back the session
# option). The matching/restore behaviour that consumes these ids is
# covered by Tier-1 unit tests (restore_groups_tier0_*).
#
# Pass: @ptm_id set on the session AND _PTM_ID set on the window AND equal.
# Fail: either property missing, or they disagree.
#
# Tools required: Xvfb, xdotool, tmux, xterm, openbox, xdpyinfo, scrot.

set -uo pipefail

DISPLAY_NUM=":99"
PTM="${PTM_BIN:-/tmp/ptm-dev/release/ptm}"
HOME_DIR=$(mktemp -d -t ptm-e2e-id.XXXXXX)
SHOTS=$(mktemp -d -t ptm-e2e-id-shots.XXXXXX)
export TMUX_TMPDIR=$(mktemp -d -t ptm-e2e-id-tmux.XXXXXX)

cleanup() {
    [[ -n "${PTM_PID:-}" ]] && kill "$PTM_PID" 2>/dev/null || true
    [[ -n "${WM_PID:-}" ]] && kill "$WM_PID" 2>/dev/null || true
    [[ -n "${XVFB_PID:-}" ]] && kill "$XVFB_PID" 2>/dev/null || true
    pkill -f "$DISPLAY_NUM.*xterm" 2>/dev/null || true
    tmux kill-server 2>/dev/null || true
    rm -rf "$HOME_DIR" "$TMUX_TMPDIR" "$SHOTS"
    wait 2>/dev/null || true
}
trap cleanup EXIT

if [[ ! -x "$PTM" ]]; then
    echo "FAIL: ptm binary not found at $PTM"; exit 2
fi
for tool in Xvfb xdotool tmux xterm xdpyinfo scrot openbox xprop; do
    command -v "$tool" >/dev/null || { echo "FAIL: missing $tool"; exit 2; }
done

Xvfb "$DISPLAY_NUM" -screen 0 1024x768x24 >/dev/null 2>&1 &
XVFB_PID=$!
for i in {1..20}; do
    DISPLAY="$DISPLAY_NUM" xdpyinfo >/dev/null 2>&1 && break
    sleep 0.1
done
DISPLAY="$DISPLAY_NUM" xdpyinfo >/dev/null 2>&1 || { echo "FAIL: Xvfb did not become responsive"; exit 2; }
export DISPLAY="$DISPLAY_NUM"

openbox --sm-disable >/dev/null 2>&1 &
WM_PID=$!
sleep 0.4

export PTM_TERMINAL_CMD=xterm

HOME="$HOME_DIR" "$PTM" >/tmp/ptm-e2e-id.log 2>&1 &
PTM_PID=$!

WID=""
for i in {1..30}; do
    WID=$(xdotool search --name "^ptm$" 2>/dev/null | head -1 || true)
    [[ -n "$WID" ]] && break
    sleep 0.1
done
[[ -z "$WID" ]] && { echo "FAIL: ptm window did not appear"; exit 1; }
echo "[setup] ptm wid=$WID pid=$PTM_PID"

sleep 0.6  # initial paint

BEFORE_SESSIONS=$(tmux ls -F '#{session_name}' 2>/dev/null | sort -u || true)
echo "[setup] sessions before: ${BEFORE_SESSIONS:-<none>}"

# Click "+ New tmux" via window-relative coords.
echo "[act ] click '+ New tmux' at window-relative 184,18"
xdotool mousemove --window "$WID" --sync 184 18; sleep 0.1
xdotool click 1

# Wait for the new xterm.
TERM_WID=""
for i in {1..40}; do
    TERM_WID=$(xdotool search --class "xterm" 2>/dev/null | head -1 || true)
    [[ -n "$TERM_WID" ]] && break
    sleep 0.1
done
if [[ -z "$TERM_WID" ]]; then
    echo "FAIL: no xterm appeared within 4s of clicking '+ New tmux'"
    echo "ptm log tail:"; tail -20 /tmp/ptm-e2e-id.log 2>&1 | sed 's/^/  /'
    exit 1
fi
echo "[diag] xterm wid=$TERM_WID"

# Identify the new session.
SESSION=""
for i in {1..30}; do
    AFTER=$(tmux ls -F '#{session_name}' 2>/dev/null | sort -u || true)
    SESSION=$(comm -13 <(printf '%s\n' "$BEFORE_SESSIONS") <(printf '%s\n' "$AFTER") | head -1)
    [[ -n "$SESSION" ]] && break
    sleep 0.1
done
if [[ -z "$SESSION" ]]; then
    echo "FAIL: '+ New tmux' did not create a new tmux session"
    echo "tmux ls:"; tmux ls 2>&1 | sed 's/^/  /'
    exit 1
fi
echo "[diag] new tmux session: $SESSION"

# Let PTM's claim tier stamp the window (happens on the refresh that first
# sees the new wid).
sleep 0.8
scrot "$SHOTS/01-after-spawn.png"

# ── (a) session @ptm_id ──
SESSION_ID=$(tmux show-options -v -t "$SESSION" @ptm_id 2>/dev/null || true)
if [[ -z "$SESSION_ID" ]]; then
    echo "FAIL: tmux session '$SESSION' has no @ptm_id option"
    echo "      → create_new_tmux_session did not tag the session"
    echo "session options:"; tmux show-options -t "$SESSION" 2>&1 | sed 's/^/  /'
    exit 1
fi
echo "[ok  ] (a) session @ptm_id = $SESSION_ID"

# ── (b) window _PTM_ID ──
WIN_ID=""
for i in {1..30}; do
    RAW=$(xprop -id "$TERM_WID" _PTM_ID 2>/dev/null || true)
    # xprop prints: _PTM_ID(UTF8_STRING) = "value"
    if [[ "$RAW" == *'='* && "$RAW" != *"not found"* ]]; then
        WIN_ID=$(printf '%s' "$RAW" | sed -n 's/.*= "\(.*\)"/\1/p')
        [[ -n "$WIN_ID" ]] && break
    fi
    sleep 0.1
done
if [[ -z "$WIN_ID" ]]; then
    echo "FAIL: xterm wid $TERM_WID has no _PTM_ID property"
    echo "      → the claim tier did not stamp the spawned window"
    echo "xprop:"; xprop -id "$TERM_WID" 2>&1 | grep -i ptm | sed 's/^/  /'
    echo "ptm log tail:"; tail -20 /tmp/ptm-e2e-id.log | sed 's/^/  /'
    exit 1
fi
echo "[ok  ] (b) window _PTM_ID = $WIN_ID"

# ── (c) they must match ──
if [[ "$SESSION_ID" != "$WIN_ID" ]]; then
    echo "FAIL: session @ptm_id ($SESSION_ID) != window _PTM_ID ($WIN_ID)"
    exit 1
fi

echo "[ok  ] (c) session @ptm_id == window _PTM_ID"
echo "PASS: '+ New tmux' stamped a matching persistent id on both the session and the window"
exit 0
