# Cluster 4 (Stage I) — overnight implementation notes

All 9 commits landed cleanly. 272 unit tests pass (was 222 baseline,
+50 new). Both dev and release builds compile clean.

## Commits (in landing order)

```
309f0ac feat(confirm): override-redirect Y/N popup helper (T4.1)
2a72a42 feat(tmux): kill attached session via menu (T4.2)
c9a505e refactor(group): introduce GroupKind { Normal, TmuxSystem } (T4.3)
fb40602 feat(persistence): GROUP line accepts optional kind field (T4.4a)
41343a3 feat(tmux): Tmux Sessions system group renders all sessions (T4.4)
e239d34 feat(tmux): persist Tmux Sessions group across PTM restarts (T4.5)
a8de16e feat(tmux): + New tmux button creates session and attaches (T4.5b)
9d54a5d fix(drag): windows cannot drop into Tmux Sessions system group (T4.6)
6a696c2 feat(tmux): [x] close glyph on system-group session rows (T4.7)
a2175a1 feat(tmux): single-click [x] opens kill confirmation (T4.8)
7329435 docs(plan): mark Cluster 4 / Stage I tasks complete
```

## What still needs you (UAT)

The plan's UAT-4 checklist requires interactive verification of the
GUI. None of this can be tested headless / from Claude Code:

- Visual: confirm "+ New terminal" / "+ New tmux" sit side-by-side
  cleanly at default width (250px). Strings should fit (14 + 10
  chars × 8px = 112 + 80; both < ~115px half-width).
- Visual: confirm popup centring, button border visibility, message
  truncation behavior with very long session names.
- Functional: every UAT-4 line in `~/.claude/plans/delegated-painting-fairy.md`
  (or section "Verification" in the plan).
- Downgrade safety: bring up PTM with the new binary so it writes
  4-field GROUP, then `git checkout HEAD~10 -- src/main.rs` and rebuild
  to confirm groups still load (T4.4a's tolerance kicks in).

## Decisions I made during implementation

These weren't blockers but you may want to revisit:

1. **`x` glyph colour** — I used `text_dim_pixel` so it sits subtly
   on the row (the popup is the destructive moment, not the glyph).
   If you'd prefer it pop more, swap to `text_pixel` or a red accent
   in `draw_session_row` (~line 3100).

2. **Confirmation popup keyboard shortcuts** — I wired Y/y as accept
   and N/n as cancel, in addition to Enter / Esc. The plan said
   "Enter / Y → accept; Esc / N → cancel" so this matches; just
   flagging that I made it case-insensitive.

3. **Popup placement** — the popup opens at the click root_xy
   (clamped to screen). For the menu-triggered popup it opens at the
   menu's origin. Both feel natural in head-tests but you'll want
   to verify it doesn't hide behind the sidebar on tall sessions
   lists near the bottom of the screen.

4. **Optimistic UI for orphan-session kill** — preserved from before.
   When you right-click an orphan and pick Kill Session (no popup
   for orphans, per plan), the row is removed from the system group's
   members immediately. Next refresh re-confirms via list-sessions.

5. **Session-row drag is now NoOp** — the old test
   `classify_drop_session_source_just_inserts` was deleted. The plan
   T4.6 explicitly says sessions are derived and not user-managed,
   so dragging them was always going to be silently overwritten.
   Replaced with a comment + new `classify_drop_session_source_is_noop` test.

6. **Tmux probe is once at startup** — `app.tmux_available` is set
   when main() runs and never re-probed. If you install/remove tmux
   while PTM is running, you'll need to restart PTM to see the new
   button state. Plan explicitly accepts this.

7. **`refresh_items` doesn't auto-create the system group** — only
   the startup main() does. If `ensure_tmux_system_group` ever needs
   to be called more eagerly (e.g., after the user manually deletes
   it via direct App mutation, which can't happen from the UI today),
   the call is idempotent and cheap.

## Open questions worth thinking about

None blocking — all OQ-I3* are answered in the plan. Two minor
nice-to-haves I noticed while implementing:

1. **Hover state for `[x]` glyph** — currently no hover highlight on
   the close band itself; the whole session row highlights on hover.
   Could draw a subtle box around `[x]` when the cursor is in its
   band, but this is polish for later.

2. **Close-band width vs marker-dot reserve** — chose 16px band +
   8px extra for marker. With the 220px row, label has ~196px
   useful (24+ chars). Tested with longest standard session name
   ("default" = 7 chars) — plenty of room. Will need re-checking
   if you encourage long session names.

## Test count by task

| Task   | New tests | Cumulative |
|--------|-----------|------------|
| baseline |         | 222        |
| T4.1   | 3         | 225        |
| T4.2   | 5         | 230        |
| T4.3   | 1         | 231        |
| T4.4a  | 5         | 236        |
| T4.4   | 7 (— 1 deleted) | 242 |
| T4.5   | 7         | 249        |
| T4.5b  | 7         | 256        |
| T4.6   | 9         | 265        |
| T4.7   | 4         | 269        |
| T4.8   | 3         | 272        |

Total: +50 new tests, 1 deleted (obsolete by design), net +49.

## Branch state

All commits on `main`, no remote pushes. Run `git push origin main`
when you're ready (after UAT passes).
