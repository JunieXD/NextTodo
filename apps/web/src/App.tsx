import { useEffect, useState } from "react";
import * as local from "./local";
import { Session } from "./session";
import { Root } from "./ui/Root";
import { started } from "./updates";
import { Login, StartFailedPage } from "./views/Login";
import { Main } from "./views/Main";

type State =
  | { kind: "loading" }
  | { kind: "out"; notice?: string }
  | { kind: "failed"; error: local.StartFailed }
  | { kind: "in"; session: Session };

const resume = () => (local.inLocalMode() ? local.resume() : Session.resume());

/** Where opening a session leads. */
const outcome = (opening: Promise<Session | null>): Promise<State> =>
  opening.then(
    (session) => (session ? { kind: "in", session } : { kind: "out" }),
    (e: Error) => (e instanceof local.StartFailed ? { kind: "failed", error: e } : { kind: "out", notice: e.message }),
  );

// Started once, outside React, so that development's double effects do not open two sessions.
let resuming: Promise<Session | null> | undefined;

export function App() {
  const [state, setState] = useState<State>({ kind: "loading" });
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
  if (state.kind === "failed") {
    return (
      <Root className="app">
        <StartFailedPage
          error={state.error}
          onRetry={() => outcome((resuming = local.resume())).then(setState)}
          onServer={() => local.forgetMode().then(() => setState({ kind: "out" }))}
        />
      </Root>
    );
  }
  if (state.kind === "out") {
    return (
      <Root className="app">
        <Login
          notice={state.notice}
          onSignIn={async (request) => setState({ kind: "in", session: await Session.open(request) })}
          onLocal={(session) => setState({ kind: "in", session })}
        />
      </Root>
    );
  }
  const { session } = state;
  // Only while still in this session: a sign-out may finish after the user signed in again.
  const signedOut = (notice?: string) => setState((s) => (s.kind === "in" && s.session === session ? { kind: "out", notice } : s));
  return <Main key={session.user.id} session={session} onSignedOut={signedOut} />;
}
