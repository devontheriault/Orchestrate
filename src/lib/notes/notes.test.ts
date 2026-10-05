/** Run with `npm test`. */
import { test } from "node:test";
import assert from "node:assert/strict";
import { notesHostChoices } from "./notes.ts";

const label = (id: string) => id.toUpperCase();

test("no choice with only one Host", () => {
  assert.equal(notesHostChoices(["local"], "local", label, () => null), null);
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
  assert.deepEqual(choices?.map((c) => [c.id, c.disabled]), [
    ["local", false],
    ["gone", true],
  ]);
});
