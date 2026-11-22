// The privacy lock: when it locks, in every tab, and what unlocks it.

import { afterEach, beforeEach, expect, test, vi } from "vitest";

/** What the page listens to, on the window and the document. */
const heard = new Map<string, (e?: unknown) => void>();
vi.stubGlobal("addEventListener", (type: string, f: () => void) => heard.set(type, f));
vi.stubGlobal("document", { activeElement: null, addEventListener: (type: string, f: () => void) => heard.set(type, f) });
vi.stubGlobal("HTMLElement", class {});

const { isCode, isLocked, lockNow, onLock, setCode, setIdle, setPin, privacyLock, tryPin, turnOff, turnOn, startLock, unlock, wrongTries } = await import("./lock");

const MINUTE = 60_000;
const CODE = "7KQ2-M9XD-4TPW-H3RC";

beforeEach(async () => {
  vi.useFakeTimers({ toFake: ["Date", "setInterval"] });
  vi.setSystemTime(new Date(2026, 10, 22, 9));
  turnOff();
  await turnOn("2580", CODE);
});

afterEach(() => vi.useRealTimers());

const input = () => heard.get("pointermove")!();

test("it locks after the time set without input, also as the app opens after that", () => {
  startLock();
  vi.advanceTimersByTime(4 * MINUTE);
  input();
  vi.advanceTimersByTime(4 * MINUTE);
  expect(isLocked()).toBe(false);
  vi.advanceTimersByTime(MINUTE);
  expect(isLocked()).toBe(true);
});

test("the first input after a long while finds the app locked, as does opening it", async () => {
  startLock();
  // The computer asleep for an hour: moving the mouse, before the time is looked at again.
  vi.setSystemTime(Date.now() + 60 * MINUTE);
  input();
  expect(isLocked()).toBe(true);
  expect(await tryPin(privacyLock()!, "2580")).toBe(true);
  setIdle(privacyLock()!, "1");
  // Closed, and opened again later.
  vi.setSystemTime(Date.now() + 2 * MINUTE);
  startLock();
  expect(isLocked()).toBe(true);
});

test("a clock gone back counts from the time it shows", () => {
  startLock();
  input();
  vi.setSystemTime(Date.now() - 60 * MINUTE);
  // In use all along.
  for (let i = 0; i < 8; i++) {
    vi.advanceTimersByTime(MINUTE);
    input();
  }
  expect(isLocked()).toBe(false);
  vi.advanceTimersByTime(4 * MINUTE);
  expect(isLocked()).toBe(false);
  vi.advanceTimersByTime(2 * MINUTE);
  expect(isLocked()).toBe(true);
});

test("after five wrong PINs only the recovery code unlocks, typed as it may be", { timeout: 30_000 }, async () => {
  lockNow();
  const lock = privacyLock()!;
  for (const pin of ["1111", "2222", "0000", "2581", "8520"]) expect(await tryPin(lock, pin)).toBe(false);
  expect(await tryPin(lock, "2580")).toBe(false);
  expect([isLocked(), wrongTries()]).toEqual([true, 5]);
  expect(await isCode(lock, "7kq2 m9xd 4tpw h3rc")).toBe(true);
  expect(await isCode(lock, "7KQ2-M9XD-4TPW-H3RD")).toBe(false);
});

test("locked in another tab, it is locked here; and leaves what is being edited first", () => {
  startLock();
  const editing = Object.assign(new HTMLElement(), { blur: vi.fn() });
  vi.stubGlobal("document", { activeElement: editing, addEventListener() {} });
  const told = vi.fn();
  onLock(told);
  localStorage.setItem("nexttodo.locked", "1");
  heard.get("storage")!({ key: "nexttodo.locked", newValue: "1" });
  expect([isLocked(), told.mock.calls.length, editing.blur.mock.calls.length]).toEqual([true, 1, 1]);
});

test("a new PIN keeps what was changed while it was derived", async () => {
  const before = privacyLock()!;
  const setting = setPin(before, "1357");
  // Meanwhile, in another tab.
  setIdle(before, "30");
  await setCode(before, "AAAA-BBBB-CCCC-DDDD");
  await setting;
  const after = privacyLock()!;
  expect([after.idle, after.length]).toEqual(["30", 4]);
  expect(await isCode(after, "AAAA-BBBB-CCCC-DDDD")).toBe(true);
  lockNow();
  expect(await tryPin(after, "1357")).toBe(true);
});

test("a PIN checked as another tab unlocks is no wrong try", async () => {
  lockNow();
  const checking = tryPin(privacyLock()!, "1111");
  // The right one, in the other tab, while this one is checked.
  unlock();
  expect(await checking).toBe(true);
  lockNow();
  expect(wrongTries()).toBe(0);
});
