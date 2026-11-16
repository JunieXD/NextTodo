// What was typed into an add box and not added yet, per user and per place, so that it outlasts
// leaving the page and closing the app; with the files pasted or dropped there, whose bytes are kept
// in IndexedDB (`files.ts`). In localStorage, to be there as the box appears; deleted with the user's
// other data on the device (persist.ts).
//
// A change turned down for a file (docs/web.md#附件) is noted here by its id before it leaves the
// outbox: the outbox drops it on opening should it still be there, and the tab that uploads takes it
// out of the other tabs too, after which the id goes. An AI job's text and files come back here with
// it, in the same write.

import type { MutationId } from "../api/types/MutationId";

const PREFIX = "drafts:";

/** Tells this page's add boxes that a draft changed; other tabs hear of it from `storage` events. */
const changes = new EventTarget();

/** Calls `f` when the user's drafts change, here or in another tab. */
export function onDrafts(f: () => void): () => void {
  const other = (e: StorageEvent) => e.key?.startsWith(PREFIX) && f();
  changes.addEventListener("change", f);
  addEventListener("storage", other);
  return () => {
    changes.removeEventListener("change", f);
    removeEventListener("storage", other);
  };
}

/** A file waiting in an add box to go with what is added there. */
export interface Staged {
  id: string;
  name: string;
  type: string;
  size: number;
  /** Why it could not go, after the server turned it down. */
  error?: string;
}

interface Drafts {
  text: Record<string, string>;
  files: Record<string, Staged[]>;
  /** Changes turned down for a file, until they are out of the outbox and the other tabs. */
  withdrawn: MutationId[];
}

function read(userId: string): Drafts {
  const raw = JSON.parse(localStorage.getItem(PREFIX + userId) ?? "{}");
  // Before files, the drafts were just the text by place.
  return "text" in raw ? raw : { text: raw, files: {}, withdrawn: [] };
}

function write(userId: string, d: Drafts) {
  const empty = !Object.keys(d.text).length && !Object.keys(d.files).length && !d.withdrawn.length;
  if (empty) localStorage.removeItem(PREFIX + userId);
  else localStorage.setItem(PREFIX + userId, JSON.stringify(d));
}

export const draft = (userId: string, place: string) => read(userId).text[place] ?? "";

export function keepDraft(userId: string, place: string, text: string) {
  const d = read(userId);
  if (text) d.text[place] = text;
  else delete d.text[place];
  write(userId, d);
}

export const stagedFiles = (userId: string, place: string): Staged[] => read(userId).files[place] ?? [];

export function keepStaged(userId: string, place: string, files: Staged[]) {
  const d = read(userId);
  if (files.length) d.files[place] = files;
  else delete d.files[place];
  write(userId, d);
}

/** The files staged anywhere: their bytes stay on the device. */
export const allStaged = (userId: string): Staged[] => Object.values(read(userId).files).flat();

/** Notes change `id` as withdrawn, turned down for a file; an AI job's text and files go `back` to the
 * box at `place`, after what is there, in the same write. */
export function withdraw(userId: string, id: MutationId, back?: { place: string; text: string; files: Staged[] }) {
  const d = read(userId);
  if (d.withdrawn.includes(id)) return;
  if (back) {
    const { place, text, files } = back;
    d.text[place] = d.text[place] ? `${d.text[place]}\n${text}` : text;
    d.files[place] = [...(d.files[place] ?? []), ...files];
  }
  d.withdrawn.push(id);
  write(userId, d);
  changes.dispatchEvent(new Event("change"));
}

export const withdrawn = (userId: string): MutationId[] => read(userId).withdrawn;

/** Those changes are out of the outbox now. */
export function withdrawnGone(userId: string, ids: MutationId[]) {
  if (!ids.length) return;
  const d = read(userId);
  d.withdrawn = d.withdrawn.filter((id) => !ids.includes(id));
  write(userId, d);
}

export const forgetDrafts = (userId: string) => localStorage.removeItem(PREFIX + userId);

export function forgetOtherDrafts(userId: string) {
  const keys = Array.from({ length: localStorage.length }, (_, i) => localStorage.key(i)!);
  for (const key of keys) if (key.startsWith(PREFIX) && key !== PREFIX + userId) localStorage.removeItem(key);
}
