import { useEffect, useState } from "react";
import { Session } from "./session";
import { Root } from "./ui/Root";
import { Login } from "./views/Login";
import { Main } from "./views/Main";

type State = { kind: "loading" } | { kind: "out"; notice?: string } | { kind: "in"; session: Session };

// Started once, outside React, so that development's double effects do not open two sessions.
let resuming: Promise<Session | null> | undefined;

export function App() {
  const [state, setState] = useState<State>({ kind: "loading" });
  useEffect(() => {
    (resuming ??= Session.resume()).then(
      (session) => setState(session ? { kind: "in", session } : { kind: "out" }),
      (e: Error) => setState({ kind: "out", notice: e.message }),
    );
  }, []);

  if (state.kind === "loading") return null;
  if (state.kind === "out") {
    return (
      <Root className="app">
        <Login notice={state.notice} onSignedIn={async (user) => setState({ kind: "in", session: await Session.open(user) })} />
      </Root>
    );
  }
  const { session } = state;
  // Only while still in this session: a sign-out may finish after the user signed in again.
  const signedOut = (notice?: string) => setState((s) => (s.kind === "in" && s.session === session ? { kind: "out", notice } : s));
  return <Main key={session.user.id} session={session} onSignedOut={signedOut} />;
}
