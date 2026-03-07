import { getBodyBgColor } from "../helpers/dom.js";

describe("Theme", () => {
  it("should use dark background", async () => {
    const bg = await getBodyBgColor();
    // Parse rgb(r, g, b) format
    const match = bg.match(/rgb\((\d+),\s*(\d+),\s*(\d+)\)/);
    expect(match).not.toBeNull();

    const [, r, g, b] = match.map(Number);
    // Relative luminance formula (simplified)
    const luminance = (0.299 * r + 0.587 * g + 0.114 * b) / 255;
    expect(luminance).toBeLessThan(0.3);
  });
});
