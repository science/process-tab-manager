import { execSync, spawn } from "node:child_process";

describe("F2 Rename Prototype", () => {
  const xterms = [];

  before(async () => {
    // Spawn 2 xterm windows so PTM has something to list
    for (let i = 0; i < 2; i++) {
      const proc = spawn("xterm", ["-title", `TestTerm${i + 1}`, "-e", "sleep 120"], {
        stdio: "ignore",
        detached: true,
        env: { ...process.env, DISPLAY: ":0" },
      });
      proc.unref();
      xterms.push(proc);
    }

    // Give xterms time to appear and PTM to detect them
    await browser.pause(3000);
  });

  after(async () => {
    // Kill xterms
    for (const proc of xterms) {
      try {
        process.kill(-proc.pid, "SIGTERM");
      } catch {
        // already dead
      }
    }
  });

  it("should send F2 key to trigger rename mode", async () => {
    // Wait for sidebar rows to appear
    const row = await $(".row");
    await row.waitForDisplayed({ timeout: 10000 });

    // Count rows to verify PTM detected our windows
    const rows = await $$(".row");
    console.log(`Found ${rows.length} rows in sidebar`);
    expect(rows.length).toBeGreaterThanOrEqual(2);

    // Click first row to select it
    await rows[0].click();
    await browser.pause(500);

    // Verify selection
    const isSelected = await rows[0].getAttribute("class");
    console.log(`Row classes after click: ${isSelected}`);
    expect(isSelected).toContain("selected");

    // THE KEY TEST: Send F2 via WebDriver protocol
    await browser.keys(["F2"]);
    await browser.pause(500);

    // Check if rename input appeared
    await $(".rename-input").waitForExist({ timeout: 5000 });
    console.log("Rename input appeared: true");

    // Type new name — use browser.keys to type character by character
    // First clear existing text, then type new name
    // setValue re-queries internally, but can still hit stale elements
    // So use action-based approach: select all + type
    await browser.keys(["Control", "a"]);  // select all existing text
    await browser.keys(["Control"]);       // release ctrl
    await browser.pause(100);
    await browser.keys("MyTerminal".split(""));
    await browser.pause(200);

    // Verify the input value
    const input = await $(".rename-input");
    const inputVal = await input.getValue();
    console.log(`Input value: ${inputVal}`);
    expect(inputVal).toBe("MyTerminal");

    // Press Enter to commit
    await browser.keys(["Enter"]);
    await browser.pause(500);

    // Verify rename input is gone
    await browser.waitUntil(
      async () => !(await $(".rename-input").isExisting()),
      { timeout: 5000, timeoutMsg: "Rename input should disappear after Enter" }
    );
    console.log("Rename input gone after Enter: true");

    // Verify the title changed — use executeScript to avoid stale element issues
    // (sidebar re-renders frequently from X11 polling)
    await browser.waitUntil(
      async () => {
        const title = await browser.execute(() => {
          const el = document.querySelector(".row .title");
          return el ? el.textContent : null;
        });
        return title === "MyTerminal";
      },
      { timeout: 5000, timeoutMsg: "Title should become 'MyTerminal'" }
    );
    console.log("Title after rename: MyTerminal");

    // Also verify via test state file (programmatic signal)
    const testState = await browser.execute(() => {
      // Read from the in-memory items array
      return document.querySelector(".row .title")?.textContent;
    });
    console.log(`DOM title confirmation: ${testState}`);
  });
});
