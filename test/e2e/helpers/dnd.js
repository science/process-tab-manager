// DnD simulation helper — uses XI2 bridge functions (__ptm_xi2_press/move/release).
// XI2 is the sole mouse input path; no native pointer event handlers on rows.

/**
 * Simulate a drag-and-drop from one sidebar item index to another.
 * Goes through the XI2 handler path (press → move past threshold → move to target → release).
 * @param {number} fromIndex - Source item index (0-based in .row/.group-header list)
 * @param {number} toIndex - Target drop position (0-based)
 */
export async function dragAndDrop(fromIndex, toIndex) {
  await browser.execute((from, to) => {
    const rows = document.querySelectorAll(".row, .group-header");
    const source = rows[from];
    if (!source) return;

    const sourceRect = source.getBoundingClientRect();
    const startX = sourceRect.left + sourceRect.width / 2;
    const startY = sourceRect.top + sourceRect.height / 2;

    // Compute target center Y
    let targetY;
    if (to < rows.length) {
      const rect = rows[to].getBoundingClientRect();
      targetY = rect.top + rect.height / 2;
    } else {
      // After last row
      const lastRect = rows[rows.length - 1].getBoundingClientRect();
      targetY = lastRect.bottom + 5;
    }

    // 1. Press on source
    window.__ptm_xi2_press(startX, startY, 1);

    // 2. Move past threshold
    const midY = startY + (targetY > startY ? 10 : -10);
    window.__ptm_xi2_move(startX, midY);

    // 3. Move to target position
    window.__ptm_xi2_move(startX, targetY);
  }, fromIndex, toIndex);

  // Release must be awaited (completeDrop is async)
  await browser.execute(async (from, to) => {
    const rows = document.querySelectorAll(".row, .group-header");
    let targetY;
    if (to < rows.length) {
      const rect = rows[to].getBoundingClientRect();
      targetY = rect.top + rect.height / 2;
    } else {
      const lastRect = rows[rows.length - 1].getBoundingClientRect();
      targetY = lastRect.bottom + 5;
    }
    const source = rows[from];
    const startX = source ? source.getBoundingClientRect().left + source.getBoundingClientRect().width / 2 : 100;
    await window.__ptm_xi2_release(startX, targetY);
  }, fromIndex, toIndex);

  // Wait for the async drop handler (invoke + refreshSidebar)
  await browser.pause(1500);
}

/**
 * Check if any row currently has a drop indicator line.
 * Returns { element: "row"|"group-header", position: "before"|"after", index } or null.
 */
export async function getDropHighlight() {
  return browser.execute(() => {
    const before = document.querySelector(".drop-before");
    const after = document.querySelector(".drop-after");
    const el = before || after;
    if (!el) return null;
    const rows = document.querySelectorAll(".row, .group-header");
    let index = -1;
    for (let i = 0; i < rows.length; i++) {
      if (rows[i] === el) { index = i; break; }
    }
    return {
      element: el.classList.contains("group-header") ? "group-header" : "row",
      position: before ? "before" : "after",
      index,
    };
  });
}

/**
 * Simulate press + move (without release) to verify visual highlights.
 * @param {number} fromIndex - Source item index
 * @param {number} overIndex - Item index to hover over
 */
export async function dragOver(fromIndex, overIndex) {
  await browser.execute((from, over) => {
    const rows = document.querySelectorAll(".row, .group-header");
    const source = rows[from];
    if (!source) return;

    const sourceRect = source.getBoundingClientRect();
    const startX = sourceRect.left + sourceRect.width / 2;
    const startY = sourceRect.top + sourceRect.height / 2;

    const target = rows[over];
    if (!target) return;
    const targetRect = target.getBoundingClientRect();
    const targetY = targetRect.top + targetRect.height / 2;

    // 1. Press
    window.__ptm_xi2_press(startX, startY, 1);

    // 2. Move past threshold
    const midY = startY + (targetY > startY ? 10 : -10);
    window.__ptm_xi2_move(startX, midY);

    // 3. Move to target
    window.__ptm_xi2_move(startX, targetY);
  }, fromIndex, overIndex);
}

/**
 * Cancel an in-progress drag.
 */
export async function cancelDrag() {
  await browser.execute(() => {
    window.__ptm_xi2_cancel();
  });
}
