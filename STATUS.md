# PTM Status -- 2026-03-06

## Environment
Development now happens directly inside the VM (cinnamon-dev). Claude Code runs in a desktop terminal.
- `~/dev/process-tab-manager` = `/mnt/host-dev/process-tab-manager` (virtiofs mount)
- Binary: `target/release/process-tab-manager` (workspace root target, NOT src-tauri/target/)

## What's Working
- **Unit tests: 54/54 pass** (`cargo test -p ptm-core`)
- **E2E tests: 12/18 pass** consistently (`bash test/tauri-e2e-runner.sh`)
- All core features work: window listing, click activation, dark theme, dynamic updates, save/restore, groups, keyboard nav, keyboard reorder events

## E2E Test Failures (6 tests)

### Root Cause: xdotool focus management with WebKitGTK
Clicking a sidebar item calls `activate_window()` which gives X11 focus to the target window. Subsequent xdotool keyboard commands then go to that window, not PTM. The `focus_ptm()` helper restores focus but is unreliable in some test sequences.

Key finding: keyboard events DO work when PTM has verified X11 focus. The `send_key` helper was added to ensure focus before every key press.

### Failing tests:
1. **test_f2_rename** -- F2 key reaches DOM but selectedWid is null (item click doesn't register in some test flows)
2. **test_right_click_menu** -- right-click contextmenu event not firing (similar focus/timing issue)
3. **test_save_on_exit** -- depends on F2 rename working (cascading failure)
4. **test_state_persistence** -- depends on F2 rename working (cascading failure)
5. **test_keyboard_reorder (order check)** -- Ctrl+Shift+Down event fires but actual reorder doesn't change first item (may be selecting wrong item)

### Intermittent issue: PTM window destruction
PTM occasionally crashes with `GdkWindow unexpectedly destroyed` during xdotool interactions. May be related to Cinnamon WM restart at test start or GTK event loop race.

## Completed in this session
1. Updated CLAUDE.md for local development (removed VM-from-host instructions)
2. Updated run.sh for local use (no VM SSH)
3. Updated test/tauri-e2e-runner.sh -- PROJECT/BINARY paths, focus_ptm improvements, send_key helper
4. Fixed binary path: workspace `target/` not `src-tauri/target/`
5. Removed stale src-tauri/target/ directory
6. Updated .gitignore (added claude-max-key.txt, claude-install.sh)
7. Removed debug scripts and version marker div
8. Added safety note to CLAUDE.md about never restarting lightdm from desktop session

## Next steps
1. Fix test_f2_rename -- likely needs the click to verify selectedWid before proceeding with F2
2. Fix test_right_click_menu -- may need to ensure PTM is raised before right-click
3. The keyboard reorder order-check may need to select via click + verify selection before reordering
4. Consider adding a small JS helper that receives test commands via a file/pipe to bypass xdotool keyboard limitations
5. Commit all changes
