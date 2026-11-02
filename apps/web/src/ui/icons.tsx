// Lucide icons drawn the way the design asks (24 grid, 1.8 stroke), plus the few Lucide lacks.

import { type LucideIcon, createLucideIcon } from "lucide-react";

export function Icon({ of: I, size = 20, width = 1.8 }: { of: LucideIcon; size?: number; width?: number }) {
  return <I size={size} strokeWidth={width} aria-hidden />;
}

/** Waits for others. */
export const Inbound = createLucideIcon("inbound", [
  ["path", { d: "M19 5v6a4 4 0 0 1-4 4H5", key: "line" }],
  ["path", { d: "M8.5 11.5L5 15l3.5 3.5", key: "head" }],
]);

/** Others wait for it. */
export const Outbound = createLucideIcon("outbound", [
  ["path", { d: "M5 5v6a4 4 0 0 0 4 4h10", key: "line" }],
  ["path", { d: "M15.5 11.5L19 15l-3.5 3.5", key: "head" }],
]);

/** One after another. */
export const Chain = createLucideIcon("chain", [
  ["circle", { cx: "6", cy: "6", r: "2.5", key: "top" }],
  ["circle", { cx: "18", cy: "18", r: "2.5", key: "bottom" }],
  ["path", { d: "M6 8.5v3.5a3 3 0 0 0 3 3h6a3 3 0 0 1 3 3v-2.5", key: "link" }],
]);
