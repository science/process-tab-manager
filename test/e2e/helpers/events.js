import fs from "node:fs";

const EVENT_LOG_PATH = "/tmp/ptm-events.log";

export function getEvents() {
  try {
    return fs.readFileSync(EVENT_LOG_PATH, "utf8").split("\n").filter(Boolean);
  } catch {
    return [];
  }
}

export function hasEvent(pattern) {
  const events = getEvents();
  if (pattern instanceof RegExp) {
    return events.some(e => pattern.test(e));
  }
  return events.some(e => e.includes(pattern));
}

export async function waitForEvent(pattern, timeout = 5000) {
  await browser.waitUntil(
    () => hasEvent(pattern),
    { timeout, timeoutMsg: `Event matching "${pattern}" not found` }
  );
}

export function clearEvents() {
  try { fs.writeFileSync(EVENT_LOG_PATH, ""); } catch {}
}
