import { spawn } from "node:child_process";
import process from "node:process";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const projectRoot = path.resolve(__dirname, "../..");
const binary = path.join(projectRoot, "target/release/process-tab-manager");

let tauriDriver;

export const config = {
  specs: ["./test/**/*.e2e.js"],
  maxInstances: 1,
  hostname: "localhost",
  port: 4444,
  capabilities: [
    {
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

  onPrepare: function () {
    tauriDriver = spawn(
      path.join(process.env.HOME, ".cargo/bin/tauri-driver"),
      [],
      {
        stdio: ["ignore", "pipe", "pipe"],
        env: { ...process.env, DISPLAY: ":0" },
      }
    );

    tauriDriver.stderr.on("data", (data) => {
      console.error(`[tauri-driver] ${data}`);
    });

    // Wait for tauri-driver to be ready
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
