// The Markdown renderer, a large part of the app not needed to start: loaded when first shown, or a
// while after the app has started, so as to be there when a description is opened.

import { lazy } from "react";

const load = () => import("./Markdown");

export const Description = lazy(() => load().then((m) => ({ default: m.Description })));
export const Prose = lazy(() => load().then((m) => ({ default: m.Prose })));

/** Loads it once the app has had its start. */
export const preloadMarkdown = () => void setTimeout(load, 2000);
