import { expect, test } from "vitest";
import type { TaskId } from "../api/types/TaskId";
import { type Edge, reduce } from "./graph";

test("reduce drops exactly the dependencies that a longer path implies", () => {
  let seed = 7;
  const random = () => (seed = (seed * 1103515245 + 12345) % 2 ** 31) / 2 ** 31;
  for (let round = 0; round < 300; round++) {
    // A random DAG: edges go from an earlier task to a later one in a shuffled order.
    const n = 2 + Math.floor(random() * 10);
    const order = Array.from({ length: n }, (_, i) => `${i}` as TaskId).sort(() => random() - 0.5);
    const edges: Edge[] = [];
    for (let a = 0; a < n; a++) for (let b = a + 1; b < n; b++) if (random() < 0.4) edges.push([order[a], order[b]]);
    const reaches = (from: TaskId, to: TaskId, without: Edge): boolean =>
      edges.some((e) => e !== without && e[0] === from && (e[1] === to || reaches(e[1], to, without)));
    expect(reduce(edges)).toEqual(edges.filter((e) => !reaches(e[0], e[1], e)));
  }
});

test("reduce copes with a long chain", () => {
  const ids = Array.from({ length: 20_000 }, (_, i) => `t${i}` as TaskId);
  const chain = ids.slice(1).map((id, i): Edge => [ids[i], id]);
  expect(reduce([...chain, [ids[0], ids.at(-1)!]])).toEqual(chain);
});
