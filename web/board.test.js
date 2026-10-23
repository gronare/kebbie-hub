import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import { importIdentity } from "../js/protocol.js";
import { arrange, findTask, followHubNames, highestRevs, isAsleep, openBoards, SLEEP_AFTER, splitAddress } from "./board.js";

const load = (name) => JSON.parse(readFileSync(new URL(`../vectors/${name}`, import.meta.url), "utf8"));
const { parties } = load("keys.json");
const envelopes = load("envelopes.json");
const vector = envelopes.cases.find((candidate) => candidate.name === "board-opened-by-a-device");
const now = envelopes.now;
const machine = { id: parties.machine.id, keys: parties.machine.public, name: "Kontoret" };
const entry = (changes = {}) => ({ machine: machine.id, name: "devbox", last_seen: now - 5, rev: vector.expect.payload.rev, stored_at: now - 60, envelope: vector.envelope, ...changes });
const phone = () => importIdentity(parties.phone);

test("a board sealed to this phone opens to the machine's payload", async () => {
  const [opened] = await openBoards([entry()], await phone(), [machine], now);
  assert.equal(opened.problem, null);
  assert.equal(opened.name, "Kontoret");
  assert.deepEqual(opened.board, vector.expect.payload);
});

test("a board that fails to verify or decrypt is kept out with a reason", async () => {
  const flipped = vector.envelope.ct.startsWith("A") ? `B${vector.envelope.ct.slice(1)}` : `A${vector.envelope.ct.slice(1)}`;
  const cases = [
    [entry({ envelope: { ...vector.envelope, ct: flipped } }), [machine], "bad-signature"],
    [entry({ envelope: { ...vector.envelope, at: vector.envelope.at + 1 } }), [machine], "bad-signature"],
    [entry(), [{ id: machine.id, keys: parties["other-machine"].public }], "bad-signature"],
    [entry(), [], "unknown-machine"],
    [entry({ envelope: null }), [machine], "not-sealed-to-this-device"],
    [entry({ rev: vector.expect.payload.rev + 1 }), [machine], "revision-differs"],
  ];
  for (const [given, machines, problem] of cases) {
    const [opened] = await openBoards([given], await phone(), machines, now);
    assert.equal(opened.board, null, problem);
    assert.equal(opened.problem, problem);
  }
});

test("a board is not opened by a device it was not sealed to", async () => {
  const [opened] = await openBoards([entry()], await importIdentity(parties.intruder), [machine], now);
  assert.equal(opened.problem, "not-a-recipient");
});

const item = (address, group, urgency = 0, extra = {}) => ({ ...vector.expect.payload.items[0], address, group, urgency, question: null, ...extra });
const machineWith = (id, name, lastSeen, items) => ({ machine: id, name, lastSeen, rev: 1, storedAt: lastSeen ?? 0, board: { ...vector.expect.payload, items }, problem: null });

test("tasks from awake machines group in the terminal's order, most urgent first", () => {
  const opened = [
    machineWith("mch_a", "devbox", now - 10, [item("search@web", "WORKING"), item("refunds@billing", "NEEDS YOU", 1, { question: vector.expect.payload.items[0].question })]),
    machineWith("mch_b", "Macen", now - 30, [item("skeleton@ios", "NEEDS YOU", 3), item("dates@billing", "READY TO PUSH")]),
  ];
  const view = arrange(opened, now);
  assert.deepEqual(view.groups.map((group) => [group.title, group.rows.map((row) => `${row.task}/${row.name}`)]), [
    ["Needs you", ["skeleton/Macen", "refunds/devbox"]],
    ["Working", ["search/devbox"]],
    ["Ready to push", ["dates/Macen"]],
  ]);
  assert.deepEqual(view.questions.map((row) => row.item.address), ["refunds@billing"]);
  assert.deepEqual(view.sleeping, []);
});

test("a machine not heard from shows as sleeping with what was working, not as tasks", () => {
  const opened = [
    machineWith("mch_a", "devbox", now - 10, [item("search@web", "WORKING")]),
    machineWith("mch_vm", "VM:en", now - SLEEP_AFTER - 1, [item("deps@infra", "WORKING"), item("refunds@billing", "NEEDS YOU", 2, { question: vector.expect.payload.items[0].question })]),
  ];
  const view = arrange(opened, now);
  assert.deepEqual(view.groups.flatMap((group) => group.rows.map((row) => row.item.address)), ["search@web"]);
  assert.deepEqual(view.sleeping, [{ machine: "mch_vm", name: "VM:en", lastSeen: now - SLEEP_AFTER - 1, working: 1 }]);
  assert.deepEqual(view.questions, []);
  assert.equal(isAsleep({ lastSeen: null }, now), true);
  assert.equal(isAsleep({ lastSeen: now - SLEEP_AFTER }, now), false);
});

test("problems are listed by machine and never mixed into the board", () => {
  const view = arrange([{ machine: "mch_x", name: "x", lastSeen: now, rev: 1, storedAt: now, board: null, problem: "bad-signature" }], now);
  assert.deepEqual(view.groups, []);
  assert.deepEqual(view.problems, [{ machine: "mch_x", name: "x", problem: "bad-signature" }]);
});

test("a task is found by machine and address", () => {
  const opened = [machineWith("mch_a", "devbox", now, [item("refunds@billing", "NEEDS YOU")])];
  assert.equal(findTask(opened, "mch_a", "refunds@billing").task, "refunds");
  assert.equal(findTask(opened, "mch_b", "refunds@billing"), null);
  assert.deepEqual(splitAddress("a@b@repo"), { task: "a@b", repo: "repo" });
  assert.deepEqual(splitAddress("loose"), { task: "loose", repo: "" });
});

test("the phone's own name for a machine wins over the hub's", async () => {
  const [named] = await openBoards([entry()], await phone(), [machine], now);
  assert.equal(named.name, "Kontoret");
  const [unnamed] = await openBoards([entry()], await phone(), [{ ...machine, name: null }], now);
  assert.equal(unnamed.name, "devbox");
  const [renamed] = await openBoards([entry({ name: "studio" })], await phone(), [{ ...machine, name: null }], now);
  assert.equal(renamed.name, "studio");
});

test("a board older than the newest one read from that machine is refused as a rollback", async () => {
  const rev = vector.expect.payload.rev;
  const [older] = await openBoards([entry()], await phone(), [machine], now, { [machine.id]: rev + 1 });
  assert.equal(older.board, null);
  assert.equal(older.problem, "rolled-back");
  const [same] = await openBoards([entry()], await phone(), [machine], now, { [machine.id]: rev });
  assert.equal(same.problem, null);
  assert.deepEqual(highestRevs([same, older], { other: 3, [machine.id]: rev - 1 }), { other: 3, [machine.id]: rev });
  assert.deepEqual(highestRevs([older], { [machine.id]: rev + 1 }), { [machine.id]: rev + 1 });
});

test("a phone paired before names followed the hub drops a stored name that is the hub's, and then follows its renames", async () => {
  const stored = [{ ...machine, name: "devbox" }, { ...machine, id: "mch_other", name: "Kontoret" }];
  const followed = followHubNames(stored, [entry(), entry({ machine: "mch_other", name: "devbox" })]);
  assert.deepEqual(followed.map((known) => known.name), [null, "Kontoret"]);
  assert.equal(followHubNames(followed, [entry()]), null, "nothing left to drop changes nothing");
  const [renamed] = await openBoards([entry({ name: "studio" })], await phone(), followed, now);
  assert.equal(renamed.name, "studio");
});
