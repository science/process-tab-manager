import sidebar from "../pageobjects/sidebar.page.js";
import rename from "../pageobjects/rename.page.js";
import { openXterms, closeAllXterms } from "../helpers/xterm.js";
import { hasRenameInput } from "../helpers/dom.js";

describe("Rename", () => {
  before(async () => {
    openXterms(2);
    await browser.pause(2000);
    await sidebar.waitForRows(2);
  });

  after(() => {
    closeAllXterms();
  });

  it("should open rename input on F2", async () => {
    await sidebar.clickRow(0);
    await rename.startRename();
    const hasInput = await hasRenameInput();
    expect(hasInput).toBe(true);

    // Cancel to reset state
    await rename.cancelRename();
  });

  it("should commit rename on Enter", async () => {
    await sidebar.clickRow(0);
    await rename.renameSelectedTo("MyTerminal");

    // Verify title changed in DOM
    await sidebar.waitForRowWithTitle("MyTerminal");
    const row = await sidebar.getRowByTitle("MyTerminal");
    expect(row).not.toBeNull();
  });

  it("should cancel rename on Escape", async () => {
    await sidebar.clickRow(0);

    await rename.startRename();
    await rename.typeNewName("ShouldNotStick");
    await rename.cancelRename();

    // Title should NOT be "ShouldNotStick" — the rename was cancelled
    const afterRow = await sidebar.getRowByIndex(0);
    expect(afterRow.title).not.toBe("ShouldNotStick");
  });
});
