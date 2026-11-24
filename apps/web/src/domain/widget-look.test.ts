import { expect, test } from "vitest";
import { type WidgetLook, widgetColors } from "./widget-look";

const look: WidgetLook = { background: "system", opacity: 100, text: "auto", title: true, refresh: true, add: true, size: "standard" };

test("by default the widgets are as the system is, light or dark", () => {
  expect([widgetColors(look, false).text, widgetColors(look, true).text]).toEqual(["#171D1A", "#DEE4DF"]);
  expect(widgetColors(look, true).background).toBe("rgb(15 21 18 / 1)");
});

test("on a background of its own the text goes by how light it is, unless mostly the wallpaper shows", () => {
  expect(widgetColors({ ...look, background: "white" }, true).text).toBe("#171D1A");
  expect(widgetColors({ ...look, background: "#1b6b51" }, false).text).toBe("#DEE4DF");
  expect(widgetColors({ ...look, background: "black", opacity: 40 }, false)).toMatchObject({ text: "#171D1A", background: "rgb(0 0 0 / 0.4)" });
});

test("a text colour of one's own, fainter for the rest; the marks those for its background", () => {
  const c = widgetColors({ ...look, text: "white" }, false);
  expect([c.text, c.secondary, c.divider, c.overdue]).toEqual(["#ffffff", "rgb(255 255 255 / 0.7)", "rgb(255 255 255 / 0.15)", "#FFB4AB"]);
});
