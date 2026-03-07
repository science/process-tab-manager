import { spawn } from "node:child_process";
import process from "node:process";
import path from "node:path";
import fs from "node:fs";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const projectRoot = path.resolve(__dirname, "../..");
const binary = path.join(projectRoot, "target/release/process-tab-manager");

let tauriDriver;

export const config = {
  specs: ["./specs/**/*.e2e.js"],
  maxInstances: 1,
  hostname: "localhost",
  port: 4444,
  capabilities: [
    {
      maxInstances: 1,
      "tauri:options": {
        application: binary,
      },
    },
  ],
  logLevel: "warn",
  waitforTimeout: 10000,
  connectionRetryTimeout: 30000,
  connectionRetryCount: 3,
  framework: "mocha",
  reporters: ["spec"],
  mochaOpts: {
    ui: "bdd",
    timeout: 60000,
  },

  before: async function () {
    // Kill any stale xterms from prior spec runs
    const { execSync } = await import("node:child_process");
    try { execSync("pkill -f 'xterm.*-e.*sleep 300' 2>/dev/null || true", { stdio: "ignore" }); } catch {}

    // Clean test files
    for (const f of ["/tmp/ptm-test-state.json", "/tmp/ptm-events.log"]) {
      try { fs.unlinkSync(f); } catch {}
    }

    // Custom command: wait for sidebar rows to appear via browser.execute()
    browser.addCommand("waitForSidebarReady", async function (minRows = 1, timeout = 15000) {
      await browser.waitUntil(
        async () => {
          const count = await browser.execute(() =>
            document.querySelectorAll(".row").length
          );
          return count >= minRows;
        },
        { timeout, timeoutMsg: `Expected at least ${minRows} sidebar rows` }
      );
    });

    // Custom command: type text character by character
    browser.addCommand("typeText", async function (text) {
      await browser.keys(text.split(""));
    });

    // Custom command: select all text (Ctrl+A)
    browser.addCommand("selectAll", async function () {
      await browser.keys(["Control", "a"]);
      await browser.keys(["Control"]); // release
    });
  },

  afterSession: function () {
    // Clean config state between sessions so next spec starts clean.
    // PTM loads config at startup, so cleaning must happen BEFORE the next session.
    try { fs.unlinkSync(path.join(process.env.HOME, ".config/process-tab-manager/state.json")); } catch {}
  },

  onPrepare: function () {
    // PTM_NO_DOCK prevents setting DOCK window type, which triggers a Muffin bug
    // when rapidly creating/destroying windows (as E2E tests do).
    tauriDriver = spawn(
      path.join(process.env.HOME, ".cargo/bin/tauri-driver"),
      [],
      {
        stdio: ["ignore", "pipe", "pipe"],
        env: { ...process.env, DISPLAY: ":0", PTM_NO_DOCK: "1" },
      }
    );

    tauriDriver.stderr.on("data", (data) => {
      console.error(`[tauri-driver] ${data}`);
    });

    return new Promise((resolve, reject) => {
      const timeout = setTimeout(() => {
        resolve(); // Proceed anyway after 3s
      }, 3000);

      tauriDriver.stdout.on("data", (data) => {
        const msg = data.toString();
        console.log(`[tauri-driver] ${msg}`);
        if (msg.includes("listening")) {
          clearTimeout(timeout);
          resolve();
        }
      });

      tauriDriver.on("error", (err) => {
        clearTimeout(timeout);
        reject(err);
      });
    });
  },

  onComplete: function () {
    if (tauriDriver) {
      tauriDriver.kill();
      tauriDriver = null;
    }
  },
};
