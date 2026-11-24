// Sharing folders (docs/ui.md#共享): people's avatars, the share panel, and choosing who is to do a
// task. Adding and taking off people go to the server straight away rather than through the local
// copy: who a username is, is only known there.

import { type CSSProperties, type FormEvent, useRef, useState } from "react";
import { v7 as uuidv7 } from "uuid";
import { useLocation } from "wouter";
import { ApiError, api } from "../api/http";
import type { Applied } from "../api/types/Applied";
import type { Effect } from "../api/types/Effect";
import type { FolderId } from "../api/types/FolderId";
import type { MutationId } from "../api/types/MutationId";
import type { Op } from "../api/types/Op";
import type { Board } from "../domain/board";
import { useView } from "../session";
import { Box } from "../ui/controls";
import { Modal, Tip, forUser, useFeedback } from "../ui/feedback";
import { folderUrl } from "./nav";

/** Sends a change to the server and waits for it to be made. */
async function send(ops: Op[]): Promise<{ id: MutationId; effects: Effect[] }> {
  const id = uuidv7();
  const { effects } = await api<Applied>("POST", "/mutations", { id, ops });
  return { id, effects };
}

const sized = (size: number): CSSProperties => ({ "--size": `${size}px` });

/** Someone's avatar, `size` across: the first character of their name, beside which it stands. */
function Avatar({ board, id, size }: { board: Board; id: string; size: number }) {
  return (
    <span className="person" style={sized(size)} aria-hidden>
      {[...board.name(id)][0]}
    </span>
  );
}

/** People's avatars, `size` across, overlapping, each named on hovering it; past `max`, how many more. */
export function Avatars({ board, ids, size, max }: { board: Board; ids: string[]; size: number; max: number }) {
  const shown = ids.length > max ? ids.slice(0, max - 1) : ids;
  return (
    <span className="people" style={sized(size)}>
      {shown.map((id) => (
        <Tip key={id} label={board.name(id)}>
          <span className="person">{[...board.name(id)][0]}</span>
        </Tip>
      ))}
      {shown.length < ids.length && <span className="person more">+{ids.length - shown.length}</span>}
    </span>
  );
}

/** Who a shared folder is shared by and with, its owner first. */
export const sharers = (board: Board, f: FolderId) => {
  const share = board.share(f)!;
  return [share.owner, ...share.members];
};

/** `Alice`, `Alice、Bob`, `Alice、Bob 等 3 人`. */
export function namesOf(board: Board, ids: string[]): string {
  const names = ids.slice(0, 2).map((id) => board.name(id));
  return ids.length > 2 ? `${names.join("、")} 等 ${ids.length} 人` : names.join("、");
}

/** Who shares folder `f` with the user, when it is a folder someone else shares. */
export function sharer(board: Board, f: FolderId): string | null {
  const share = board.share(f);
  return share && share.folder_id === f && share.owner !== board.me ? share.owner : null;
}

/** A folder's name, and who shares it when that is someone else: `装修 · Alice`. */
export function FolderName({ board, id }: { board: Board; id: FolderId | null }) {
  const by = id && sharer(board, id);
  return (
    <>
      {board.folderName(id)}
      {by && <span className="shared-by"> · {board.name(by)}</span>}
    </>
  );
}

/** Whether a folder can be shared, or is: not the root, not in a shared folder, none inside it. */
export function shareable(board: Board, f: FolderId | null): f is FolderId {
  if (!f) return false;
  const root = board.sharedRoot(f);
  return root ? root === f : !board.sharesWithin(f);
}

/** How many dependencies link tasks in folder `f` to tasks outside it. */
function crossing(board: Board, f: FolderId): number {
  const inside = (id: string) => board.inSubtree(board.task(id)!.folder_id, f);
  return board.snapshot.dependencies.filter((d) => inside(d.before) !== inside(d.after)).length;
}

/** What a move took away, if anything: `，去掉了 2 条依赖和 1 位负责人`. */
export function lost(effects: Effect[]): string {
  let deps = 0;
  let people = 0;
  for (const e of effects) {
    if (e.kind === "dependency" && !e.after) deps++;
    if (e.kind === "task" && e.before && e.after) people += e.before.assignees.filter((u) => !e.after!.assignees.includes(u)).length;
  }
  const parts = [deps && `${deps} 条依赖`, people && `${people} 位负责人`].filter(Boolean);
  return parts.length ? `，去掉了 ${parts.join("和 ")}` : "";
}

/** One of the people a folder is shared with, as the panels list them. */
function Person({ board, id, children }: { board: Board; id: string; children?: React.ReactNode }) {
  return (
    <div className="row-inner">
      <Avatar board={board} id={id} size={32} />
      <span className="folder-main">
        <span>
          {board.name(id)}
          {id === board.me && "（我）"}
        </span>
        <span className="folder-sub">{board.person(id)?.username}</span>
      </span>
      {children}
    </div>
  );
}

/** The share panel of folder `folder`: who it is shared with; its owner adds and takes off people,
 * or stops sharing it, and the others can leave it. */
export function SharePanel({ folder, onClose }: { folder: FolderId; onClose(): void }) {
  const { board } = useView();
  const { snack, confirm } = useFeedback();
  const [location, navigate] = useLocation();
  const [username, setUsername] = useState("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const input = useRef<HTMLInputElement>(null);
  const f = board.folder(folder);
  // Gone meanwhile, deleted, or no longer shared with the user.
  if (!f) return null;
  const share = board.share(folder);
  const owner = share?.owner ?? board.me;
  const members = share?.members ?? [];
  const mine = owner === board.me;

  const undo = (id: MutationId) =>
    void send([{ op: "undo", mutation: id }]).then(
      () => snack("已撤销"),
      (e: unknown) => snack(forUser("撤销", e)),
    );
  /** Sends `ops` and tells what they did, `said` of the effects, with undo unless it is leaving. */
  const change = async (ops: Op[], said: (effects: Effect[]) => string, undoable = true) => {
    setBusy(true);
    try {
      const { id, effects } = await send(ops);
      snack(undoable ? { text: said(effects), action: "撤销", onAction: () => undo(id) } : said(effects));
    } finally {
      setBusy(false);
    }
  };

  const add = async (e: FormEvent) => {
    e.preventDefault();
    const name = username.trim();
    if (!name || busy) return;
    try {
      // By their name: in the change when new to the user, or known from another folder.
      await change([{ op: "share.add", folder_id: folder, username: name }], (effects) => {
        const met = effects.flatMap((x) => (x.kind === "person" && x.after ? [x.after] : []));
        const known = board.snapshot.people.find((p) => p.username.toLowerCase() === name.toLowerCase());
        return `已添加 ${(met[0] ?? known)!.display_name}`;
      });
      setUsername("");
    } catch (e) {
      // Under the field, the reason says it all: “没有用户“dave””.
      setError(e instanceof ApiError ? e.message : forUser("添加", e));
    }
    input.current?.focus();
  };
  const remove = (user: string) =>
    change([{ op: "share.remove", folder_id: folder, user_id: user }], () => `已移除 ${board.name(user)}`).catch((e: unknown) =>
      snack(forUser("移除", e)),
    );
  const stop = async () => {
    const ok = await confirm({ title: `停止共享“${f.name}”？`, body: "所有成员将无法查看这个文件夹，事项的负责人一并去掉。", ok: "停止共享", danger: true });
    if (!ok) return;
    const ops = members.map((user): Op => ({ op: "share.remove", folder_id: folder, user_id: user }));
    try {
      await change(ops, () => `已停止共享“${f.name}”`);
      onClose();
    } catch (e) {
      snack(forUser("停止共享", e));
    }
  };
  const leave = async () => {
    const body = `退出后将无法查看这个文件夹，须 ${board.name(owner)} 重新添加才能回来。`;
    if (!(await confirm({ title: `退出共享“${f.name}”？`, body, ok: "退出", danger: true }))) return;
    try {
      await change([{ op: "share.remove", folder_id: folder, user_id: board.me }], () => `已退出共享“${f.name}”`, false);
    } catch (e) {
      snack(forUser("退出共享", e));
      return;
    }
    onClose();
    // Out of a page that has gone with it.
    const shown = location.match(/^\/folders\/([^/]+)$/)?.[1];
    if (shown && board.hasFolder(shown) && board.inSubtree(shown, folder)) navigate(folderUrl(null), { replace: true });
  };

  const crossed = share ? 0 : crossing(board, folder);
  return (
    <Modal
      title={`共享“${f.name}”`}
      onClose={onClose}
      focus={mine ? input : undefined}
      footer={
        (!mine || members.length > 0) && (
          <button className="text-btn danger-text" disabled={busy} onClick={() => void (mine ? stop() : leave())}>
            {mine ? "停止共享" : "退出共享"}
          </button>
        )
      }
    >
      {mine && (
        <form className={`field${error ? " error" : ""}`} onSubmit={add}>
          <span className="share-add">
            <input
              ref={input}
              value={username}
              onChange={(e) => (setUsername(e.target.value), setError(""))}
              placeholder="输入用户名添加"
              aria-label="用户名"
              aria-invalid={error ? true : undefined}
              autoCapitalize="off"
              autoComplete="off"
              spellCheck={false}
            />
            <button className="tonal-btn" disabled={!username.trim() || busy}>
              添加
            </button>
          </span>
          {error && (
            <span className="hint error-text" role="alert">
              {error}
            </span>
          )}
        </form>
      )}
      {!members.length && (
        <p className="share-note">
          共享后，成员可以查看和修改这个文件夹里的一切。
          {crossed > 0 && `连到外面的 ${crossed} 条依赖将去掉。`}
        </p>
      )}
      <div className="group share-people">
        {[owner, ...members].map((id) => (
          <div key={id} className="row">
            <Person board={board} id={id}>
              {id === owner ? (
                <span className="share-role">共享人</span>
              ) : (
                mine && (
                  <button className="text-btn danger-text" disabled={busy} onClick={() => void remove(id)}>
                    移除
                  </button>
                )
              )}
            </Person>
          </div>
        ))}
      </div>
    </Modal>
  );
}

/** Beside the title of a shared folder's page, who it is shared by and with, which opens its share
 * panel. */
export function SharedWith({ folder }: { folder: FolderId | null }) {
  const { board } = useView();
  const [open, setOpen] = useState(false);
  if (!folder || board.sharedRoot(folder) !== folder) return null;
  const ids = sharers(board, folder);
  return (
    <>
      <button className="shared-with" aria-label={`共享：${namesOf(board, ids)}`} onClick={() => setOpen(true)}>
        <Avatars board={board} ids={ids} size={28} max={4} />
      </button>
      {open && <SharePanel folder={folder} onClose={() => setOpen(false)} />}
    </>
  );
}

/** Chooses who is to do a task in shared folder `folder`, of those it is shared with: each tap counts
 * at once (`onToggle`), or, given `onDone`, all of them once confirmed. */
export function AssigneePicker({
  folder,
  chosen,
  onToggle,
  onDone,
  onClose,
}: {
  folder: FolderId;
  chosen: string[];
  onToggle?: (user: string, on: boolean) => void;
  onDone?: (users: string[]) => void;
  onClose(): void;
}) {
  const { board } = useView();
  const [picked, setPicked] = useState(chosen);
  const now = onDone ? picked : chosen;
  const tap = (id: string) => {
    const on = !now.includes(id);
    if (onDone) setPicked(on ? [...picked, id] : picked.filter((x) => x !== id));
    else onToggle!(id, on);
  };
  // No longer shared with the user, or not at all.
  if (!board.share(folder)) return null;
  return (
    <Modal
      title="负责人"
      onClose={onClose}
      footer={
        onDone && (
          <>
            <button className="text-btn" onClick={onClose}>
              取消
            </button>
            <button className="filled-btn" onClick={() => (onDone(picked), onClose())}>
              确定
            </button>
          </>
        )
      }
    >
      <div className="group share-people">
        {sharers(board, folder).map((id) => (
          <button key={id} className="row" role="checkbox" aria-checked={now.includes(id)} onClick={() => tap(id)}>
            <Person board={board} id={id}>
              <Box checked={now.includes(id)} />
            </Person>
          </button>
        ))}
      </div>
    </Modal>
  );
}
