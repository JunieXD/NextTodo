// What was typed into an add box and not added yet, per user and per place, so that it outlasts
// leaving the page and closing the app. In localStorage, to be there as the box appears; deleted with
// the user's other data on the device (persist.ts).

const PREFIX = "drafts:";

const read = (userId: string): Record<string, string> => JSON.parse(localStorage.getItem(PREFIX + userId) ?? "{}");

export const draft = (userId: string, place: string) => read(userId)[place] ?? "";

export function keepDraft(userId: string, place: string, text: string) {
  const drafts = read(userId);
  if (text) drafts[place] = text;
  else delete drafts[place];
  if (Object.keys(drafts).length) localStorage.setItem(PREFIX + userId, JSON.stringify(drafts));
  else localStorage.removeItem(PREFIX + userId);
}

export const forgetDrafts = (userId: string) => localStorage.removeItem(PREFIX + userId);

export function forgetOtherDrafts(userId: string) {
  const keys = Array.from({ length: localStorage.length }, (_, i) => localStorage.key(i)!);
  for (const key of keys) if (key.startsWith(PREFIX) && key !== PREFIX + userId) localStorage.removeItem(key);
}
