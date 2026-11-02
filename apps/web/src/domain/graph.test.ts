import { expect, test } from "vitest";
import type { TaskId } from "../api/types/TaskId";
import { type Edge, reduce } from "./graph";

test("reduce keeps exactly the dependencies that no longer path implies", () => {
  for (let round = 0; round < 200; round++) {
    // A random DAG: edges only go from a lower number to a higher one.
    const n = 2 + Math.floor(Math.random() * 10);
    const edges: Edge[] = [];
    for (let a = 0; a < n; a++) for (let b = a + 1; b < n; b++) if (Math.random() < 0.4) edges.push([`${a}` as TaskId, `${b}` as TaskId]);
    const reaches = (from: TaskId, to: TaskId, without: Edge): boolean =>
      edges.some((e) => e !== without && e[0] === from && (e[1] === to || reaches(e[1], to, without)));
    expect(reduce(edges)).toEqual(edges.filter((e) => !reaches(e[0], e[1], e)));
  }
});
