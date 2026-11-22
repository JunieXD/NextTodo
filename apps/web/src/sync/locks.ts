// Work that one tab does at a time, under a Web Lock.

const held = new Map<string, Promise<unknown>>();

/** Runs `f` holding the lock `name`: one at a time across the tabs. A page on plain HTTP has no Web
 * Locks, which browsers give secure pages only: there it is one at a time in this tab, and the tabs
 * go on their own (docs/web.md#经-http-打开). */
export function exclusive<T>(name: string, f: () => Promise<T>): Promise<T> {
  if ("locks" in navigator) return navigator.locks.request(name, f);
  const done = (held.get(name) ?? Promise.resolve()).then(f);
  // The next waits for this one, however it ends; its caller hears how.
  // eslint-disable-next-line no-restricted-syntax -- the caller hears of the failure from `done`
  held.set(name, done.then(() => undefined, () => undefined));
  return done;
}
