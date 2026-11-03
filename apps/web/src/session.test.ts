// Signing in and out with several tabs open. Each tab loads its own copy of the app, as a browser
// would; they share the device's storage, Web Locks and BroadcastChannel, and a server that knows
// which session the browser's cookie is.

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

/** A session on the server, which the cookie stands for. */
interface Cookie {
  user: User;
}

/** The browser's cookie, the sessions ended on the server, and requests held back. */
const server = {
  cookie: null as Cookie | null,
  ended: new Set<Cookie>(),
  held: undefined as { path: string; enter(): void; until: Promise<void> } | undefined,
};
vi.stubGlobal("fetch", async (url: string, init: RequestInit) => {
  const path = url.replace("/api/v1", "");
  // The cookie goes with the request as it is sent.
  const sent = server.cookie;
  if (server.held?.path === path) {
    server.held.enter();
    await server.held.until;
  }
  const json = (body: unknown, status = 200) => new Response(JSON.stringify(body), { status, headers: { "Content-Type": "application/json" } });
  if (!sent || server.ended.has(sent)) return json({ code: "unauthenticated", message: "请先登录" }, 401);
  if (path === "/auth/logout" && init.method === "POST") {
    server.ended.add(sent);
    server.cookie = null;
    return new Response(null, { status: 204 });
  }
  if (path === "/me") return json(sent.user);
  if (path === "/snapshot") return json({ user: sent.user.id, seq: 0, folders: [], tasks: [], dependencies: [] });
  throw new Error(`${init.method} ${path}`);
});

/** Holds back requests to `path`: `entered` once one has come, until `release`. */
function hold(path: string) {
  let enter!: () => void;
  let release!: () => void;
  const entered = new Promise<void>((r) => (enter = r));
  server.held = { path, enter, until: new Promise((r) => (release = r)) };
  return { entered, release };
}

/** The sign-in request, as the server answers it. */
const as = (u: User) => async () => {
  server.cookie = { user: u };
  return u;
};

const user = (n: number): User => ({ id: `019a4b2c-0000-7000-8000-0000000000${String(n).padStart(2, "0")}`, username: `u${n}`, display_name: "", is_admin: false });

/** The app as a new tab loads it. */
async function tab() {
  vi.resetModules();
  return (await import("./session")).Session;
}

/** Signs in as `u` in a new tab, and makes a change there that the server has not confirmed. */
async function signIn(u: User) {
  const s = await (await tab()).open(as(u));
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

const login = () => JSON.parse(storage.get("nexttodo.login") ?? "null");
const marked = () => storage.get("nexttodo.signedOut");

beforeEach(() => {
  storage.clear();
  server.cookie = null;
  server.held = undefined;
});

test("a tab the server stops accepting leaves at once, also while another tab takes long to sign out", async () => {
  const u = user(1);
  const a = await signIn(u);
  const b = (await (await tab()).resume())!;
  const heard = watch(b);
  const logout = hold("/auth/logout");
  const out = a.signOut();
  kick(sockets.at(-1)!);
  const left = heard.signedOut;
  logout.release();
  await out;
  expect([left, await saved(u)]).toEqual([true, [0, 0]]);
});

test("signing out ends every tab of the user and deletes their data, also tabs no longer holding the sign-in", async () => {
  const u = user(2);
  const a = await signIn(u);
  // B hears from the server that the session has ended and drops the sign-in while A is still
  // signing out; C, offline, hears nothing from the server.
  await (await tab()).resume();
  const bSocket = sockets.at(-1)!;
  const c = (await (await tab()).resume())!;
  const heard = watch(c);
  const logout = hold("/auth/logout");
  const out = a.signOut();
  await logout.entered;
  kick(bSocket);
  logout.release();
  await out;
  await vi.waitFor(() => expect(heard.signedOut).toBe(true));
  change(c);
  await c.close();
  expect([await saved(u), login(), marked()]).toEqual([[0, 0], null, undefined]);
});

test("signing out ends the user's tabs of an earlier sign-in too", async () => {
  const u = user(3);
  const a = await signIn(u);
  const heard = watch(a);
  const b = await (await tab()).open(as(u));
  await b.signOut();
  await vi.waitFor(() => expect(heard.signedOut).toBe(true));
  await a.close();
  expect(await saved(u)).toEqual([0, 0]);
});

test("signing out in a tab of an earlier sign-in ends the user's tabs of a later one too", async () => {
  const u = user(20);
  const a = await signIn(u);
  await (await tab()).open(as(u));
  const bSocket = sockets.at(-1)!;
  const c = (await (await tab()).resume())!;
  const heard = watch(c);
  // The server stops accepting B's sign-in, which drops it; C, offline, hears nothing.
  kick(bSocket);
  await vi.waitFor(() => expect(login()).toBeNull());
  await a.signOut();
  await vi.waitFor(() => expect(heard.signedOut).toBe(true));
  change(c);
  await c.close();
  expect(await saved(u)).toEqual([0, 0]);
});

test("a tab of an earlier sign-in that the server stops accepting leaves the latest sign-in alone", async () => {
  const u = user(4);
  await signIn(u);
  const [first, aSocket] = [server.cookie!, sockets.at(-1)!];
  const b = await (await tab()).open(as(u));
  // The earlier session is ended on another device, and A hears of it.
  server.ended.add(first);
  kick(aSocket);
  await vi.waitFor(async () => expect(await saved(u)).toEqual([1, 1]));
  expect(login().user.id).toBe(u.id);
  await b.signOut();
  expect([server.cookie, await saved(u)]).toEqual([null, [0, 0]]);
});

test("signing out while another tab signs in as the same user leaves that sign-in alone", async () => {
  const u = user(5);
  const a = await signIn(u);
  let release!: () => void;
  const answered = new Promise<void>((r) => (release = r));
  const opening = (await tab()).open(async () => {
    await answered;
    return as(u)();
  });
  const out = a.signOut();
  release();
  const b = await opening;
  await out;
  const cookie = server.cookie!;
  expect([server.ended.has(cookie), login().user.id, marked()]).toEqual([false, u.id, undefined]);
  await b.signOut();
  expect(server.ended.has(cookie)).toBe(true);
});

test("a sign-in or sign-out heard late leaves a later sign-in alone", async () => {
  const [u, v] = [user(17), user(18)];
  const b = await signIn(u);
  const heard = watch(b);
  const { id } = change(b);
  await vi.waitFor(() => expect(b.store.unconfirmed).toHaveLength(2));
  const late = new BroadcastChannel("nexttodo");
  late.postMessage({ signedOut: { id: 0, user: u } });
  late.postMessage({ signedIn: { id: 0, user: v } });
  // Messages come in order: once the change is dropped, the two before it have been heard.
  late.postMessage({ rejected: id });
  await vi.waitFor(() => expect(b.store.unconfirmed).toHaveLength(1));
  late.close();
  expect([heard.signedOut, login().user.id]).toEqual([false, u.id]);
});

test("a tab left open while the site's storage is cleared leaves a later sign-in alone", async () => {
  const u = user(21);
  const a = await signIn(u);
  storage.clear();
  server.cookie = null;
  const db = await openDB("nexttodo", 1);
  await Promise.all([db.clear("copies"), db.clear("outbox")]);
  // Signing in again takes a moment.
  await new Promise((r) => setTimeout(r, 5));
  await (await tab()).open(as(u));
  const cookie = server.cookie!;
  await a.signOut();
  expect([server.ended.has(cookie), login().user.id]).toEqual([false, u.id]);
});

test("a session ended before the UI listens tells it once it does", async () => {
  const a = await signIn(user(19));
  kick(sockets.at(-1)!);
  expect(watch(a).signedOut).toBe(true);
});

test("signing out after the server stopped accepting the session deletes the data after all", async () => {
  const u = user(6);
  const a = await signIn(u);
  kick(sockets.at(-1)!);
  await a.signOut();
  expect(await saved(u)).toEqual([0, 0]);
});

test("a session the server stops accepting keeps its data for when the same user signs in again", async () => {
  const u = user(7);
  await signIn(u);
  server.cookie = null;
  kick(sockets.at(-1)!);
  await vi.waitFor(() => expect(login()).toBeNull());
  expect(await saved(u)).toEqual([1, 1]);
  const Session = await tab();
  expect(await Session.resume()).toBeNull();
  const again = await Session.open(as(u));
  expect(again.store.unconfirmed).toHaveLength(1);
});

test("signing in as someone else ends the other user's tabs and deletes their data, also a tab's already leaving", async () => {
  const [u, v] = [user(8), user(9)];
  const a = await signIn(u);
  const heard = watch(a);
  // The server stops accepting A's session as B signs in: either way, A keeps nothing.
  kick(sockets.at(-1)!);
  await (await tab()).open(as(v));
  await vi.waitFor(async () => expect(await saved(u)).toEqual([0, 0]));
  expect(heard.signedOut).toBe(true);

  const c = await signIn(u);
  const cHeard = watch(c);
  await (await tab()).open(as(v));
  await vi.waitFor(() => expect(cHeard.signedOut).toBe(true));
  await c.close();
  expect(await saved(u)).toEqual([0, 0]);
});

test("a tab still downloading when the device signs in as someone else does not open", async () => {
  const [u, v] = [user(10), user(11)];
  const snapshot = hold("/snapshot");
  const a = (await tab()).open(as(u));
  await snapshot.entered;
  const b = (await tab()).open(as(v));
  await vi.waitFor(() => expect(login().user.id).toBe(v.id));
  snapshot.release();
  await expect(a).rejects.toThrow("登录已经变了");
  expect((await b).user.id).toBe(v.id);
});

test("a sign-out is marked from its start until done", async () => {
  const u = user(12);
  const a = await signIn(u);
  const logout = hold("/auth/logout");
  const out = a.signOut();
  await logout.entered;
  const meanwhile = marked();
  logout.release();
  await out;
  expect([JSON.parse(meanwhile!).user.id, marked()]).toEqual([u.id, undefined]);
});

test("a sign-out cut short is finished when the app opens next, with what it left behind", async () => {
  const u = user(13);
  const a = await signIn(u);
  const heard = watch(a);
  // Signed out, and the tab closed before anything else.
  storage.set("nexttodo.signedOut", JSON.stringify(login()));
  expect(await (await tab()).resume()).toBeNull();
  expect([server.cookie, login(), marked(), await saved(u)]).toEqual([null, null, undefined, [0, 0]]);
  await vi.waitFor(() => expect(heard.signedOut).toBe(true));
});

test("finishing a sign-out deletes the user's data also when the sign-in is gone", async () => {
  const u = user(14);
  await signIn(u);
  const signedIn = login();
  // The sign-out was cut short after another tab, which the server stopped accepting, dropped the
  // sign-in and kept the data.
  kick(sockets.at(-1)!);
  await vi.waitFor(() => expect(login()).toBeNull());
  storage.set("nexttodo.signedOut", JSON.stringify(signedIn));
  expect(await (await tab()).resume()).toBeNull();
  expect([marked(), await saved(u)]).toEqual([undefined, [0, 0]]);
});

test("opening asks the server who is signed in only when no tab is signing in or out", async () => {
  const [u, v] = [user(15), user(16)];
  server.cookie = { user: u };
  const me = hold("/me");
  const resuming = (await tab()).resume();
  await me.entered;
  // Another tab signs in as someone else meanwhile: its request waits for the answer to be used.
  let asked = false;
  const opening = (await tab()).open(async () => ((asked = true), as(v)()));
  await vi.waitFor(async () => expect((await navigator.locks.query()).pending).toHaveLength(1)).finally(me.release);
  expect(asked).toBe(false);
  // The first tab may find the device signed in as someone else when it has downloaded.
  await Promise.allSettled([resuming]);
  expect((await opening).user.id).toBe(v.id);
  expect(login().user.id).toBe(v.id);
});
