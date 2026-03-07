import { spawn, execSync } from "node:child_process";
import { readTestState } from "./state.js";

const tracked = [];

/**
 * Kill any leftover xterm test fixtures. Call before spawning new ones
 * to avoid stale windows from prior specs.
 */
export function killAllTestXterms() {
  for (const proc of tracked) {
    try { process.kill(-proc.pid, "SIGTERM"); } catch {}
  }
  tracked.length = 0;
  try { execSync("pkill -f 'xterm.*-e.*sleep 300' 2>/dev/null || true", { stdio: "ignore" }); } catch {}
}

export function openXterms(count = 1, prefix = "TestXterm") {
  const procs = [];
  for (let i = 0; i < count; i++) {
    const title = `${prefix}${i + 1}`;
    const proc = spawn("xterm", ["-title", title, "-e", "sleep 300"], {
      stdio: "ignore",
      detached: true,
      env: { ...process.env, DISPLAY: ":0" },
    });
    proc.unref();
    proc._xtermTitle = title;
    procs.push(proc);
    tracked.push(proc);
  }
  return procs;
}

export function closeAllXterms() {
  killAllTestXterms();
}

export async function closeXtermByTitle(title) {
  // Find wid from test state, then close via Tauri command
  const state = readTestState();
  const item = state?.items?.find(i => i.title === title);
  if (item) {
    await browser.execute(
      (wid) => window.__TAURI__.core.invoke("close_window", { wid }),
      item.wid
    );
  }
  // Remove from tracked
  const idx = tracked.findIndex(p => p._xtermTitle === title);
  if (idx !== -1) tracked.splice(idx, 1);
}
