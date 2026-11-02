// The graph view of a folder: tasks laid out by ELK, dependencies as curves. React Flow pans and
// zooms; picking a task highlights the chain through it; 连线 links two tasks; tapping a dependency
// offers to delete it.

import {
  Background,
  BackgroundVariant,
  BaseEdge,
  type Edge as FlowEdge,
  type EdgeProps,
  Handle,
  type Node,
  type NodeHandle,
  type NodeProps,
  Position,
  ReactFlow,
  ReactFlowProvider,
  useReactFlow,
} from "@xyflow/react";
import "./graph.css";
import { Check as CheckIcon, Clock, Folder as FolderIcon, Link, X } from "lucide-react";
import { type ReactNode, useEffect, useMemo, useRef, useState } from "react";
import type { FolderId } from "../api/types/FolderId";
import type { Task } from "../api/types/Task";
import type { TaskId } from "../api/types/TaskId";
import { formatDue } from "../domain/dates";
import { type Edge, lineage, scene } from "../domain/graph";
import { useView } from "../session";
import { useFeedback } from "../ui/feedback";
import { Icon, Inbound } from "../ui/icons";
import { useTaskActions } from "./actions";
import { type Layout, type Size, edgeKey, layout, widest } from "./graph-layout";
import { useMarks } from "./marks";
import { dueClass } from "./rows";

const NODE_HEIGHT = 60;
/** How far the first view may zoom out to show everything, while titles stay readable. */
const MIN_START_ZOOM = 0.8;

type TaskNode = Node<{ task: Task; className: string; folder?: string; meta?: ReactNode; onClick(): void }, "task">;
type DepEdge = FlowEdge<{ d: string; marker: string }, "dep">;

const nodeTypes = { task: TaskNodeView };
const edgeTypes = { dep: DepEdgeView };

export default function Graph(props: { folder: FolderId; across: boolean; now: Date }) {
  return (
    <ReactFlowProvider>
      <Canvas {...props} />
    </ReactFlowProvider>
  );
}

function Canvas({ folder, across, now }: { folder: FolderId; across: boolean; now: Date }) {
  const { board } = useView();
  const { flash } = useMarks();
  const actions = useTaskActions();
  const { confirm } = useFeedback();
  const flow = useReactFlow();
  const wrap = useRef<HTMLDivElement>(null);
  const placed = useRef(false);
  const [done, setDone] = useState(false);
  const [sub, setSub] = useState(false);
  const [picked, setPicked] = useState<TaskId | null>(null);
  const [connect, setConnect] = useState<{ from: TaskId | null } | null>(null);

  const s = useMemo(() => scene(board, folder, { done, sub }), [board, folder, done, sub]);
  const ids = s.tasks.map((t) => t.id).sort();
  const sorted = s.edges.toSorted((a, b) => (edgeKey(a) < edgeKey(b) ? -1 : 1));
  const n = widest(ids, sorted);
  const node: Size = { width: across ? 184 : Math.floor(Math.max(104, Math.min(146, (innerWidth - 48 - (n - 1) * 12) / n))), height: NODE_HEIGHT };

  // Lay out again only when the shape changes, not when a title, a status or the list order does.
  const shape = JSON.stringify([ids, sorted, node]);
  const [laid, setLaid] = useState<Layout | null>(null);
  useEffect(() => {
    let current = true;
    const [ids, edges, node] = JSON.parse(shape) as [TaskId[], Edge[], Size];
    layout(ids, edges, node, across).then((l) => {
      if (!current) return;
      if (!placed.current && ids.length) {
        // Start at full size, or a little smaller if then it all fits; centred if it fits, from the
        // first tasks if not.
        placed.current = true;
        const { clientWidth: w, clientHeight: h } = wrap.current!;
        const zoom = Math.max(MIN_START_ZOOM, Math.min(1, w / l.width, h / l.height));
        flow.setViewport({ x: Math.max(0, (w - l.width * zoom) / 2), y: Math.max(0, (h - l.height * zoom) / 2), zoom });
      }
      setLaid(l);
    });
    return () => {
      current = false;
    };
  }, [shape, across, flow]);

  const hl = picked && s.tasks.some((t) => t.id === picked) ? picked : null;
  const chain = hl ? lineage(s.edges, hl) : null;
  const inChain = (id: TaskId) => !chain || id === hl || chain.up.has(id) || chain.down.has(id);
  const onSide = (side: Set<TaskId>, id: TaskId) => id === hl || side.has(id);
  const lit = ([a, b]: Edge) => !!chain && ((onSide(chain.up, a) && onSide(chain.up, b)) || (onSide(chain.down, a) && onSide(chain.down, b)));

  // The task picked first when linking, as long as it is still shown.
  const source = connect?.from && s.tasks.some((t) => t.id === connect.from) ? connect.from : null;
  const tap = (t: Task) => {
    if (!connect) {
      setPicked(t.id);
      actions.open(t);
    } else if (!source) {
      setConnect({ from: t.id });
    } else if (source !== t.id) {
      actions.link(source, t.id);
      setConnect(null);
    }
  };
  const unlink = async (before: TaskId, after: TaskId) => {
    const [a, b] = [board.task(before)!, board.task(after)!];
    if (await confirm({ title: "删除这条依赖？", body: `删除后，“${b.title}”不再需要等“${a.title}”完成。`, ok: "删除", danger: true })) actions.unlink(before, after);
  };

  // Sizes and handles given up front, so that the drawing doesn't wait for React Flow to measure
  // the new node objects every render. Where the handles sit doesn't matter: the paths come from ELK.
  const handles: NodeHandle[] = [
    { type: "target", position: Position.Top, x: 0, y: 0 },
    { type: "source", position: Position.Bottom, x: 0, y: 0 },
  ];
  const nodes: TaskNode[] = [];
  // Until a new layout is ready, tasks it doesn't have yet wait, and the rest keep their places and size.
  for (const t of s.tasks) {
    const at = laid?.at.get(t.id);
    if (!at) continue;
    const outside = s.outside.has(t.id);
    const status = board.status(t);
    // Tasks from elsewhere show their folder instead.
    let meta: ReactNode;
    if (!outside && t.due) {
      meta = (
        <span className={`n-meta ${dueClass(t, now)}`}>
          <Icon of={Clock} size={12} />
          {formatDue(t.due, now)}
        </span>
      );
    } else if (!outside && status === "waiting") {
      meta = (
        <span className="n-meta">
          <Icon of={Inbound} size={12} />等 {board.waitingOn(t)} 项
        </span>
      );
    }
    const className = ["node", outside ? "ghost" : status, (meta || outside) && "has-meta", t.id === hl && "sel", !inChain(t.id) && "dim", source === t.id && "source", flash.has(t.id) && "unlocked"]
      .filter(Boolean)
      .join(" ");
    const folder = outside ? (t.folder_id ? board.folder(t.folder_id)!.name : "根目录") : undefined;
    nodes.push({ id: t.id, type: "task", position: at, ...laid!.node, handles, data: { task: t, className, folder, meta, onClick: () => tap(t) } });
  }
  const edges: DepEdge[] = s.edges.flatMap((e) => {
    const d = laid?.paths.get(edgeKey(e));
    if (!d) return [];
    const on = lit(e);
    return [{ id: edgeKey(e), source: e[0], target: e[1], type: "dep", className: on ? "hl" : chain ? "dim" : undefined, data: { d, marker: on ? "url(#arrow-hl)" : "url(#arrow)" } }];
  });

  const chip = (on: boolean, set: (on: boolean) => void, label: string) => (
    <button className={`chip${on ? " on" : ""}`} aria-pressed={on} onClick={() => set(!on)}>
      {on && <Icon of={CheckIcon} size={16} />}
      {label}
    </button>
  );
  const toggleConnect = () => {
    setConnect(connect ? null : { from: null });
    setPicked(null);
  };
  const from = source ? board.task(source) : undefined;
  return (
    <>
      <div className="graph-area">
        <div className="graph-tools">
          {chip(done, setDone, "显示已完成")}
          {board.subfolders(folder).length > 0 && chip(sub, setSub, "包含子文件夹")}
          {across && (
            <>
              <span className="spacer" />
              <button className={`chip${connect ? " on" : ""}`} onClick={toggleConnect}>
                <Icon of={connect ? X : Link} size={16} />
                {connect ? "取消连线" : "连线"}
              </button>
            </>
          )}
        </div>
        <div className="graph-wrap" ref={wrap}>
          {connect && (
            <div className="connect-banner" role="status">
              {from ? `再点后续事项，接在“${from.title}”后面` : "先点前置事项"}
            </div>
          )}
          <svg className="graph-defs" aria-hidden>
            <defs>
              <marker id="arrow" viewBox="0 0 10 10" refX="7" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse">
                <path d="M1 1.5L8 5 1 8.5" />
              </marker>
              <marker id="arrow-hl" viewBox="0 0 10 10" refX="7" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse">
                <path d="M1 1.5L8 5 1 8.5" />
              </marker>
            </defs>
          </svg>
          {s.tasks.length === 0 ? (
            <p className="empty">这个文件夹里没有待办事项。</p>
          ) : (
          <ReactFlow
            nodes={nodes}
            edges={edges}
            nodeTypes={nodeTypes}
            edgeTypes={edgeTypes}
            onEdgeClick={(_, e) => unlink(e.source, e.target)}
            onPaneClick={() => setPicked(null)}
            nodesDraggable={false}
            nodesConnectable={false}
            nodesFocusable={false}
            edgesFocusable={false}
            elementsSelectable={false}
            panOnScroll
            minZoom={0.35}
            maxZoom={2.5}
            attributionPosition="top-right"
          >
            <Background variant={BackgroundVariant.Dots} gap={22} size={1.2} />
          </ReactFlow>
          )}
        </div>
      </div>
      {!across && (
        <button className={`fab${connect ? " on" : ""}`} onClick={toggleConnect}>
          <Icon of={connect ? X : Link} />
          {connect ? "取消连线" : "连线"}
        </button>
      )}
    </>
  );
}

function TaskNodeView({ data, width, height }: NodeProps<TaskNode>) {
  return (
    <>
      <Handle type="target" position={Position.Top} isConnectable={false} />
      <button className={data.className} style={{ width, height }} onClick={data.onClick}>
        {data.folder && (
          <span className="n-folder">
            <Icon of={FolderIcon} size={12} />
            {data.folder}
          </span>
        )}
        <span className="n-title">{data.task.title}</span>
        {data.meta}
      </button>
      <Handle type="source" position={Position.Bottom} isConnectable={false} />
    </>
  );
}

function DepEdgeView({ data }: EdgeProps<DepEdge>) {
  return <BaseEdge path={data!.d} markerEnd={data!.marker} interactionWidth={18} />;
}
