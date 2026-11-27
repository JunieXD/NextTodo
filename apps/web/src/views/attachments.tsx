// Attachments (docs/ui/attachments.md): a task's in its details, pictures as thumbnails and other files as
// rows, which open, save, are renamed and deleted there; the picture viewer; and the files staged in an
// add box to go with what is added there.

import {
  Camera,
  ChevronLeft,
  ChevronRight,
  CircleAlert,
  CloudOff,
  Copy,
  Download,
  ExternalLink,
  FileText,
  File as FileIcon,
  FolderOpen,
  Image as ImageIcon,
  Images,
  LoaderCircle,
  type LucideIcon,
  Pencil,
  Plus,
  Quote,
  Trash2,
  X,
} from "lucide-react";
import { Dialog } from "radix-ui";
import { type ReactNode, useContext, useEffect, useRef, useState } from "react";
import { NextTodo, writeAttachment } from "../mobile";
import { ApiError, app, phoneApp } from "../api/http";
import type { Attachment } from "../api/types/Attachment";
import type { Mutation } from "../api/types/Mutation";
import type { TaskId } from "../api/types/TaskId";
import { PICTURES, type Planned, addOps, bytesOf, isPicture, plan, split } from "../attach";
import * as desktop from "../desktop";
import { checkFileName } from "../domain/text";
import { useCommit, useConnection, useSession, useUndo, useView } from "../session";
import type { Staged } from "../sync/drafts";
import { dropFiles } from "../sync/files";
import { useBack } from "../ui/back";
import { RootContext, Tip, forUser, useFeedback } from "../ui/feedback";
import { useMedia } from "../ui/hooks";
import { Icon } from "../ui/icons";
import { useLeave } from "../ui/leave";
import { ContextMenu, MenuButton, type MenuItem } from "../ui/menu";

/** What a browser opens in a tab of its own rather than downloading: what runs no script there
 * (docs/web.md#附件). Text is opened as plain text. */
const TABBED = new Set([...PICTURES, "application/pdf"]);

/** `320 KB`, `1.2 MB`: as the server's tools say it (`crates/server/src/tools/text.rs`). */
export function sizeLabel(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${Math.ceil(n / 1024)} KB`;
  return `${(n / 1024 / 1024).toFixed(1).replace(/\.0$/, "")} MB`;
}

export const fileIcon = (type: string): LucideIcon => (type.startsWith("image/") ? ImageIcon : type.startsWith("text/") || type === "application/pdf" ? FileText : FileIcon);

/** The attachments the outbox has yet to send, waiting for their files to be uploaded. */
const waitingIn = (queued: Mutation[]) => new Set(queued.flatMap((m) => m.ops.flatMap((op) => (op.op === "attachment.add" ? [op.id] : []))));

/** An object URL of the file, kept here or downloaded, while it is shown; `failed` when it could not
 * be had, tried again once the server can be reached, or when the picture is one the browser cannot
 * show, as its `<img>` says by calling `unreadable`. */
function useFileUrl(id: string, type: string) {
  const { files } = useSession();
  const online = useConnection() === "online";
  const [shown, setShown] = useState<{ url?: string; failed?: "unreached" | "unreadable" }>({});
  const again = shown.failed === "unreached" && online;
  useEffect(() => {
    let live = true;
    let url: string | undefined;
    files.blob({ id, type }).then(
      (b) => {
        if (!live) return;
        url = URL.createObjectURL(b);
        setShown({ url });
      },
      (e) => {
        if (!(e instanceof ApiError)) throw e;
        if (live) setShown({ failed: "unreached" });
      },
    );
    return () => {
      live = false;
      if (url) URL.revokeObjectURL(url);
    };
  }, [files, id, type, again]);
  return { url: shown.url, failed: !!shown.failed, unreadable: () => setShown({ failed: "unreadable" }) };
}

/** Saves `file`, what `as` says it is: in the apps where the user picks, in a browser as a download.
 * What to say of where it went; nothing when the user cancelled, or when the browser has it. */
export async function saveFile(as: Pick<Attachment, "id" | "name" | "type">, file: Blob): Promise<string | undefined> {
  if (phoneApp) {
    await writeAttachment(as, file);
    const { saved } = await NextTodo.saveAttachment({ id: as.id, name: as.name, type: as.type });
    return saved && `已保存“${saved}”`;
  }
  if (app === "desktop") {
    const to = await desktop.saveAttachment(as, file);
    return to ? `已保存到 ${to}` : undefined;
  }
  download(as.name, file);
}

function download(name: string, b: Blob) {
  const url = URL.createObjectURL(b);
  const link = document.createElement("a");
  link.href = url;
  link.download = name;
  link.click();
  // Once the browser has taken it.
  setTimeout(() => URL.revokeObjectURL(url), 60_000);
}

/** Opening and saving attachments' files: in the app by its native part, in a browser in a tab or as
 * a download (docs/web.md#附件). */
function useFileActions() {
  const { files } = useSession();
  const { snack } = useFeedback();
  const failed = (what: string) => (e: unknown) => snack(forUser(what, e));
  return {
    open(a: Attachment) {
      if (phoneApp) {
        return void files
          .blob(a)
          .then((b) => writeAttachment(a, b))
          .then(() => NextTodo.openAttachment({ id: a.id, name: a.name, type: a.type }))
          .catch(failed("打开附件"));
      }
      if (app === "desktop") return void files.blob(a).then((b) => desktop.openAttachment(a, b)).catch(failed("打开附件"));
      const text = a.type.startsWith("text/");
      // The tab opens now, while the click still allows it; the file comes after.
      const tab = text || TABBED.has(a.type) ? window.open("", "_blank") : null;
      files.blob(a).then(
        (b) => {
          if (!tab) return download(a.name, b);
          tab.opener = null;
          const url = URL.createObjectURL(text ? new Blob([b], { type: "text/plain; charset=utf-8" }) : b);
          tab.location.href = url;
          setTimeout(() => URL.revokeObjectURL(url), 60_000);
        },
        (e) => {
          tab?.close();
          failed("打开附件")(e);
        },
      );
    },
    save(a: Attachment) {
      files
        .blob(a)
        .then((b) => saveFile(a, b))
        .then((said) => said && snack(said), failed(app ? "保存附件" : "下载附件"));
    },
    copy(a: Attachment) {
      // Written now, while the click or key still allows it, with the picture to come.
      void navigator.clipboard.write([new ClipboardItem({ "image/png": files.blob(a).then(png) })]).then(() => snack("已拷贝图片"), failed("拷贝图片"));
    },
  };
}

/** Whether pictures can be copied: not in the phone apps, nor where the page is not served securely
 * (a server on plain HTTP), as there is no clipboard to write to then. */
const copyable = !phoneApp && "ClipboardItem" in window;

/** A picture as PNG, the one kind of picture every clipboard takes. */
async function png(b: Blob): Promise<Blob> {
  if (b.type === "image/png") return b;
  const picture = await createImageBitmap(b);
  const canvas = new OffscreenCanvas(picture.width, picture.height);
  canvas.getContext("2d")!.drawImage(picture, 0, 0);
  return canvas.convertToBlob();
}

/** What can be done to an attachment, from its menu and the viewer's bar. */
function useAttachmentActions(refer: (a: Attachment) => void) {
  const commit = useCommit();
  const undo = useUndo();
  const { prompt, snack } = useFeedback();
  const file = useFileActions();
  const rename = async (a: Attachment) => {
    const name = await prompt({ title: "重命名附件", label: "名称", value: a.name, select: split(a.name)[0].length, ok: "保存", check: checkFileName });
    if (name !== null && name !== a.name) commit([{ op: "attachment.update", id: a.id, name }]);
  };
  /** `afterRemove`: what follows deleting it, once deleted. */
  const remove = (a: Attachment, afterRemove?: () => void) => {
    const r = commit([{ op: "attachment.delete", id: a.id }]);
    if (!r) return;
    snack({ text: `已删除“${a.name}”`, action: "撤销", onAction: () => undo(r.id) });
    afterRemove?.();
  };
  const menu = (a: Attachment, afterRemove?: () => void): MenuItem[] => {
    const picture = isPicture(a.type);
    return [
      { icon: Quote, label: "在描述中引用", onSelect: () => refer(a) },
      { icon: Pencil, label: "重命名", onSelect: () => void rename(a) },
      ...(picture && copyable ? [{ icon: Copy, label: "拷贝图片", onSelect: () => file.copy(a) }] : []),
      // Any other file opens so as it is clicked.
      ...(picture && app === "desktop" ? [{ icon: ExternalLink, label: "用默认应用打开", onSelect: () => file.open(a) }] : []),
      { icon: Download, label: app ? "保存到…" : "下载", onSelect: () => file.save(a) },
      { icon: Trash2, label: "删除", danger: true, onSelect: () => remove(a, afterRemove) },
    ];
  };
  return { open: file.open, copy: file.copy, refer, remove, menu };
}

/** Keeps planned files on the device, to be uploaded once added: those kept, to act on at once; none
 * once the session has ended. Tells the user of each that could not be, such as a photo still too
 * large made smaller. */
export function useKeepFiles() {
  const { files } = useSession();
  const { snack } = useFeedback();
  return async (planned: Planned[]): Promise<Staged[]> => {
    const kept: Staged[] = [];
    for (const p of planned) {
      try {
        const blob = await bytesOf(p);
        await files.keep(p.id, blob);
        kept.push({ id: p.id, name: p.name, type: p.type, size: blob.size });
      } catch (e) {
        snack(forUser(`添加“${p.name}”`, e));
      }
    }
    // Ended meanwhile: what was kept goes with the rest of the user's data.
    return files.stopped ? [] : kept;
  };
}

/** Files being added to a task: those planned, at once, to refer to; and once they are kept on the
 * device, the ids of those added. */
export interface Adding {
  planned: Planned[];
  added: Promise<string[]>;
}

/** Adds `files` to task `task`, each in a change of its own, so that one the server turns down leaves
 * the others. */
export function useAddFiles() {
  const { user, store } = useSession();
  const commit = useCommit();
  const keepFiles = useKeepFiles();
  const { snack } = useFeedback();
  const add = async (task: TaskId, planned: Planned[]) => {
    const kept = await keepFiles(planned);
    const added = kept.filter((f) => commit(addOps([f], { task_id: task })));
    void dropFiles(
      user.id,
      kept.filter((f) => !added.includes(f)).map((f) => f.id),
    );
    return added.map((f) => f.id);
  };
  return (task: TaskId, files: File[]): Adding => {
    const planned = plan(files, store.getView().board.attachments(task).map((a) => a.name), snack);
    return { planned, added: add(task, planned) };
  };
}

/** Choosing files to add, with the button `children` makes: on a touch screen from the camera, the
 * photos or the files, else from the files. Calls `onAdd` with those chosen. */
export function ChooseFiles({ className, label, onAdd, children }: { className: string; label?: string; onAdd(files: File[]): void; children: ReactNode }) {
  const touch = useMedia("(pointer: coarse)");
  const files = useRef<HTMLInputElement>(null);
  const photos = useRef<HTMLInputElement>(null);
  const camera = useRef<HTMLInputElement>(null);
  const chosen = (e: React.ChangeEvent<HTMLInputElement>) => {
    const list = [...(e.target.files ?? [])];
    // Choosing the same file again is a change too.
    e.target.value = "";
    onAdd(list);
  };
  const button = (
    <button type="button" className={className} onClick={() => files.current!.click()}>
      {children}
    </button>
  );
  return (
    <>
      {touch ? (
        <MenuButton
          className={className}
          label={label}
          align="start"
          items={[
            { icon: Camera, label: "拍照", onSelect: () => camera.current!.click() },
            { icon: Images, label: "相册", onSelect: () => photos.current!.click() },
            { icon: FolderOpen, label: "文件", onSelect: () => files.current!.click() },
          ]}
        >
          {children}
        </MenuButton>
      ) : label ? (
        <Tip label={label}>{button}</Tip>
      ) : (
        button
      )}
      <input ref={files} type="file" multiple hidden onChange={chosen} />
      <input ref={photos} type="file" accept="image/*" multiple hidden onChange={chosen} />
      <input ref={camera} type="file" accept="image/*" capture="environment" hidden onChange={chosen} />
    </>
  );
}

/** The state of a file not uploaded yet, over its thumbnail: uploading, or offline. */
function Waiting() {
  const offline = useConnection() === "offline";
  return <span className={`att-state${offline ? "" : " spin"}`}>{<Icon of={offline ? CloudOff : LoaderCircle} size={14} />}</span>;
}

/** A picture's thumbnail; with a mouse over it, the actions most used on it in its corner. */
function Thumb({ a, waiting, actions, onOpen }: { a: Attachment; waiting: boolean; actions: ReturnType<typeof useAttachmentActions>; onOpen(): void }) {
  const { url, failed, unreadable } = useFileUrl(a.id, a.type);
  return (
    <ContextMenu items={actions.menu(a)}>
      <div className="att-thumb">
        <button type="button" className="att-open" onClick={onOpen} aria-label={a.name}>
          {url ? (
            <img src={url} alt="" draggable={false} onError={unreadable} />
          ) : (
            <span className={`att-empty${failed ? "" : " spin"}`}>
              <Icon of={failed ? ImageIcon : LoaderCircle} />
            </span>
          )}
          {waiting && <Waiting />}
        </button>
        <span className="att-quick">
          <Tip label="在描述中引用">
            <button type="button" onClick={() => actions.refer(a)}>
              <Icon of={Quote} size={16} />
            </button>
          </Tip>
          <Tip label="删除">
            <button type="button" onClick={() => actions.remove(a)}>
              <Icon of={Trash2} size={16} />
            </button>
          </Tip>
        </span>
      </div>
    </ContextMenu>
  );
}

function FileRow({ a, waiting, menu, onOpen }: { a: Attachment; waiting: boolean; menu: MenuItem[]; onOpen(): void }) {
  return (
    <ContextMenu items={menu}>
      <div className="row att-file" onClick={onOpen}>
        <div className="row-inner">
          <Icon of={fileIcon(a.type)} />
          {/* The whole row opens it; this button is how keyboards and screen readers get there. */}
          <button type="button" className="att-name">
            {a.name}
          </button>
          <span className="att-size">{waiting ? "等待上传" : sizeLabel(a.size)}</span>
          <MenuButton label="附件操作" items={menu} className="icon-btn sm" />
        </div>
      </div>
    </ContextMenu>
  );
}

/** Task `task`'s attachments: the section in its details, below the description, and opening one, as
 * from a reference to it. `refer` puts a reference to one in the description; `onAdd` adds files. */
export function useAttachments(task: TaskId, refer: (a: Attachment) => void, onAdd: (files: File[]) => void) {
  const { board } = useView();
  const { store } = useSession();
  const actions = useAttachmentActions(refer);
  const [viewing, setViewing] = useState<number | null>(null);
  const list = board.attachments(task);
  const pictures = list.filter((a) => isPicture(a.type));
  // All deleted meanwhile, as on another device, it closes.
  if (viewing !== null && !pictures.length) setViewing(null);
  const waiting = waitingIn(store.queued);
  const open = (a: Attachment) => (isPicture(a.type) ? setViewing(pictures.findIndex((p) => p.id === a.id)) : actions.open(a));
  const section = (
    <>
      <div className="d-sec">
        <h3>附件</h3>
      </div>
      <div className="group">
        {pictures.length > 0 && (
          <div className="att-grid">
            {pictures.map((a) => (
              <Thumb key={a.id} a={a} waiting={waiting.has(a.id)} actions={actions} onOpen={() => open(a)} />
            ))}
          </div>
        )}
        {list
          .filter((a) => !isPicture(a.type))
          .map((a) => (
            <FileRow key={a.id} a={a} waiting={waiting.has(a.id)} menu={actions.menu(a)} onOpen={() => open(a)} />
          ))}
        <ChooseFiles className="row add-row" onAdd={onAdd}>
          <div className="row-inner">
            <Icon of={Plus} />
            添加附件
          </div>
        </ChooseFiles>
      </div>
      {viewing !== null && <Viewer key={viewing} pictures={pictures} start={viewing} actions={actions} onClose={() => setViewing(null)} />}
    </>
  );
  return { section, open };
}

/** How far the viewer zooms in, at most, from fitting the screen. */
const MAX_ZOOM = 8;

type Zoom = { scale: number; x: number; y: number };
const FIT: Zoom = { scale: 1, x: 0, y: 0 };

/** `z` zoomed to `scale` about the point (`fx`, `fy`) of the stage, from its centre: that point stays
 * where it is. */
function zoomAt(z: Zoom, scale: number, fx: number, fy: number): Zoom {
  const s = Math.min(MAX_ZOOM, Math.max(1, scale));
  if (s === 1) return FIT;
  return { scale: s, x: fx - ((fx - z.x) * s) / z.scale, y: fy - ((fy - z.y) * s) / z.scale };
}

/** The pictures full screen, one at a time from `start`: zoomed about the pointer by the wheel (a
 * trackpad's scroll and pinch too), by pinching and by + − 0, and dragged about then; a double tap or
 * click goes between fitting the window and the picture's own size. Not zoomed, a swipe, the arrow keys
 * or the ‹ › go to the next or the one before. A tap off the picture closes it, as deleting it does. */
function Viewer({ pictures, start, actions, onClose }: { pictures: Attachment[]; start: number; actions: ReturnType<typeof useAttachmentActions>; onClose(): void }) {
  const root = useContext(RootContext);
  const wide = !useMedia("(pointer: coarse)");
  const [index, setIndex] = useState(start);
  // One deleted meanwhile, as on another device: the next one takes its place.
  const at = Math.min(index, pictures.length - 1);
  const a = pictures[at];
  const [zoom, setZoom] = useState(FIT);
  const page = useRef<HTMLDivElement>(null);
  const stage = useRef<HTMLDivElement>(null);
  const img = useRef<HTMLImageElement>(null);
  const { url, failed, unreadable } = useFileUrl(a.id, a.type);
  useLeave([page]);
  useBack("dialog", onClose);
  const go = (by: number) => {
    const next = at + by;
    if (next < 0 || next >= pictures.length) return;
    setIndex(next);
    setZoom(FIT);
  };
  /** Zooms by `f` about the middle. */
  const zoomBy = (f: number) => setZoom((z) => zoomAt(z, z.scale * f, 0, 0));

  const latest = useRef({ zoom, go, close: onClose });
  useEffect(() => {
    latest.current = { zoom, go, close: onClose };
  });

  useEffect(() => {
    const el = stage.current!;
    const centre = (x: number, y: number) => {
      const r = el.getBoundingClientRect();
      return [x - r.left - r.width / 2, y - r.top - r.height / 2] as const;
    };
    /** The picture's own size, as a scale of fitting the screen; twice that for one smaller. */
    const own = () => {
      const i = img.current;
      return i?.naturalWidth ? Math.max(2, i.naturalWidth / i.getBoundingClientRect().width) * latest.current.zoom.scale : 2;
    };
    /** Where the picture is drawn now; nowhere while it loads, or when it can't be shown. */
    const drawn = () => img.current?.getBoundingClientRect();
    const pointers = new Map<number, { x: number; y: number }>();
    let pinch: { distance: number; zoom: Zoom } | null = null;
    let press: { x: number; y: number; at: number; moved: boolean } | null = null;
    let lastTap: { x: number; y: number; at: number } | null = null;
    const distance = () => {
      const [p, q] = [...pointers.values()];
      return Math.hypot(p.x - q.x, p.y - q.y);
    };
    const down = (e: PointerEvent) => {
      // A finger, a pen or a mouse's main button; the others leave the picture be.
      if (e.button !== 0) return;
      el.setPointerCapture(e.pointerId);
      pointers.set(e.pointerId, { x: e.clientX, y: e.clientY });
      if (pointers.size === 2) {
        pinch = { distance: distance(), zoom: latest.current.zoom };
        press = null;
      } else press = { x: e.clientX, y: e.clientY, at: e.timeStamp, moved: false };
    };
    const move = (e: PointerEvent) => {
      const last = pointers.get(e.pointerId);
      if (!last) return;
      pointers.set(e.pointerId, { x: e.clientX, y: e.clientY });
      if (pinch && pointers.size === 2) {
        const [p, q] = [...pointers.values()];
        const [fx, fy] = centre((p.x + q.x) / 2, (p.y + q.y) / 2);
        setZoom(zoomAt(pinch.zoom, (pinch.zoom.scale * distance()) / pinch.distance, fx, fy));
        return;
      }
      if (!press) return;
      if (Math.hypot(e.clientX - press.x, e.clientY - press.y) > 10) press.moved = true;
      // Zoomed in, it follows; not zoomed, the swipe is told when it ends.
      if (latest.current.zoom.scale > 1) setZoom((z) => ({ ...z, x: z.x + e.clientX - last.x, y: z.y + e.clientY - last.y }));
    };
    const up = (e: PointerEvent) => {
      pointers.delete(e.pointerId);
      if (pointers.size < 2) pinch = null;
      const p = press;
      press = null;
      if (!p || pointers.size) return;
      const dx = e.clientX - p.x;
      if (p.moved) {
        if (latest.current.zoom.scale === 1 && Math.abs(dx) > 60 && Math.abs(dx) > Math.abs(e.clientY - p.y)) latest.current.go(dx < 0 ? 1 : -1);
        return;
      }
      // A tap: off the picture it closes; on it, a second one soon after, near the first, zooms.
      const on = drawn();
      if (!on || e.clientX < on.left || e.clientX > on.right || e.clientY < on.top || e.clientY > on.bottom) return latest.current.close();
      if (lastTap && e.timeStamp - lastTap.at < 300 && Math.hypot(e.clientX - lastTap.x, e.clientY - lastTap.y) < 30) {
        lastTap = null;
        const [fx, fy] = centre(e.clientX, e.clientY);
        setZoom((z) => (z.scale > 1 ? FIT : zoomAt(z, own(), fx, fy)));
      } else lastTap = { x: e.clientX, y: e.clientY, at: e.timeStamp };
    };
    // Taken by the system, as by a swipe from the screen's edge: not a tap, a swipe or a pinch.
    const cancel = (e: PointerEvent) => {
      pointers.delete(e.pointerId);
      pinch = null;
      press = null;
    };
    const wheel = (e: WheelEvent) => {
      e.preventDefault();
      const on = drawn();
      // Nothing to zoom yet.
      if (!on) return;
      // About the pointer, or the picture's nearest edge to it: about a point off the picture, zooming
      // in would move it out of the window.
      const within = (v: number, from: number, to: number) => Math.min(to, Math.max(from, v));
      const [fx, fy] = centre(within(e.clientX, on.left, on.right), within(e.clientY, on.top, on.bottom));
      // In pixels: Firefox gives a mouse wheel's in lines.
      const dy = e.deltaMode === WheelEvent.DOM_DELTA_LINE ? e.deltaY * 40 : e.deltaY;
      // A trackpad's pinch comes as the wheel with Ctrl, in steps smaller than a wheel's.
      setZoom((z) => zoomAt(z, z.scale * Math.exp(-dy / (e.ctrlKey ? 100 : 300)), fx, fy));
    };
    el.addEventListener("pointerdown", down);
    el.addEventListener("pointermove", move);
    el.addEventListener("pointerup", up);
    el.addEventListener("pointercancel", cancel);
    el.addEventListener("wheel", wheel, { passive: false });
    return () => {
      el.removeEventListener("pointerdown", down);
      el.removeEventListener("pointermove", move);
      el.removeEventListener("pointerup", up);
      el.removeEventListener("pointercancel", cancel);
      el.removeEventListener("wheel", wheel);
    };
  }, []);

  return (
    <Dialog.Root open onOpenChange={(o) => !o && onClose()}>
      <Dialog.Portal container={root}>
        <Dialog.Content
          ref={page}
          className="viewer enter"
          aria-describedby={undefined}
          onKeyDown={(e) => {
            if (e.metaKey || e.ctrlKey) {
              if (e.key === "c" && copyable) {
                e.preventDefault();
                actions.copy(a);
              }
              return;
            }
            if (e.key === "ArrowLeft") go(-1);
            if (e.key === "ArrowRight") go(1);
            if (e.key === "+" || e.key === "=") zoomBy(1.25);
            if (e.key === "-") zoomBy(0.8);
            if (e.key === "0") setZoom(FIT);
          }}
          // The snackbar over it tells of what was done here, such as copying the picture; tapping it
          // leaves the viewer open.
          onInteractOutside={(e) => (e.target as Element).closest(".snackbar") && e.preventDefault()}
        >
          <div className="viewer-bar" data-tauri-drag-region="deep">
            <Tip label="关闭">
              <Dialog.Close className="icon-btn">
                <Icon of={X} />
              </Dialog.Close>
            </Tip>
            <Dialog.Title className="viewer-title">
              <span>{a.name}</span>
              {pictures.length > 1 && (
                <span className="tabular-nums">
                  {at + 1}/{pictures.length}
                </span>
              )}
            </Dialog.Title>
            {actions.menu(a, onClose).map((item) => (
              <Tip key={item.label} label={item.label}>
                <button className="icon-btn" onClick={item.onSelect}>
                  <Icon of={item.icon} />
                </button>
              </Tip>
            ))}
          </div>
          <div ref={stage} className="viewer-stage">
            {url ? (
              <img
                ref={img}
                key={a.id}
                src={url}
                alt={a.name}
                draggable={false}
                onError={unreadable}
                style={{ transform: `translate(${zoom.x}px, ${zoom.y}px) scale(${zoom.scale})` }}
              />
            ) : (
              <span className={`viewer-empty${failed ? "" : " spin"}`}>
                <Icon of={failed ? ImageIcon : LoaderCircle} size={32} />
              </span>
            )}
          </div>
          {wide && pictures.length > 1 && (
            <>
              <button className="viewer-step prev" disabled={at === 0} onClick={() => go(-1)} aria-label="上一张">
                <Icon of={ChevronLeft} size={28} />
              </button>
              <button className="viewer-step next" disabled={at === pictures.length - 1} onClick={() => go(1)} aria-label="下一张">
                <Icon of={ChevronRight} size={28} />
              </button>
            </>
          )}
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

/** A file staged in an add box: a picture's thumbnail or the file's name, with × to take it out; one
 * the server turned down is marked, and says why when tapped. */
function StagedFile({ f, unseen, onRemove }: { f: Staged; unseen?: string; onRemove(): void }) {
  const picture = isPicture(f.type);
  const { snack } = useFeedback();
  const tapped = () => {
    if (f.error) snack(`“${f.name}”上传失败：${f.error}`);
    else if (unseen) snack(`AI 无法查看“${f.name}”：${unseen}，仅发送名称和类型`);
  };
  return (
    <div className={`staged${picture ? " picture" : ""}${f.error ? " refused" : ""}`} title={f.name}>
      <button type="button" className="staged-main" onClick={tapped}>
        {picture ? (
          <StagedPicture f={f} />
        ) : (
          <>
            <Icon of={fileIcon(f.type)} size={16} />
            <span>{f.name}</span>
          </>
        )}
        {unseen && <span className="staged-unseen">AI 无法查看</span>}
      </button>
      {f.error && (
        <span className="staged-error">
          <Icon of={CircleAlert} size={14} />
        </span>
      )}
      <Tip label="去掉">
        <button type="button" className="staged-remove" onMouseDown={(e) => e.preventDefault()} onClick={onRemove}>
          <Icon of={X} size={12} width={2.4} />
        </button>
      </Tip>
    </div>
  );
}

function StagedPicture({ f }: { f: Staged }) {
  const { url, unreadable } = useFileUrl(f.id, f.type);
  return url ? <img src={url} alt={f.name} draggable={false} onError={unreadable} /> : <Icon of={ImageIcon} size={16} />;
}

/** The files staged in an add box, above where its text is typed; scrolling sideways when they do not
 * fit. Those AI organizing does not show the model, `unseen`, say so. */
export function StagedFiles({ files, unseen, onRemove }: { files: Staged[]; unseen?: Map<string, string>; onRemove(f: Staged): void }) {
  if (!files.length) return null;
  return (
    <div className="staged-list">
      {files.map((f) => (
        <StagedFile key={f.id} f={f} unseen={unseen?.get(f.id)} onRemove={() => onRemove(f)} />
      ))}
    </div>
  );
}
