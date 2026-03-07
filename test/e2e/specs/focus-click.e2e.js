import sidebar from "../pageobjects/sidebar.page.js";
import { openXterms, closeAllXterms } from "../helpers/xterm.js";
import { readTestState } from "../helpers/state.js";

describe("Focus Click", () => {
  before(async () => {
    openXterms(3);
    await browser.pause(2000);
    await sidebar.waitForRows(3);
  });

  after(() => {
    closeAllXterms();
  });

  it("should activate different window on WebDriver row click", async () => {
    const xtermRows = await browser.execute(() => {
      const rows = document.querySelectorAll(".row");
      const results = [];
      for (let i = 0; i < rows.length; i++) {
        const title = rows[i].querySelector(".title");
        if (title && title.textContent.startsWith("TestXterm")) {
          results.push({ index: i, wid: Number(rows[i].dataset.wid) });
        }
      }
      return results;
    });
    expect(xtermRows.length).toBeGreaterThanOrEqual(2);

    // Click first xterm
    await sidebar.clickRow(xtermRows[0].index);
    await browser.pause(500);
    const state1 = readTestState();

    // Re-read rows (sidebar re-renders after activation, changing indices)
    const freshRows = await browser.execute(() => {
      const rows = document.querySelectorAll(".row");
      const results = [];
      for (let i = 0; i < rows.length; i++) {
        const title = rows[i].querySelector(".title");
        if (title && title.textContent.startsWith("TestXterm")) {
          results.push({ index: i, wid: Number(rows[i].dataset.wid) });
        }
      }
      return results;
    });
    const otherXterm = freshRows.find(r => r.wid !== state1.selectedWid);
    expect(otherXterm).toBeDefined();

    // Click second xterm
    await sidebar.clickRow(otherXterm.index);
    await browser.pause(500);
    const state2 = readTestState();

    // Should have selected different windows
    expect(state1.selectedWid).not.toBe(state2.selectedWid);
  });

  it("should mark activated window as active in DOM", async () => {
    const xtermRows = await browser.execute(() => {
      const rows = document.querySelectorAll(".row");
      const results = [];
      for (let i = 0; i < rows.length; i++) {
        const title = rows[i].querySelector(".title");
        if (title && title.textContent.startsWith("TestXterm")) {
          results.push({ index: i, wid: Number(rows[i].dataset.wid) });
        }
      }
      return results;
    });

    await sidebar.clickRow(xtermRows[0].index);
    await browser.pause(500);

    // The clicked row should be selected
    const selected = await sidebar.getSelectedRow();
    expect(selected).not.toBeNull();
    expect(selected.wid).toBe(xtermRows[0].wid);
  });
});
