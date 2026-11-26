// Android's quick add card over another app (docs/android.md#快速添加小窗): the quick add bar of what
// is next on a sheet at the bottom, with what another app shared in it; closed once what was added
// has reached the server.

import { type ReactNode, useCallback, useEffect, useEffectEvent, useRef, useState } from "react";
import { useLocation } from "wouter";
import { aiSetUp, localAiConfig, setAiMode, useLocalRunner } from "../ai";
import { plan } from "../attach";
import { NextTodo, settle, takeShared } from "../mobile";
import { type Session, useConnection, useSession, useView } from "../session";
import { draft, keepDraft, keepStaged, stagedFiles } from "../sync/drafts";
import { useBack } from "../ui/back";
import { forUser, useFeedback } from "../ui/feedback";
import { useKeepFiles } from "./attachments";
import { QuickAdd, type Sent, quickAddPlace } from "./lists";

/** Closes the card once what another app shared is in the bar (`shared`), and what was made here is
 * safe, written on this device or confirmed by the server, and the widgets and reminders have it: the
 * page goes with the card. What the server has not confirmed yet goes on from the app's window, if
 * open. */
async function close(session: Session, shared: Promise<void>) {
  await shared;
  if (!(await session.store.saved()) && session.store.getView().pending > 0) return;
  await settle();
  session.handOver();
  await NextTodo.closeCard();
}

export function Card() {
  const session = useSession();
  const { store } = session;
  const { board, pending } = useView();
  const connection = useConnection();
  const { snack } = useFeedback();
  const keepFiles = useKeepFiles();
  const [location] = useLocation();
  const [taken, setTaken] = useState(false);
  /** Resolves once `taken`. */
  const [taking] = useState(() => {
    let done!: () => void;
    return { promise: new Promise<void>((r) => (done = r)), done };
  });
  const [sent, setSent] = useState<Sent>();
  /** The day it adds to, and the time on it, opened from a day of the “一周” widget or an hour of the
   * “时间轴” one: as the calendar's quick add. */
  const [{ on, at }] = useState(() => {
    const params = new URLSearchParams(window.location.search);
    return { on: params.get("on") ?? undefined, at: params.get("at") };
  });
  const place = quickAddPlace(undefined, on);
  /** The server would not start the AI job sent with the settings on this device. */
  const [unstarted, setUnstarted] = useState(false);
  /** Left, or asked to close. */
  const [leaving, setLeaving] = useState(false);
  const closing = useRef(false);
  /** Closes the card unless that is under way; once it has stopped short, again as asked. */
  const tryClose = useCallback(() => {
    if (closing.current) return;
    closing.current = true;
    void close(session, taking.promise).finally(() => (closing.current = false));
  }, [session, taking]);

  // What another app shared joins what the bar has: the text, and the files staged. Shared text goes to
  // AI organizing where it is set up, being a passage or a screenshot more often than a title.
  useEffect(() => {
    const user = session.user.id;
    const take = async () => {
      const { text, files, errors } = await takeShared();
      for (const e of errors) snack(e);
      if (!text && !files.length) return;
      const ai = await aiSetUp(user);
      await setAiMode(ai);
      if (text) keepDraft(user, place, [draft(user, place), ai ? text : text.replace(/\s*\n\s*/g, " ")].filter(Boolean).join(ai ? "\n" : " "));
      const kept = await keepFiles(plan(files, stagedFiles(user, place).map((f) => f.name), snack));
      if (kept.length) keepStaged(user, place, [...stagedFiles(user, place), ...kept]);
    };
    void take()
      .catch((e: unknown) => snack(forUser("读取分享的内容", e)))
      .finally(() => {
        setTaken(true);
        taking.done();
      });
    // eslint-disable-next-line react-hooks/exhaustive-deps -- takes once when opened
  }, []);

  // Elsewhere, such as the settings a link leads to, is the app's.
  useEffect(() => {
    if (location !== "/quick-add") void NextTodo.openApp({ url: `nexttodo://page${location}` });
  }, [location]);

  // The card may be the device online with the AI settings when a job is added.
  useLocalRunner(store, session.user.id, (job, e) => {
    snack(forUser("开始 AI 整理", e));
    if (job === sent?.job) setUnstarted(true);
  });

  const standing = sent?.changes.map((id) => store.standing(id)) ?? [];
  // For the reason shown, the bar comes back.
  const refused = standing.includes("turned down") || unstarted;
  const saving = sent !== undefined && !refused;
  const jobs = board.snapshot.ai_jobs;
  const job = jobs.find((j) => j.id === sent?.job);
  // With the settings on this device, an AI job is handed to the server from here, unless another is
  // being organized; offline, by the next device online with them.
  const handing = job?.state === "waiting" && localAiConfig(session.user.id) !== null && !jobs.some((j) => j.state === "running");
  const done = saving && (connection === "offline" || (standing.every((s) => s === "confirmed") && !handing));
  // Again as changes settle, should saving on this device have failed.
  useEffect(() => {
    if (leaving || done) tryClose();
  }, [leaving, done, pending, tryClose]);

  return (
    <CardPage
      onClose={() => {
        setLeaving(true);
        tryClose();
      }}
    >
      {saving ? (
        <p className="card-note">正在保存…</p>
      ) : (
        taken && (
          <QuickAdd
            on={on}
            at={at}
            onSent={(s) => {
              setSent(s);
              setUnstarted(false);
            }}
          />
        )
      )}
    </CardPage>
  );
}

/** Not signed in, or the app could not start: the app opens to sign in, or tell why. */
export function CardOut({ failed }: { failed?: string }) {
  return (
    <CardPage onClose={() => void NextTodo.closeCard()}>
      <div className="card-out">
        {failed && <p>{failed}</p>}
        <button className="filled-btn" onClick={() => void NextTodo.openApp({ url: "nexttodo://home" })}>
          {failed ? "打开 NextTodo" : "打开 NextTodo 登录"}
        </button>
      </div>
    </CardPage>
  );
}

/** The sheet at the bottom; tapping above it, back, or leaving for another app closes the card. */
function CardPage({ onClose, children }: { onClose: () => void; children: ReactNode }) {
  useBack("page", onClose);
  const leave = useEffectEvent(onClose);
  useEffect(() => {
    const left = () => document.visibilityState === "hidden" && leave();
    left();
    document.addEventListener("visibilitychange", left);
    return () => document.removeEventListener("visibilitychange", left);
  }, []);
  return (
    <div className="card-page" onClick={(e) => e.target === e.currentTarget && onClose()}>
      <div className="card-sheet">{children}</div>
    </div>
  );
}
