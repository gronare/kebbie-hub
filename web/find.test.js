import assert from "node:assert/strict";
import { test } from "node:test";

import { PRIORITIES } from "./backlog.js";
import { chipsFor, cleared, emptyFind, FIND_KEY, findIn, keepFind, loadFind, marked, pacer, readFind, snippet, textsOf, toggled } from "./find.js";

const row = (item, fields = {}) => ({ item, title: item, priority: "medium", repo: "kebbie", state: "to-write", filed: 100, home: { kind: item.startsWith("found:") ? "found" : "vault", label: "x" }, questions: 0, blocking: 0, goal: null, body: null, ...fields });
const entry = (item, fields = {}) => ({ id: item, key: item, vault: true, row: row(item, fields), machines: [{ machine: "mch_a", name: "Mac" }] });
const body = (text, question = "Is it?") => ({ sections: [{ key: "goal", heading: "Goal", text, required: true }], questions: [{ text: question, blocks: true, pending_answer: null }], questions_key: null });

const lock = entry("vault:lock.md", { title: "Lock screen shows only a count", repo: "kebbie-hub", priority: "high", state: "drafting", body: { id: "blb_lock" } });
const queue = entry("vault:queue.md", { title: "Questions gather in a queue", repo: "kebbie", body: { id: "blb_queue" } });
const ready = entry("vault:ready.md", { title: "Release notes", repo: "kebbie", state: "ready", goal: "Notices name the release." });
const found = entry("found:f1", { title: "Notices double up", repo: "kebbie-hub", priority: "none", state: "ready", found: { sources: ["a@kebbie-hub"], count: 1 } });
const ALL = [lock, queue, ready, found];
const BODIES = { blb_lock: body("A notice on the lock screen says nothing about the work."), blb_queue: body("A <b>notice</b> goes out when the queue gets a new question.") };
const textOf = textsOf((one) => BODIES[one.row.body?.id] ?? null);
const ids = (list) => list.map((one) => one.id);

test("the search finds a word in a title, a goal or an item's text, in any case, and the filters narrow it", () => {
  assert.deepEqual(ids(findIn(ALL, { ...emptyFind(), q: "NOTICE" }, textOf)), ["vault:lock.md", "vault:queue.md", "vault:ready.md", "found:f1"]);
  assert.deepEqual(ids(findIn(ALL, { ...emptyFind(), q: "notice" }, textsOf(() => null))), ["vault:ready.md", "found:f1"], "without the text loaded only titles and goals match");
  assert.deepEqual(ids(findIn(ALL, { ...emptyFind(), q: "  queue " }, textOf)), ["vault:queue.md"]);
  assert.deepEqual(ids(findIn(ALL, { ...emptyFind(), q: "notice", repos: ["kebbie"] }, textOf)), ["vault:queue.md", "vault:ready.md"]);
  assert.deepEqual(ids(findIn(ALL, { ...emptyFind(), repos: ["kebbie", "kebbie-hub"], states: ["to-write", "drafting"] }, textOf)), ["vault:lock.md", "vault:queue.md"], "chips of one kind add up, kinds narrow each other");
  assert.deepEqual(ids(findIn(ALL, { ...emptyFind(), priorities: ["none"] }, textOf)), [], "what tasks found has no priority of its own");
  assert.deepEqual(ids(findIn(ALL, { ...emptyFind(), found: true }, textOf)), ["found:f1"]);
});

test("each chip counts what it would show with the other filters on, and an active repo stays even when nothing has it", () => {
  const chips = chipsFor(ALL, { ...emptyFind(), q: "notice", repos: ["gone"], states: ["ready"] }, textOf, PRIORITIES);
  const [repos, priorities, states, foundChip] = chips;
  assert.deepEqual(repos.map((one) => [one.label, one.n, one.on]), [["kebbie", 1, false], ["kebbie-hub", 0, false], ["gone", 0, true]]);
  assert.deepEqual(priorities.map((one) => one.label), ["High", "Medium", "Low", "New"]);
  assert.deepEqual(states.map((one) => [one.label, one.n, one.on]), [["to write", 0, false], ["drafting", 0, false], ["ready", 0, true]]);
  assert.deepEqual([foundChip[0].label, foundChip[0].n], ["Found by tasks", 0]);
  const open = chipsFor(ALL, emptyFind(), textOf, PRIORITIES);
  assert.deepEqual(open[0].map((one) => [one.label, one.n]), [["kebbie", 2], ["kebbie-hub", 2]]);
  assert.deepEqual(open[2].map((one) => one.n), [1, 1, 1]);
});

test("the choices are kept on the device and read back safely", () => {
  const kept = new Map();
  const storage = { getItem: (key) => kept.get(key) ?? null, setItem: (key, value) => kept.set(key, value) };
  const find = { ...toggled(toggled(emptyFind(), "repo", "kebbie"), "state", "ready"), q: "notice", sort: "newest" };
  keepFind(storage, find);
  assert.deepEqual(loadFind(storage), find);
  assert.ok(kept.has(FIND_KEY));
  assert.deepEqual(toggled(find, "repo", "kebbie").repos, []);
  assert.deepEqual(cleared(find), { ...emptyFind(), q: "notice", sort: "newest" });
  kept.set(FIND_KEY, "{not json");
  assert.deepEqual(loadFind(storage), emptyFind());
  assert.deepEqual(readFind({ q: 3, repos: ["a", 4, "a"], priorities: ["urgent", "low"], states: ["done"], found: "yes", sort: "oldest" }), { ...emptyFind(), repos: ["a"], priorities: ["low"] });
  const broken = { getItem: () => { throw new Error("blocked"); }, setItem: () => { throw new Error("blocked"); } };
  assert.deepEqual(loadFind(broken), emptyFind());
  assert.doesNotThrow(() => keepFind(broken, find));
  assert.deepEqual(loadFind(null), emptyFind());
});

test("a match is marked in the title and a text match shows as a short escaped snippet", () => {
  assert.equal(marked("Notice <b> notice", "notice"), "<mark>Notice</mark> &#60;b&#62; <mark>notice</mark>");
  assert.equal(marked("a+b (c)", "+b ("), "a<mark>+b (</mark>c)");
  assert.equal(marked("plain", ""), "plain");
  const long = `${"word ".repeat(20)}the notice goes out ${"after ".repeat(20)}`;
  const shown = snippet(["nothing here", long], "notice");
  assert.match(shown, /^….*<mark>notice<\/mark> goes out .*…$/);
  assert.ok(shown.replace(/<\/?mark>/g, "").length < 110, shown);
  assert.equal(snippet(["A <b>notice</b>."], "notice"), "A &#60;b&#62;<mark>notice</mark>&#60;/b&#62;.");
  assert.equal(snippet(["nothing"], "notice"), null);
});

test("the background loader keeps to its burst and rate, skips what is held, and stops when told", async () => {
  let now = 0;
  const loaded = [];
  const sleeps = [];
  const held = new Set(["blb_0"]);
  const loader = pacer({
    load: async (ref) => loaded.push(ref.id),
    held: (id) => held.has(id),
    burst: 3,
    perSecond: 2,
    concurrency: 2,
    clock: () => now,
    sleep: async (ms) => {
      sleeps.push(ms);
      now += ms;
    },
  });
  loader.want(Array.from({ length: 8 }, (_, index) => ({ id: `blb_${index}` })));
  loader.want([{ id: "blb_1" }]);
  for (let turn = 0; turn < 50 && loaded.length < 7; turn += 1) await new Promise((done) => setImmediate(done));
  assert.deepEqual([...loaded].sort(), ["blb_1", "blb_2", "blb_3", "blb_4", "blb_5", "blb_6", "blb_7"]);
  assert.ok(now >= 2000 && now <= 2600, `four loads past the burst of three take two seconds at two a second, took ${now}`);
  assert.equal(loader.pending(), 0);
  loader.want([{ id: "blb_8" }, { id: "blb_9" }]);
  loader.stop();
  await new Promise((done) => setImmediate(done));
  assert.equal(loader.pending(), 0);
  assert.ok(loaded.length <= 9);
});

test("a refused load pauses the background loader before the next one", async () => {
  let now = 0;
  const at = [];
  const loader = pacer({
    load: async (ref) => (at.push([ref.id, now]), ref.id === "blb_1" ? { failed: true } : { body: {} }),
    held: () => false,
    burst: 10,
    perSecond: 10,
    concurrency: 1,
    failed: (result) => Boolean(result?.failed),
    pause: 30_000,
    clock: () => now,
    sleep: async (ms) => {
      now += ms;
    },
  });
  loader.want([{ id: "blb_1" }, { id: "blb_2" }]);
  for (let turn = 0; turn < 20 && at.length < 2; turn += 1) await new Promise((done) => setImmediate(done));
  assert.deepEqual(at.map(([id]) => id), ["blb_1", "blb_2"]);
  assert.ok(at[1][1] >= 30_000, `the next load waits out the pause, came at ${at[1][1]}`);
});
