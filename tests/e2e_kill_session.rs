//! Cargo integration wrapper for the X11 e2e reproducers under
//! `tests/e2e/*.sh`. Each `#[test]` shells out to one script, asserts on
//! exit status, and prints the script's combined output on failure.
//!
//! The two tests share a single Xvfb display and the user's tmux server so
//! they're serialized via `E2E_LOCK`. Each script picks unique tmux session
//! names (PID-derived) so they don't trample each other across runs.
//!
//! Required system tools: Xvfb, xdotool, tmux, scrot, xdpyinfo.
//! Install: `sudo apt install xvfb xdotool tmux scrot x11-utils`.
//!
//! The wrapper points each script at the cargo-built binary via
//! `CARGO_BIN_EXE_ptm` (set automatically by cargo for binary crates) so a
//! standalone `cargo test` works without pre-building the release binary.

use std::path::PathBuf;
use std::process::Command;
use std::sync::Mutex;

static E2E_LOCK: Mutex<()> = Mutex::new(());

const PTM_BIN: &str = env!("CARGO_BIN_EXE_ptm");

fn run_e2e_script(script_name: &str) -> (bool, String) {
    let _guard = E2E_LOCK.lock().unwrap_or_else(|p| p.into_inner());

    let script: PathBuf = [env!("CARGO_MANIFEST_DIR"), "tests", "e2e", script_name]
        .iter()
        .collect();

    let output = Command::new("bash")
        .arg(&script)
        .env("PTM_BIN", PTM_BIN)
        .output()
        .expect("failed to spawn bash");

    let combined = format!(
        "--- script: {} ---\n--- stdout ---\n{}\n--- stderr ---\n{}",
        script.display(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    (output.status.success(), combined)
}

/// Control: right-click on an orphan tmux session row -> "Kill Session"
/// kills the session. This path bypasses the confirmation popup entirely
/// (orphan kills go straight through `execute_menu_action`).
#[test]
fn menu_right_click_kills_orphan_session() {
    let (ok, out) = run_e2e_script("menu_kills_session.sh");
    assert!(
        ok,
        "right-click -> Kill Session should kill the orphan session\n{}",
        out
    );
}

/// Reproducer: clicking the [x] glyph -> popup -> Enter should kill the
/// orphan session, but currently does not. Expected to FAIL today; will
/// pass once the popup-accept ordering bug at src/main.rs:4730/4765 is
/// fixed (close_confirm_popup consumes app.confirm before dispatch_confirm
/// can read the action).
#[test]
fn x_button_popup_accept_kills_orphan_session() {
    let (ok, out) = run_e2e_script("x_button_kills_session.sh");
    assert!(
        ok,
        "[x] -> popup -> Enter should kill the orphan session\n{}",
        out
    );
}

/// Reproducer: clicking "+ New tmux" should spawn the attached terminal
/// at the sidebar anchor (immediately right of ptm), matching the
/// positioning ptm applies when activating an existing window via click.
/// Today the new terminal lands at the WM/X server's default position
/// (typically overlapping the sidebar) — RED.
#[test]
fn new_tmux_button_positions_terminal_at_sidebar_anchor() {
    let (ok, out) = run_e2e_script("spawn_position.sh");
    assert!(
        ok,
        "'+ New tmux' should snap the new terminal to the sidebar anchor\n{}",
        out
    );
}

/// Phase 5c MVP: a saved tmux row reattaches to its live xterm via Tier 0a
/// (session-name match) after PTM restarts. The script pre-seeds a v2
/// groups file with a stale pane_pid sentinel, starts the tmux session +
/// xterm, launches PTM, closes it via WM_DELETE_WINDOW, and verifies the
/// shutdown save updated the pane_pid (proof that Tier 0a matched and
/// capture-at-save ran).
#[test]
fn tmux_row_reattaches_via_tier_0a_after_restart() {
    let (ok, out) = run_e2e_script("recipes_survive_restart.sh");
    assert!(
        ok,
        "saved tmux row should reattach via session-name match after restart\n{}",
        out
    );
}
