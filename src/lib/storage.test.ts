/** Run with `npm test`. */
import { test } from "node:test";
import assert from "node:assert/strict";
import { readJson, writeJson } from "./storage.ts";

const stored = new Map<string, string>();
Object.defineProperty(globalThis, "localStorage", {
  configurable: true,
  value: {
    getItem: (k: string) => stored.get(k) ?? null,
    setItem: (k: string, v: string) => void stored.set(k, v),
  },
});

test("a setting reads back as it was written", () => {
  writeJson("order", ["a", "b"]);
  assert.deepEqual(readJson<string[]>("order", []), ["a", "b"]);
  writeJson("hosts", { p: "local" });
  assert.deepEqual(readJson<Record<string, string>>("hosts", {}), { p: "local" });
});

test("a missing, corrupt or misshapen setting falls back", () => {
  assert.deepEqual(readJson("never-written", { x: 1 }), { x: 1 });
  stored.set("corrupt", "{not json");
  assert.deepEqual(readJson("corrupt", []), []);
  stored.set("array", "[1]");
  assert.deepEqual(readJson("array", {}), {});
  stored.set("object", "{}");
  assert.deepEqual(readJson("object", []), []);
  stored.set("null", "null");
  assert.deepEqual(readJson("null", {}), {});
});

test("storage that refuses a write isn't an error", () => {
  const full = globalThis.localStorage.setItem;
  globalThis.localStorage.setItem = () => {
    throw new Error("QuotaExceededError");
  };
  assert.doesNotThrow(() => writeJson("big", { a: 1 }));
  globalThis.localStorage.setItem = full;
});
