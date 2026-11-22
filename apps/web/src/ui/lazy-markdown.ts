// The Markdown renderer, a large part of the app not needed to start: loaded when first shown, or a
// while after the app has started, so as to be there when a description is opened.

import { lazy } from "react";

const load = () => import("./Markdown");

export const Description = lazy(() => load().then((m) => ({ default: m.Description })));

/** Loads it once the app has had its start. Failing, offline say, it is no news: a description
 * opened loads it again, and tells of a failure then. */
export const preloadMarkdown = () =>
  void setTimeout(
    // eslint-disable-next-line no-restricted-syntax -- a description opened tells of a failure
    () => load().catch((e) => console.warn("预加载 Markdown 失败", e)),
    2000,
  );
