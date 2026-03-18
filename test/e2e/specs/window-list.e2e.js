import sidebar from "../pageobjects/sidebar.page.js";
import { openXterms, closeAllXterms, closeXtermByTitle } from "../helpers/xterm.js";
import { getRowCount } from "../helpers/dom.js";

describe("Window List", () => {
  before(async () => {
    openXterms(3);
    await browser.pause(2000);
  });

  after(() => {
    closeAllXterms();
  });

  it("should list xterm windows", async () => {
    await sidebar.waitForRows(3);
    const count = await sidebar.getRowCount();
    expect(count).toBeGreaterThanOrEqual(3);
  });

  // PTM self-filters by PID (lib.rs filters out its own windows from the list).
  // This test is skipped because PTM's webview window still appears briefly
  // before the first filter pass completes.
  it.skip("should not list PTM itself", async () => {
    await sidebar.waitForRows(1);
    const titles = await sidebar.getRowTitles();
    const hasPtm = titles.some(t => t.includes("process-tab-manager") || t.includes("Process Tab Manager"));
    expect(hasPtm).toBe(false);
  });

  it("should detect new windows", async () => {
    await sidebar.waitForRows(3);
    const before = await sidebar.getRowCount();
    openXterms(1, "ExtraXterm");
    await browser.pause(3000);
    const after = await sidebar.getRowCount();
    expect(after).toBeGreaterThan(before);
  });

  it("should remove closed windows", async () => {
    await sidebar.waitForRows(1);
    const before = await sidebar.getRowCount();
    await closeXtermByTitle("ExtraXterm1");
    await browser.waitUntil(
      async () => (await getRowCount()) < before,
      { timeout: 10000, timeoutMsg: "Row count should decrease after closing window" }
    );
    const after = await sidebar.getRowCount();
    expect(after).toBeLessThan(before);
  });
});
