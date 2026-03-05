# Process Tab Manager

Vertical sidebar for managing application windows on Linux/X11. See PLAN.md for full architecture.

## Tech Stack

- **Rust 1.93+** with **GTK4-rs** (v0.10, glib v0.21) for UI and **x11rb** (v0.13) for X11 window management
- Single-threaded: x11rb FD registered with GLib main loop via `glib::unix_fd_add_local`
- System GTK4: 4.14.5 — use `load_from_data` for CSS (not `load_from_string` which requires GTK 4.16+)

## Build & Test

```bash
source "$HOME/.cargo/env"
cargo build                    # Build main binary
cargo test                     # Run pure module unit tests (no display needed)
cargo run                      # Run the app (requires X11 display)
```

## VM Testing

PTM requires an X11 desktop to run. Use the `ptm-test` VM (libvirt/KVM) for E2E testing and manual evaluation.

### VM basics

```bash
virsh start ptm-test                              # Start VM
virsh domifaddr ptm-test --source lease            # Get IP
ssh steve@<IP>                                     # SSH in
virt-viewer ptm-test                               # SPICE desktop viewer (from host)
./run.sh                                           # One-command bootstrap + launch
```

The project directory is mounted inside the VM at `/mnt/host-dev/process-tab-manager` via virtiofs — edits on the host are immediately visible in the VM.

### Running E2E tests

```bash
PTM_VM=ptm-test bash test/vm-e2e-test.sh           # Run all E2E tests
PTM_VM=ptm-test bash test/vm-e2e-test.sh test_name  # Run a single test
```

The E2E script handles: VM health check, build, prerequisite checks, Xauthority cookie sync, launching/stopping PTM, xterm window management, and screenshot capture to `test/screenshots/`.

### VM recovery

If the VM becomes non-functional (broken desktop, corrupted state, login loop):

**Option 1 — Revert to snapshot** (if `ptm-ready` snapshot exists, ~30s):
```bash
virsh snapshot-revert ptm-test ptm-ready
virsh start ptm-test
```

**Option 2 — Nuke and re-clone** (from scratch, ~5 min):
```bash
virsh destroy ptm-test 2>/dev/null; virsh undefine ptm-test --remove-all-storage
sudo bash vm/clone-vm.sh ptm-test --ram 4096 --cpus 2 --mount /home/steve/dev:devmount
./run.sh   # Bootstraps Rust, packages, builds, launches
```

**After any recovery, take a snapshot** so the next recovery is instant:
```bash
# Clean up first
ssh steve@<IP> "pkill -f process.tab.manager; pkill xterm; true"
virsh shutdown ptm-test && sleep 15
virsh snapshot-create-as ptm-test ptm-ready --description "Cinnamon desktop, Rust, GTK4 dev libs, xdotool, xterm, imagemagick — ready for PTM E2E"
```

### VM rules (avoid breaking the VM)

- **Never `snapshot-revert` to an unknown snapshot.** The `test-env-ready` snapshot was from a different project and broke the VM. Only revert to `ptm-ready` (or a snapshot you created and verified).
- **For transient WM issues** (stuck `_NET_ACTIVE_WINDOW`, frozen Cinnamon), prefer lightweight fixes in this order:
  1. Restart Cinnamon: `vm_ssh "DISPLAY=:0 nohup cinnamon --replace &"`
  2. Restart LightDM: `vm_ssh "sudo systemctl restart lightdm"` (+ re-sync xauth)
  3. Reboot VM: `virsh reboot ptm-test`
  4. Revert to `ptm-ready` snapshot (last resort before nuke)
- **Never `snapshot-revert` as a first response** to a test failure or WM glitch — it's a nuclear option.
- **Snapshots are fragile** across VM configuration changes. When in doubt, nuke+re-clone is safer than reverting an old snapshot.

### VM quirks

- **Xauthority cookies** — cloned/reverted VMs can have stale X cookies. The E2E script auto-syncs them; for manual use: `DISPLAY=:0 xdpyinfo` to verify, and if broken, run the cookie sync from the E2E preflight
- **LightDM autologin** — cloud-init configures `cinnamon2d` session in `/etc/lightdm/lightdm.conf.d/50-autologin.conf`. If the desktop isn't up after boot, check that file exists and LightDM is running: `systemctl status lightdm`
- **virtiofs stale mtimes** — `cargo build --release` in the VM may not detect source changes made on the host. The E2E script auto-touches source files before building. For manual builds, run `find src -name '*.rs' -exec touch {} +` inside the VM first

## Module Separation Rule

**Pure modules** (`state.rs`, `config.rs`, `geometry.rs`, `filter.rs`) — zero GTK/X11 imports. Tested with `cargo test`.

**Impure modules** (`app.rs`, `sidebar.rs`, `row.rs`, `bridge.rs`, `x11/*`) — depend on GTK4/x11rb. Tested via VM E2E.

## x11rb API Notes

- `AtomEnum` variants (e.g. `AtomEnum::WM_CLASS`) can be passed directly to `get_property` — do NOT call `.into()` on them (causes ambiguous type inference)
- CSS: use `CssProvider::load_from_data(str)`, not `load_from_string` (unavailable in GTK 4.14)
- gtk4-rs 0.10 uses glib 0.21 and gio 0.21 (not 0.20 as originally planned)

## TDD Workflow

Every behavior change starts with a failing test (RED), then implementation (GREEN), then verification. Pure modules use `cargo test`. Impure modules use VM E2E tests — `cargo test` alone is insufficient for desktop behavior.

### Verification Tiers

**Tier 1 — Programmatic signals (preferred).** Use `xdotool getactivewindow`, window counts, file existence, process checks. These return deterministic pass/fail.
- Example: verify focus didn't change by comparing window IDs before/after a click
- Example: confirm state.json was created after shutdown
- Always prefer this tier when a behavior produces a measurable signal

**Tier 2 — Cropped screenshots with LLM evaluation.** When programmatic signals are insufficient (visual layout, CSS styling, icon rendering, DnD mid-drag feedback), capture screenshots, crop to the region of interest, and use the LLM (via `Read` tool on the image) as a visual evaluator.
- Use `screenshot` helper for full-screen capture (already in E2E script)
- Use `screenshot_crop` helper: capture + ImageMagick `convert -crop WxH+X+Y` for targeted regions
- Use `xdotool getwindowgeometry` to get window position dynamically for accurate crops
- Sequential screenshots (before/after/during) for behavior that unfolds over time
- Cropped images saved to `test/screenshots/` for LLM to read and evaluate

**Tier 3 — Manual visual review.** For subjective polish (does it "feel right"?), launch the full app in the VM via `virt-viewer ptm-test` and evaluate interactively.

### TDD Steps

1. Write or update a test in the appropriate location (unit test in module, or E2E in `vm-e2e-test.sh`)
2. Run the test — confirm it fails (RED)
3. Implement the change
4. Run the test — confirm it passes (GREEN)
5. Run the full test suite — confirm no regressions
6. Commit test and implementation together

## Project Structure

See PLAN.md § Project Structure for the full layout.
