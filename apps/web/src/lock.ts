// The privacy lock (docs/design.md#隐私锁): a PIN that hides the app after a while without input. Kept
// in this browser's storage, which its tabs and the desktop app's windows share: they lock and unlock
// together, and stay locked through a reload. It keeps others from looking, no more: what it hides is
// still on the device.

import { useSyncExternalStore } from "react";

/** Minutes without input before it locks: the choices, as the settings name them. */
export const IDLE = ["1", "5", "15", "30"] as const;

export type Idle = (typeof IDLE)[number];

/** What the lock is set to. The PIN and the recovery code are kept only as PBKDF2 makes them, with a
 * salt of this device's (docs/design.md#哈希用途); all three in base64. */
export interface PrivacyLock {
  /** The PIN's length: it is tried as soon as that many digits are in. */
  length: number;
  idle: Idle;
  salt: string;
  pin: string;
  code: string;
}

const LOCK = "nexttodo.lock";
const LOCKED = "nexttodo.locked";
/** Wrong PINs since the last unlock. */
const TRIES = "nexttodo.lockTries";
/** When the last input was, in any tab or window. */
const ACTIVE = "nexttodo.active";

/** Wrong PINs after which only the recovery code or the account's password unlocks. */
export const MAX_TRIES = 5;

/** As OWASP has it for PBKDF2-HMAC-SHA256. */
const ITERATIONS = 600_000;

/** How often the time without input is looked at, and the last input written down. */
const CHECK_MS = 1000;
const NOTE_MS = 5000;

/** Browsers give the Web Crypto that derives the PIN's hash to secure pages only (docs/web.md#经-http-打开). */
export const canLock = () => isSecureContext;

const listeners = new Set<() => void>();

// The same object until it changes, as React wants of what it subscribes to.
let lock: PrivacyLock | null | undefined;

const changed = () => {
  lock = undefined;
  for (const l of listeners) l();
};

export const privacyLock = (): PrivacyLock | null => (lock !== undefined ? lock : (lock = JSON.parse(localStorage.getItem(LOCK) ?? "null")));

export const isLocked = () => privacyLock() !== null && localStorage.getItem(LOCKED) === "1";

export const wrongTries = () => Number(localStorage.getItem(TRIES) ?? 0);

function save(l: PrivacyLock | null) {
  if (l) localStorage.setItem(LOCK, JSON.stringify(l));
  else localStorage.removeItem(LOCK);
  changed();
}

const base64 = (bytes: ArrayBuffer | Uint8Array) => btoa(String.fromCharCode(...new Uint8Array(bytes)));

async function derive(secret: string, salt: string): Promise<string> {
  const key = await crypto.subtle.importKey("raw", new TextEncoder().encode(secret), "PBKDF2", false, ["deriveBits"]);
  const salted = Uint8Array.from(atob(salt), (c) => c.charCodeAt(0));
  return base64(await crypto.subtle.deriveBits({ name: "PBKDF2", hash: "SHA-256", salt: salted, iterations: ITERATIONS }, key, 256));
}

/** Without the 0, O, 1, I, L and U that are read for one another. */
const CODE_LETTERS = "23456789ABCDEFGHJKMNPQRSTVWXYZ";

/** A new recovery code, such as `7KQ2-M9XD-4TPW-H3RC`. */
export function newCode(): string {
  const letters: string[] = [];
  while (letters.length < 16) {
    // Only bytes below the largest multiple of the letters' count, for each letter to be as likely.
    for (const b of crypto.getRandomValues(new Uint8Array(16))) if (b < 240 && letters.length < 16) letters.push(CODE_LETTERS[b % 30]);
  }
  return letters.join("").replace(/(.{4})(?!$)/g, "$1-");
}

/** A code as typed: in either case, with the dashes or without, spaced out or not. */
const plain = (code: string) => code.toUpperCase().replace(/[\s-]/g, "");

export async function turnOn(pin: string, code: string) {
  const salt = base64(crypto.getRandomValues(new Uint8Array(16)));
  const [p, c] = await Promise.all([derive(pin, salt), derive(plain(code), salt)]);
  localStorage.setItem(ACTIVE, String(Date.now()));
  localStorage.removeItem(LOCKED);
  localStorage.removeItem(TRIES);
  save({ length: pin.length, idle: "5", salt, pin: p, code: c });
}

export function turnOff() {
  localStorage.removeItem(LOCKED);
  localStorage.removeItem(TRIES);
  save(null);
}

/** Puts `part` in place in the lock as it is by then: another tab may have changed it while the hash
 * was derived. Not in one set up again since, with another salt. */
function update(salt: string, part: Partial<PrivacyLock>) {
  const l = privacyLock();
  if (l?.salt === salt) save({ ...l, ...part });
}

export const setIdle = (l: PrivacyLock, idle: Idle) => update(l.salt, { idle });

export const setPin = async (l: PrivacyLock, pin: string) => update(l.salt, { length: pin.length, pin: await derive(pin, l.salt) });

/** In place of the recovery code, which no longer unlocks. */
export const setCode = async (l: PrivacyLock, code: string) => update(l.salt, { code: await derive(plain(code), l.salt) });

/** The edits being made are saved as what they are made in loses the focus, before the page goes. */
function leaveEdits() {
  if (document.activeElement instanceof HTMLElement) document.activeElement.blur();
}

export function lockNow() {
  if (!privacyLock()) return;
  leaveEdits();
  localStorage.setItem(LOCKED, "1");
  changed();
}

/** Unlocks every tab and window: the PIN was right, the user signed in, or proved who they are some
 * other way and set a new PIN. */
export function unlock() {
  localStorage.removeItem(LOCKED);
  localStorage.removeItem(TRIES);
  localStorage.setItem(ACTIVE, String(Date.now()));
  changed();
}

/** Unlocks if `pin` is the PIN; after `MAX_TRIES` wrong ones, none is. */
export async function tryPin(l: PrivacyLock, pin: string): Promise<boolean> {
  if (wrongTries() >= MAX_TRIES) return false;
  const right = (await derive(pin, l.salt)) === l.pin;
  // Unlocked meanwhile, in another tab say: no try of this time.
  if (!isLocked()) return true;
  if (right) {
    unlock();
    return true;
  }
  localStorage.setItem(TRIES, String(wrongTries() + 1));
  changed();
  return false;
}

export const isCode = async (l: PrivacyLock, code: string) => (await derive(plain(code), l.salt)) === l.code;

/** Locks the app once it has had no input for the time set, also when it opens: having been closed,
 * or the computer asleep, counts; and with the other tabs and windows. Started before the app shows
 * anything. */
export function startLock() {
  const check = () => {
    const l = privacyLock();
    if (!l || isLocked()) return;
    const now = Date.now();
    const active = Number(localStorage.getItem(ACTIVE));
    // From now instead, should the clock have gone back.
    if (active > now) localStorage.setItem(ACTIVE, String(now));
    else if (now - active >= Number(l.idle) * 60_000) lockNow();
  };
  let noted = 0;
  // Looked at before the input is noted: the first one after a long while finds the app locked.
  const input = () => {
    const now = Date.now();
    // Noted again at once should the clock have gone back.
    if (now >= noted && now - noted < NOTE_MS) return;
    check();
    if (!privacyLock() || isLocked()) return;
    noted = now;
    localStorage.setItem(ACTIVE, String(now));
  };
  for (const type of ["keydown", "pointerdown", "pointermove", "wheel", "touchstart"]) addEventListener(type, input, { capture: true, passive: true });
  document.addEventListener("visibilitychange", check);
  setInterval(check, CHECK_MS);
  check();
  // Locked, unlocked or set in another tab, or the desktop app's other window.
  addEventListener("storage", (e) => {
    if (e.key === LOCKED && e.newValue) leaveEdits();
    if (e.key === null || e.key === LOCK || e.key === LOCKED || e.key === TRIES) changed();
  });
}

/** Calls `f` each time the lock is set, locked or unlocked, or a PIN is wrong; returns how to stop. */
export function onLock(f: () => void): () => void {
  listeners.add(f);
  return () => void listeners.delete(f);
}

export const usePrivacyLock = () => useSyncExternalStore(onLock, privacyLock);
export const useLocked = () => useSyncExternalStore(onLock, isLocked);
export const useWrongTries = () => useSyncExternalStore(onLock, wrongTries);
