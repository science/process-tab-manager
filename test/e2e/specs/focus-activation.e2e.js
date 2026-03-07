import sidebar from "../pageobjects/sidebar.page.js";
import { openXterms, closeAllXterms } from "../helpers/xterm.js";
import { readTestState, waitForState } from "../helpers/state.js";
import { getEvents, clearEvents } from "../helpers/events.js";

/**
 * Tests for the "inverted double-click" bug:
 *
 * BUG: When PTM has focus and user clicks a row, activate_window sends
 * X11 focus to the target window. PTM's webview loses focus (blur fires).
 * The user's next physical click on PTM is eaten by the WM to re-focus
 * the window — no DOM mousedown or click event fires. Only a focus event
 * fires. User must click twice.
 *
 * CURRENT WORKAROUND (main.js:547-588): Listens for mousedown while
 * windowBlurred and synthetically replays the click. This works when
 * GTK delivers mousedown but not click, but FAILS when the WM eats
 * the entire click (no mousedown at all).
 *
 * NEEDED FIX: On focus-after-blur with no intervening mousedown, query
 * pointer position and activate the row under it.
 *
 * These tests simulate the event sequences to verify both paths.
 */
describe("Focus Activation Workaround", () => {
  before(async () => {
    openXterms(3, "FocusTest");
    await browser.pause(2000);
    await sidebar.waitForRows(3);
  });

  after(() => {
    closeAllXterms();
  });

  async function getTestRows() {
    return browser.execute(() => {
      const rows = document.querySelectorAll(".row");
      const results = [];
      for (let i = 0; i < rows.length; i++) {
        const title = rows[i].querySelector(".title");
        if (title && title.textContent.startsWith("FocusTest")) {
          const rect = rows[i].getBoundingClientRect();
          results.push({
            index: i,
            wid: Number(rows[i].dataset.wid),
            title: title.textContent,
            midY: rect.top + rect.height / 2,
            midX: rect.left + rect.width / 2,
            isActive: rows[i].classList.contains("active"),
          });
        }
      }
      return results;
    });
  }

  it("should activate via mousedown-while-blurred (existing workaround)", async () => {
    // This tests the existing workaround: blur → mousedown → synthetic click
    const rows = await getTestRows();
    expect(rows.length).toBeGreaterThanOrEqual(2);

    // Activate row A first
    await sidebar.clickRow(rows[0].index);
    await browser.pause(1500);

    clearEvents();
    const rowB = (await getTestRows()).find(r => r.wid !== rows[0].wid);

    // Simulate: blur (PTM lost focus) → mousedown at row B (no click follows)
    await browser.execute((targetY, targetX) => {
      window.dispatchEvent(new Event("blur"));
      setTimeout(() => {
        document.dispatchEvent(new MouseEvent("mousedown", {
          bubbles: true, cancelable: true,
          clientX: targetX, clientY: targetY, button: 0,
        }));
      }, 50);
    }, rowB.midY, rowB.midX);

    await browser.pause(2000);

    const events = getEvents();
    const clickEvents = events.filter(e => e.includes("click") && e.includes(`wid=${rowB.wid}`));
    expect(clickEvents.length).toBeGreaterThanOrEqual(1);
  });

  it("should activate via focus-after-blur when WM eats entire click", async () => {
    // THIS IS THE BUG SCENARIO — currently no code handles it.
    // Sequence: blur → focus (no mousedown in between) → should activate
    // the row under the pointer.
    //
    // In real usage: user clicks PTM after window activation moved focus away.
    // WM eats the entire click (no mousedown). Only focus event fires.

    const rows = await getTestRows();
    expect(rows.length).toBeGreaterThanOrEqual(2);

    // Activate row A first
    await sidebar.clickRow(rows[0].index);
    await browser.pause(1500);

    clearEvents();
    const afterRows = await getTestRows();
    const rowB = afterRows.find(r => r.wid !== rows[0].wid);
    expect(rowB).toBeDefined();

    // Simulate the sequence that the WM produces when eating a click:
    // 1. blur (from activate_window moving focus away)
    // 2. focus (from user clicking PTM — but WM eats the click, only focus fires)
    // We set the pointer coordinates so the fix can query them.
    await browser.execute((targetY, targetX) => {
      // Blur first (activate_window stole focus)
      window.dispatchEvent(new Event("blur"));

      // After a moment, focus fires (WM re-focused PTM from user's click)
      // But NO mousedown fires — the WM ate it.
      // Move the pointer to row B's position first (simulates where user clicked)
      document.dispatchEvent(new MouseEvent("mousemove", {
        bubbles: true, clientX: targetX, clientY: targetY,
      }));

      setTimeout(() => {
        window.dispatchEvent(new Event("focus"));
      }, 100);
    }, rowB.midY, rowB.midX);

    // Wait for the fix to detect focus-after-blur and activate
    await browser.pause(2000);

    // The fix should have activated row B
    const events = getEvents();
    const clickEvents = events.filter(e => e.includes("click") && e.includes(`wid=${rowB.wid}`));
    expect(clickEvents.length).toBeGreaterThanOrEqual(1);

    // X11 active window should have changed to B
    const finalRows = await getTestRows();
    const activatedB = finalRows.find(r => r.wid === rowB.wid);
    expect(activatedB.isActive).toBe(true);
  });
});
