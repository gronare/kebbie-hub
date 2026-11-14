import assert from "node:assert/strict";
import { test } from "node:test";

import { arrange } from "./board.js";
import { taskKey } from "./screens.js";
import { backHref, boardSections, firstTaskOf, sectionPills, sectionSheet, sectionsButton, sectionView } from "./sections.js";

const NOW = 1_761_070_000;
const item = (address, group, extra = {}) => ({ address, group, urgency: 0, agent: { kind: "claude", status: "idle" }, summary: "1 commit", ...extra });
const board = {
  items: [item("refunds@shop", "NEEDS YOU", { title: "Refunds through the new gateway" }), item("hub@kebbie", "NEEDS YOU", { urgency: 1 }), item("payments@api", "WORKING"), item("ci@docs", "CI RUNNING")],
  next: [
    { address: "hub@kebbie", line: "answer the prompt", part: "stopped", rank: 2, since: NOW - 60 },
    { address: "refunds@shop", line: "answer the question", part: "stopped", rank: 1, since: NOW - 60 },
  ],
  later: [{ address: "editor@app", line: "nothing yet", question: false, state: "parked", title: null, until: null }],
  routines: [{ id: "deps", title: "Nightly deps", repo: "shop", next_run: null, last_outcome: "ok" }],
};
const view = arrange([{ machine: "mch_a", name: "studio", lastSeen: NOW - 5, board, problem: null, expired: false }], NOW);
const hrefs = (html) => [...html.matchAll(/<a\b[^>]*href="([^"]*)"/g)].map(([, href]) => href);
const text = (html) => html.replace(/<[^>]+>/g, " ").replace(/\s+/g, " ").trim();

test("the board's sections are Next, then its groups, parked and queued tasks and routines, each with its count", () => {
  assert.deepEqual(boardSections(view).map((section) => [section.key, section.title, section.count]), [["next", "Next", 2], ["needs-you", "Needs you", 2], ["working", "Working", 1], ["ci-running", "CI running", 1], ["parked", "Parked", 1], ["routines", "Routines", 1]]);
});

test("a pill opens its section, and Next has no pill because the overview is Next", () => {
  const pills = sectionPills(boardSections(view));
  assert.deepEqual(hrefs(pills), ["#/section/needs-you", "#/section/working", "#/section/ci-running", "#/section/parked", "#/section/routines"]);
  assert.match(text(pills), /^Needs you 2 Working 1/);
});

test("a section lists its tasks, each opening its card, with the selected one marked", () => {
  const html = sectionView(view, "needs-you", NOW, { selected: taskKey("mch_a", "hub@kebbie") });
  assert.match(text(html), /^Needs you 2 tasks Refunds through the new gateway studio/);
  assert.deepEqual(hrefs(html), ["#/task/mch_a/refunds%40shop", "#/task/mch_a/hub%40kebbie"]);
  assert.match(html, /<a class="task sel" href="#\/task\/mch_a\/hub%40kebbie" aria-current="true">/);
  assert.equal(sectionView(view, "nowhere", NOW), "");
});

test("Next marks the selected row and is headed by the board's counts", () => {
  const html = sectionView(view, "next", NOW, { selected: taskKey("mch_a", "refunds@shop"), nextLine: "2 need you · 1 machine" });
  assert.match(html, /<a class="nrow sel" href="#\/task\/mch_a\/refunds%40shop" aria-current="true">/);
  assert.match(text(html), /^Next 2 need you · 1 machine/);
});

test("the task shown beside a section is Next's first by rank, else the section's first task, and none for routines", () => {
  assert.deepEqual(firstTaskOf(view, "next"), ["mch_a", "refunds@shop"]);
  assert.deepEqual(firstTaskOf(view, "working"), ["mch_a", "payments@api"]);
  assert.equal(firstTaskOf(view, "routines"), null);
});

test("back from a task card returns to where it was opened, and back from a section to the board", () => {
  assert.equal(backHref("task", "section/working"), "#/section/working");
  assert.equal(backHref("task", "board"), "#/board");
  assert.equal(backHref("card", "questions"), "#/questions");
  assert.equal(backHref("section", "section/working"), "#/board");
});

test("the sections button opens a sheet of sections that closes from its scrim or button, with the shown section marked", () => {
  assert.match(sectionsButton(false), /data-action="sections" aria-label="Show sections" aria-expanded="false"/);
  const sheet = sectionSheet(boardSections(view), "working");
  assert.equal([...sheet.matchAll(/data-action="close-sections"/g)].length, 2);
  assert.deepEqual(hrefs(sheet), ["#/section/next", "#/section/needs-you", "#/section/working", "#/section/ci-running", "#/section/parked", "#/section/routines"]);
  assert.match(sheet, /<a class="item on" href="#\/section\/working" aria-current="page">/);
});
