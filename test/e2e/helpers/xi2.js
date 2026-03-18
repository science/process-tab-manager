/**
 * XI2 click helpers — simulate clicks via pointer events (pointerdown → pointerup → click).
 * This exercises the real click path including drag threshold detection.
 */

export async function xi2ClickRow(index) {
    await browser.execute((i) => {
        const row = document.querySelectorAll(".row")[i];
        if (!row) return;
        const rect = row.getBoundingClientRect();
        const x = rect.left + rect.width / 2;
        const y = rect.top + rect.height / 2;
        row.dispatchEvent(new PointerEvent("pointerdown", {
            bubbles: true, cancelable: true,
            clientX: x, clientY: y, button: 0, pointerId: 1,
        }));
        document.getElementById("sidebar").dispatchEvent(new PointerEvent("pointerup", {
            bubbles: true, cancelable: true,
            clientX: x, clientY: y, button: 0, pointerId: 1,
        }));
        row.dispatchEvent(new MouseEvent("click", {
            bubbles: true, cancelable: true,
            clientX: x, clientY: y, button: 0,
        }));
    }, index);
    await browser.pause(300);
}

export async function xi2RightClickRow(index) {
    await browser.execute((i) => {
        const row = document.querySelectorAll(".row")[i];
        if (!row) return;
        const rect = row.getBoundingClientRect();
        window.__ptm_xi2_click(rect.left + rect.width / 2, rect.top + rect.height / 2, 3);
    }, index);
    await browser.pause(300);
}

export async function xi2ClickGroup(index) {
    await browser.execute((i) => {
        const header = document.querySelectorAll(".group-header")[i];
        if (!header) return;
        const rect = header.getBoundingClientRect();
        const x = rect.left + rect.width / 2;
        const y = rect.top + rect.height / 2;
        header.dispatchEvent(new PointerEvent("pointerdown", {
            bubbles: true, cancelable: true,
            clientX: x, clientY: y, button: 0, pointerId: 1,
        }));
        document.getElementById("sidebar").dispatchEvent(new PointerEvent("pointerup", {
            bubbles: true, cancelable: true,
            clientX: x, clientY: y, button: 0, pointerId: 1,
        }));
        header.dispatchEvent(new MouseEvent("click", {
            bubbles: true, cancelable: true,
            clientX: x, clientY: y, button: 0,
        }));
    }, index);
    await browser.pause(300);
}
