/** Run with `npm test`. */
import { test } from "node:test";
import assert from "node:assert/strict";
import { fromHome, notesHostChoices } from "./notes.ts";

const label = (id: string) => id.toUpperCase();

test("a lone Host is still listed, so the menu says where the notes are", () => {
  assert.deepEqual(notesHostChoices(["local"], "local", label, () => null), [
    { id: "local", name: "LOCAL", note: undefined, disabled: false },
  ]);
});

test("every Host is listed, an unreachable one saying why", () => {
  const choices = notesHostChoices(["local", "box"], "local", label, (id) =>
    id === "box" ? "box is offline" : null,
  );
  assert.deepEqual(choices, [
    { id: "local", name: "LOCAL", note: undefined, disabled: false },
    { id: "box", name: "BOX", note: "box is offline", disabled: true },
  ]);
});

test("a removed Host the notes are still on stays named", () => {
  const choices = notesHostChoices(["local"], "gone", label, () => null);
  assert.deepEqual(choices.map((c) => [c.id, c.disabled]), [
    ["local", false],
    ["gone", true],
  ]);
});

test("a folder under the Host's home is written from ~", () => {
  assert.equal(fromHome("/home/me/Notes", "/home/me/Notes"), "~/Notes");
  assert.equal(fromHome("/home/me/Documents/Notes", "/home/me/Notes"), "~/Documents/Notes");
  assert.equal(fromHome("C:\\Users\\me\\Notes", "C:\\Users\\me\\Notes"), "~\\Notes");
});

test("a folder elsewhere, or with no home known, is left whole", () => {
  assert.equal(fromHome("/srv/notes", "/home/me/Notes"), "/srv/notes");
  assert.equal(fromHome("/home/meg/Notes", "/home/me/Notes"), "/home/meg/Notes");
  assert.equal(fromHome("/home/me/Notes", null), "/home/me/Notes");
});
