import sidebar from "../pageobjects/sidebar.page.js";
import { openXterms, closeAllXterms } from "../helpers/xterm.js";
import { readConfigState, deleteConfigState } from "../helpers/state.js";
import { waitForEvent, clearEvents } from "../helpers/events.js";

describe("Persistence", () => {
  before(async () => {
    // Clean state from prior specs so PTM starts fresh
    deleteConfigState();
    // Wait for any prior PTM close-saves to flush
    await new Promise(r => setTimeout(r, 2000));
    // Delete again in case a close-save wrote after first delete
    deleteConfigState();

    openXterms(2);
    await browser.pause(2000);
    await sidebar.waitForRows(2);
  });

  after(() => {
    closeAllXterms();
  });

  it("should create config state via click interaction", async () => {
    // Any user interaction triggers save_tx. A simple click + selection
    // change is enough to trigger the debounced save.
    await sidebar.clickRow(0);
    await browser.pause(500);

    // Verify selection happened
    const selected = await sidebar.getSelectedRow();
    expect(selected).not.toBeNull();

    // The click dispatched invoke("activate_window"), which doesn't trigger save_tx.
    // But the sidebar-update from X11 poll triggers writeTestState.
    // To trigger save_tx, we need a state-modifying action. Let's use reorder.
    clearEvents();
    await sidebar.reorderDown();
    await waitForEvent("keyboard-reorder");

    // Wait for debounced save
    await browser.waitUntil(
      () => readConfigState() !== null,
      { timeout: 15000, timeoutMsg: "Config state should be saved after reorder" }
    );

    const state = readConfigState();
    expect(state).not.toBeNull();
  });

  it("should have valid JSON in config state", async () => {
    const state = readConfigState();
    expect(state).not.toBeNull();
    expect(typeof state).toBe("object");
  });
});
