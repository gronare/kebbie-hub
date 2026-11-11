import assert from "node:assert/strict";
import { test } from "node:test";

import { choosePose, overviewPose, railSide } from "./pose.js";

const pose = (width, height, strip = false) => choosePose({ width, height, strip });

test("phones keep the phone layout in portrait and in small landscape, and a large phone in landscape splits with a rail", () => {
  assert.equal(pose(430, 932), "phone");
  assert.equal(pose(382, 580), "phone");
  assert.equal(pose(932, 430), "land");
  assert.equal(pose(874, 402), "land");
  assert.equal(pose(667, 375), "phone");
  assert.equal(pose(844, 320), "phone", "a Safari toolbar that leaves the page too short falls back");
});

test("the Duo's poses and the desktop are unchanged", () => {
  assert.equal(pose(951, 669, true), "split");
  assert.equal(pose(951, 585), "split");
  assert.equal(pose(669, 869), "stack");
  assert.equal(pose(466, 678, true), "stack");
  assert.equal(pose(678, 466, true), "stack");
  assert.equal(pose(1280, 800), "desk");
});

test("the board opens on the overview in the stack pose and on a phone held upright", () => {
  assert.equal(overviewPose("stack", false), true);
  assert.equal(overviewPose("phone", true), true);
  assert.equal(overviewPose("phone", false), false);
  assert.equal(overviewPose("land", false), false);
});

test("the rail sits opposite the Dynamic Island: on the right at 90 degrees, on the left at 270", () => {
  assert.equal(railSide(90), "right");
  assert.equal(railSide(270), "left");
  assert.equal(railSide(undefined), "right");
});
