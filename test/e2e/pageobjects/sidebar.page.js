import * as dom from "../helpers/dom.js";

class SidebarPage {
  // ── Query methods (stale-safe) ─────────────────────────────────

  async getRowCount() {
    return dom.getRowCount();
  }

  async getGroupCount() {
    return dom.getGroupCount();
  }

  async getRowTitles() {
    return dom.getRowTexts();
  }

  async getRowByIndex(n) {
    return dom.getRowAt(n);
  }

  async getRowByTitle(title) {
    return browser.execute((t) => {
      const rows = document.querySelectorAll(".row");
      for (let i = 0; i < rows.length; i++) {
        const titleEl = rows[i].querySelector(".title");
        if (titleEl && titleEl.textContent === t) {
          return {
            wid: Number(rows[i].dataset.wid),
            index: i,
            classes: rows[i].className,
          };
        }
      }
      return null;
    }, title);
  }

  async getSelectedRow() {
    return dom.getSelectedRow();
  }

  async hasRenameInput() {
    return dom.hasRenameInput();
  }

  // ── Action methods ─────────────────────────────────────────────

  async clickRow(index) {
    // Route through XI2 bridge — sole mouse input path
    await browser.execute((i) => {
      const row = document.querySelectorAll(".row")[i];
      if (!row) return;
      const rect = row.getBoundingClientRect();
      window.__ptm_xi2_click(rect.left + rect.width / 2, rect.top + rect.height / 2, 1);
    }, index);
    await browser.pause(300);
  }

  async clickRowByTitle(title) {
    const row = await this.getRowByTitle(title);
    if (row !== null) {
      await this.clickRow(row.index);
    }
  }

  async clickGroup(index) {
    await browser.execute((i) => {
      const header = document.querySelectorAll(".group-header")[i];
      if (!header) return;
      const rect = header.getBoundingClientRect();
      window.__ptm_xi2_click(rect.left + rect.width / 2, rect.top + rect.height / 2, 1);
    }, index);
    await browser.pause(300);
  }

  async rightClickRow(index) {
    // Route through XI2 bridge — sole mouse input path
    await browser.execute((i) => {
      const row = document.querySelectorAll(".row")[i];
      if (!row) return;
      const rect = row.getBoundingClientRect();
      window.__ptm_xi2_click(rect.left + rect.width / 2, rect.top + rect.height / 2, 3);
    }, index);
    await browser.pause(300);
  }

  async pressKey(...keys) {
    await browser.keys(keys);
  }

  async navigateDown(count = 1) {
    for (let i = 0; i < count; i++) {
      await browser.keys(["ArrowDown"]);
      await browser.pause(200);
    }
  }

  async navigateUp(count = 1) {
    for (let i = 0; i < count; i++) {
      await browser.keys(["ArrowUp"]);
      await browser.pause(200);
    }
  }

  async reorderDown() {
    // Dispatch keyboard event directly — WebDriver modifier combos
    // may not work reliably with wry/WebKitGTK
    await browser.execute(() => {
      document.dispatchEvent(new KeyboardEvent("keydown", {
        key: "ArrowDown", code: "ArrowDown",
        ctrlKey: true, shiftKey: true,
        bubbles: true, cancelable: true,
      }));
    });
    await browser.pause(500);
  }

  async reorderUp() {
    await browser.execute(() => {
      document.dispatchEvent(new KeyboardEvent("keydown", {
        key: "ArrowUp", code: "ArrowUp",
        ctrlKey: true, shiftKey: true,
        bubbles: true, cancelable: true,
      }));
    });
    await browser.pause(500);
  }

  // ── Wait methods ───────────────────────────────────────────────

  async waitForRows(minCount, timeout = 15000) {
    await browser.waitForSidebarReady(minCount, timeout);
  }

  async waitForRowWithTitle(title, timeout = 10000) {
    await browser.waitUntil(
      async () => {
        const row = await this.getRowByTitle(title);
        return row !== null;
      },
      { timeout, timeoutMsg: `Row with title "${title}" not found` }
    );
  }

  async waitForRowCount(exact, timeout = 10000) {
    await browser.waitUntil(
      async () => (await this.getRowCount()) === exact,
      { timeout, timeoutMsg: `Expected exactly ${exact} rows` }
    );
  }

  async waitForNoRenameInput(timeout = 5000) {
    await browser.waitUntil(
      async () => !(await this.hasRenameInput()),
      { timeout, timeoutMsg: "Rename input should have disappeared" }
    );
  }
}

export default new SidebarPage();
