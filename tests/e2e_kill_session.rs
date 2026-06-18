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

/// Reproducer for the missing green tmux marker / missing tmux status bar:
/// clicking "+ New tmux" must produce an xterm that is actually a tmux
/// client (keystroke round-trip via xdotool → tmux capture-pane) AND PTM
/// must bind that row's `item.session` so the sidebar marker renders
/// (verified via SIGUSR1 dump). Fails today because the in-flight refactor
/// removed `bind_sessions`' claim+carry tiers.
#[test]
fn new_tmux_runs_tmux_and_binds_session() {
    let (ok, out) = run_e2e_script("tmux_attach_runs_tmux.sh");
    assert!(
        ok,
        "'+ New tmux' should spawn a tmux client AND PTM should bind item.session\n{}",
        out
    );
}

/// Reproducer for the Debian-wrapper bug: when PTM's chosen terminal is a
/// `.wrapper` Perl/bash shim (mimicking `/usr/bin/gnome-terminal.wrapper`),
/// the spawn must use `-e` so the wrapper sees and forwards the command.
/// Sibling test above sets PTM_TERMINAL_CMD=xterm, which dodges this code
/// path; this script installs a fake `gnome-terminal.wrapper` that mimics
/// the real wrapper's "drop everything not behind -e" behaviour and
/// verifies the marker still round-trips. Would have caught the original
/// "no green tmux status bar inside the spawned terminal" report.
#[test]
fn new_tmux_through_wrapper_still_runs_tmux() {
    let (ok, out) = run_e2e_script("tmux_attach_through_wrapper.sh");
    assert!(
        ok,
        "'+ New tmux' through a Debian wrapper shim must still run tmux\n{}",
        out
    );
}

/// Reproducer: right-clicking a Normal group header → "New terminal"
/// should snap the spawned xterm to the sidebar anchor (same as the
/// tmux variant). Today the bare-terminal-in-group path lands at the
/// WM default position — RED until the snap gate is fixed.
#[test]
fn new_terminal_in_group_snaps_to_sidebar_anchor() {
    let (ok, out) = run_e2e_script("spawn_terminal_in_group.sh");
    assert!(
        ok,
        "group-context 'New terminal' should snap the spawned xterm to the sidebar anchor\n{}",
        out
    );
}

/// Persistent-identity scheme: clicking "+ New tmux" must stamp a matching
/// `@ptm_id` tmux option on the session AND a `_PTM_ID` X11 property on the
/// spawned window. This is the half of the anti-drift design that unit
/// tests can't reach (real X server + real tmux server); the matching that
/// consumes these ids is covered by the `restore_groups_tier0_*` unit tests.
#[test]
fn new_tmux_stamps_matching_ptm_id_on_session_and_window() {
    let (ok, out) = run_e2e_script("ptm_id_stamped_on_spawn.sh");
    assert!(
        ok,
        "'+ New tmux' should stamp a matching persistent id on the session and the window\n{}",
        out
    );
}
