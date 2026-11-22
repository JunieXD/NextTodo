import { expect, test } from "vitest";
import cases from "../../../../fixtures/text.json";
import { type Checked, checkDescription, checkFileName, checkFolderName, checkJobText, checkNoteText, checkTitle } from "./text";

const checks: Record<string, (s: string) => Checked> = {
  title: checkTitle,
  folder_name: checkFolderName,
  description: checkDescription,
  file_name: checkFileName,
  job_text: checkJobText,
  note_text: checkNoteText,
};

// The server takes what the app makes of the text as it is, and turns down the same; it turns down
// what the app cleans, as `server_error` says (crates/core/tests/text.rs).
test.each(cases)("text is checked as the fixtures say: $name", (c) => {
  const input = c.input.repeat(c.times ?? 1);
  expect(checks[c.kind](input)).toEqual("error" in c ? { ok: false, error: c.error } : { ok: true, value: c.value ?? input });
});
