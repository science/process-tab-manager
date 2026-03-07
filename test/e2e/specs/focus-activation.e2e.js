import sidebar from "../pageobjects/sidebar.page.js";
import { openXterms, closeAllXterms } from "../helpers/xterm.js";
import { readTestState, waitForState } from "../helpers/state.js";
import { getEvents, clearEvents } from "../helpers/events.js";

/**
 * Tests for the "inverted double-click" bug:
 *
 * BUG: After activate_window moves X11 focus away, the user's next click
 * on PTM alternates between needing 1 click and 2 clicks. Root cause:
 * when the WM refocuses PTM AND delivers the click through, both the
 * focus workaround (Case B) and the normal click handler fire, causing
 * a double-activation that corrupts state for the next click.
 *
 * FIX: Case B (focus-after-blur) schedules activation on a timer.
 * If mousedown arrives before the timer fires, the timer is cancelled
 * and the normal click path handles it. Only one activation per click.
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

  it("Case B: should activate via focus-after-blur when WM eats entire click", async () => {
    const rows = await getTestRows();
    expect(rows.length).toBeGreaterThanOrEqual(2);

    await sidebar.clickRow(rows[0].index);
    await browser.pause(1500);

    clearEvents();
    const afterRows = await getTestRows();
    const rowB = afterRows.find(r => r.wid !== rows[0].wid);

    // Simulate: blur (activate_window stole focus) → focus (user clicked PTM,
    // WM ate the click entirely — no mousedown follows)
    await browser.execute((targetY) => {
      document.dispatchEvent(new MouseEvent("mousemove", {
        bubbles: true, clientX: 100, clientY: targetY,
      }));
      window.dispatchEvent(new Event("blur"));
      setTimeout(() => {
        window.dispatchEvent(new Event("focus"));
      }, 50);
    }, rowB.midY);

    await browser.pause(2000);

    const events = getEvents();
    const clickEvents = events.filter(e => e.includes("click") && e.includes(`wid=${rowB.wid}`));
    expect(clickEvents.length).toBeGreaterThanOrEqual(1);
  });

  it("should cancel Case B when mousedown follows focus (no double-fire)", async () => {
    // When WM refocuses PTM AND delivers the click through, both focus
    // and mousedown fire. Case B's timer must be cancelled by mousedown
    // so only the normal click handler fires (exactly 1 activation).
    const rows = await getTestRows();
    expect(rows.length).toBeGreaterThanOrEqual(2);

    await sidebar.clickRow(rows[0].index);
    await browser.pause(1500);

    clearEvents();
    const afterRows = await getTestRows();
    const rowB = afterRows.find(r => r.wid !== rows[0].wid);

    // Simulate: blur → focus → mousedown → click (WM delivers everything)
    // The focus handler schedules Case B timer (80ms).
    // The mousedown handler cancels the timer.
    // The click event fires the normal row click handler.
    // Result: exactly 1 activation, not 2.
    await browser.execute((idx) => {
      window.dispatchEvent(new Event("blur"));
      setTimeout(() => {
        window.dispatchEvent(new Event("focus"));
        // mousedown follows shortly after focus (same user click)
        setTimeout(() => {
          // Simulate mousedown (cancels Case B timer)
          document.dispatchEvent(new MouseEvent("mousedown", {
            bubbles: true, cancelable: true, button: 0,
          }));
          // Then the normal click on the row
          const row = document.querySelectorAll(".row")[idx];
          if (row) row.click();
        }, 10);
      }, 50);
    }, rowB.index);

    await browser.pause(2000);

    const events = getEvents();
    const clickEvents = events.filter(e => e.includes("click") && e.includes(`wid=${rowB.wid}`));
    // Should be exactly 1 — the normal click. Case B should NOT have fired.
    expect(clickEvents.length).toBe(1);
  });

  it("should not alternate: 3 sequential focus-after-blur activations", async () => {
    // The original bug: activations alternate between working and not working.
    // This test performs 3 sequential blur→focus sequences (simulating the user
    // clicking 3 different rows after each activation steals focus).
    // All 3 should activate successfully — no alternation.
    const rows = await getTestRows();
    expect(rows.length).toBeGreaterThanOrEqual(3);

    for (let i = 0; i < rows.length; i++) {
      clearEvents();
      const target = (await getTestRows())[i];

      // Simulate: activate moved focus away → user clicks back on PTM
      await browser.execute((targetY) => {
        document.dispatchEvent(new MouseEvent("mousemove", {
          bubbles: true, clientX: 100, clientY: targetY,
        }));
        window.dispatchEvent(new Event("blur"));
        setTimeout(() => {
          window.dispatchEvent(new Event("focus"));
        }, 50);
      }, target.midY);

      await browser.pause(2000);

      const events = getEvents();
      const clickEvents = events.filter(e => e.includes("click") && e.includes(`wid=${target.wid}`));
      expect(clickEvents.length).toBeGreaterThanOrEqual(1);

      // Verify X11 active window changed
      const activeWid = await browser.execute(
        () => window.__TAURI__.core.invoke("get_active_window_id")
      );
      expect(activeWid).toBe(target.wid);
    }
  });
});
