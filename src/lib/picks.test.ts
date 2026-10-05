/** Run with `npm test`. */
import { test } from "node:test";
import assert from "node:assert/strict";
import { MODES, modeChoices } from "./picks.ts";

test("the mode picker lists every mode", () => {
  assert.deepEqual(
    modeChoices("plan").map((m) => m.id),
    MODES.map((m) => m.id),
  );
});

test("a mode this build doesn't know still gets a row of its own", () => {
  const choices = modeChoices("acceptEdits");
  assert.equal(choices.length, MODES.length + 1);
  assert.deepEqual(choices.at(-1), { id: "acceptEdits", name: "acceptEdits", note: "" });
});
