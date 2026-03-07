import * as dom from "../helpers/dom.js";

class RenamePage {
  async startRename() {
    await browser.keys(["F2"]);
    await browser.waitUntil(
      async () => await dom.hasRenameInput(),
      { timeout: 5000, timeoutMsg: "Rename input should appear after F2" }
    );
    await browser.pause(200);
  }

  async typeNewName(name) {
    // Set the input value directly via browser.execute() — browser.keys() for
    // Ctrl+A + typing is unreliable because sidebar re-renders can destroy/recreate
    // the input element between keystrokes.
    await browser.execute((newName) => {
      const input = document.querySelector(".rename-input");
      if (input) {
        input.focus();
        input.value = newName;
        // Trigger input event so any listeners see the change
        input.dispatchEvent(new Event("input", { bubbles: true }));
      }
    }, name);
    await browser.pause(200);
  }

  async commitRename() {
    // Dispatch Enter keydown directly on the rename input via browser.execute().
    // browser.keys(["Enter"]) can fail when sidebar re-renders steal focus.
    await browser.execute(() => {
      const input = document.querySelector(".rename-input");
      if (input) {
        input.focus();
        input.dispatchEvent(new KeyboardEvent("keydown", {
          key: "Enter", code: "Enter", keyCode: 13, bubbles: true,
        }));
      }
    });
    await browser.waitUntil(
      async () => !(await dom.hasRenameInput()),
      { timeout: 5000, timeoutMsg: "Rename input should disappear after Enter" }
    );
  }

  async cancelRename() {
    // Dispatch Escape keydown directly on the rename input via browser.execute().
    await browser.execute(() => {
      const input = document.querySelector(".rename-input");
      if (input) {
        input.focus();
        input.dispatchEvent(new KeyboardEvent("keydown", {
          key: "Escape", code: "Escape", keyCode: 27, bubbles: true,
        }));
      }
    });
    await browser.waitUntil(
      async () => !(await dom.hasRenameInput()),
      { timeout: 5000, timeoutMsg: "Rename input should disappear after Escape" }
    );
  }

  async renameSelectedTo(name) {
    await this.startRename();
    await this.typeNewName(name);
    await this.commitRename();
  }

  async getCurrentValue() {
    return dom.getRenameInputValue();
  }
}

export default new RenamePage();
