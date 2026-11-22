// The change log (docs/ui.md#改动记录): the batches the user sees, newest first, by day, each to be
// undone; or only those that delete something. As the `log` tool gives them, fetched from the top
// again as changes come in, and undone by the `undo` tool: the local log has only the latest.

import { useEffect, useRef, useState, useSyncExternalStore } from "react";
import { api } from "../api/http";
import type { LogBatch } from "../api/types/LogBatch";
import { formatDay, isoTime } from "../domain/dates";
import { useSession } from "../session";
import { Seg } from "../ui/controls";
import { forUser, useFeedback } from "../ui/feedback";
import { useNow } from "../ui/hooks";

const PAGE = 50;

const page = async (deleted: boolean, before?: number) =>
  (await api<{ data: LogBatch[] }>("POST", "/tools/log", { limit: PAGE, deleted, before })).data;

export function LogPage() {
  const { store } = useSession();
  const seq = useSyncExternalStore(store.subscribe, () => store.since);
  const { snack } = useFeedback();
  const now = useNow();
  const [deleted, setDeleted] = useState(false);
  /** None while the first page comes. */
  const [batches, setBatches] = useState<LogBatch[] | null>(null);
  const [more, setMore] = useState(false);
  const [loading, setLoading] = useState(false);
  const [failed, setFailed] = useState<string | null>(null);
  const [tries, setTries] = useState(0);
  const [undoing, setUndoing] = useState<ReadonlySet<number>>(new Set());
  /** Answers to what was asked for another list are dropped: before the filter changed, changes
   * came in or the first page came. */
  const asked = useRef(0);

  useEffect(() => {
    const at = ++asked.current;
    page(deleted).then(
      (first) => {
        if (at !== asked.current) return;
        asked.current++;
        setBatches(first);
        setMore(first.length === PAGE);
        setFailed(null);
      },
      (e: unknown) => at === asked.current && setFailed(forUser("读取改动记录", e)),
    );
  }, [deleted, seq, tries]);

  const loadMore = (shown: LogBatch[]) => {
    const at = asked.current;
    setLoading(true);
    page(deleted, shown[shown.length - 1].batch)
      .then(
        (next) => {
          if (at !== asked.current) return;
          setBatches((list) => list && [...list, ...next]);
          setMore(next.length === PAGE);
        },
        (e: unknown) => at === asked.current && snack(forUser("读取改动记录", e)),
      )
      .finally(() => setLoading(false));
  };

  const undo = (batch: number) => {
    setUndoing((s) => new Set(s).add(batch));
    // Its changes (docs/agent.md#输出): none when what the batch changed is gone or changed back.
    api<{ data: { changes: unknown[] } }>("POST", "/tools/undo", { batch })
      .then(
        ({ data }) => {
          if (data.changes.length) setBatches((list) => list && list.map((b) => (b.batch === batch ? { ...b, undone: true } : b)));
          snack(data.changes.length ? "已撤销" : "没有变化");
        },
        (e: unknown) => snack(forUser("撤销", e)),
      )
      .finally(() =>
        setUndoing((s) => {
          const left = new Set(s);
          left.delete(batch);
          return left;
        }),
      );
  };

  const days = new Map<string, LogBatch[]>();
  for (const b of batches ?? []) {
    const day = formatDay(b.created_at, now);
    days.set(day, [...(days.get(day) ?? []), b]);
  }
  return (
    <>
      <div style={{ margin: "0 16px 8px" }}>
        <Seg<"all" | "deleted">
          label="显示哪些"
          items={[
            ["all", "全部"],
            ["deleted", "只看删除"],
          ]}
          value={deleted ? "deleted" : "all"}
          onChange={(v) => {
            setBatches(null);
            setDeleted(v === "deleted");
          }}
        />
      </div>
      {failed ? (
        <p className="empty">
          {failed}
          <br />
          <button className="text-btn" onClick={() => setTries((n) => n + 1)}>
            重试
          </button>
        </p>
      ) : (
        batches &&
        !batches.length && <p className="empty">{deleted ? "90 天内没有删除过东西" : "90 天内没有改动"}</p>
      )}
      {[...days].map(([day, list]) => (
        <section key={day}>
          <h2 className="group-title">{day}</h2>
          <div className="group">
            {list.map((b) => (
              <div className="row" key={b.batch}>
                <div className="row-inner">
                  <span className="folder-main">
                    <span className="log-what">{b.summary}</span>
                    <span className="folder-sub">
                      {b.source} · {isoTime(new Date(b.created_at))}
                    </span>
                  </span>
                  {b.undone ? (
                    <span className="folder-sub">已撤销</span>
                  ) : (
                    <button className="text-btn" disabled={undoing.has(b.batch)} onClick={() => undo(b.batch)}>
                      撤销
                    </button>
                  )}
                </div>
              </div>
            ))}
          </div>
        </section>
      ))}
      {batches && more && (
        <button className="text-btn log-more" disabled={loading} onClick={() => loadMore(batches)}>
          加载更多
        </button>
      )}
    </>
  );
}
