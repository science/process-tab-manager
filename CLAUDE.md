# Process Tab Manager

Vertical sidebar for managing application windows on Linux/X11. Pure X11 via x11rb, single binary.

## Build & Test

```bash
source "$HOME/.cargo/env"
./build.sh dev                                                    # Build dev binary into /tmp/ptm-dev (run in place)
./build.sh release                                                # Build + copy binary to ~/.local/bin/ptm (persistent)
./install.sh                                                      # build.sh release + desktop entry/icon/tmux dep
CARGO_TARGET_DIR=/tmp/ptm-dev cargo test                          # 506 unit + 10 e2e (Xvfb, ~30s)
CARGO_TARGET_DIR=/tmp/ptm-dev cargo test --bin ptm                # unit tests only (~50ms)
CARGO_TARGET_DIR=/tmp/ptm-dev cargo test --test e2e_kill_session  # only the e2e suite (~25s)
DISPLAY=:0 /tmp/ptm-dev/release/ptm                               # Run dev build (needs X11 desktop)
PTM_BIN=/tmp/ptm-dev/release/ptm tests/e2e/<name>.sh              # Run a single e2e script standalone
```

The crate is a single-binary package (no `[lib]` target), so unit tests live under the `ptm` binary — `cargo test --lib` errors out; use `--bin ptm` instead.

**Dev vs production**: `build.sh` owns the build+place logic. Both modes compile into `/tmp` because `~/dev` is a `noexec` virtiofs mount — a binary built into the repo fails with "Bad address (os error 14)" (see `fix-virtiofs-exec.md`). `dev` targets `/tmp/ptm-dev` and you run it in place; `release` targets `/tmp/ptm-target` and then **copies** the binary to `~/.local/bin/ptm`. The copy (not a symlink into `/tmp`) is deliberate: `/tmp` is wiped on every reboot, so a symlink would dangle and PTM would look uninstalled after a restart — `~/.local/bin` is on persistent, exec-capable ext4. Separate target dirs keep a dev rebuild from clobbering the release artifact.

**System dependencies**: building needs Rust + X11 dev headers. Running needs an X11 display. The e2e suite is driven by `tests/e2e_kill_session.rs`, a thin Cargo integration wrapper that shells out to one shell script per `#[test]`. Ten scripts live under `tests/e2e/`:

- `menu_kills_session.sh` — right-click → Kill Session via the context menu, popup-accept path.
- `x_button_kills_session.sh` — click the `[x]` glyph on a session row, popup-accept path.
- `spawn_position.sh` — clicking `+ New tmux` snaps the spawned terminal to the sidebar anchor.
- `spawn_terminal_in_group.sh` — right-clicking a group header → "New terminal" snaps the spawned xterm to the sidebar anchor (same anchor as `+ New tmux`).
- `tmux_attach_runs_tmux.sh` — `+ New tmux` actually spawns a tmux client AND PTM binds the row's `item.session` so the sidebar marker renders.
- `tmux_attach_through_wrapper.sh` — when PTM's chosen terminal is a Debian `.wrapper` shim, the spawn uses `-e` so the wrapper forwards the command correctly.
- `recipes_survive_restart.sh` — Phase 5c Tier 0a: a saved tmux MEMBER reattaches to its live xterm by session-name match after PTM restart.
- `ptm_id_stamped_on_spawn.sh` — `+ New tmux` stamps a matching `@ptm_id` on the session AND `_PTM_ID` on the spawned window (persistent-identity scheme).
- `session_rebind_survives_restart.sh` — the rebind tier: `_PTM_SESSION` is stamped on the bound window, and after a hard PTM restart under a forged `_NET_WM_PID` collision (walk tier useless) the session binding is recovered from the stamp.
- `geometry_roundtrip_restart.sh` — the sidebar's screen position survives a graceful quit + relaunch under openbox (geometry is saved as the visible frame's root origin, never raw frame-relative ConfigureNotify coords).

Each script needs `xvfb`, `xdotool`, `xterm`, `openbox`, `tmux`, `scrot`, `wmctrl`, `xdpyinfo` (`sudo apt install xvfb xdotool xterm openbox tmux scrot wmctrl x11-utils`). The shared prelude/teardown lives in `tests/e2e/lib.sh`, which every script sources first: isolated Xvfb display on `:99` (`e2e_start_xvfb`), openbox when the test spawns terminals (`e2e_start_wm`), fresh `HOME` dirs registered for cleanup (`e2e_mktemp_dir`), PTM launch + window wait (`e2e_launch_ptm`), tool preflight (`e2e_require`), and one EXIT trap. Critically, lib.sh also isolates tmux (`TMUX_TMPDIR` + `unset TMUX` — the unset matters: with `$TMUX` leaked from a tmux-hosted shell, tmux ignores `TMUX_TMPDIR` and cleanup's `kill-server` would destroy the USER'S server; this happened once, don't reintroduce it). Per-script extra teardown goes in an `e2e_extra_cleanup()` function. Tests serialize via a `Mutex` in the wrapper (`E2E_LOCK`); each script picks PID-derived session names so reruns don't collide. The spawn-position test sets `PTM_TERMINAL_CMD=xterm` so it doesn't depend on gnome-terminal/DBus, runs openbox inside Xvfb so ptm sees `_NET_CLIENT_LIST` updates, and uses window-relative `xdotool mousemove --window` clicks so the openbox frame offset doesn't affect coordinates.

## Project Structure

```
Cargo.toml              # Package: process-tab-manager, binary: ptm
src/
  main.rs               # Everything: data model, EWMH, rendering, event loop, unit tests
tests/
  e2e_kill_session.rs   # Cargo integration harness: one #[test] per script under tests/e2e/
  e2e/
    lib.sh                          # shared prelude/teardown (tmux/X isolation, cleanup trap)
    menu_kills_session.sh           # right-click → Kill Session
    x_button_kills_session.sh       # [x] glyph + popup-accept
    spawn_position.sh               # `+ New tmux` snaps terminal to sidebar anchor
    spawn_terminal_in_group.sh      # group-header right-click → New terminal also snaps
    tmux_attach_runs_tmux.sh        # `+ New tmux` actually runs tmux + binds session marker
    tmux_attach_through_wrapper.sh  # Debian `.wrapper` shim forwards command via -e
    recipes_survive_restart.sh      # Phase 5c: tmux row reattaches after PTM restart
    ptm_id_stamped_on_spawn.sh      # @ptm_id + _PTM_ID stamped at spawn
    session_rebind_survives_restart.sh  # _PTM_SESSION rebind survives PTM restart
    geometry_roundtrip_restart.sh   # sidebar position survives graceful quit + relaunch
LICENSE
README.md
```

## Architecture

Single-file binary (~12000 LOC) with clean separation of concerns within `main.rs`:

- **Data model** (top): `Item`, `Group`, `DisplaySlot`, `DisplayRow`, `App` — all state management, group operations, drag-and-drop resolution
- **EWMH helpers**: `get_client_list`, `get_active_window`, `get_window_title`, `activate_window`, `snap_to_sidebar` — thin wrappers over X11 properties
- **Renderer**: double-buffered drawing to pixmap, copies to window. Items, group headers, ghost drag, drop indicators, hover, active highlight
- **Context menu**: override-redirect popup with pointer grab. `build_menu_entries` / `open_context_menu` / `draw_context_menu`
- **Event loop**: single `wait_for_event` loop with two modes: context menu (grab active) and normal
- **Tests**: `#[cfg(test)] mod tests` at bottom of `src/main.rs` — 496 unit tests covering pure state logic. The Xvfb-driven e2e harness lives separately under `tests/`.

### What's testable without X11

All `App` methods: `build_display_rows`, `hit_test_row`, `drop_index_from_y`, group operations (`create_group`, `add_to_group`, `remove_from_group`, `delete_group`, `rename_group`, `toggle_collapse`), drag resolution (`handle_drop`, `is_gap_in_group`, `reorder_within_group`), and `build_menu_entries`.

### What requires X11

Rendering, EWMH property queries, window activation/snapping, context menu popup, color allocation.

## x11rb API Notes

- `AtomEnum` variants (e.g. `AtomEnum::WM_CLASS`) can be passed directly to `get_property` — do NOT call `.into()` on them (causes ambiguous type inference)
- Font: Nimbus Mono L 13px with fallback to fixed 13px
- `image_text8` is Latin-1 only — no Unicode, use ASCII (`+`/`-` for collapse)
- Override-redirect windows need explicit event masks + pointer grab for menus
- `grab_pointer(owner_events: false)` routes all pointer events to grab window with relative coords

## Development Environment

Development happens directly on the X11 desktop (Cinnamon on Debian/Ubuntu). The project directory is at `~/dev/process-tab-manager`.

### Desktop recovery

If Cinnamon crashes or becomes unresponsive:

1. Restart Cinnamon: `DISPLAY=:0 nohup cinnamon --replace &`
2. Restart LightDM: `sudo systemctl restart lightdm` (from SSH only — see warning below)
3. Reboot: `sudo reboot` (from SSH only — see warning below)

**NEVER run `systemctl restart lightdm` or `sudo reboot` from within the desktop session.** These commands kill the X11 session, which destroys ALL terminals — including Claude Code itself.

### Environment notes

- **DISPLAY=:0** — all GUI commands need this when running from a non-GUI terminal (SSH, tmux)
- **CARGO_TARGET_DIR=/tmp/ptm-dev** — dev builds; `/tmp/ptm-target` is reserved for the installed production binary via `install.sh`. Both needed because virtiofs mount doesn't support exec
- **sudo** uses fingerprint GUI popup (works from Claude Code shell)
- **git/gh** auth is configured for account `science` (the origin account)

## Git Workflow

This is a solo project with no web deployment and no GitHub Actions, so the
workflow is deliberately lightweight: **work directly on `main`.**

- **Work, commit, and push on `main`.** No feature branches or PRs needed — commit to
  `main` locally and push `main` to `origin` directly. (This overrides the generic "branch
  before committing on the default branch" guidance.)
- **Local commits are encouraged.** Make intermediate commits freely to checkpoint progress.
- **Pushing to origin is fine without asking** — it's just `main` on GitHub, nothing
  deploys or runs from it.

## TDD Workflow

Every behavior change starts with a failing test (RED), then implementation (GREEN), then verification.

**Tier 1 — Unit tests (preferred).** `cargo test --bin ptm` for all state/group/DnD/parsing logic. No display needed; runs in ~50 ms.

**Tier 2 — Xvfb e2e tests.** `cargo test --test e2e_kill_session` for end-to-end flows that need a real X server, real tmux, and real keyboard/mouse events. Each `#[test]` shells out to a script under `tests/e2e/` (`menu_kills_session.sh`, `x_button_kills_session.sh`, `spawn_position.sh`). Use this tier for behaviors that can't be reproduced from pure state — popup focus interactions, window-spawn snapping, EWMH side effects. Slow (~30 s for the suite); add a script only when a Tier-1 test cannot reach the bug.

**Tier 3 — Manual visual review.** For rendering, colors, layout — launch the app and verify interactively.

### TDD Steps

1. Write or update a test:
   - Tier 1 → in the `#[cfg(test)] mod tests` block at the bottom of `src/main.rs`
   - Tier 2 → add a new `#[test]` in `tests/e2e_kill_session.rs` and a driver under `tests/e2e/<name>.sh`; `chmod +x` it and start it with `source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"` (provides `set -uo pipefail`, tmux/X isolation, and the cleanup trap)
2. Run the test — confirm it fails (RED)
3. Implement the change
4. Run the test — confirm it passes (GREEN)
5. Run the full test suite (`cargo test`) — confirm no regressions across both tiers
6. Commit test and implementation together

## Phase 5a — Recipe-capture UAT (Stage E)

Phase 5a observes — for every visible window — the info PTM would need to relaunch it after a reboot: the controlling executable (Layer 1: `exe` / `cmdline` / `cwd`) and the foreground job running inside any wrapping shell or tmux pane (Layer 2: workload). It does **not** persist or restore anything yet; the data is dumped on SIGUSR1 to a markdown file the user reviews against the live sidebar.

The UAT verdict gates everything else in Cluster 5: if /proc-based capture systematically misses workloads for the user's real apps, Phases 5b–5f don't ship.

### How to trigger a dump

```bash
kill -USR1 $(pgrep ptm)
```

PTM writes `$XDG_CACHE_HOME/ptm/recipes-snapshot.md` (falling back to `~/.cache/ptm/recipes-snapshot.md`) and prints the path on stderr if PTM was launched from a terminal. The dump is fast (a few ms — one /proc walk + one `tmux display-message` per attached session) and side-effect-free with respect to PTM state.

### How to review the dump

1. Open the windows you'd care about restoring after a reboot — at minimum the ones you remember relaunching manually after the last reboot. Include a mix: GUI apps (Firefox, file manager), plain terminals, tmux-attached terminals running real work (`claude`, `npm run dev`, `vim`).
2. Trigger the dump.
3. Open the markdown file. Each window gets a vertical block with a scan-signal header — e.g. `## 2 — ✓ Layer 1, ✓ Layer 2 (Job)` — followed by per-field breakdown.
4. Walk each block in sidebar order. The **PTM label** and **live title** fields disambiguate "which window is this row?" — match them against the sidebar and the window's title bar.
5. For each cell, judge correctness. Annotate inline with HTML comments where something looks wrong:
   ```markdown
   - cwd: `/home/steve`  <!-- ✗ should be /home/steve/Downloads -->
   ```

### What to check per block

- **Layer 1 cwd** matches `pwd` inside the terminal (or the app's actual working directory).
- **Layer 1 cmdline** reproduces what you'd type to relaunch the app. For terminals, this may be the wrapper (`gnome-terminal-server …`) rather than the shell — that's expected.
- **Tmux binding**, when present, names the correct session and points at a non-zero `pane_pid`.
- **Layer 2 workload** is what's actually running in the foreground. For an idle shell, the block should read `✓ Layer 2 (Idle)`. For a tmux'd `claude` session, it should read `✓ Layer 2 (Job)` with `cmdline: claude`. If it reads `✗ Layer 2 unreachable`, the printed `reason` should tell you why (no shell descendant, ambiguous shell parentage, etc.) — those are the bugs worth flagging.

### Verdict that gates Cluster 5

Green-light Phases 5b–5f when:

- ≥ 90 % of rows show ✓ on Layer 1, AND
- All tmux-attached workflow apps (claude, npm dev server, vim under tmux) show ✓ Layer 2 (Job) with sensible cmdlines.

If a non-trivial fraction of rows show ✗ Layer 2 with the same reason (e.g. "N shell descendants of gnome-terminal-server"), pause and decide whether to add a disambiguation strategy before 5b — restoring with the wrong workload would be worse than the current "you relaunch manually" status quo.

## Phases 5b + 5c — Persistence v2 + recipe-tier matching (MVP)

5b persists the Phase-5a-captured `LaunchRecipe` per group member to disk. 5c uses that data at restore time to anchor saved groups to live windows by tmux session or `_NET_WM_PID`, rather than the brittle (label, wm_class) cascade alone. Together they turn the "I restarted PTM and my groups re-shattered" failure mode into "groups re-attach cleanly".

### Wire format (v2)

Header bumps `v1` → `v2`. v1 files still load (members come back with `recipe: None`). Each MEMBER can carry up to three optional lines, in any order, until the next MEMBER or GROUP:

```
v2
GROUP\t<name>\t<collapsed>\t<kind>
MEMBER\t<label>\t<wm_class>\t<custom_prefix>
LAYER1\t<exe>\t<cwd>\t<pid>\t<argc>[\t<arg0>...]
TMUX\t<session_name>\t<session_id>\t<pane>\t<pane_pid>
LAYER2\tjob\t<exe>\t<cwd>\t<argc>[\t<arg0>...]
LAYER2\tidle
LAYER2\tunreachable\t<reason>
```

Field values are percent-encoded: `%` → `%25`, `\t` → `%09`, `\n` → `%0a`. Empty fields (`\t\t`) are the "no value" sentinel for exe/cwd/pid/session_id. v2 reader skips unknown line types (forward-compat for a hypothetical v3); v1 stays strict.

### Matching cascade (5 tiers, head-first)

`restore_groups` and `refresh_items` ghost re-match both route through the same logic:

1. **Tier 0a — Tmux session match** (Normal groups only). `member.recipe.tmux.session_name` against `item.session`.
2. **Tier 0b — Pid + corroborator** (Normal groups only). `member.recipe.pid_at_save` against `item.pid` AND (`item.label == member.label` OR `item.wm_class == member.wm_class`). The corroborator prevents pid-collision false matches when many windows share `gnome-terminal-server`'s pid.
3. **Tier 1 — Exact** `(label, wm_class)`.
4. **Tier 2 — Label-only**.
5. **Tier 3 — wm_class-only**.

TmuxSystem groups skip Tier 0a/0b because they're rebuilt every refresh from `list_tmux_sessions()`.

### Session-binding cascade (`bind_sessions`, 4 tiers)

Distinct from the group-member cascade above: this one decides which tmux session a
window row is bound to (`item.session`, the green marker). Runs every refresh:

1. **Claim** — a freshly-appeared wid consumes the head pending attach (FIFO). The only
   tier that fires at spawn under gnome-terminal-server's shared pid.
2. **Walk** — walk the tmux client pid up to the owning window via `_NET_WM_PID`.
   Returns None on pid collision (all gnome-terminal windows share the server pid), so
   it only works for xterm-style one-pid-per-window terminals.
3. **Rebind** — an unbound window carrying a `_PTM_SESSION` stamp (the session's
   `@ptm_id` uuid) rebinds to the live session with that uuid. The stamp lives on the X
   server, so this survives WM restarts (`cinnamon --replace`), desktop switches, and
   PTM restarts — everything that wipes carry's one-refresh memory. Stale stamps are
   inert (uuid absent from the live map) and never deleted. Rebind outranks carry
   because uuid identity beats name equality: after a rename plus name reuse, carry
   would bind the old name to the wrong session.
4. **Carry** — inherit the previous refresh's binding for the same wid, if the session
   name is still live. One refresh deep; still needed for unstamped windows.

`refresh_items` stamps `_PTM_SESSION` on every bound window after the cascade
(`plan_session_stamps` — diff-suppressed, so steady state is zero X writes; sessions
without `@ptm_id` get tagged via `ensure_session_ptm_id` first).

### Capture-at-save cost

`save_groups` calls `capture_recipes_for_save(&app)` first — one `ProcSnapshot::capture_all()` + one `query_tmux_pane` per distinct `item.session`. ~5–10 ms per call. Save fires at most a handful of times per minute under the existing debounce; cost is negligible.

### MVP UAT

After installing (`./install.sh`) and relaunching PTM:

1. Create a labelled group named `mvp-test` and drag a tmux-wrapped terminal row into it.
2. Inspect `~/.config/ptm/profiles/default/groups`: header is `v2`; the MEMBER for the tmux row is followed by a `LAYER1` line, a `TMUX` line, and a `LAYER2` line.
3. Quit PTM (Ctrl+Q or WM close). Reopen.
4. The `mvp-test` group should reappear with the tmux row already attached — no momentary ghosting, no row-shuffle.
5. Send `kill -USR1 $(pgrep ptm)` to dump current state; verify the dump shows the row under the `mvp-test` group with its `Tmux binding` line populated and a non-zero `pane_pid`.

If step 4 shows the row as a ghost (greyed) or in the wrong group, Tier 0a probably didn't fire — capture some data with `kill -USR1` and check that the saved member's TMUX line names the right session.
