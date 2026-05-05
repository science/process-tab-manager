# UAT test cases — Clusters 1, 2, 3 manual validation

These are the manual UX checks for the work that landed in the overnight session (Clusters 1 + 2 + 3 of `MVP_PLAN.md`). Designed to be run through in 20–30 minutes; flag anything that doesn't match the expected outcome.

## Setup before testing

1. From the repo: `./install.sh` (already run; should succeed cleanly).
2. Open PTM from your start menu (search "Process Tab Manager"). If it doesn't appear, right-click your Cinnamon menu → "Reload menu", or log out and back in.
3. Have at least 3 normal windows open on the desktop (terminals, browser, anything). PTM needs windows to manage.
4. **Suggestion:** `mv ~/.config/ptm ~/.config/ptm.bak` first if you want a clean slate; restore at the end if anything important got lost.

---

## Cluster 1 — Rename UX (Stage H)

The text input got a real selection model + standard keyboard shortcuts. Open a rename in PTM (right-click any group → "Rename Group", or right-click a window → "Rename Tab") to test.

### C1-1: Pre-select on open

1. Open rename on a group named "Group 1".
2. **Expected:** the text "Group 1" is shown with a blue highlight covering the whole word.
3. Type any letter (e.g., `x`).
4. **Expected:** the text is replaced — field now shows just `x`.
5. Press Escape.
6. **Expected:** rename closes; group still named "Group 1".

### C1-2: Type-to-replace (the headline behavior)

1. Open rename on a group.
2. Without clicking or arrow-keying first, type `MyProject`.
3. Press Enter.
4. **Expected:** group is now named "MyProject" — the original text was discarded.

### C1-3: Plain cursor motion

1. Open rename on "MyProject".
2. Press Right arrow once.
3. **Expected:** selection clears, cursor lands at end of text. (Right arrow on a selection collapses to end.)
4. Press Home.
5. **Expected:** cursor jumps to start, no selection.
6. Press End.
7. **Expected:** cursor jumps back to end, no selection.
8. Press Left arrow 3 times.
9. **Expected:** cursor moves left 3 chars (now between 'j' and 'e').

### C1-4: Shift+arrow selection extension

1. From C1-3 ending state (cursor between 'j' and 'e' in "MyProject").
2. Press Shift+Right twice.
3. **Expected:** "ec" is highlighted.
4. Press Shift+Right twice more.
5. **Expected:** "ect" is highlighted (extending from anchor).
6. Press Shift+Left once.
7. **Expected:** highlight shrinks to "ec".
8. Press Shift+Home.
9. **Expected:** selection extends from current cursor back to position 0; "MyProj" is highlighted.

### C1-5: Ctrl+A select-all

1. With cursor anywhere in "MyProject", press Ctrl+A.
2. **Expected:** the whole word is highlighted.
3. Press Backspace.
4. **Expected:** field is now empty (selection deleted; no chars before cursor).

### C1-6: Word motion (Ctrl+arrows)

1. Reset to a group renamed to "abc def ghi".
2. Open rename. Selection covers "abc def ghi".
3. Press End to clear selection, cursor at end.
4. Press Ctrl+Left once.
5. **Expected:** cursor jumps to start of "ghi" (between space and 'g').
6. Press Ctrl+Left again.
7. **Expected:** cursor jumps to start of "def".
8. Press Ctrl+Right.
9. **Expected:** cursor jumps to end of "def" (between 'f' and space).

### C1-7: Word delete (Ctrl+Backspace / Ctrl+Delete)

1. From "abc def ghi" with cursor at end.
2. Press Ctrl+Backspace.
3. **Expected:** "ghi" is deleted; field shows "abc def ".
4. Press Ctrl+Backspace.
5. **Expected:** "def " is deleted; field shows "abc ".
6. Press Home, then Ctrl+Delete.
7. **Expected:** "abc " is deleted; field is empty.

### C1-8: Selection-aware Backspace/Delete

1. Reset to "Hello World".
2. Open rename. Selection on full text.
3. Press Backspace.
4. **Expected:** field empty (selection deleted as a unit, not char-by-char).
5. Type "Foo Bar". Press End. Press Shift+Home.
6. **Expected:** "Foo Bar" highlighted.
7. Press Delete (forward delete).
8. **Expected:** field empty.

### C1-9: Cancel preserves

1. Open rename on a group. Selection visible.
2. Type aggressive nonsense (`asdfasdfasdf`).
3. Press Escape.
4. **Expected:** rename closes; group keeps its original name (the typed nonsense is discarded).

### C1-10: Commit empty doesn't blank the name

1. Open rename on a group.
2. Press Ctrl+A then Backspace (field now empty).
3. Press Enter.
4. **Expected:** rename closes; group keeps its original name (PTM rejects empty/whitespace-only names).

### C1-11: Tab rename

1. Right-click a window (not a group) → "Rename Tab".
2. **Expected:** rename input opens. If a tab prefix was already set, it's shown pre-selected. If no prefix, field is empty (no selection).
3. Type `Browser` and press Enter.
4. **Expected:** the window's row now shows `Browser: <original title>`.
5. Right-click → Rename Tab → Backspace until empty → Enter.
6. **Expected:** prefix is cleared; row shows just the original title.

### C1-12: Session rename (only if you have an orphan tmux session)

1. From a terminal: `tmux new -s test-uat-session ; <ctrl-b d>` to create + detach.
2. PTM should show a "test-uat-session" row (orphan dot).
3. Right-click → "Rename Session".
4. **Expected:** rename opens with name pre-selected.
5. Type `renamed-session` and press Enter.
6. **Expected:** the row updates to "renamed-session"; `tmux ls` from a terminal shows the rename took effect on the tmux server.
7. Cleanup: `tmux kill-session -t renamed-session`.

---

## Cluster 2 — Persistence (Stage F)

The on-disk format is now `~/.config/ptm/profiles/default/{groups,geometry}` (was `~/.config/ptm/{groups,geometry}`). Save fires automatically 250 ms after any state change; survives SIGTERM.

### C2-1: Auto-save (no explicit close needed)

1. Run PTM. Pick any window in the sidebar.
2. Right-click → "New Group".
3. Wait one second.
4. Inspect from a separate terminal:
   ```
   ls -la ~/.config/ptm/profiles/default/
   cat ~/.config/ptm/profiles/default/groups
   ```
5. **Expected:** both `groups` and `geometry` exist; the groups file shows the new "Group 1" with the window you just grouped — without you having quit PTM.

### C2-2: One-time migration (only if upgrading from a pre-2026-05 PTM)

If you had a `~/.config/ptm/groups` file from before Phase 2a:

1. Quit PTM. `mv ~/.config/ptm/profiles ~/.config/ptm/profiles.bak`.
2. Verify: `ls ~/.config/ptm/` shows both old `groups` + `geometry` AND no `profiles/`.
3. Launch PTM.
4. **Expected:** the old files moved into `~/.config/ptm/profiles/default/`. Run `ls ~/.config/ptm/` — only the `profiles/` directory should remain at the top level.

### C2-3: SIGTERM survives state

1. Make a group via right-click → New Group.
2. Wait ~1s for the save tick.
3. From a terminal: `kill -TERM $(pgrep -f /home/steve/.local/bin/ptm)` (or use `pkill -TERM ptm`).
4. **Expected:** PTM disappears immediately (no clean WM_DELETE).
5. Re-launch PTM.
6. **Expected:** the group you just made is back, with its members.

### C2-4: Ghost member — close and reopen

1. Spawn a test terminal: `gnome-terminal --title=UAT-test &` (or any other distinguishable terminal).
2. In PTM, right-click that "UAT-test" row → New Group.
3. Close the test terminal externally (click the X, or `Ctrl+D` in it).
4. **Expected:** the group's row disappears from the sidebar, BUT the group header stays. (The member became a ghost.)
5. Re-spawn the terminal: `gnome-terminal --title=UAT-test &`.
6. Wait a couple seconds for PTM's refresh.
7. **Expected:** the new terminal automatically rejoins the group (without you having to drag it in).

### C2-5: Group survives PTM restart with member missing

1. Same setup as C2-4: group containing a `UAT-test` terminal.
2. Quit PTM cleanly (close button on PTM's window, or right-click PTM titlebar → Close).
3. Close the `UAT-test` terminal.
4. Re-launch PTM.
5. **Expected:** Group 1 is still there. (The plan calls this the FM-2 fix — pre-2c the empty group would have been silently wiped.)
6. **Caveat:** if there's another terminal of the same wm_class still alive (e.g., your normal terminal), it might be claimed into the group's empty slot — see QUESTIONS Q1 in `.claude/QUESTIONS_FOR_USER.md`. Drag it back out if so.

### C2-6: Title drift (wm_class fallback)

1. Spawn a terminal `gnome-terminal --title=before &`.
2. Group it via PTM.
3. Quit PTM.
4. From a terminal: change the test terminal's title using `printf '\033]0;after\007'` typed inside it (or `xdotool set_window --name after $(xdotool search --name before)`).
5. Re-launch PTM.
6. **Expected:** the group still has the terminal as a live member, even though its title changed. (Phase 2d wm_class fallback matched it.)

---

## Cluster 3 — Drag-and-drop fluency (Stage G)

The drag classifier and indicator both became one function (so the blue line never lies); the entire vertical extent of a group is a join target now (not just the gap range); after-drop the destination row flashes blue.

### C3-1: Indicator matches landing

1. PTM with at least 3 ungrouped windows.
2. Press-and-hold a window row, drag it slowly upward and downward.
3. **Expected:** the blue horizontal line reliably appears above whichever row you'd insert before. Releasing puts the row exactly where the line was. (Pre-Stage-G the line and landing could disagree.)

### C3-2: G-1 — drop into group body joins

1. Have one group with 1–2 members and one ungrouped window.
2. Drag the ungrouped window onto the BODY of the group (not the header — onto a member row, top half).
3. **Expected:** the window joins the group at the position above the row you dropped on. (Pre-Stage-G this would have landed as ungrouped.)

### C3-3: G-2 — reorder small overshoot stays in group

1. Group with 2 members, one ungrouped window directly below the group.
2. Drag the SECOND group member down by just a few pixels — past the bottom of the last member's row but not yet onto the ungrouped row's middle.
3. **Expected:** the dragged member stays in the group (this is a no-op visually — same position as before). Pre-Stage-G this small overshoot ejected it.

### C3-4: G-2 — clearly outside still ejects

1. Same setup. Drag a group member to *clearly* below the ungrouped row (or to the very bottom of the sidebar).
2. **Expected:** the member is removed from the group and lands as a separate row.

### C3-5: T3.4 — group outline on drag-over

1. Drag any window over an existing expanded group (don't release).
2. **Expected:** the entire group (header + all members) gets a faint blue rectangular border for as long as the cursor is over its vertical extent.
3. Move the cursor outside the group's extent.
4. **Expected:** the outline disappears.

### C3-6: T3.5 — post-drop highlight

1. Do any successful drag-drop (e.g., reorder two members).
2. **Expected:** the row that just moved gets a blue background highlight for ~1.5 seconds, then returns to normal.
3. **Expected:** a no-op drag (drop on yourself) does NOT highlight.

### C3-7: Drop on header inserts at TOP

1. Group with 2 members [A, B].
2. Drag a 3rd window C onto the group's HEADER row.
3. **Expected:** C is inserted at the TOP of the group → order is now [C, A, B].
4. **NOTE:** this is a behavior change from pre-Stage-G (which appended). If you preferred append, see QUESTIONS Q3 in `.claude/QUESTIONS_FOR_USER.md`.

### C3-8: Reorder within group

1. Group with 3 members [A, B, C].
2. Drag B above A.
3. **Expected:** order becomes [B, A, C]; the moved row (B) flashes blue briefly.

### C3-9: Drag a group as a whole

1. Two groups + some ungrouped windows.
2. Drag the second group's HEADER above the first group.
3. **Expected:** group order swaps. (Sessions and group-headers still use the gap-based old behavior — no body-of-group special handling for them, per OQ-G3 deferral.)

---

## Sanity / regression checks

These are pre-existing behaviors that should still work — quick checks that the cluster work didn't break anything.

### S1: Click-to-focus + snap

1. Click a window's row in PTM.
2. **Expected:** that window gets activated and snaps next to PTM (existing behavior).

### S2: Hover highlight

1. Mouse over a row.
2. **Expected:** subtle background darkens (existing).

### S3: Active window stripe

1. Click on a window in another app to give it focus.
2. **Expected:** PTM shows the blue accent stripe + tinted background on whichever row corresponds to the focused window.

### S4: Collapse / expand a group

1. Click on a group's `-` arrow.
2. **Expected:** members hide; arrow becomes `+`; member count appears next to group name.
3. Click again.
4. **Expected:** members show; arrow returns to `-`.

### S5: + New terminal

1. Click the "+ New terminal" header.
2. **Expected:** a new terminal opens (using `$PTM_TERMINAL_CMD` if set, else system defaults).

### S6: Right-click context menus

1. Right-click on each row type (group header, grouped window, ungrouped window, orphan tmux session if any).
2. **Expected:** appropriate menu entries. Window not in group: "Rename Tab", "New Group". Window in group: those + "Add to Group...", "Remove from Group". Group header: "Rename Group", "Delete Group". Session: "Attach", "Rename Session", "Kill Session".

---

## What to flag

If anything in C1-* doesn't behave like a normal text input (compared to e.g. the GNOME address bar): bug.

If C2-1 doesn't show a saved file within ~1 second: the auto-save tick isn't firing — bug.

If C2-3 (SIGTERM) loses state: bug.

If C2-4/C2-5 (ghost rejoin) doesn't reconnect a reopened window: bug.

If C3-1 indicator and landing disagree by even one row: that's a regression of the G-3 fix.

If C3-2 / C3-3 (drop into group body / small overshoot) ejects: regression of G-1 / G-2.

If C3-6 doesn't show the post-drop highlight at all: T3.5 is broken.

For C3-7's behavior change (drop on header → TOP) — that's expected per the plan; don't flag as a bug, just decide whether you want to revert per Q3.
