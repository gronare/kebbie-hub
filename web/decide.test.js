import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import { answerBlock, answerReady, answerStatus, choosable, gate, standingOn, latestSent, machinePolicy, notTakenBlock, promptAnswerBlock, promptStatus, pushBlock, pushStatus, pushReady, refusedBlock, sentState, speaksFor, takes, takesPromptAnswers, viewOnlyBlock } from "./decide.js";
import { pushableOf } from "../js/protocol.js";

const HEAD = "9f3c2a1e5b7d4c6a8b0e2f4d6c8a0b2e4f6d8c0a";
const AT = 1_761_069_600;
const question = { id: "q1", text: "Keep the old endpoint?", recommend: 1, options: [{ label: "Keep it", meaning: "deprecate now" }, { label: "Remove it", meaning: "drop it", comment: true }] };
const idle = () => ({ picked: null, free: false, note: "", confirming: false, busy: false, error: null });
const answer = (extra = {}) => ({ id: "cmd_1", kind: "answer", machine: "mch_1", machineName: "mac", address: "refunds@shop", question: "q1", choice: 0, label: "Keep it", at: AT, exp: AT + 3600, result: null, ...extra });

test("the gate is off when the hub takes no commands, refused once the hub refused one, view only until this device allows changes", () => {
  assert.equal(gate({ info: { commands: false }, refused: null, allowed: true }), "off");
  assert.equal(gate({ info: null, refused: null, allowed: true }), "off");
  assert.equal(gate({ info: { commands: true }, refused: { url: null }, allowed: true }), "refused");
  assert.equal(gate({ info: { commands: true }, refused: null, allowed: false }), "view-only");
  assert.equal(gate({ info: { commands: true }, refused: null }), "view-only", "off until this device allows changes");
  assert.equal(gate({ info: { commands: true }, refused: null, allowed: true }), "ready");
});

test("a machine says what it takes from the hub through its board's accepts", () => {
  assert.equal(machinePolicy(["answer", "push", "prompt-answer"]), "takes answers and pushes");
  assert.equal(machinePolicy(["answer", "prompt-answer"]), "takes answers, not pushes");
  assert.equal(machinePolicy([]), "takes nothing from the hub");
  assert.equal(machinePolicy(null), "takes nothing from the hub");
  assert.equal(takes(["answer"], "push"), false);
  assert.equal(takes(undefined, "answer"), false);
});

test("a view-only device and a machine that does not take a kind say so in the owner's words", () => {
  assert.equal(viewOnlyBlock(), `<div class="pro">View only on this device. Allow changes in <a class="link inline" href="/account">Account</a></div>`);
  assert.equal(notTakenBlock("push", "devbox", ["answer", "prompt-answer"]), `<div class="pro">Not pushed: devbox takes answers, not pushes. Push at the machine, or allow pushes in its kebbie settings.</div>`);
  assert.match(notTakenBlock("push", "mac<mini>", []), /Not pushed: mac&#60;mini&#62; takes nothing from the hub\. Push at the machine/);
  assert.match(notTakenBlock("answer", "devbox", []), /Not answered: devbox takes nothing from the hub\. Answer at the machine, or allow answers in its kebbie settings\./);
  assert.match(notTakenBlock("prompt-answer", "devbox", []), /Not chosen: devbox takes nothing from the hub/);
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
  assert.match(waiting, /Sent from this device at \d\d:\d\d · expires \d\d:\d\d/);
  const done = answerStatus(answer({ result: { outcome: "done", text: "Answered." } }), "done");
  assert.match(done, /class="sent done"/);
  assert.match(done, /✓ Answered: Keep it/);
  assert.match(done, /from this device at \d\d:\d\d · the lead got a copy/);
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

test("a task with a pushable run offers to push just that run from the top of its push order, and the task is ready to push because of it", () => {
  const run = { address: "payments@api", group: "WORKING", head: "9".repeat(40), git: { ahead: 7 }, pushable: { count: 3, through: "3".repeat(40), subtasks: ["env", "fm"] } };
  assert.equal(pushReady(run), true);
  const idle = { confirming: false, busy: false, error: null };
  assert.match(pushBlock({ item: run, machineName: "mac", ui: idle, sent: null, state: null }), /data-action="push-ask">Push 3 commits · 3333333</);
  const confirm = pushBlock({ item: run, machineName: "mac", ui: { ...idle, confirming: true }, sent: null, state: null });
  assert.match(confirm, /<b>Push 3 commits · 3333333 to main\?<\/b><\/p><p>3 commits from the top of the push order; the rest stay here\. mac rebases/);
  assert.equal(pushReady({ ...run, pushable: undefined }), false, "a working task without a run offers no push");
});

test("the push sends the run the rule computes: past a subtask's accepted commit, never into a group it would split", () => {
  const envelopes = JSON.parse(readFileSync(new URL("../vectors/envelopes.json", import.meta.url), "utf8"));
  const item = envelopes.cases.find((vector) => vector.name === "board-with-subtasks-opened-by-the-account").expect.payload.items[0];
  assert.deepEqual(item.pushable, pushableOf(item.order));
  const idle = { confirming: false, busy: false, error: null };
  assert.match(pushBlock({ item, machineName: "mac", ui: idle, sent: null, state: null }), new RegExp(`data-action="push-ask">Push 2 commits · ${item.order[1].sha.slice(0, 7)}<`));
  const order = item.order.map((row) => ({ ...row, accepted: true }));
  const whole = { ...item, order, pushable: pushableOf(order) };
  assert.match(pushBlock({ item: whole, machineName: "mac", ui: idle, sent: null, state: null }), new RegExp(`Push 4 commits · ${item.head.slice(0, 7)}<`), "once its last commit is accepted the group goes out whole");
});

test("a ready task offers a push, asks once, then reports the machine's result", () => {
  const item = { address: "mail-dns@shop", group: "READY TO PUSH", head: HEAD, git: { ahead: 3 }, verdict: { verdict: "ok", stale: false } };
  assert.equal(pushReady(item), true);
  assert.equal(pushReady({ ...item, head: "9f3c2a1" }), false);
  assert.equal(pushReady({ ...item, group: "WORKING" }), false);
  assert.match(pushBlock({ item, machineName: "mac", ui: idle(), sent: null, state: null }), /data-action="push-ask">Push 3 commits · 9f3c2a1</);
  const confirm = pushBlock({ item, machineName: "mac", ui: { ...idle(), confirming: true }, sent: null, state: null });
  assert.match(confirm, /Push 3 commits · 9f3c2a1 to main\?/);
  assert.match(confirm, /3 commits, reviewer passed\. mac rebases onto the latest main first; on a conflict its Claude resolves it before pushing\./);
  assert.match(confirm, /data-action="push-cancel"/);
  const pushed = { id: "cmd_p", kind: "push", machine: "mch_1", machineName: "mac", address: item.address, head: HEAD, short: "9f3c2a1", at: AT, exp: AT + 3600 };
  const done = pushBlock({ item, machineName: "mac", ui: idle(), sent: { ...pushed, result: { outcome: "done", text: "CI on main is running." } }, state: "done" });
  assert.match(done, /✓ Pushed 9f3c2a1 to main/);
  assert.doesNotMatch(done, /push-ask/);
  const refused = pushBlock({ item, machineName: "mac", ui: idle(), sent: { ...pushed, result: { outcome: "refused", text: "Not pushed yet: conflicts with main in app/models/refund.rb; its Claude resolves it" } }, state: "refused" });
  assert.match(refused, /<b>Not pushed yet: conflicts with main in app\/models\/refund\.rb; its Claude resolves it<\/b>/, "the machine's own words head the block, once");
  assert.match(pushBlock({ item, machineName: "mac", ui: idle(), sent: { ...pushed, result: { outcome: "failed", text: "git crashed" } }, state: "refused" }), /<b>Not pushed<\/b><span>git crashed<\/span>/);
  const policy = pushBlock({ item, machineName: "devbox", ui: idle(), sent: { ...pushed, result: { outcome: "refused", reason: "not-allowed", text: "Not pushed: devbox takes answers, not pushes. Push at the machine, or allow pushes in its kebbie settings." } }, state: "refused" });
  assert.match(policy, /<b>Not pushed: devbox takes answers, not pushes\.<\/b><span>Push at the machine, or allow pushes in its kebbie settings\.<\/span>/, "a machine that refused a push heads the block with what it takes");
  assert.match(refused, /push-ask/, "a refused push can be asked again");
  assert.match(pushBlock({ item, machineName: "mac", ui: idle(), sent: { ...pushed, result: null }, state: "waiting" }), /Waiting for mac/);
});

const prompt = { id: "3f2a9c1d", question: "Run it?", options: [{ label: "Yes, proceed (y)", takes_text: false, key: "y" }, { label: "No, and tell Codex what to do differently (esc)", takes_text: false, key: "esc" }, { label: "Type something", takes_text: true, key: "3" }] };
const chose = (extra = {}) => ({ id: "cmd_q", kind: "prompt-answer", machine: "mch_1", machineName: "mac", address: "hub@kebbie", prompt: "3f2a9c1d", key: "y", label: "Yes, proceed (y)", at: AT, exp: AT + 600, result: null, ...extra });

test("a prompt is answered from here only on a machine whose board says it takes prompt answers", () => {
  assert.equal(takesPromptAnswers({ accepts: ["answer", "push", "prompt-answer"] }), true);
  assert.equal(takesPromptAnswers({ accepts: ["answer", "push"] }), false);
  assert.equal(takesPromptAnswers({ accepts: null }), false, "a kebbie from before prompt answers sends no list");
});

test("an option that takes text is shown as answered at the machine and can't be tapped, even when picked", () => {
  assert.deepEqual(prompt.options.map(choosable), [true, true, false]);
  assert.equal(choosable({ label: "Yes", takes_text: false }), false, "an option without a key can't be chosen");
  const idleBlock = promptAnswerBlock({ prompt, ui: idle(), sent: null, state: null });
  assert.equal((idleBlock.match(/data-action="pick"/g) ?? []).length, 2);
  assert.match(idleBlock, /<div class="opt"><b>Type something<\/b><small>answer at the machine<\/small><\/div>/);
  assert.doesNotMatch(idleBlock, /data-action="choose"/);
  assert.doesNotMatch(promptAnswerBlock({ prompt, ui: { ...idle(), picked: 2 }, sent: null, state: null }), /data-action="choose"/);
});

test("picking a prompt option asks to confirm the choice, and while it is on its way the options are fixed", () => {
  const picked = promptAnswerBlock({ prompt, ui: { ...idle(), picked: 1 }, sent: null, state: null });
  assert.match(picked, /class="options chosen"/);
  assert.match(picked, /class="opt pick picked"/);
  assert.match(picked, /data-action="choose">Choose: No, and tell Codex what to do differently \(esc\)</);
  assert.match(promptAnswerBlock({ prompt, ui: { ...idle(), picked: 0, busy: true }, sent: null, state: null }), /data-action="choose" disabled>/);
  const pending = promptAnswerBlock({ prompt, ui: idle(), sent: chose(), state: "sending" });
  assert.doesNotMatch(pending, /data-action="(pick|choose)"/);
  assert.match(pending, /class="opt picked"/);
  assert.match(pending, /mac chooses "Yes, proceed \(y\)" if the prompt still shows\./);
  const refused = promptAnswerBlock({ prompt, ui: idle(), sent: chose({ result: { outcome: "refused", reason: "already-settled", text: "The prompt 3f2a9c1d is not on the task's screen any more" } }), state: "refused" });
  assert.match(refused, /<b>Not chosen<\/b><span>The prompt 3f2a9c1d is not on the task/);
  assert.match(refused, /data-action="pick"/, "a refused choice can be made again");
});

test("a prompt answer expires after ten minutes and its status lines say what happened", () => {
  assert.equal(sentState(chose(), { now: AT + 599, asleep: true }), "waiting");
  assert.equal(sentState(chose(), { now: AT + 600, asleep: true }), "expired");
  assert.match(promptStatus(chose(), "expired"), /Not delivered: mac didn't take it within ten minutes/);
  const done = promptStatus(chose({ result: { outcome: "done", text: "Chose it." } }), "done");
  assert.match(done, /✓ Chose: Yes, proceed \(y\)/);
  assert.match(done, /from this device at \d\d:\d\d · the lead got a copy/);
  assert.match(promptStatus(chose(), "waiting"), /Waiting for mac/);
});

test("the latest prompt answer shown is the one for the prompt on screen now", () => {
  const before = chose({ id: "cmd_a", prompt: "0000aaaa" });
  const now = chose({ id: "cmd_b", at: AT - 60 });
  assert.equal(latestSent([before, now], { machine: "mch_1", address: "hub@kebbie", kind: "prompt-answer", prompt: "3f2a9c1d" }).id, "cmd_b");
  assert.equal(latestSent([before], { machine: "mch_1", address: "hub@kebbie", kind: "prompt-answer", prompt: "3f2a9c1d" }), null);
});

test("a command from a device the machine hasn't approved waits, says when it is approved automatically, and settles as not delivered or denied", () => {
  const device = { id: "dev_01JB8Q6Z3V9K2M4N5P6R7S8T9V", fingerprint: "7F3A-91C2" };
  const sent = answer({ device: device.id, deviceName: "iPhone 16", machineName: "MacBook", label: "Per invoice" });
  const board = { devices: { [device.id]: { state: "waiting", fingerprint: "7F3A-91C2", until: AT + 23 * 3600 + 120 } } };
  const standing = standingOn(board, device);
  assert.equal(standingOn(board, { ...device, fingerprint: "0000-0000" }), null, "a standing for another key is not this device's");
  assert.equal(sentState(sent, { now: AT + 60, asleep: false, standing }), "approval");
  const waiting = answerStatus(sent, "approval", { standing, now: AT + 60 });
  assert.match(waiting, /class="sent wait"/);
  assert.match(waiting, /<b>Waiting for MacBook to approve this iPhone 16<\/b>/);
  assert.match(waiting, /Approve it in kebbie, or it is approved automatically in 23\u00a0h\. Your answer "Per invoice" goes to the task if MacBook approves before \d\d:\d\d\./);
  assert.match(waiting, /Sent from iPhone 16 at \d\d:\d\d · expires \d\d:\d\d/);
  assert.match(answerStatus(sent, "approval", { standing: { ...standing, until: null }, now: AT }), /Approve it in kebbie on MacBook\./, "a machine that never approves on its own says so");
  assert.equal(sentState(sent, { now: AT + 3600, asleep: false, standing }), "not-approved");
  assert.equal(sentState({ ...sent, result: { outcome: "refused", reason: "not-approved", text: "" } }, { now: AT, asleep: false }), "not-approved");
  const expired = answerStatus(sent, "not-approved");
  assert.match(expired, /class="sent refused"/);
  assert.match(expired, /Not delivered: MacBook didn't approve this iPhone 16 within the hour/);
  assert.match(expired, /Approve it in kebbie on MacBook, then answer again\./);
  assert.equal(sentState({ ...sent, result: { outcome: "refused", reason: "denied", text: "" } }, { now: AT, asleep: false }), "denied");
  assert.equal(sentState(sent, { now: AT, asleep: false, standing: { state: "denied", fingerprint: "7F3A-91C2", at: AT } }), "denied");
  const denied = answerStatus(sent, "denied");
  assert.match(denied, /<b>MacBook doesn't take changes from this iPhone 16<\/b><span>Approve it in kebbie on MacBook\.<\/span>/);
  for (const state of ["not-approved", "denied"]) assert.match(answerBlock({ key: "k", question, ui: idle(), sent, state }), /data-action="pick"/, `${state} lets the owner answer again`);
  const done = answerStatus({ ...sent, result: { outcome: "done", text: "" } }, "done", { standing: { state: "approved", fingerprint: "7F3A-91C2", at: AT + 390 } });
  assert.match(done, /from iPhone 16 at \d\d:\d\d · approved on MacBook at \d\d:\d\d · the lead got a copy/);
  assert.match(pushStatus({ ...sent, kind: "push", short: "9f3c2a1" }, "approval", { standing, now: AT }), /Your push of 9f3c2a1 goes out if MacBook approves before/);
  assert.match(promptStatus({ ...sent, kind: "prompt-answer" }, "not-approved"), /then choose again\./);
  for (const html of [waiting, expired, denied, done]) assert.doesNotMatch(html, /\u2014/, "no dashes in the copy");
});


test("a push that went out for an earlier stage gives way to the next stage's Push once it has its result", () => {
  const sha = (c) => c.repeat(40);
  const stage = (id, state, extra = {}) => ({ id, title: id, state, requirement: { kind: "deploy" }, ...extra });
  const item = {
    address: "t@r", head: sha("c"), group: "WORKING", git: { ahead: 3 }, subtasks: [],
    order: [{ sha: sha("b"), subtask: "2", accepted: true, stage: "s2" }, { sha: sha("c"), subtask: "3", accepted: true, stage: "s3" }],
    pushable: { count: 1, through: sha("b"), subtasks: ["2"] },
    stages: [stage("s1", "met", { through: sha("a"), pushed_at: 1, met_at: 2 }), stage("s2", "open"), stage("s3", "locked")],
  };
  const ui = { confirming: false, busy: false, error: null };
  const pushed = { kind: "push", head: sha("a"), short: "aaaaaaa", at: 100, exp: 3700, machineName: "mac", result: { outcome: "done", text: "pushed" } };
  const forHead = (one) => one.head === item.pushable.through;
  const done = sentState(pushed, { now: 5000, asleep: false });
  assert.equal(speaksFor(pushed, done, forHead), null);
  assert.match(pushBlock({ item, machineName: "mac", ui, sent: null, state: null }), /data-action="push-ask">Push 1 commit · stage 2</);
  const going = { ...pushed, result: null, exp: 9000 };
  assert.equal(speaksFor(going, sentState(going, { now: 5000, asleep: false }), forHead), going, "a push still on its way keeps its status");
  const current = { ...pushed, head: sha("b") };
  assert.equal(speaksFor(current, done, forHead), current, "the push for the offered run still speaks for it");
});
