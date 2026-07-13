# Shared prelude/teardown for the e2e scripts. Source this FIRST, straight
# after the shebang and doc comment:
#
#     source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
#     e2e_require xdotool tmux xterm openbox scrot   # Xvfb/xdpyinfo implied
#     e2e_start_xvfb                                 # exports DISPLAY=:99
#     e2e_start_wm                                   # openbox (optional)
#     e2e_mktemp_dir HOME_DIR ptm-e2e-home.XXXXXX    # auto-removed at exit
#     e2e_launch_ptm "$HOME_DIR" /tmp/ptm-e2e-foo.log  # sets PTM_PID + WID
#
# Sourcing (not exec-ing) keeps every script runnable standalone:
#     PTM_BIN=/tmp/ptm-dev/release/ptm tests/e2e/<name>.sh
#
# A script needing extra teardown defines `e2e_extra_cleanup()` (killing
# its own pids, etc.); it runs first inside the EXIT trap.

set -uo pipefail

DISPLAY_NUM=":99"
PTM="${PTM_BIN:-/tmp/ptm-dev/release/ptm}"

# Per-script isolated tmux server socket so staged sessions can't leak into
# (or from) the user's real server or a sibling script's.
export TMUX_TMPDIR=$(mktemp -d -t ptm-e2e-tmux.XXXXXX)
# Claude Code and dev shells often run INSIDE tmux. A leaked $TMUX makes
# every tmux call here target the user's real server (TMUX_TMPDIR is
# ignored when $TMUX is set) -- cleanup's kill-server would then nuke all
# of the user's sessions. Sever the link before the first tmux command.
# This fired for real once (2026-07-09); do not remove.
unset TMUX

# Temp dirs removed by the EXIT trap. Register more via e2e_mktemp_dir.
# (Screenshot dirs are typically created with plain mktemp instead, so
# failure screenshots survive for debugging.)
E2E_TMPDIRS=("$TMUX_TMPDIR")

# e2e_mktemp_dir <varname> <mktemp-template> — create a temp dir, assign it
# to <varname>, and register it for removal at exit. Assigns via printf -v
# because a $(...) capture would run in a subshell and lose the
# registration.
e2e_mktemp_dir() {
    local d
    d=$(mktemp -d -t "$2")
    printf -v "$1" '%s' "$d"
    E2E_TMPDIRS+=("$d")
}

e2e_cleanup() {
    declare -F e2e_extra_cleanup >/dev/null && e2e_extra_cleanup
    [[ -n "${PTM_PID:-}" ]] && kill "$PTM_PID" 2>/dev/null || true
    [[ -n "${WM_PID:-}" ]] && kill "$WM_PID" 2>/dev/null || true
    [[ -n "${XVFB_PID:-}" ]] && kill "$XVFB_PID" 2>/dev/null || true
    pkill -f "$DISPLAY_NUM.*xterm" 2>/dev/null || true
    # Guarded so a failed mktemp can never leave kill-server pointing at
    # the user's default socket.
    [[ -n "${TMUX_TMPDIR:-}" && -d "$TMUX_TMPDIR" ]] && tmux kill-server 2>/dev/null || true
    rm -rf "${E2E_TMPDIRS[@]}"
    wait 2>/dev/null || true
}
trap e2e_cleanup EXIT

# e2e_require <tool>... — verify the ptm binary and every needed tool.
# Xvfb and xdpyinfo are implied; list the rest per script.
e2e_require() {
    if [[ ! -x "$PTM" ]]; then
        echo "FAIL: ptm binary not found at $PTM (build with: CARGO_TARGET_DIR=/tmp/ptm-dev cargo build --release)"
        exit 2
    fi
    local tool
    for tool in Xvfb xdpyinfo "$@"; do
        command -v "$tool" >/dev/null || {
            echo "FAIL: missing $tool (sudo apt install xvfb xdotool xterm openbox tmux scrot wmctrl x11-utils)"
            exit 2
        }
    done
}

# Start a private Xvfb on $DISPLAY_NUM and export DISPLAY once responsive.
e2e_start_xvfb() {
    Xvfb "$DISPLAY_NUM" -screen 0 1024x768x24 >/dev/null 2>&1 &
    XVFB_PID=$!
    local i
    for i in {1..20}; do
        DISPLAY="$DISPLAY_NUM" xdpyinfo >/dev/null 2>&1 && break
        sleep 0.1
    done
    DISPLAY="$DISPLAY_NUM" xdpyinfo >/dev/null 2>&1 || { echo "FAIL: Xvfb did not become responsive"; exit 2; }
    export DISPLAY="$DISPLAY_NUM"
}

# Run openbox inside the Xvfb display. Needed whenever the script depends
# on _NET_CLIENT_LIST updates or snap_to_sidebar's ConfigureRequest being
# honoured (i.e. anything that spawns terminals). openbox decorates, so
# scripts address ptm's content area with window-relative clicks
# (xdotool mousemove --window).
e2e_start_wm() {
    openbox --sm-disable >/dev/null 2>&1 &
    WM_PID=$!
    sleep 0.4
}

# e2e_launch_ptm <home_dir> <log_file> — launch ptm with an isolated HOME,
# wait for its sidebar window, and set PTM_PID + WID. Fails the script if
# the window never appears (printing the log tail). Safe to call again for
# restart flows: PTM_PID/WID are simply re-assigned.
e2e_launch_ptm() {
    HOME="$1" "$PTM" >"$2" 2>&1 &
    PTM_PID=$!
    WID=""
    local i
    for i in {1..30}; do
        WID=$(xdotool search --name "^ptm$" 2>/dev/null | head -1 || true)
        [[ -n "$WID" ]] && break
        sleep 0.1
    done
    if [[ -z "$WID" ]]; then
        echo "FAIL: ptm window did not appear"
        tail -20 "$2" 2>/dev/null | sed 's/^/    /'
        exit 1
    fi
    echo "[setup] ptm wid=$WID pid=$PTM_PID"
}
