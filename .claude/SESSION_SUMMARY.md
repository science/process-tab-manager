# Overnight session summary (2026-05-03 → 2026-05-04)

Worked clusters from `MVP_PLAN.md` while you were asleep. Everything below is on `main`, ahead of `origin/main` by 17 commits (no push). Working tree clean. 221 tests pass. Release build clean.

## What landed (chronological)

| Cluster | Stage | Status | Commit summary |
|---|---|---|---|
| 1 | H — rename UX (selection model + word ops + pre-select + render) | ✓ COMPLETE | T1.3 → T1.6 + a real keysym-fallback bug found via live UAT |
| 2 | F — durability (paths, atomic writes, debounced save, ghost members, wm_class fallback) | ✓ COMPLETE | All 5 phases (2a–2e) |
| 3 | G — drag-and-drop fluency (classifier, indicator, group outline, post-drop flash) | ✓ MOSTLY (T3.0 + T3.6 deferred) | T3.1 → T3.5 done; G-4 needs real-use repro |

T1.1 + T1.2 were already on `main` from a prior session before I started. I picked up at T1.3.

## Files to read first when you wake up

1. **`.claude/UAT_RESULTS.md`** — what I tested live, with screenshot references in `/tmp/uat*.png`. Includes one important caveat (Q1 in QUESTIONS) about wm_class-only fallback at restore time.
2. **`.claude/QUESTIONS_FOR_USER.md`** — five questions accumulated. Q1 is the only one I'd call "important to review before deploying". Q2–Q5 are tunables / minor.
3. **`.claude/INSTALLED_DEPS.md`** — what I had to install on dev-2 (xdotool, scrot, wmctrl) and bootstrap-script suggestions.
4. **`git log --oneline d94d9f7..HEAD`** — full commit history.

## Commits since session start

```
051be30 docs: refresh test/LOC counts after Cluster 3
37235d8 docs: UAT-3 results + question updates for Cluster 3
78ff00b feat(drag): post-drop row highlight (Stage G T3.5 / G-5)
8e7f145 feat(drag): group-outline highlight on drag-over (Stage G T3.4)
eef9c0c feat(drag): drop indicator derived from DropTarget (Stage G T3.3)
2600747 feat(drag): DropTarget classifier + handle_drop dispatch (Stage G T3.1+T3.2)
fb4e845 docs: update test/LOC counts to reflect Cluster 1+2 work
4836e63 docs: UAT results for Clusters 1 and 2 (overnight session)
2b5c4c2 fix(persistence): runtime re-match respects already-displayed wids
59a34f1 feat(persistence): wm_class-only fallback in match cascade (Phase 2d)
61b047d feat(persistence): ghost members + identity-on-refresh (Phase 2c)
656e6b4 feat(persistence): debounced auto-save with 30s backstop (Phase 2b)
8a47b12 feat(persistence): profile-aware paths + atomic writes (Phase 2a)
0b5af13 feat(rename): selection rendering + keysym col-1 fallback (T1.6)
d4e9c7d feat(rename): pre-select existing text on rename open (T1.5)
39112a0 feat(rename): selection-aware backspace/delete + replace-on-insert (T1.4)
921ce3b feat(rename): Ctrl+A select-all and Ctrl word motion/delete (T1.3)
```

(Plus the two commits on `main` already when I started: T1.1 = `f157672`, T1.2 = `324833d`.)

## Behavioural changes worth flagging

1. **Drop on group header now inserts at TOP of group** (was: append). One regression test was updated; documented in commit `2600747`. Per Q3 if you prefer append, ~2-line revert in `classify_drop`.
2. **wm_class-only match at restore time** can pull unrelated terminals into a saved group when the original member is gone. Documented in Q1 (per OQ-F3 design, but UX-surprising in practice).
3. **Auto-save fires every 250 ms after a mutation** (was: only on clean WM_DELETE). 30 s backstop bounds worst-case loss. Per Q2 these intervals are tunable.
4. **Group state preserved as ghosts** when windows close — group survives PTM-restart-while-app-not-running. New tests prove it; live UAT confirms a closed-and-reopened terminal rejoins its group automatically.

## Things I deliberately did NOT do

- **No push to origin.** Per CLAUDE.md and your instructions, push needs explicit ask.
- **No Cluster 4 (Stage I — tmux session control) or Cluster 5 (Stage E — relaunch).** These need user UX decisions that the plan flagged as design-shaping (e.g., OQ-I3c arming UX, OQ-E1 capture confirmation).
- **No machine reboot recovery test.** Would have killed this session.
- **No 1-day soak test for Cluster 2 cluster-gate.** Requires you using PTM in real workflow, per the gate's definition.
- **No fix for G-4 "bouncing drops".** The no-op detect in T3.2 may have already covered the most-likely root cause, but T3.0's logging investigation needs real-use repro to confirm.

## Things I tested live (with screenshots)

All in `/tmp/uat*.png` and `/tmp/ptm-*.png`. They'll be gone on next reboot of dev-2 — copy them somewhere if you want to keep them.

- Cluster 1: `uat_01_rename_open.png` through `uat_10_after_commit.png` (10 stages of rename UAT).
- Cluster 2: `uat2_grp1.png`, `uat2_after_drag.png`, `uat2_after_restart.png`, `uat2_ghost_state.png`, `uat2_state3.png`, `uat2_after_sigterm.png`.
- Cluster 3: `uat3_outline_drag.png` (group outline mid-drag), `uat3_drop_highlight.png` (post-drop blue flash), `uat3_after_fade.png` (highlight gone 2 s later).

## Suggested next-session priorities

1. Review `QUESTIONS_FOR_USER.md` Q1 (wm_class-only restore behaviour). This is the most likely thing you'll want to revisit.
2. If satisfied, push and deploy to dev-1.
3. Real-use Cluster 2 cluster-gate UAT (1 day of normal use).
4. If G-4 "bouncing" still happens after all the above, revisit T3.0/T3.6.
5. Then Cluster 4 or 5 if comfortable.

## State on dev-2 when I left

- PTM not running.
- `~/.config/ptm` wiped (so next launch starts from a clean state).
- Two test gnome-terminals from UAT — closed at end of session.
- Working tree clean on `main`.
- xdotool/scrot/wmctrl installed (per INSTALLED_DEPS.md).
