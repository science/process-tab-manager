# Process Tab Manager

Vertical sidebar for managing application windows on Linux/X11. See PLAN.md for full architecture.

## Tech Stack

- **Tauri v2** with HTML/CSS/JS frontend and Rust backend
- **ptm-core** library crate — pure Rust modules (state, config, geometry, filter, bridge, x11) with **x11rb** (v0.13) for X11 window management
- **src-tauri** — Tauri app crate, depends on ptm-core. Background thread polls X11 events and emits Tauri events to frontend
- **frontend/** — vanilla HTML/CSS/JS (no framework). Dark theme, Firefox-style sidebar
- Cargo workspace: root `Cargo.toml` with members `ptm-core` and `src-tauri`

## Build & Test

```bash
source "$HOME/.cargo/env"
cargo test -p ptm-core                              # Run unit tests (no display needed)
cargo build -p process-tab-manager --release        # Build Tauri binary
cd test/e2e && npx wdio run wdio.conf.js            # Run all E2E tests (needs X11 desktop)
cd test/e2e && npx wdio run wdio.conf.js --spec specs/rename.e2e.js  # Run single spec
```

The release binary is at `target/release/process-tab-manager`.

Quick launch for interactive use: `./run.sh` (builds, launches PTM + test xterms).

## Project Structure

```
Cargo.toml                  # Workspace root (members: ptm-core, src-tauri)
ptm-core/                   # Pure Rust library crate
  src/
    lib.rs                  # Re-exports
    state.rs                # Window/group state, renames, display ordering
    config.rs               # XDG config paths
    geometry.rs             # Monitor geometry, window snapping
    filter.rs               # Window class filtering
    bridge.rs               # X11 event translation (PtmEvent enum)
    x11/
      mod.rs                # X11 connection setup, atom cache
      connection.rs         # get_property helpers
      actions.rs            # activate, close, snap, minimize
      ewmh.rs               # EWMH property queries
      monitor.rs            # Xinerama monitor detection
  tests/                    # Unit tests
src-tauri/                  # Tauri app crate
  Cargo.toml
  tauri.conf.json
  src/
    main.rs                 # Entry point
    lib.rs                  # Tauri setup, commands, event wiring
    x11_monitor.rs          # Background thread: polls X11, emits sidebar-update
    icon_resolver.rs        # .desktop file icon lookup (no GTK dependency)
frontend/                   # Web frontend (served by Tauri webview)
  index.html
  main.js                   # Tauri event listeners, sidebar rendering, interactions
  style.css                 # Firefox dark theme (#1c1b22)
test/
  e2e/                      # WebdriverIO E2E test suite
    wdio.conf.js            # Config: tauri-driver lifecycle, custom commands
    package.json            # npm deps
    helpers/                # dom.js, state.js, events.js, xterm.js
    pageobjects/            # sidebar.page.js, context-menu.page.js, rename.page.js
    specs/                  # *.e2e.js test files
  tauri-e2e-runner.sh       # Legacy bash E2E tests
  screenshots/              # E2E test screenshots
run.sh                      # One-command build + launch
```

## Module Separation Rule

**ptm-core** (pure Rust, no UI deps) — `state.rs`, `config.rs`, `geometry.rs`, `filter.rs`, `bridge.rs`, `x11/*`. Tested with `cargo test -p ptm-core`.

**src-tauri** (Tauri backend) — `lib.rs`, `x11_monitor.rs`, `icon_resolver.rs`. Depends on ptm-core + tauri. Tested via E2E.

**frontend** (HTML/CSS/JS) — `main.js`, `style.css`, `index.html`. Tested via E2E.

## x11rb API Notes

- `AtomEnum` variants (e.g. `AtomEnum::WM_CLASS`) can be passed directly to `get_property` — do NOT call `.into()` on them (causes ambiguous type inference)

## Development Environment

Development happens directly on the X11 desktop (Cinnamon on Debian/Ubuntu). The project directory is at `~/dev/process-tab-manager` (also accessible via `/mnt/host-dev/process-tab-manager` — same virtiofs mount from the host).

### Running locally

```bash
./run.sh                                    # Build + launch PTM + test xterms
cd test/e2e && npx wdio run wdio.conf.js    # Run all E2E tests
cd test/e2e && npx wdio run wdio.conf.js --spec specs/rename.e2e.js  # Single spec
cargo test -p ptm-core                      # Unit tests only
```

### Desktop recovery

If Cinnamon crashes or becomes unresponsive:

1. Restart Cinnamon: `DISPLAY=:0 nohup cinnamon --replace &`
2. Restart LightDM: `sudo systemctl restart lightdm` (from SSH only — see warning below)
3. Reboot: `sudo reboot` (from SSH only — see warning below)

**NEVER run `systemctl restart lightdm` or `sudo reboot` from within the desktop session.** These commands kill the X11 session, which destroys ALL terminals — including Claude Code itself. Only use these from an external SSH session. For WM issues when working locally, use option 1 (Cinnamon restart) which is safe.

### Environment notes

- **DISPLAY=:0** — all GUI commands need this when running from a non-GUI terminal (SSH, tmux)
- **sudo** is passwordless for user `steve`
- **Tauri v2 embeds frontend at compile time** via `generate_context!()` proc macro. If frontend files change, you need a rebuild. For stale mtime issues: `find src-tauri -name '*.rs' -exec touch {} + && find frontend -type f -exec touch {} +`
- **git/gh** auth is configured for account `science` (the origin account)

## Git Workflow

- **Local commits are encouraged.** Make intermediate commits freely to checkpoint progress — they're cheap and reversible.
- **Never push to origin without asking the user first.** Pushing affects the shared remote and should always be explicitly confirmed.

## TDD Workflow

Every behavior change starts with a failing test (RED), then implementation (GREEN), then verification. Pure modules use `cargo test -p ptm-core`. UI/integration behaviors use E2E tests.

### Verification Tiers

**Tier 1 — Unit tests + E2E specs (preferred).** `cargo test -p ptm-core` for pure Rust. `npx wdio run wdio.conf.js` for UI/integration via WebdriverIO.

**Tier 2 — CSS checks in E2E.** Use `browser.execute()` with `getComputedStyle()` for visual verification (colors, layout).

**Tier 3 — Manual visual review.** For subjective polish, launch the full app and evaluate interactively.

### E2E Testing Notes (WebdriverIO + tauri-driver)

- **WebdriverIO** delivers keystrokes via WebDriver protocol — `browser.keys()` bypasses X11 input entirely, solving keyboard focus issues.
- **Stale element rule**: DOM re-renders every ~1s from X11 polling. **Never store `$()` refs across `await` boundaries.** All DOM reads go through `browser.execute()`.
- **Page Object Model**: `test/e2e/pageobjects/` own all selectors. Specs are declarative — never contain CSS selectors.
- **State file**: Frontend writes `/tmp/ptm-test-state.json` on every sidebar-update for programmatic verification.
- **Event log**: Frontend appends to `/tmp/ptm-events.log` for event verification.
- **Helpers**: `dom.js` (stale-safe queries), `state.js` (test state reader), `events.js` (event log), `xterm.js` (fixture windows).

### TDD Steps

1. Write or update a test (unit in ptm-core, or E2E spec in `test/e2e/specs/`)
2. Run the test — confirm it fails (RED)
3. Implement the change
4. Run the test — confirm it passes (GREEN)
5. Run the full test suite — confirm no regressions
6. Commit test and implementation together
