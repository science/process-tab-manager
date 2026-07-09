#!/usr/bin/env bash
# E2E: a tmux session binding must survive a PTM restart even when the
# walk tier is useless, by rebinding through the `_PTM_SESSION` window
# property (the session's `@ptm_id` uuid stamped on the X window).
#
# This is the e2e half of the WM-restart bug fix: under gnome-terminal
# every window shares gnome-terminal-server's _NET_WM_PID, so
# `walk_to_window_owner` always collides and the carry tier's
# one-refresh-deep memory is the only thing keeping `item.session`
# alive. A `cinnamon --replace` (or a PTM restart, as simulated here)
# wipes that memory and the binding was permanently lost.
#
# xterm normally dodges the collision (one pid per window), so the
# script forces it: a second xterm gets its _NET_WM_PID overwritten to
# the first xterm's pid, making the walk tier return None — exactly the
# gnome-terminal topology. A fresh $HOME for the restarted PTM keeps
# restore_groups' recipe rebind out of play, isolating the rebind tier.
#
#   (a) after '+ New tmux', the spawned window carries _PTM_SESSION
#       equal to the session's @ptm_id, and
#   (b) after PTM is killed and restarted under a pid collision, the
#       row's Tmux binding is back (SIGUSR1 dump).
#
# Pass: both (a) and (b). Fail: property missing or binding lost.
#
# Tools required: Xvfb, xdotool, tmux, xterm, openbox, xdpyinfo, scrot.

set -uo pipefail

DISPLAY_NUM=":99"
PTM="${PTM_BIN:-/tmp/ptm-dev/release/ptm}"
HOME1=$(mktemp -d -t ptm-e2e-rebind-h1.XXXXXX)
HOME2=$(mktemp -d -t ptm-e2e-rebind-h2.XXXXXX)
SHOTS=$(mktemp -d -t ptm-e2e-rebind-shots.XXXXXX)
export TMUX_TMPDIR=$(mktemp -d -t ptm-e2e-rebind-tmux.XXXXXX)
# Claude Code and dev shells often run INSIDE tmux. A leaked $TMUX makes
# every tmux call here target the user's real server (TMUX_TMPDIR is
# ignored when $TMUX is set) -- cleanup's kill-server would then nuke all
# of the user's sessions. Sever the link before the first tmux command.
unset TMUX

cleanup() {
    [[ -n "${PTM_PID:-}" ]] && kill "$PTM_PID" 2>/dev/null || true
    [[ -n "${WM_PID:-}" ]] && kill "$WM_PID" 2>/dev/null || true
    [[ -n "${XVFB_PID:-}" ]] && kill "$XVFB_PID" 2>/dev/null || true
    pkill -f "$DISPLAY_NUM.*xterm" 2>/dev/null || true
    [[ -n "${TMUX_TMPDIR:-}" && -d "$TMUX_TMPDIR" ]] && tmux kill-server 2>/dev/null || true
    rm -rf "$HOME1" "$HOME2" "$TMUX_TMPDIR" "$SHOTS"
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

HOME="$HOME1" "$PTM" >/tmp/ptm-e2e-rebind-1.log 2>&1 &
PTM_PID=$!

WID=""
for i in {1..30}; do
    WID=$(xdotool search --name "^ptm$" 2>/dev/null | head -1 || true)
    [[ -n "$WID" ]] && break
    sleep 0.1
done
[[ -z "$WID" ]] && { echo "FAIL: ptm window did not appear"; exit 1; }
echo "[setup] ptm#1 wid=$WID pid=$PTM_PID"

sleep 0.6  # initial paint

BEFORE_SESSIONS=$(tmux ls -F '#{session_name}' 2>/dev/null | sort -u || true)

# Click "+ New tmux" via window-relative coords.
echo "[act ] click '+ New tmux' at window-relative 184,18"
xdotool mousemove --window "$WID" --sync 184 18; sleep 0.1
xdotool click 1

# Wait for the new xterm. Recorded while it's the only xterm alive, so
# the later plain-xterm spawn can't confuse the search.
TERM_WID=""
for i in {1..40}; do
    TERM_WID=$(xdotool search --class "xterm" 2>/dev/null | head -1 || true)
    [[ -n "$TERM_WID" ]] && break
    sleep 0.1
done
if [[ -z "$TERM_WID" ]]; then
    echo "FAIL: no xterm appeared within 4s of clicking '+ New tmux'"
    echo "ptm log tail:"; tail -20 /tmp/ptm-e2e-rebind-1.log 2>&1 | sed 's/^/  /'
    exit 1
fi
echo "[diag] tmux xterm wid=$TERM_WID"

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

SESSION_UUID=$(tmux show-options -v -t "$SESSION" @ptm_id 2>/dev/null || true)
if [[ -z "$SESSION_UUID" ]]; then
    echo "FAIL: tmux session '$SESSION' has no @ptm_id option"
    exit 1
fi
echo "[diag] session @ptm_id = $SESSION_UUID"

# ── (a) window _PTM_SESSION stamped with the session's @ptm_id ──
STAMP=""
for i in {1..30}; do
    RAW=$(xprop -id "$TERM_WID" _PTM_SESSION 2>/dev/null || true)
    # xprop prints: _PTM_SESSION(UTF8_STRING) = "value"
    if [[ "$RAW" == *'='* && "$RAW" != *"not found"* ]]; then
        STAMP=$(printf '%s' "$RAW" | sed -n 's/.*= "\(.*\)"/\1/p')
        [[ -n "$STAMP" ]] && break
    fi
    sleep 0.1
done
if [[ -z "$STAMP" ]]; then
    echo "FAIL: xterm wid $TERM_WID has no _PTM_SESSION property"
    echo "      → the stamp pass did not run after item.session was bound"
    echo "xprop:"; xprop -id "$TERM_WID" 2>&1 | grep -i ptm | sed 's/^/  /'
    echo "ptm log tail:"; tail -20 /tmp/ptm-e2e-rebind-1.log | sed 's/^/  /'
    exit 1
fi
if [[ "$STAMP" != "$SESSION_UUID" ]]; then
    echo "FAIL: window _PTM_SESSION ($STAMP) != session @ptm_id ($SESSION_UUID)"
    exit 1
fi
echo "[ok  ] (a) window _PTM_SESSION == session @ptm_id"
scrot "$SHOTS/01-stamped.png"

# ── Force the walk-tier collision (gnome-terminal topology) ──
#
# Spawn a plain xterm and forge its _NET_WM_PID to the tmux xterm's pid.
# pid_to_wid then maps one pid to two wids and walk_to_window_owner
# refuses to guess — the restarted PTM cannot rebind via the walk tier.
xterm -title "decoy" &
XTERM2_PID=$!
DECOY_WID=""
for i in {1..40}; do
    DECOY_WID=$(xdotool search --class "xterm" 2>/dev/null | grep -v "^${TERM_WID}$" | head -1 || true)
    [[ -n "$DECOY_WID" ]] && break
    sleep 0.1
done
[[ -z "$DECOY_WID" ]] && { echo "FAIL: decoy xterm did not appear"; exit 1; }

TERM_PID_RAW=$(xprop -id "$TERM_WID" _NET_WM_PID 2>/dev/null || true)
TERM_PID=$(printf '%s' "$TERM_PID_RAW" | sed -n 's/.*= \([0-9]*\)/\1/p')
[[ -z "$TERM_PID" ]] && { echo "FAIL: tmux xterm has no _NET_WM_PID"; exit 1; }
xprop -id "$DECOY_WID" -f _NET_WM_PID 32c -set _NET_WM_PID "$TERM_PID"
echo "[act ] forged decoy wid=$DECOY_WID _NET_WM_PID=$TERM_PID (collision armed)"

# ── Kill PTM hard, restart with a FRESH home ──
#
# SIGKILL: no shutdown save. Fresh $HOME2: no groups file, so
# restore_groups' recipe rebind can't produce a false green — the
# _PTM_SESSION stamp is the only recovery path left.
echo "[act ] kill -9 ptm#1 (pid $PTM_PID), restart with fresh HOME"
kill -9 "$PTM_PID" 2>/dev/null || true
wait "$PTM_PID" 2>/dev/null || true

HOME="$HOME2" "$PTM" >/tmp/ptm-e2e-rebind-2.log 2>&1 &
PTM_PID=$!

WID2=""
for i in {1..30}; do
    WID2=$(xdotool search --name "^ptm$" 2>/dev/null | head -1 || true)
    [[ -n "$WID2" ]] && break
    sleep 0.1
done
[[ -z "$WID2" ]] && { echo "FAIL: restarted ptm window did not appear"; exit 1; }
echo "[setup] ptm#2 wid=$WID2 pid=$PTM_PID"

sleep 1.0  # first refresh: rebind tier reads _PTM_SESSION off the window
scrot "$SHOTS/02-after-restart.png"

# ── (b) binding survived: SIGUSR1 dump shows the Tmux binding ──
echo "[act ] SIGUSR1 to ptm#2 pid=$PTM_PID"
kill -USR1 "$PTM_PID"

DUMP_FILE="$HOME2/.cache/ptm/recipes-snapshot.md"
for i in {1..30}; do
    [[ -f "$DUMP_FILE" ]] && break
    sleep 0.1
done
[[ -f "$DUMP_FILE" ]] || { echo "FAIL: dump file not written at $DUMP_FILE"; exit 1; }
sleep 0.3  # let the dump fully flush

TERM_WID_HEX=$(printf '0x%08x' "$TERM_WID")
echo "[diag] looking for wid=$TERM_WID_HEX in dump"

# One block per window separated by "---"; scope the assertion to the
# tmux xterm's block so a binding on another row can't pass the test.
AWK_SCRIPT='
BEGIN { RS="\n---\n"; }
$0 ~ wid_re { print; }
'
TERM_BLOCK=$(awk -v wid_re="$TERM_WID_HEX" "$AWK_SCRIPT" "$DUMP_FILE")
if [[ -z "$TERM_BLOCK" ]]; then
    echo "FAIL: no dump block found referencing wid $TERM_WID_HEX"
    echo "dump:"; sed 's/^/  /' "$DUMP_FILE"
    exit 1
fi

if ! printf '%s' "$TERM_BLOCK" | grep -q "Tmux binding:.*session=.*${SESSION}"; then
    echo "FAIL: after restart, the tmux row's block has no Tmux binding for session '$SESSION'"
    echo "      → rebind tier did not recover the binding from _PTM_SESSION"
    echo "block:"; printf '%s\n' "$TERM_BLOCK" | sed 's/^/  /'
    echo "ptm#2 log tail:"; tail -20 /tmp/ptm-e2e-rebind-2.log | sed 's/^/  /'
    exit 1
fi

echo "[ok  ] (b) binding for session '$SESSION' survived the restart under pid collision"
echo "PASS: _PTM_SESSION stamp + rebind tier keep the session binding across restarts"
exit 0
