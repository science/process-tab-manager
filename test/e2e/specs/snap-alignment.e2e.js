import sidebar from "../pageobjects/sidebar.page.js";
import { openXterms, closeAllXterms } from "../helpers/xterm.js";
import { readTestState } from "../helpers/state.js";

describe("Snap Alignment", () => {
  before(async () => {
    openXterms(2);
    await browser.pause(2000);
    await sidebar.waitForRows(2);
  });

  after(() => {
    closeAllXterms();
  });

  it("should snap activated window to the right of PTM sidebar", async () => {
    // Click a row to activate+snap
    await sidebar.clickRow(0);
    await browser.pause(1000);

    // Get PTM and target window positions via Tauri commands
    const state = readTestState();
    const firstWindow = state.items.find(i => i.kind === "window");
    expect(firstWindow).toBeDefined();

    // Get PTM geometry via Tauri command
    const ptmGeo = await browser.execute(
      () => window.__TAURI__.core.invoke("get_ptm_window_geometry")
    );
    const ptmRightEdge = ptmGeo.x + ptmGeo.width;

    // Get target window geometry via Tauri command
    const targetGeo = await browser.execute(
      (wid) => window.__TAURI__.core.invoke("get_window_geometry", { wid }),
      firstWindow.wid
    );
    const targetX = targetGeo.x;

    // The target window's left edge should be near PTM's right edge
    // Allow some tolerance for frame extents (WM decorations)
    const gap = targetX - ptmRightEdge;
    console.log(`PTM right edge: ${ptmRightEdge}, Target left: ${targetX}, gap: ${gap}`);

    // The gap should be small (0-50px for frame extents), NOT negative (overlapping)
    // and NOT hundreds of pixels off
    expect(gap).toBeGreaterThanOrEqual(-5); // not overlapping PTM
    expect(gap).toBeLessThan(50); // not far away
  });
});
