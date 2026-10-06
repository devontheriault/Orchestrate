/** Run with `npm test`. */
import { test } from "node:test";
import assert from "node:assert/strict";
import { axisOf, goesThrough, resist, settleMs } from "./swipe.ts";

test("a touch says nothing about its way until it has moved", () => {
  assert.equal(axisOf({ x: 100, y: 100 }, { x: 106, y: 104 }), null);
});

test("sideways only when well more across than down", () => {
  assert.equal(axisOf({ x: 100, y: 100 }, { x: 140, y: 110 }), "x");
  assert.equal(axisOf({ x: 100, y: 100 }, { x: 70, y: 105 }), "x");
  assert.equal(axisOf({ x: 100, y: 100 }, { x: 130, y: 125 }), "y");
  assert.equal(axisOf({ x: 100, y: 100 }, { x: 102, y: 40 }), "y");
});

test("a swipe goes through when it's past the middle or flicked", () => {
  assert.equal(goesThrough(200, 390, 0), true);
  assert.equal(goesThrough(-200, 390, 0), true);
  assert.equal(goesThrough(60, 390, 0.1), false);
  assert.equal(goesThrough(60, 390, 0.6), true);
  assert.equal(goesThrough(-60, 390, -0.6), true);
});

test("a swipe pulled back before letting go doesn't go through", () => {
  assert.equal(goesThrough(220, 390, -0.5), false);
  assert.equal(goesThrough(0, 390, 1), false);
});

test("a pull with nowhere to go moves less and less", () => {
  assert.ok(resist(100) < 100);
  assert.ok(resist(400) - resist(300) < resist(100));
  assert.equal(resist(-100), -resist(100));
});

test("a fast swipe finishes quicker, within bounds", () => {
  assert.ok(settleMs(300, 2) < settleMs(300, 0));
  assert.equal(settleMs(1000, 0), 320);
  assert.equal(settleMs(5, 3), 160);
});
