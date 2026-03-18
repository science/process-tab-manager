/**
 * XI2 click helpers — call __ptm_xi2_click directly.
 * XI2 is the sole mouse input path; no native DOM handlers on rows.
 */

export async function xi2ClickRow(index) {
    await browser.execute((i) => {
        const row = document.querySelectorAll(".row")[i];
        if (!row) return;
        const rect = row.getBoundingClientRect();
        window.__ptm_xi2_click(rect.left + rect.width / 2, rect.top + rect.height / 2, 1);
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
        window.__ptm_xi2_click(rect.left + rect.width / 2, rect.top + rect.height / 2, 1);
    }, index);
    await browser.pause(300);
}
