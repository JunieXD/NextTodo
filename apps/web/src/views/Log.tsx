// The change log (docs/ui.md#改动记录): the batches the user sees, newest first, by day, each to be
// undone; or only those that delete something. As the `log` tool gives them, fetched from the top
// again as changes come in, and undone by the `undo` tool: the local log has only the latest.

import { useEffect, useRef, useState, useSyncExternalStore } from "react";
import { api } from "../api/http";
import type { LogBatch } from "../api/types/LogBatch";
import { formatDay, formatMoment, isoTime } from "../domain/dates";
import { useSession } from "../session";
import { Seg } from "../ui/controls";
import { Modal, forUser, useFeedback } from "../ui/feedback";
import { useNow } from "../ui/hooks";

const PAGE = 50;

const log = async (args: { limit?: number; deleted?: boolean; before?: number; batch?: number }) =>
  (await api<{ data: LogBatch[] }>("POST", "/tools/log", args)).data;

const page = (deleted: boolean, before?: number) => log({ limit: PAGE, deleted, before });

export function LogPage() {
  const { store } = useSession();
  const seq = useSyncExternalStore(store.subscribe, () => store.since);
  const { snack } = useFeedback();
  const now = useNow();
  const [deleted, setDeleted] = useState(false);
  /** The batches shown and whether there are older ones; none while the first page comes. */
  const [shown, setShown] = useState<{ batches: LogBatch[]; more: boolean } | null>(null);
  const batches = shown?.batches ?? null;
  const [loading, setLoading] = useState(false);
  const [failed, setFailed] = useState<string | null>(null);
  const [tries, setTries] = useState(0);
  const [undoing, setUndoing] = useState<ReadonlySet<number>>(new Set());
  const [open, setOpen] = useState<LogBatch | null>(null);
  /** Answers to what was asked for another list are dropped: before the filter changed, changes
   * came in or the first page came. */
  const asked = useRef(0);
  /** The oldest batch shown: changes coming in fetch the list again as far back, keeping the place. */
  const oldest = useRef<number>(undefined);
  useEffect(() => {
    oldest.current = batches?.at(-1)?.batch;
  }, [batches]);

  useEffect(() => {
    const at = ++asked.current;
    const back = oldest.current;
    void (async () => {
      let list = await page(deleted);
      let full = list.length === PAGE;
      while (full && back !== undefined && list[list.length - 1].batch > back && at === asked.current) {
        const next = await page(deleted, list[list.length - 1].batch);
        list = [...list, ...next];
        full = next.length === PAGE;
      }
      const fresh = back === undefined ? list : list.filter((b) => b.batch >= back);
      return { fresh, more: full || fresh.length < list.length };
    })().then(
      ({ fresh, more }) => {
        if (at !== asked.current) return;
        asked.current++;
        // A page loaded meanwhile stays, and whether there are more after it.
        setShown((s) =>
          back !== undefined && s?.batches.length && s.batches[s.batches.length - 1].batch < back
            ? { batches: [...fresh, ...s.batches.filter((b) => b.batch < back)], more: s.more }
            : { batches: fresh, more },
        );
        setFailed(null);
      },
      (e: unknown) => at === asked.current && setFailed(forUser("读取改动记录", e)),
    );
  }, [deleted, seq, tries]);

  const loadMore = (list: LogBatch[]) => {
    const at = asked.current;
    setLoading(true);
    page(deleted, list[list.length - 1].batch)
      .then(
        (next) => {
          if (at !== asked.current) return;
          setShown((s) => s && { batches: [...s.batches, ...next], more: next.length === PAGE });
        },
        (e: unknown) => at === asked.current && snack(forUser("读取改动记录", e)),
      )
      .finally(() => setLoading(false));
  };

  /** Whether it changed something. */
  const undo = (batch: number) => {
    setUndoing((s) => new Set(s).add(batch));
    // Its changes (docs/agent.md#输出): none when what the batch changed is gone or changed back.
    return api<{ data: { changes: unknown[] } }>("POST", "/tools/undo", { batch })
      .then(
        ({ data }) => {
          const changed = data.changes.length > 0;
          if (changed) setShown((s) => s && { ...s, batches: s.batches.map((b) => (b.batch === batch ? { ...b, undone: true } : b)) });
          snack(changed ? "已撤销" : "没有变化");
          return changed;
        },
        (e: unknown) => {
          snack(forUser("撤销", e));
          return false;
        },
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
      <div style={{ marginInline: 16, marginBottom: 8 }}>
        <Seg<"all" | "deleted">
          label="显示哪些"
          items={[
            ["all", "全部"],
            ["deleted", "已删除"],
          ]}
          value={deleted ? "deleted" : "all"}
          onChange={(v) => {
            setShown(null);
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
        !batches.length && <p className="empty">{deleted ? "90 天内暂无删除" : "90 天内暂无改动"}</p>
      )}
      {[...days].map(([day, list]) => (
        <section key={day}>
          <h2 className="group-title">{day}</h2>
          <div className="group">
            {list.map((b) => (
              <div className="row" key={b.batch}>
                <div className="row-inner">
                  <button className="folder-main" onClick={() => setOpen(b)}>
                    <span>{b.summary}</span>
                    <span className="folder-sub log-sub">
                      <span>{b.source}</span>
                      <span>{` · ${isoTime(new Date(b.created_at))} · 批次 ${b.batch}`}</span>
                    </span>
                  </button>
                  {b.undone ? (
                    <span className="folder-sub">已撤销</span>
                  ) : (
                    <button className="text-btn" disabled={undoing.has(b.batch)} onClick={() => void undo(b.batch)}>
                      撤销
                    </button>
                  )}
                </div>
              </div>
            ))}
          </div>
        </section>
      ))}
      {shown?.more && (
        <button className="text-btn log-more" disabled={loading} onClick={() => loadMore(shown.batches)}>
          加载更多
        </button>
      )}
      {open && (
        <LogDetail
          listed={batches?.find((b) => b.batch === open.batch) ?? open}
          undoing={undoing.has(open.batch)}
          undo={() => undo(open.batch)}
          onClose={() => setOpen(null)}
        />
      )}
    </>
  );
}

/** A batch in full: who, when and every change, to be undone here too. */
function LogDetail({ listed, undoing, undo, onClose }: { listed: LogBatch; undoing: boolean; undo(): Promise<boolean>; onClose(): void }) {
  const { store } = useSession();
  const seq = useSyncExternalStore(store.subscribe, () => store.since);
  const now = useNow();
  const [full, setFull] = useState<LogBatch | null>(null);
  /** No longer in the log: older than it keeps. */
  const [gone, setGone] = useState(false);
  const [failed, setFailed] = useState<string | null>(null);
  const [tries, setTries] = useState(0);
  useEffect(() => {
    let live = true;
    log({ batch: listed.batch }).then(
      (found) => {
        if (!live) return;
        setFull(found.length ? found[0] : null);
        setGone(!found.length);
        setFailed(null);
      },
      (e: unknown) => live && setFailed(forUser("读取改动详情", e)),
    );
    return () => {
      live = false;
    };
  }, [listed.batch, seq, tries]);
  const b = full ?? listed;
  const fact = (name: string, value: string) => (
    <div className="row prop">
      <div className="row-inner controls">
        <span className="row-name">{name}</span>
        <span className="row-value">{value}</span>
      </div>
    </div>
  );
  return (
    <Modal
      title="改动详情"
      onClose={onClose}
      footer={
        <button className="text-btn" disabled={b.undone || undoing || gone} onClick={() => void undo().then((changed) => changed && setFull((f) => f && { ...f, undone: true }))}>
          {b.undone ? "已撤销" : "撤销"}
        </button>
      }
    >
      <div className="group">
        {fact("来源", b.source)}
        {fact("时间", formatMoment(b.created_at, now))}
        {fact("批次", String(b.batch))}
        {full?.undoes != null && fact("撤销的批次", String(full.undoes))}
      </div>
      <h2 className="group-title">改动</h2>
      {full ? (
        <div className="group">
          {(full.changes.length ? full.changes : ["没有变化"]).map((line, i) => (
            <div key={i} className="row prop">
              <div className="row-inner">{line}</div>
            </div>
          ))}
        </div>
      ) : gone ? (
        <p className="empty">这个批次已不在改动记录里</p>
      ) : failed ? (
        <p className="empty">
          {failed}
          <br />
          <button className="text-btn" onClick={() => setTries((n) => n + 1)}>
            重试
          </button>
        </p>
      ) : (
        <p className="empty">正在读取…</p>
      )}
    </Modal>
  );
}
