import sidebar from "../pageobjects/sidebar.page.js";
import contextMenu from "../pageobjects/context-menu.page.js";
import { openXterms, closeAllXterms } from "../helpers/xterm.js";
import { waitForEvent } from "../helpers/events.js";

describe("Context Menu", () => {
  before(async () => {
    openXterms(2);
    await browser.pause(2000);
    await sidebar.waitForRows(2);
  });

  after(() => {
    closeAllXterms();
  });

  it("should show context menu on right-click", async () => {
    await sidebar.rightClickRow(0);
    await contextMenu.waitForVisible();

    const items = await contextMenu.getItems();
    expect(items).toContain("Rename");
    expect(items).toContain("Close Window");
    expect(items).toContain("Create Group");

    await contextMenu.dismiss();
    await contextMenu.waitForHidden();
  });

  it("should create group from context menu", async () => {
    await sidebar.rightClickRow(0);
    await contextMenu.waitForVisible();
    await contextMenu.clickItem("Create Group");

    // Wait for group header to appear
    await browser.waitUntil(
      async () => (await sidebar.getGroupCount()) >= 1,
      { timeout: 5000, timeoutMsg: "Group header should appear" }
    );
    await waitForEvent("create-group");
  });
});
