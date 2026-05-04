# UAT results — Clusters 1 + 2 (overnight session 2026-05-03 / 2026-05-04)

Performed by Claude Opus 4.7 against `/tmp/ptm-dev/release/ptm` on dev-2 VM.
All UAT was driven by `xdotool` synthetic input + `scrot` screenshots; no
human-in-the-loop verification.

## Test bench

- `dev-2` VM, X11 on `:0`, Cinnamon running.
- Two test gnome-terminals spawned with distinct titles
  (`UAT-window-A` / `UAT-window-B`) plus the parent `claude` terminal
  hosting the Claude Code session itself.
- 205 automated unit tests pass at HEAD (up from 110 at session start).

## UAT-1 (Cluster 1, Stage H — rename UX)

| Behaviour | Method | Pass? |
|---|---|---|
| Pre-select on rename-open shows highlighted text | screenshot of "Group 1" with blue highlight | ✓ (uat_01_rename_open.png) |
| Escape cancels and preserves old name | screenshot before/after Escape | ✓ (uat_02_after_cancel.png) |
| Type-to-replace clears selection in one keystroke | type 'a' over "Group 1" → field shows "a" | ✓ (uat_03_after_type_a.png) |
| Subsequent typing extends text | "bc def ghi" appended | ✓ (uat_04_after_more_text.png) |
| Ctrl+A selects all | full-text highlight visible | ✓ (uat_05_after_ctrl_a.png) |
| Right arrow clears selection, lands at end | cursor at end, no highlight | ✓ (uat_06_after_right_arrow.png) |
| Ctrl+Left jumps by word | from end of "abc def ghi" → cursor at start of "ghi" | ✓ (uat_07_after_ctrl_left.png) |
| Ctrl+Shift+Left extends selection by word | "def" highlighted | ✓ (uat_08_after_ctrl_shift_left.png) |
| Ctrl+Backspace deletes the selected word | "def" gone → "abc ghi" | ✓ (uat_09_after_ctrl_bksp.png) |
| Enter commits | group renamed to "abc ghi" | ✓ (uat_10_after_commit.png) |

Screenshots in `/tmp/uat_*.png`.

### Bug fixed during UAT-1

The keysym lookup helper (`keysym_from_keycode`) returned `0` (NoSymbol) for
the shifted column of arrow/Home/End keys. Without an X11-spec col-1
fallback to col-0, every Shift-modified motion was silently dropped at the
event handler — Shift+End / Shift+arrow extension didn't work end-to-end
even though every unit test for the underlying RenameState methods passed.
Refactored the column picker into a pure `select_keysym` helper, added 5
fallback tests (commit `0b5af13`).

## UAT-2 (Cluster 2, Stage F — durability)

### Phase 2a (paths + atomic writes)

| Scenario | Result |
|---|---|
| Pre-2a files at `~/.config/ptm/{groups,geometry}` | Migrated on first launch into `~/.config/ptm/profiles/default/`. Old location empty after. ✓ |
| Atomic write leaves no `.tmp` after success | `ls` confirms only `groups` and `geometry` after each save. ✓ |

### Phase 2b (debounced auto-save)

| Scenario | Result |
|---|---|
| Create a group via right-click → New Group | File appears at `~/.config/ptm/profiles/default/groups` within 250 ms with no explicit close/quit. ✓ |
| Killing PTM with `kill -TERM` (no clean WM_DELETE) | State preserved across restart (last save tick caught it). ✓ — see uat2_after_sigterm.png |

### Phase 2c (ghost members + identity re-match)

| Scenario | Result |
|---|---|
| Group two terminals, close one externally, reopen with same title | Closed terminal becomes a ghost; reopened terminal (different wid, same identity) auto-rejoins the group on next refresh. ✓ — uat2_state2 and uat2_state3 |
| Restart PTM with all group members live | Both members rejoin group immediately. ✓ — uat2_after_restart.png |
| Restart PTM with one group member missing | Group renders with the live member; missing member kept as ghost (saved file preserves both). ⚠ See note below |

**⚠ Phase 2c live note (also covered in QUESTIONS Q1):**
restore_groups still allows the wm_class-only fallback to claim an
unrelated currently-displayed window into a ghost slot if no other class
match is available. In my UAT, restarting PTM with `UAT-window-A` closed
caused `claude` to be pulled into Group 1 because it was the only other
Gnome-terminal alive. Per OQ-F3 in MVP_PLAN.md this is the intended
trade-off ("cost of a false match is low — fixable in a click"). The
runtime re-match in `refresh_items` was already fixed not to do this, but
the startup restore path keeps the looser semantics on purpose.

### Phase 2d (wm_class-only fallback)

| Scenario | Result |
|---|---|
| Saved member with title `claude - process-tab-manager`; current window has same wm_class but title `bash - other-dir` | Matches by class, joins group. ✓ (unit test) |
| Saved Gnome-terminal member; current Firefox-class window | No cross-class match. ✓ (unit test) |

Live UAT for terminal title drift was NOT directly performed (the dev-2
gnome-terminals don't drift their titles enough during a short session).
The wm_class fallback is exercised indirectly by the "ghost rejoin"
scenarios above.

### Phase 2e (refactor)

`grep -c member_wids src/main.rs` returns 0 for production code; refactor
was integrated into Phase 2c. No behavioural changes; existing tests cover.

## Outstanding follow-ups (in QUESTIONS_FOR_USER.md)

- **Q1**: wm_class-only fallback at restore time can pull unrelated windows
  into a group. Per OQ-F3 design but worth a UX review.
- **Q2**: Auto-save backstop interval (30 s) is tunable.

## Not tested in this session (by design)

- Full machine reboot recovery (would kill the Claude Code session).
- Title-drifting tmux sessions over a long period.
- 1-day soak: requires the user actually using PTM in their normal workflow,
  per the cluster-gate definition.
