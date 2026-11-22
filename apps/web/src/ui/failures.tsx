// The failures that nothing on their way handled (docs/web.md#出错): a promise rejected with nothing
// to hear of it, and an error thrown from an event handler or a timer, which the error boundary does
// not see. The browser has put each in the console; the user hears of it from the snackbar.

import { useEffect } from "react";
import { ApiError } from "../api/http";
import { useCopy } from "./clipboard";
import { saidToUser, useFeedback } from "./feedback";

/** Tells the user of a failure, while `Failures` is shown. */
let tell: ((e: unknown) => void) | null = null;
/** The failures before it was. */
const untold: unknown[] = [];

/** Starts listening. An error event without an error, such as the ResizeObserver's notice or one from
 * a script of another origin, has nothing to tell. */
export function listenForFailures() {
  const failed = (e: unknown) => (tell ? tell(e) : untold.push(e));
  addEventListener("unhandledrejection", (e) => failed(e.reason));
  addEventListener("error", (e) => e.error != null && failed(e.error));
}

/** What the console has of `e`, for the user to pass on when reporting the problem. */
function detailsOf(e: unknown): string {
  if (!(e instanceof Error)) return String(e);
  // Chromium's stack starts with the error's name and message; Safari's and Firefox's do not.
  const stack = e.stack ?? "";
  return stack.startsWith(String(e)) ? stack : `${String(e)}\n${stack}`.trim();
}

/** Tells the user of each failure: the reason, when it gives one written for them; otherwise that
 * something broke, with what the console has of it a tap away. */
export function Failures() {
  const { snack, confirm } = useFeedback();
  const copy = useCopy();
  useEffect(() => {
    const told = (e: unknown) => {
      if (e instanceof ApiError) return snack(`操作失败：${e.message}`);
      const said = saidToUser(e);
      if (said) return snack(said);
      const details = detailsOf(e);
      const show = async () => {
        if (await confirm({ title: "错误详情", body: "反馈问题时请附上以下内容。", detail: details, ok: "复制", cancel: "关闭" })) await copy(details, "错误详情");
      };
      snack({ text: "页面出错，操作可能未完成", action: "错误详情", onAction: () => void show(), ms: 8000 });
    };
    tell = told;
    for (const e of untold.splice(0)) told(e);
    return () => {
      if (tell === told) tell = null;
    };
  }, [snack, confirm, copy]);
  return null;
}
