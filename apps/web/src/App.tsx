import { useEffect, useState } from "react";
import * as local from "./local";
import { Session } from "./session";
import { Root } from "./ui/Root";
import { forUser } from "./ui/feedback";
import { inPanel } from "./desktop";
import { unlock, useLocked, usePrivacyLock } from "./lock";
import { started } from "./updates";
import { LockScreen } from "./views/Lock";
import { Login, StartFailedPage } from "./views/Login";
import { Main } from "./views/Main";
import { PanelLocked, PanelOut } from "./views/Panel";

type State =
  | { kind: "loading" }
  | { kind: "out"; notice?: string }
  | { kind: "failed"; message: string; retry: boolean; settings: boolean }
  | { kind: "in"; session: Session };

const resume = () => (local.inLocalMode() ? local.resume() : Session.resume());

/** Where opening a session leads. */
const outcome = (opening: Promise<Session | null>): Promise<State> =>
  opening.then(
    (session) => (session ? { kind: "in", session } : { kind: "out" }),
    (e: Error): State => {
      if (e instanceof local.StartFailed) return { kind: "failed", message: e.message, retry: e.reason !== null, settings: e.reason === "blocked" };
      // The server started, but its data would not open: the page could not reach it, say. Not the
      // sign-in page, where in local mode there is nothing to sign in to.
      if (local.inLocalMode()) return { kind: "failed", message: forUser("打开本机数据", e), retry: true, settings: false };
      return { kind: "out", notice: forUser("读取登录信息", e) };
    },
  );

// Started once, outside React, so that development's double effects do not open two sessions.
let resuming: Promise<Session | null> | undefined;

export function App() {
  const [state, setState] = useState<State>({ kind: "loading" });
  const lock = usePrivacyLock();
  const locked = useLocked();
  // The first page has rendered, the sign-in page or the app: a build being tried works. Should it
  // fail to render, the error boundary takes its place, and this never runs.
  const shown = state.kind !== "loading";
  useEffect(() => {
    if (shown) started();
  }, [shown]);
  useEffect(() => {
    void outcome((resuming ??= resume())).then(setState);
  }, []);

  if (state.kind === "loading") return null;
  if (inPanel && state.kind !== "in") {
    return (
      <Root className="app phone panel">
        <PanelOut failed={state.kind === "failed" ? state.message : undefined} />
      </Root>
    );
  }
  if (state.kind === "failed") {
    return (
      <Root className="app">
        <StartFailedPage message={state.message} retry={state.retry} settings={state.settings} onRetry={() => outcome((resuming = local.resume(true))).then(setState)} />
      </Root>
    );
  }
  if (state.kind === "out") {
    return (
      <Root className="app">
        <Login
          notice={state.notice}
          // Who signed in has shown who they are: no PIN after that.
          onSignIn={async (request) => {
            const session = await Session.open(request);
            unlock();
            setState({ kind: "in", session });
          }}
          onLocal={(session) => {
            unlock();
            setState({ kind: "in", session });
          }}
        />
      </Root>
    );
  }
  // The session goes on underneath, syncing and handing over reminders.
  if (lock && locked) {
    return (
      <Root className={inPanel ? "app phone panel" : "app"}>{inPanel ? <PanelLocked lock={lock} /> : <LockScreen lock={lock} />}</Root>
    );
  }
  const { session } = state;
  // Only while still in this session: a sign-out may finish after the user signed in again.
  const signedOut = (notice?: string) => setState((s) => (s.kind === "in" && s.session === session ? { kind: "out", notice } : s));
  return <Main key={session.user.id} session={session} onSignedOut={signedOut} />;
}
