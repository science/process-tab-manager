import fs from "node:fs";
import path from "node:path";
import os from "node:os";

const TEST_STATE_PATH = "/tmp/ptm-test-state.json";
const EVENT_LOG_PATH = "/tmp/ptm-events.log";
const CONFIG_STATE_PATH = path.join(
  os.homedir(), ".config", "process-tab-manager", "state.json"
);

export function readTestState() {
  try {
    return JSON.parse(fs.readFileSync(TEST_STATE_PATH, "utf8"));
  } catch {
    return null;
  }
}

export function getWindowItems() {
  const state = readTestState();
  if (!state) return [];
  return state.items.filter(i => i.kind === "window");
}

export function getGroupItems() {
  const state = readTestState();
  if (!state) return [];
  return state.items.filter(i => i.kind === "group");
}

export function getSelectedWid() {
  const state = readTestState();
  return state?.selectedWid ?? null;
}

export function hasWindowWithTitle(title) {
  return getWindowItems().some(w => w.title === title);
}

export async function waitForState(predicate, timeout = 10000) {
  await browser.waitUntil(
    () => {
      const state = readTestState();
      return state && predicate(state);
    },
    { timeout, timeoutMsg: "State predicate not satisfied" }
  );
  return readTestState();
}

export function readConfigState() {
  try {
    return JSON.parse(fs.readFileSync(CONFIG_STATE_PATH, "utf8"));
  } catch {
    return null;
  }
}

export function cleanTestFiles() {
  for (const f of [TEST_STATE_PATH, EVENT_LOG_PATH]) {
    try { fs.unlinkSync(f); } catch {}
  }
}

export function deleteConfigState() {
  try { fs.unlinkSync(CONFIG_STATE_PATH); } catch {}
}

export { CONFIG_STATE_PATH };
