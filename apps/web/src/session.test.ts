// Signing in and out with several tabs open. Each tab loads its own copy of the app, as a browser
// would; they share the device's storage, Web Locks and BroadcastChannel, and a server that knows
// whom the browser's cookie signs in.

import "fake-indexeddb/auto";
import { openDB } from "idb";
import { beforeEach, expect, test, vi } from "vitest";
import type { User } from "./api/types/User";
import type { Session } from "./session";

const storage = new Map<string, string>();
vi.stubGlobal("localStorage", {
  getItem: (k: string) => storage.get(k) ?? null,
  setItem: (k: string, v: string) => storage.set(k, v),
  removeItem: (k: string) => storage.delete(k),
});
vi.stubGlobal("location", { protocol: "http:", host: "localhost" });
vi.stubGlobal("document", { visibilityState: "visible", addEventListener() {}, removeEventListener() {} });
vi.stubGlobal("addEventListener", () => {});
vi.stubGlobal("removeEventListener", () => {});

/** The connections the tabs opened, newest last. */
const sockets: FakeSocket[] = [];
class FakeSocket {
  /** Set by the app; a test calls it to play the server. */
  onmessage?: (e: { data: string }) => void;
  constructor() {
    sockets.push(this);
  }
  send() {}
  close() {}
}
vi.stubGlobal("WebSocket", FakeSocket);

/** Whom the cookie signs in, whether the server can be reached, and a request held back. */
const server = { cookie: null as User | null, down: false, held: undefined as { path: string; until: Promise<void> } | undefined };
vi.stubGlobal("fetch", async (url: string, init: RequestInit) => {
  const path = url.replace("/api/v1", "");
  if (server.held?.path === path) await server.held.until;
  if (server.down) throw new TypeError("Failed to fetch");
  const json = (body: unknown, status = 200) => new Response(JSON.stringify(body), { status });
  if (!server.cookie) return json({ code: "unauthenticated", message: "请先登录" }, 401);
  if (path === "/auth/logout" && init.method === "POST") {
    server.cookie = null;
    return new Response(null, { status: 204 });
  }
  if (path === "/me") return json(server.cookie);
  if (path === "/snapshot") return json({ user: server.cookie.id, seq: 0, folders: [], tasks: [], dependencies: [] });
  throw new Error(`${init.method} ${path}`);
});

/** Holds back requests to `path` until the returned function is called. */
function hold(path: string) {
  let release!: () => void;
  server.held = { path, until: new Promise((r) => (release = r)) };
  return release;
}

const user = (n: number): User => ({ id: `019a4b2c-0000-7000-8000-00000000000${n}`, username: `u${n}`, display_name: "", is_admin: false });

/** The app as a new tab loads it. */
async function tab() {
  vi.resetModules();
  return (await import("./session")).Session;
}

/** Signs in as `u` in a new tab, and makes a change there that the server has not confirmed. */
async function signIn(u: User) {
  const s = await (await tab()).open(async () => (server.cookie = u));
  change(s);
  await vi.waitFor(async () => expect(await saved(u)).toEqual([1, 1]));
  return s;
}

const change = (s: Session) => s.store.commit([{ op: "task.create", id: crypto.randomUUID(), title: "交电费", sort_key: "a1" }]);

/** Ends the session as when the server stops accepting it. */
const kick = (socket: FakeSocket) => socket.onmessage!({ data: JSON.stringify({ type: "error", code: "unauthenticated", message: "请先登录" }) });

/** Whether the session's tab went back to signing in. */
function watch(s: Session) {
  const heard = { signedOut: false };
  s.listen({ rejected() {}, arrived() {}, unsaved() {}, signedOut: () => (heard.signedOut = true) });
  return heard;
}

/** How many copies and pending changes of the user's are on the device. */
async function saved(u: User) {
  const db = await openDB("nexttodo", 1);
  const mine = IDBKeyRange.bound([u.id], [u.id, []]);
  return [await db.count("copies", mine), await db.count("outbox", mine)];
}

beforeEach(() => {
  storage.clear();
  Object.assign(server, { cookie: null, down: false, held: undefined });
});

test("a tab the server stops accepting leaves at once, also while another tab takes long to sign out", async () => {
  const u = user(1);
  const a = await signIn(u);
  const b = (await (await tab()).resume())!;
  const heard = watch(b);
  const release = hold("/auth/logout");
  const out = a.signOut();
  kick(sockets.at(-1)!);
  expect(heard.signedOut).toBe(true);
  release();
  await out;
  expect(await saved(u)).toEqual([0, 0]);
});

test("signing out ends every tab of the sign-in and deletes its data, also tabs not holding the sign-in any more", async () => {
  const u = user(2);
  const a = await signIn(u);
  // B hears from the server that the session has ended and drops the sign-in while A is still
  // signing out; C, offline, hears nothing from the server.
  await (await tab()).resume();
  const bSocket = sockets.at(-1)!;
  const c = (await (await tab()).resume())!;
  const heard = watch(c);
  const release = hold("/auth/logout");
  const out = a.signOut();
  kick(bSocket);
  release();
  await out;
  await vi.waitFor(() => expect(heard.signedOut).toBe(true));
  change(c);
  await c.close();
  expect([await saved(u), storage.get("nexttodo.login")]).toEqual([[0, 0], undefined]);
});

test("signing out after the server stopped accepting the session deletes the data after all", async () => {
  const u = user(3);
  const a = await signIn(u);
  kick(sockets.at(-1)!);
  await a.signOut();
  expect(await saved(u)).toEqual([0, 0]);
});

test("a session the server stops accepting keeps its data for when the same user signs in again", async () => {
  const u = user(4);
  await signIn(u);
  server.cookie = null;
  kick(sockets.at(-1)!);
  await vi.waitFor(() => expect(storage.get("nexttodo.login")).toBeUndefined());
  expect(await saved(u)).toEqual([1, 1]);
  const Session = await tab();
  expect(await Session.resume()).toBeNull();
  const again = await Session.open(async () => (server.cookie = u));
  expect(again.store.unconfirmed).toHaveLength(1);
});

test("signing in as someone else ends the other user's tabs and deletes their data, also a tab's already leaving", async () => {
  const [u, v] = [user(5), user(6)];
  const a = await signIn(u);
  const heard = watch(a);
  // The server stops accepting A's session as B signs in: either way, A keeps nothing.
  kick(sockets.at(-1)!);
  await (await tab()).open(async () => (server.cookie = v));
  await vi.waitFor(async () => expect(await saved(u)).toEqual([0, 0]));
  expect(heard.signedOut).toBe(true);

  const c = await signIn(u);
  const cHeard = watch(c);
  await (await tab()).open(async () => (server.cookie = v));
  await vi.waitFor(() => expect(cHeard.signedOut).toBe(true));
  await c.close();
  expect(await saved(u)).toEqual([0, 0]);
});

test("signing in again as the same user takes the user's open tabs into the new sign-in", async () => {
  const u = user(10);
  const a = await signIn(u);
  const heard = watch(a);
  const b = await (await tab()).open(async () => (server.cookie = u));
  await new Promise((r) => setTimeout(r, 20));
  await b.signOut();
  await vi.waitFor(() => expect(heard.signedOut).toBe(true));
  await a.close();
  expect(await saved(u)).toEqual([0, 0]);
});

test("a sign-out the server did not hear is finished when the app opens next, with what it left behind", async () => {
  const u = user(7);
  const a = await signIn(u);
  const heard = watch(a);
  // Signed out offline, and the tab closed before deleting anything.
  storage.set("nexttodo.signedOut", "1");
  expect(await (await tab()).resume()).toBeNull();
  expect([server.cookie, storage.get("nexttodo.login"), storage.get("nexttodo.signedOut"), await saved(u)]).toEqual([null, undefined, undefined, [0, 0]]);
  await vi.waitFor(() => expect(heard.signedOut).toBe(true));
});

test("opening asks the server who is signed in only when no tab is signing in or out", async () => {
  const [u, v] = [user(8), user(9)];
  server.cookie = u;
  const release = hold("/me");
  const resuming = (await tab()).resume();
  // Another tab signs in as someone else meanwhile: its request waits for the answer to be used.
  let asked = false;
  const opening = (await tab()).open(async () => ((asked = true), (server.cookie = v)));
  await new Promise((r) => setTimeout(r, 20));
  expect(asked).toBe(false);
  release();
  // The first tab may still find the browser signed in as someone else when it downloads.
  await Promise.allSettled([resuming]);
  expect((await opening).user.id).toBe(v.id);
  expect(JSON.parse(storage.get("nexttodo.login")!).user.id).toBe(v.id);
});
