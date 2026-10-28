import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import { accountIdentity, fromB64 } from "../js/protocol.js";
import { arrange, emptyMarks, findTask, isAsleep, mergeMarks, openBoards, readMarks, SLEEP_AFTER, splitAddress, toAccept } from "./board.js";
import { get } from "./store.js";

const load = (name) => JSON.parse(readFileSync(new URL(`../vectors/${name}`, import.meta.url), "utf8"));
const { parties } = load("keys.json");
const envelopes = load("envelopes.json");
const accounts = load("accounts.json");
const vector = envelopes.cases.find((candidate) => candidate.name === "board-opened-by-the-account");
const now = envelopes.now;
const machine = parties.machine;
const entry = (changes = {}) => ({ machine: machine.id, name: "devbox", last_seen: now - 5, machine_keys: machine.public, rev: vector.expect.payload.rev, stored_at: now - 60, envelope: vector.envelope, ...changes });
const signed = { list: accounts.machines.list, signature: accounts.machines.signature };
const removed = { list: accounts.machines.removed, signature: accounts.machines.removed_signature };
const listing = (boards, list = signed) => ({ boards, list });
const open = async (boards, marks, list) => (await openBoards(listing(boards, list), await account(), now, marks)).opened;
const account = () => accountIdentity(accounts.key, { signing: fromB64(parties.account.signing_secret), encryption: fromB64(parties.account.encryption_secret) }, accounts.key.account);

test("a board sealed to the account opens to the machine's payload once the account vouches for the machine", async () => {
  const [opened] = await open([entry()]);
  assert.equal(opened.problem, null);
  assert.equal(opened.name, "devbox");
  assert.deepEqual(opened.board, vector.expect.payload);
});

test("a board that fails to verify or decrypt is kept out with a reason", async () => {
  const flipped = vector.envelope.ct.startsWith("A") ? `B${vector.envelope.ct.slice(1)}` : `A${vector.envelope.ct.slice(1)}`;
  const cases = [
    [entry({ envelope: { ...vector.envelope, ct: flipped } }), "bad-signature"],
    [entry({ envelope: { ...vector.envelope, at: vector.envelope.at + 1 } }), "bad-signature"],
    [entry({ machine_keys: parties["other-machine"].public }), "unknown-machine"],
    [entry({ machine: parties.intruder.id, machine_keys: parties.intruder.public }), "unknown-machine"],
    [entry({ envelope: null }), "not-sealed-to-this-account"],
    [entry({ rev: vector.expect.payload.rev + 1 }), "revision-differs"],
  ];
  for (const [given, problem] of cases) {
    const [opened] = await open([given]);
    assert.equal(opened.board, null, problem);
    assert.equal(opened.problem, problem);
  }
});

test("a connected machine without a board yet shows with no problem", async () => {
  const [waiting] = await open([entry({ rev: null, stored_at: null, envelope: null })]);
  assert.deepEqual([waiting.board, waiting.problem, waiting.rev], [null, null, null]);
});

test("a board sealed only to devices does not open with the account key", async () => {
  const deviceBoard = envelopes.cases.find((candidate) => candidate.name === "board-opened-by-a-device");
  const [opened] = await open([entry({ envelope: deviceBoard.envelope })]);
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

test("a board with steps opens with them, and the board counts the commits waiting to be accepted on awake machines", async () => {
  const stepped = envelopes.cases.find((candidate) => candidate.name === "board-with-steps-opened-by-the-account");
  const [opened] = await open([entry({ rev: stepped.expect.payload.rev, envelope: stepped.envelope })]);
  assert.equal(opened.problem, null);
  const [task] = opened.board.items;
  assert.deepEqual(task.steps.items.map((step) => [step.number, step.state]), [[2, "committed"], [3, "in-progress"], [4, "planned"]]);
  assert.equal(toAccept(task), 1);
  const two = structuredClone(task.steps);
  two.items[1] = { ...two.items[1], state: "committed", sha: "3333333cccc" };
  two.items.push({ number: 5, title: "Docs", state: "accepted", sha: "5555555eeee" });
  const view = arrange([
    machineWith("mch_a", "devbox", now - 10, [task, item("other@web", "WORKING", 0, { steps: two }), item("search@web", "WORKING")]),
    machineWith("mch_vm", "VM:en", now - SLEEP_AFTER - 1, [item("deps@infra", "WORKING", 0, { steps: two })]),
  ], now);
  assert.equal(view.toAccept, 3);
  assert.equal(toAccept(item("search@web", "WORKING")), 0, "a task without steps has nothing to accept");
  assert.equal(arrange([machineWith("mch_a", "devbox", now - 10, [item("search@web", "WORKING")])], now).toAccept, 0);
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

test("a machine whose last board expired is asleep with the hub's last-seen time, not a problem", async () => {
  const lastSeen = vector.envelope.exp - 3600;
  const later = vector.envelope.exp + 60;
  const { opened } = await openBoards(listing([entry({ last_seen: lastSeen })]), await account(), later);
  assert.equal(opened[0].problem, null);
  assert.equal(opened[0].expired, true);
  const view = arrange(opened, later);
  assert.deepEqual(view.problems, []);
  assert.deepEqual(view.sleeping, [{ machine: machine.id, name: "devbox", lastSeen, working: null }]);
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

test("a board older than the newest one read from that machine, by rev or by signed time, is refused", async () => {
  const rev = vector.expect.payload.rev;
  const at = vector.envelope.at;
  const marks = (mark) => ({ list: 0, machines: { [machine.id]: mark } });
  assert.equal((await open([entry()], marks({ rev: rev + 1, at: 0 })))[0].problem, "rolled-back");
  assert.equal((await open([entry()], marks({ rev: 0, at: at + 1 })))[0].problem, "rolled-back");
  const read = await openBoards(listing([entry()]), await account(), now, marks({ rev, at }));
  assert.equal(read.opened[0].problem, null);
  assert.deepEqual(read.marks.machines[machine.id], { rev, at });
  assert.equal(read.marks.list, signed.list.counter);
});

test("only machines on a list the account signed, no older than the newest seen, are trusted", async () => {
  const other = parties["other-machine"];
  const theirs = entry({ machine: other.id, machine_keys: other.public, envelope: null, rev: null });
  const fresh = await open([entry(), theirs], emptyMarks(), signed);
  assert.deepEqual(fresh.map((opened) => opened.problem), [null, null]);
  const after = await open([entry(), theirs], emptyMarks(), removed);
  assert.deepEqual(after.map((opened) => opened.problem), [null, "unknown-machine"], "a removed machine is not trusted");
  const replayed = await open([entry(), theirs], { list: removed.list.counter, machines: {} }, signed);
  assert.deepEqual(replayed.map((opened) => opened.problem), ["rolled-back-list", "rolled-back-list"]);
  const dropped = await open([entry()], { list: 1, machines: {} }, null);
  assert.equal(dropped[0].problem, "rolled-back-list", "a list the hub withholds after one was seen is a rollback");
  assert.equal((await open([entry()], emptyMarks(), { ...signed, signature: removed.signature }))[0].problem, "unknown-machine");
  assert.equal((await open([entry()], emptyMarks(), null))[0].problem, "unknown-machine");
});

test("a listed machine the hub no longer reports is named for the browser to drop", async () => {
  const result = await openBoards(listing([entry()]), await account(), now, emptyMarks());
  assert.deepEqual(result.gone, [parties["other-machine"].id]);
});

test("marks merge to the highest of each, so a store that failed loses nothing this visit", () => {
  const merged = mergeMarks({ list: 3, machines: { a: { rev: 5, at: 10 } } }, undefined, { list: 2, machines: { a: { rev: 4, at: 12 }, b: { rev: 1, at: 1 } } });
  assert.deepEqual(merged, { list: 3, machines: { a: { rev: 5, at: 12 }, b: { rev: 1, at: 1 } } });
});

test("a store that cannot be read says so instead of passing for an empty one", async () => {
  await assert.rejects(get("marks:acc_x"), "the store does not swallow its failures");
  assert.deepEqual(await readMarks(get, "marks:acc_x"), { marks: emptyMarks(), failed: true });
  assert.deepEqual(await readMarks(async () => undefined, "marks:acc_x"), { marks: emptyMarks(), failed: false });
});
