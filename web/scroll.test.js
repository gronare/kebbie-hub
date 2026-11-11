import assert from "node:assert/strict";
import { test } from "node:test";

import { outOfRangeScroll } from "./scroll.js";

test("a scroll left past the end of the page by a rotation is pulled back to the end", () => {
  assert.equal(outOfRangeScroll(82, 869, 869), 0);
  assert.equal(outOfRangeScroll(500, 1200, 869), 331);
});

test("a page scrolled within its length is left where it is", () => {
  assert.equal(outOfRangeScroll(0, 869, 869), null);
  assert.equal(outOfRangeScroll(331, 1200, 869), null);
  assert.equal(outOfRangeScroll(120, 1200, 869), null);
});
