import { describe, expect, it } from "vitest";
import { checkDescription, checkJobText, checkTitle } from "./text";

// Some chat apps, QQ on Android among them, copy control characters in place of their emoji: no one
// can see them in the box to take them out.
const pasted = "#第二周党校培训#\n\u0014第一场：\r\n\u0014\u0001时间：11月14日\t15:00\u0085";

describe("text pasted with control characters", () => {
  it("goes to AI organizing without them, its lines and tabs kept", () => {
    expect(checkJobText(pasted)).toEqual({ ok: true, value: "#第二周党校培训#\n第一场：\r\n时间：11月14日\t15:00" });
  });

  it("is kept as a description without them", () => {
    expect(checkDescription("清单\u0014\n- [ ] 签到\u007f")).toEqual({ ok: true, value: "清单\n- [ ] 签到" });
  });

  it("makes a title without them, a tab a space, as cells pasted from a spreadsheet", () => {
    expect(checkTitle("\u0014交表\t周五\u001b")).toEqual({ ok: true, value: "交表 周五" });
  });

  it("is empty when it is nothing else", () => {
    expect(checkJobText("\u0014\u0014\n")).toEqual({ ok: false, error: "内容不能为空" });
    expect(checkTitle("\u0014")).toEqual({ ok: false, error: "标题不能为空" });
  });
});
