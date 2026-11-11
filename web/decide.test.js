import assert from "node:assert/strict";
import { test } from "node:test";

import { answerBlock, answerReady, answerStatus, gate, latestSent, pushBlock, pushReady, refusedBlock, sentState } from "./decide.js";

const HEAD = "9f3c2a1e5b7d4c6a8b0e2f4d6c8a0b2e4f6d8c0a";
const AT = 1_761_069_600;
const question = { id: "q1", text: "Keep the old endpoint?", recommend: 1, options: [{ label: "Keep it", meaning: "deprecate now" }, { label: "Remove it", meaning: "drop it", comment: true }] };
const idle = () => ({ picked: null, free: false, note: "", confirming: false, busy: false, error: null });
const answer = (extra = {}) => ({ id: "cmd_1", kind: "answer", machine: "mch_1", machineName: "mac", device: "iPhone Duo", address: "refunds@shop", question: "q1", choice: 0, label: "Keep it", at: AT, exp: AT + 3600, result: null, ...extra });

test("the gate shows nothing new when the hub has commands off, the hub's refusal once it refused a command, setup until this device is listed", () => {
  assert.equal(gate({ info: { commands: false }, refused: null, device: { listed: 3 } }), "off");
  assert.equal(gate({ info: null, refused: null, device: null }), "off");
  assert.equal(gate({ info: { commands: true }, refused: { url: null }, device: { listed: 3 } }), "refused");
  assert.equal(gate({ info: { commands: true }, refused: null, device: null }), "setup");
  assert.equal(gate({ info: { commands: true }, refused: null, device: { listed: 0 } }), "setup");
  assert.equal(gate({ info: { commands: true }, refused: null, device: { listed: 2 } }), "ready");
});

test("a refused account reads that answering is not enabled, with the hub's link when it sent one", () => {
  assert.equal(refusedBlock(null), `<div class="pro">Answering and pushing are not enabled for your account on this hub.</div>`);
  assert.match(refusedBlock("https://hub.example.com/details?a=1&b=2"), /<a class="link" href="https:\/\/hub\.example\.com\/details\?a=1&#38;b=2">Details<\/a>/);
});

test("a sent command waits for a sleeping machine, expires after its hour, and settles on the machine's result", () => {
  assert.equal(sentState(answer(), { now: AT + 10, asleep: true }), "waiting");
  assert.equal(sentState(answer(), { now: AT + 10, asleep: false }), "sending");
  assert.equal(sentState(answer(), { now: AT + 3600, asleep: true }), "expired");
  assert.equal(sentState(answer({ result: { outcome: "done", text: "ok" } }), { now: AT + 9999, asleep: true }), "done", "a result beats the expiry");
  assert.equal(sentState(answer({ result: { outcome: "refused", reason: "already-settled", text: "Already answered at the machine." } }), { now: AT, asleep: false }), "refused");
  assert.equal(sentState(answer({ result: { outcome: "failed", text: "crashed" } }), { now: AT, asleep: false }), "refused");
});

test("the status lines say what happened in the owner's words", () => {
  const waiting = answerStatus(answer(), "waiting");
  assert.match(waiting, /class="sent wait"/);
  assert.match(waiting, /Waiting for mac/);
  assert.match(waiting, /Sent from iPhone Duo at \d\d:\d\d · expires \d\d:\d\d/);
  const done = answerStatus(answer({ result: { outcome: "done", text: "Answered." } }), "done");
  assert.match(done, /class="sent done"/);
  assert.match(done, /✓ Answered: Keep it/);
  assert.match(done, /from iPhone Duo at \d\d:\d\d · the lead got a copy/);
  assert.match(answerStatus(answer(), "expired"), /Not delivered: mac stayed asleep past the hour/);
  assert.match(answerStatus(answer({ result: { outcome: "refused", text: "Already answered <here>." } }), "refused"), /Already answered &#60;here&#62;\./);
});

test("picking an option dims the others, and an option that needs a note waits for one", () => {
  const none = answerBlock({ key: "k", question, ui: idle(), sent: null, state: null });
  assert.doesNotMatch(none, /chosen/);
  assert.match(none, /Write your own answer/);
  const picked = answerBlock({ key: "k", question, ui: { ...idle(), picked: 0 }, sent: null, state: null });
  assert.match(picked, /class="options chosen"/);
  assert.match(picked, /class="opt rec pick picked"/);
  assert.match(picked, /Note for the agent \(optional\)/);
  assert.match(picked, /data-action="answer">Answer: Keep it</);
  const needs = answerBlock({ key: "k", question, ui: { ...idle(), picked: 1 }, sent: null, state: null });
  assert.match(needs, /data-action="answer" disabled>Answer: Remove it</);
  assert.equal(answerReady(question.options[1], { ...idle(), picked: 1, note: "in 0.9" }), true);
  assert.equal(answerReady(undefined, { ...idle(), free: true, note: "  " }), false);
  assert.match(answerBlock({ key: "k", question, ui: { ...idle(), free: true }, sent: null, state: null }), /Send answer/);
});

test("while an answer is on its way the options are fixed; once it is refused or expired they can be picked again", () => {
  const pending = answerBlock({ key: "k", question, ui: idle(), sent: answer(), state: "waiting" });
  assert.doesNotMatch(pending, /data-action="pick"/);
  assert.match(pending, /class="opt rec picked"/);
  assert.doesNotMatch(pending, /Write your own answer/);
  const expired = answerBlock({ key: "k", question, ui: idle(), sent: answer(), state: "expired" });
  assert.match(expired, /data-action="pick"/);
  assert.match(expired, /Not delivered/);
});

test("the latest command for the task and question is the one shown", () => {
  const older = answer({ id: "cmd_a", at: AT });
  const newer = answer({ id: "cmd_b", at: AT + 60 });
  const other = answer({ id: "cmd_c", at: AT + 120, question: "q2" });
  assert.equal(latestSent([older, newer, other], { machine: "mch_1", address: "refunds@shop", kind: "answer", question: "q1" }).id, "cmd_b");
  assert.equal(latestSent([older], { machine: "mch_2", address: "refunds@shop", kind: "answer", question: "q1" }), null);
});

test("a ready task offers a push, asks once, then reports the machine's result", () => {
  const item = { address: "mail-dns@shop", group: "READY TO PUSH", head: HEAD, git: { ahead: 3 }, verdict: { verdict: "ok", stale: false } };
  assert.equal(pushReady(item), true);
  assert.equal(pushReady({ ...item, head: "9f3c2a1" }), false);
  assert.equal(pushReady({ ...item, group: "WORKING" }), false);
  assert.match(pushBlock({ item, machineName: "mac", ui: idle(), sent: null, state: null }), /data-action="push-ask">Push 3 commits · 9f3c2a1</);
  const confirm = pushBlock({ item, machineName: "mac", ui: { ...idle(), confirming: true }, sent: null, state: null });
  assert.match(confirm, /Push 9f3c2a1 to main\?/);
  assert.match(confirm, /3 commits, reviewer passed\. mac rebases onto the latest main first; on a conflict its Claude resolves it before pushing\./);
  assert.match(confirm, /data-action="push-cancel"/);
  const pushed = { id: "cmd_p", kind: "push", machine: "mch_1", machineName: "mac", device: "iPhone Duo", address: item.address, head: HEAD, short: "9f3c2a1", at: AT, exp: AT + 3600 };
  const done = pushBlock({ item, machineName: "mac", ui: idle(), sent: { ...pushed, result: { outcome: "done", text: "CI on main is running." } }, state: "done" });
  assert.match(done, /✓ Pushed 9f3c2a1 to main/);
  assert.doesNotMatch(done, /push-ask/);
  const refused = pushBlock({ item, machineName: "mac", ui: idle(), sent: { ...pushed, result: { outcome: "refused", text: "Not pushed yet: conflicts with main in app/models/refund.rb; its Claude resolves it" } }, state: "refused" });
  assert.match(refused, /<b>Not pushed yet: conflicts with main in app\/models\/refund\.rb; its Claude resolves it<\/b>/, "the machine's own words head the block, once");
  assert.match(pushBlock({ item, machineName: "mac", ui: idle(), sent: { ...pushed, result: { outcome: "failed", text: "git crashed" } }, state: "refused" }), /<b>Not pushed<\/b><span>git crashed<\/span>/);
  assert.match(refused, /push-ask/, "a refused push can be asked again");
  assert.match(pushBlock({ item, machineName: "mac", ui: idle(), sent: { ...pushed, result: null }, state: "waiting" }), /Waiting for mac/);
});
