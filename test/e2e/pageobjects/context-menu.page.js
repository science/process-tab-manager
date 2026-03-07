class ContextMenuPage {
  async isVisible() {
    return browser.execute(() => {
      const menu = document.getElementById("context-menu");
      return menu ? menu.classList.contains("visible") : false;
    });
  }

  async getItems() {
    return browser.execute(() =>
      Array.from(
        document.querySelectorAll("#context-menu .menu-item"),
        el => el.textContent
      )
    );
  }

  async clickItem(label) {
    // Find and click the menu item by label text
    await browser.execute((lbl) => {
      const items = document.querySelectorAll("#context-menu .menu-item");
      for (const item of items) {
        if (item.textContent === lbl) {
          item.click();
          return;
        }
      }
    }, label);
    await browser.pause(500);
  }

  async dismiss() {
    // Click sidebar body to dismiss
    await browser.execute(() => {
      document.getElementById("sidebar").click();
    });
    await browser.pause(300);
  }

  async waitForVisible(timeout = 5000) {
    await browser.waitUntil(
      async () => await this.isVisible(),
      { timeout, timeoutMsg: "Context menu should be visible" }
    );
  }

  async waitForHidden(timeout = 5000) {
    await browser.waitUntil(
      async () => !(await this.isVisible()),
      { timeout, timeoutMsg: "Context menu should be hidden" }
    );
  }
}

export default new ContextMenuPage();
