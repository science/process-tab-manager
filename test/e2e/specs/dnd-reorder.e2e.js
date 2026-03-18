import sidebar from "../pageobjects/sidebar.page.js";
import { openXterms, closeAllXterms } from "../helpers/xterm.js";
import { readTestState, waitForState } from "../helpers/state.js";
import { dragAndDrop, dragOver, cancelDrag, getDropHighlight } from "../helpers/dnd.js";
import { clearEvents } from "../helpers/events.js";

/**
 * Get an identity key for an item (works for both windows and groups).
 */
function itemKey(item) {
  if (item.kind === "group") return `group:${item.gid}`;
  return `window:${item.wid}`;
}

describe("DnD Reorder", () => {
  before(async () => {
    openXterms(3);
    await browser.pause(2000);
    await sidebar.waitForRows(3);
    // Wait for test xterms to appear in state
    await waitForState(
      (s) => s.items.filter(i => i.title && i.title.startsWith("TestXterm")).length >= 3,
      10000
    );
  });

  after(() => {
    closeAllXterms();
  });

  it("should reorder row down via drag-and-drop", async () => {
    const stateBefore = readTestState();
    const keysBefore = stateBefore.items.map(itemKey);
    expect(keysBefore.length).toBeGreaterThanOrEqual(3);

    // Drag item at position 0 to position 2
    // reorder(0, 2): remove[0], insert at 2 → item moves from first to third
    clearEvents();
    await dragAndDrop(0, 2);

    // After reorder(0, 2): [A,B,C,...] → [B,C,A,...]
    // Position 0 should now have what was at position 1
    const stateAfter = await waitForState(
      (s) => itemKey(s.items[0]) === keysBefore[1],
      5000
    );
    const keysAfter = stateAfter.items.map(itemKey);
    expect(keysAfter[0]).toBe(keysBefore[1]);
    expect(keysAfter[1]).toBe(keysBefore[2]);
    expect(keysAfter[2]).toBe(keysBefore[0]);
  });

  it("should reorder row up via drag-and-drop", async () => {
    const stateBefore = readTestState();
    const keysBefore = stateBefore.items.map(itemKey);
    const lastIdx = keysBefore.length - 1;

    // Drag last item to position 0
    clearEvents();
    await dragAndDrop(lastIdx, 0);

    // After reorder(lastIdx, 0): last item moves to front
    const stateAfter = await waitForState(
      (s) => itemKey(s.items[0]) === keysBefore[lastIdx],
      5000
    );
    expect(itemKey(stateAfter.items[0])).toBe(keysBefore[lastIdx]);
  });

  it("should handle drop after last row", async () => {
    const stateBefore = readTestState();
    const keysBefore = stateBefore.items.map(itemKey);
    const firstKey = keysBefore[0];
    const itemCount = keysBefore.length;

    // Drag first item past all items (to == length)
    clearEvents();
    await dragAndDrop(0, itemCount);

    // First item should now be last
    const stateAfter = await waitForState(
      (s) => s.items.length >= itemCount &&
             itemKey(s.items[s.items.length - 1]) === firstKey,
      5000
    );
    expect(itemKey(stateAfter.items[stateAfter.items.length - 1])).toBe(firstKey);
  });

  it("should not reorder when dropped on same position", async () => {
    const stateBefore = readTestState();
    const keysBefore = stateBefore.items.map(itemKey);

    clearEvents();
    await dragAndDrop(1, 1);

    await browser.pause(500);
    const stateAfter = readTestState();
    const keysAfter = stateAfter.items.map(itemKey);
    expect(keysAfter).toEqual(keysBefore);
  });

  it("should show drop highlight during drag", async () => {
    // Drag row 0 over row 1 without dropping
    await dragOver(0, 1);
    await browser.pause(200);

    const highlight = await getDropHighlight();
    expect(highlight).not.toBeNull();
    expect(highlight.index).toBe(1);

    // Clean up: cancel the drag
    await cancelDrag();
    await browser.pause(300);

    // Highlight should be gone after cancel
    const afterCancel = await getDropHighlight();
    expect(afterCancel).toBeNull();
  });
});
