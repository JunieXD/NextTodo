// Lays out the dependency graph with ELK Layered in a worker. The options were picked by comparing
// layouts of a few real plans; see docs/web.md#关系图.

import ELK, { type ElkNode } from "elkjs/lib/elk-api.js";
import workerUrl from "elkjs/lib/elk-worker.min.js?url";
import type { TaskId } from "../api/types/TaskId";
import type { Edge } from "../domain/graph";

const elk = new ELK({ workerUrl });

export interface Size {
  width: number;
  height: number;
}

export interface Layout extends Size {
  /** Top-left corners of the tasks. */
  at: Map<TaskId, { x: number; y: number }>;
  /** SVG paths of the dependencies, by `before>after`. */
  paths: Map<string, string>;
}

export const edgeKey = ([a, b]: Edge) => `${a}>${b}`;

/** Left to right on desktops, top to bottom on phones. */
export async function layout(tasks: TaskId[], edges: Edge[], node: Size, across: boolean): Promise<Layout> {
  const input: ElkNode = {
    id: "root",
    layoutOptions: {
      "elk.algorithm": "layered",
      "elk.direction": across ? "RIGHT" : "DOWN",
      "elk.padding": "[top=24,left=24,bottom=24,right=24]",
      "elk.spacing.nodeNode": across ? "16" : "12",
      "elk.layered.spacing.nodeNodeBetweenLayers": across ? "64" : "44",
      // Everything that can be done now goes in the first layer, also when it is unrelated to the rest.
      "elk.layered.layering.strategy": "LONGEST_PATH_SOURCE",
      "elk.separateConnectedComponents": "false",
      // Straighter chains and a more compact drawing than the default placement.
      "elk.layered.nodePlacement.strategy": "NETWORK_SIMPLEX",
      // Curves that keep clear of the tasks.
      "elk.edgeRouting": "SPLINES",
      "elk.layered.edgeRouting.splines.mode": "CONSERVATIVE",
    },
    children: tasks.map((id) => ({ id, ...node })),
    edges: edges.map((e) => ({ id: edgeKey(e), sources: [e[0]], targets: [e[1]] })),
  };
  const graph = await elk.layout(input);
  // A spline comes as cubic Bézier segments: its bend points and end point in threes.
  const paths = new Map(
    graph.edges!.map((e) => {
      const { startPoint: s, bendPoints = [], endPoint } = e.sections![0];
      const p = [...bendPoints, endPoint];
      let d = `M${s.x} ${s.y}`;
      for (let i = 0; i < p.length; i += 3) d += ` C${p[i].x} ${p[i].y} ${p[i + 1].x} ${p[i + 1].y} ${p[i + 2].x} ${p[i + 2].y}`;
      return [e.id, d];
    }),
  );
  return { width: graph.width!, height: graph.height!, at: new Map(graph.children!.map((n) => [n.id, { x: n.x!, y: n.y! }])), paths };
}

/** The most tasks side by side in one layer, which sets how wide nodes can be on a phone. */
export function widest(tasks: TaskId[], edges: Edge[]): number {
  const before = new Map<TaskId, TaskId[]>();
  for (const [a, b] of edges) before.set(b, [...(before.get(b) ?? []), a]);
  const layer = new Map<TaskId, number>();
  const of = (id: TaskId): number => {
    let l = layer.get(id);
    if (l === undefined) {
      l = Math.max(-1, ...(before.get(id) ?? []).map(of)) + 1;
      layer.set(id, l);
    }
    return l;
  };
  const counts = new Map<number, number>();
  for (const id of tasks) counts.set(of(id), (counts.get(of(id)) ?? 0) + 1);
  return Math.max(1, ...counts.values());
}
