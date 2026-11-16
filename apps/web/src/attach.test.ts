import { expect, test } from "vitest";
import { nameFor, reference } from "./attach";

test("pictures with names a system made up are numbered, others keep theirs", () => {
  const named = (name: string, type = "image/jpeg", taken: string[] = []) => nameFor(name, type, taken);
  expect(
    [
      "image.png",
      "IMG_20261117_093212.jpg",
      "IMG_1234.JPG",
      "Screenshot_2026-11-17-09-32-12-345_com.tencent.mm.jpg",
      "Screenshot 2026-11-17 at 09.32.12.png",
      "截屏2026-11-17 09.32.12.png",
      "屏幕截图 2026-11-17 093212.png",
      "mmexport1699999999999.jpg",
      "1699999999999.jpg",
      "image (2).png",
    ].map((n) => named(n)),
  ).toEqual(["图片 1.png", "图片 1.jpg", "图片 1.JPG", "图片 1.jpg", "图片 1.png", "图片 1.png", "图片 1.png", "图片 1.jpg", "图片 1.jpg", "图片 1.png"]);
  expect(["images.png", "Imagine.jpg", "IMG_E1234.jpg", "发票.png", "2026 年报.jpg"].map((n) => named(n))).toEqual([
    "images.png",
    "Imagine.jpg",
    "IMG_E1234.jpg",
    "发票.png",
    "2026 年报.jpg",
  ]);
  // Only pictures: a made-up name says what kind of file anything else is.
  expect(named("IMG_1234.mov", "video/quicktime")).toBe("IMG_1234.mov");
  expect(named("image.png", "image/png", ["图片 1.png", "图片 3.jpg", "图片 a.png"])).toBe("图片 4.png");
  expect(named("", "image/png")).toBe("图片 1.png");
  expect(named("", "application/octet-stream", ["文件 1"])).toBe("文件 2");
  expect(named("a/b\\c.txt", "text/plain")).toBe("a_b_c.txt");
  expect([...named(`${"长".repeat(300)}.pdf`, "application/pdf")]).toHaveLength(200);
  expect(named(`${"长".repeat(300)}.pdf`, "application/pdf").endsWith(".pdf")).toBe(true);
});

test("a reference escapes what would end the link's text", () => {
  expect(reference({ id: "1", name: "a [b] \\c.png" })).toBe("[a \\[b\\] \\\\c.png](attachment:1)");
});
