// Copying text.

import { useFeedback } from "./feedback";

/** Puts `text` on the clipboard. A page on plain HTTP has no clipboard API, which browsers give
 * secure pages only (docs/web.md#经-http-打开): there the browser's own copy command, which a click
 * may give, carries it; set in its event, as nothing is selected. */
export async function copyText(text: string) {
  if ("clipboard" in navigator) return navigator.clipboard.writeText(text);
  const put = (e: ClipboardEvent) => {
    e.clipboardData?.setData("text/plain", text);
    e.preventDefault();
  };
  document.addEventListener("copy", put);
  const copied = document.execCommand("copy");
  document.removeEventListener("copy", put);
  if (!copied) throw new Error("浏览器不允许复制");
}

/** Copies `text`, a `what` such as “令牌”, and says whether it did. */
export function useCopy() {
  const { snack } = useFeedback();
  return (text: string, what: string) =>
    copyText(text).then(
      () => snack(`已复制${what}`),
      () => snack(`无法复制${what}，请再试一次`),
    );
}
