import { expect, test } from "vitest";
import { contrast, DARK_PAGE, hexOf, hsvOfRgb, LIGHT_PAGE, ON_DARK, ON_LIGHT, parseHex, rgbOf, rgbOfHsv, roles } from "./color";

test("colours typed", () => {
  expect(["#3478F6", "3478f6", " #3478f6 ", "#3478f", "#3478f60", "#34g8f6", "#+478f6", ""].map(parseHex)).toEqual([
    "#3478f6", "#3478f6", "#3478f6", null, null, null, null, null,
  ]);
});

// Every colour at a step of 17 on each part: whites, blacks, greys and the brightest of each hue too.
const colours = Array.from({ length: 16 ** 3 }, (_, i) => hexOf([(i >> 8) * 17, ((i >> 4) & 15) * 17, (i & 15) * 17]));

test("the square and the bar give back the colour picked", () => {
  for (const c of colours) expect(hexOf(rgbOfHsv(hsvOfRgb(rgbOf(c))))).toBe(c);
});

test("a folder colour's icon is seen on its fill and on the page in either theme", () => {
  for (const c of colours) {
    const r = roles(c);
    expect([r.light, r.dark].map(parseHex)).toEqual([r.light, r.dark]);
    expect(contrast(r.on, r.fill)).toBeGreaterThanOrEqual(ON_LIGHT);
    expect(contrast(r.light, LIGHT_PAGE)).toBeGreaterThanOrEqual(ON_LIGHT);
    expect(contrast(r.dark, DARK_PAGE)).toBeGreaterThanOrEqual(ON_DARK);
    // Seen there as it is, it stays as it is.
    if (contrast(c, LIGHT_PAGE) >= ON_LIGHT) expect(r.light).toBe(c);
    if (contrast(c, DARK_PAGE) >= ON_DARK) expect(r.dark).toBe(c);
  }
});
