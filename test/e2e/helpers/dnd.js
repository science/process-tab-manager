// DnD simulation helper — dispatches synthetic PointerEvents via browser.execute()
// These go through the REAL pointer-based drag handler in main.js.

/**
 * Simulate a drag-and-drop from one sidebar item index to another.
 * Goes through the full pointer event path (pointerdown → pointermove → pointerup).
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

    // 1. pointerdown on source
    source.dispatchEvent(new PointerEvent("pointerdown", {
      bubbles: true, cancelable: true,
      clientX: startX, clientY: startY, button: 0, pointerId: 1,
    }));

    // 2. pointermove past threshold (on sidebar to trigger the listener)
    const sidebar = document.getElementById("sidebar");
    const midY = startY + (targetY > startY ? 10 : -10); // past 5px threshold
    sidebar.dispatchEvent(new PointerEvent("pointermove", {
      bubbles: true, cancelable: true,
      clientX: startX, clientY: midY, button: 0, pointerId: 1,
    }));

    // 3. pointermove to target position
    sidebar.dispatchEvent(new PointerEvent("pointermove", {
      bubbles: true, cancelable: true,
      clientX: startX, clientY: targetY, button: 0, pointerId: 1,
    }));

    // 4. pointerup at target position
    sidebar.dispatchEvent(new PointerEvent("pointerup", {
      bubbles: true, cancelable: true,
      clientX: startX, clientY: targetY, button: 0, pointerId: 1,
    }));
  }, fromIndex, toIndex);

  // Wait for the async drop handler (await invoke + await refreshSidebar)
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
 * Simulate pointerdown + pointermove (without pointerup) to verify visual highlights.
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

    // 1. pointerdown
    source.dispatchEvent(new PointerEvent("pointerdown", {
      bubbles: true, cancelable: true,
      clientX: startX, clientY: startY, button: 0, pointerId: 1,
    }));

    // 2. pointermove past threshold
    const sidebar = document.getElementById("sidebar");
    const midY = startY + (targetY > startY ? 10 : -10);
    sidebar.dispatchEvent(new PointerEvent("pointermove", {
      bubbles: true, cancelable: true,
      clientX: startX, clientY: midY, button: 0, pointerId: 1,
    }));

    // 3. pointermove to target
    sidebar.dispatchEvent(new PointerEvent("pointermove", {
      bubbles: true, cancelable: true,
      clientX: startX, clientY: targetY, button: 0, pointerId: 1,
    }));
  }, fromIndex, overIndex);
}

/**
 * Cancel an in-progress drag by dispatching pointercancel.
 */
export async function cancelDrag() {
  await browser.execute(() => {
    const sidebar = document.getElementById("sidebar");
    sidebar.dispatchEvent(new PointerEvent("pointercancel", {
      bubbles: true, cancelable: true, pointerId: 1,
    }));
  });
}
