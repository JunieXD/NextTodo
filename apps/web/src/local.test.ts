// Local mode in the desktop app: opening the local user's data, which a database with another
// history, a backup restored say, leaves no longer fitting.

import "fake-indexeddb/auto";
import { beforeEach, expect, test, vi } from "vitest";
import type { Pending } from "./sync/store";

vi.stubGlobal("__TAURI_INTERNALS__", {});
// Where Capacitor's Preferences find `localStorage`, which the app keeps its token in.
vi.stubGlobal("window", globalThis);
vi.stubGlobal("document", { visibilityState: "visible", addEventListener() {}, removeEventListener() {} });
vi.stubGlobal("addEventListener", () => {});
vi.stubGlobal("removeEventListener", () => {});
vi.stubGlobal(
  "WebSocket",
  class {
    send() {}
    close() {}
  },
);

/** The app: what `start_local` gives, the server run on the database the test says; whether it is in
 * local mode; the commands the page called, and the one that fails. */
const app = vi.hoisted(() => ({ user: "", epoch: "", mode: true, invoked: [] as string[], failing: "" }));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: async (command: string) => {
    app.invoked.push(command);
    if (command === app.failing) throw "磁盘已满";
    if (command === "start_local") return { url: "http://127.0.0.1:1", token: "nt_test", user: app.user, epoch: app.epoch };
    if (command === "local_mode") return app.mode;
    if (command === "prepare_restore") return true;
  },
}));

vi.mock("@tauri-apps/plugin-http", () => ({
  fetch: async (url: string) => {
    const path = url.replace(/^.*\/api\/v1/, "");
    const body =
      path === "/me"
        ? { id: app.user, username: "local", display_name: "本机", is_admin: false }
        : { user: app.user, seq: 0, folders: [], tasks: [], dependencies: [], ai_jobs: [] };
    return new Response(JSON.stringify(body), { headers: { "Content-Type": "application/json" } });
  },
}));

const user = (n: number) => `019a4b2c-0000-7000-8000-00000000000${n}`;
const EPOCH = "nexttodo.localEpoch";

/** The user's data on the page from an earlier run: a copy as of change 5, a change not sent yet, a
 * draft and AI settings. */
async function leftOver(u: string) {
  const { load, saver } = await import("./sync/persist");
  const { Store } = await import("./sync/store");
  const { keepDraft } = await import("./sync/drafts");
  await load(u, "earlier");
  const pending: Pending = { mutation: { id: "019a4b2c-0000-7000-8000-000000000100", ops: [] }, at: 1 };
  await saver(u, "earlier", (e) => {
    throw e;
  }).save({ ...Store.empty(), seq: 5 }, [pending], []);
  keepDraft(u, "next", "写了一半");
  localStorage.setItem(`ai:${u}`, "{}");
}

/** The page as it loads in local mode. */
async function load() {
  vi.resetModules();
  const local = await import("./local");
  await local.loadMode();
  return local.resume();
}

beforeEach(() => {
  localStorage.clear();
  Object.assign(app, { mode: true, invoked: [], failing: "" });
});

test("the same database opens the data left on the page", async () => {
  Object.assign(app, { user: user(1), epoch: "a" });
  await leftOver(user(1));
  localStorage.setItem(EPOCH, "a");
  const session = await load();
  expect([session.store.since, session.store.getView().pending]).toEqual([5, 1]);
});

test("another database's history deletes the copy and the changes not sent, keeping drafts and AI settings", async () => {
  Object.assign(app, { user: user(2), epoch: "restored" });
  await leftOver(user(2));
  localStorage.setItem(EPOCH, "before");
  const session = await load();
  const { draft } = await import("./sync/drafts");
  expect([session.store.since, session.store.getView().pending]).toEqual([0, 0]);
  expect([draft(user(2), "next"), localStorage.getItem(`ai:${user(2)}`), localStorage.getItem(EPOCH)]).toEqual(["写了一半", "{}", "restored"]);
});

test("another user, from another device's backup, is as signing in as someone else", async () => {
  Object.assign(app, { user: user(3), epoch: "a" });
  await leftOver(user(3));
  localStorage.setItem(EPOCH, "a");
  Object.assign(app, { user: user(4), epoch: "b" });
  await load();
  const { draft } = await import("./sync/drafts");
  expect([draft(user(3), "next"), localStorage.getItem(`ai:${user(3)}`)]).toEqual(["", null]);
});

test("signing in to a server outside local mode leaves the app's mode alone", async () => {
  app.mode = false;
  vi.resetModules();
  const local = await import("./local");
  await local.loadMode();
  await local.forgetMode();
  expect(app.invoked).toEqual(["local_mode"]);
});

test("a restore that did not commit leaves the session open; one that did restarts the app", async () => {
  Object.assign(app, { user: user(5), epoch: "a", failing: "commit_restore" });
  const session = await load();
  const local = await import("./local");
  const { syncToken } = await import("./api/http");
  await expect(local.restore(async () => true, session)).rejects.toBe("磁盘已满");
  expect([syncToken(), app.invoked.includes("restart")]).toEqual(["nt_test", false]);
  app.failing = "";
  await local.restore(async () => true, session);
  expect([syncToken(), app.invoked.at(-1)]).toEqual([undefined, "restart"]);
});
