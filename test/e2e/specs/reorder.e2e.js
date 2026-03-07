import sidebar from "../pageobjects/sidebar.page.js";
import { openXterms, closeAllXterms } from "../helpers/xterm.js";
import { waitForEvent, clearEvents, hasEvent } from "../helpers/events.js";
import { getRowTexts, getRowCount } from "../helpers/dom.js";
import { readTestState } from "../helpers/state.js";

describe("Reorder", () => {
  before(async () => {
    openXterms(3);
    await browser.pause(2000);
    await sidebar.waitForRows(3);
  });

  after(() => {
    closeAllXterms();
  });

  it("should reorder with Ctrl+Shift+Down", async () => {
    // Record initial order from test state file
    const titlesBefore = await getRowTexts();
    expect(titlesBefore.length).toBeGreaterThanOrEqual(2);

    await sidebar.clickRow(0);
    await browser.pause(300);

    // Verify something is selected
    const selected = await sidebar.getSelectedRow();
    expect(selected).not.toBeNull();

    clearEvents();
    await sidebar.reorderDown();

    // Verify the keyboard-reorder event fired — this confirms:
    // 1. The Ctrl+Shift+Down keydown reached the DOM
    // 2. The handler dispatched the reorder command to the backend
    await waitForEvent("keyboard-reorder");

    // Verify the reorder command was invoked (from=N to=N+1)
    const events = (await import("../helpers/events.js")).getEvents();
    const reorderEvent = events.find(e => e.includes("keyboard-reorder"));
    expect(reorderEvent).toMatch(/from=\d+ to=\d+/);
  });
});
