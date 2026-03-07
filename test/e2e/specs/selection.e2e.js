import sidebar from "../pageobjects/sidebar.page.js";
import { openXterms, closeAllXterms } from "../helpers/xterm.js";
import { hasEvent, waitForEvent, clearEvents } from "../helpers/events.js";

describe("Selection & Navigation", () => {
  before(async () => {
    openXterms(3);
    await browser.pause(2000);
    await sidebar.waitForRows(3);
  });

  after(() => {
    closeAllXterms();
  });

  it("should select row on click", async () => {
    await sidebar.clickRow(0);
    const selected = await sidebar.getSelectedRow();
    expect(selected).not.toBeNull();

    // Verify .selected class via DOM
    const row = await sidebar.getRowByIndex(0);
    expect(row.classes).toContain("selected");
  });

  it("should navigate with ArrowDown/Up", async () => {
    await sidebar.clickRow(0);
    const firstRow = await sidebar.getSelectedRow();
    expect(firstRow).not.toBeNull();

    await sidebar.navigateDown();
    const secondRow = await sidebar.getSelectedRow();
    expect(secondRow).not.toBeNull();
    expect(secondRow.wid).not.toBe(firstRow.wid);

    await sidebar.navigateUp();
    const backToFirst = await sidebar.getSelectedRow();
    expect(backToFirst.wid).toBe(firstRow.wid);
  });

  it("should activate window on Enter", async () => {
    await sidebar.clickRow(0);
    await browser.pause(300);
    clearEvents();

    await sidebar.pressKey("Enter");
    await waitForEvent("enter-activate");
  });
});
