import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import { arrange, asksYou, shownQuestion } from "./board.js";
import { answerBlock, kebbieAnswerStatus, pushBlock, pushStage } from "./decide.js";
import { isSettled } from "./notify.js";
import { pushOrder, stageLabel, stageLine, stageList, stepsWanted, subtaskLabel } from "./screens.js";
import { KEPT_ANSWER_TTL, pushableOf } from "../js/protocol.js";

const vectors = JSON.parse(readFileSync(new URL("../vectors/stages.json", import.meta.url), "utf8"));
const caseItem = (name) => structuredClone(vectors.cases.find((one) => one.name === name).item);
const staged = () => structuredClone(vectors.board.items[0]);
const text = (html) => html.replace(/<svg[\s\S]*?<\/svg>/g, " ").replace(/<[^>]+>/g, " ").replace(/&#(\d+);/g, (_, code) => String.fromCharCode(Number(code))).replace(/\s+/g, " ").trim();
const AT = vectors.board.items[0].kebbie_questions[0].at;
const time = (seconds) => new Date(seconds * 1000).toLocaleTimeString("en-GB", { hour: "2-digit", minute: "2-digit" });
const idle = () => ({ picked: null, free: false, note: "", confirming: false, busy: false, error: null });

test("each stage says where its requirement stands, and a locked one names the stage it waits for", () => {
  const item = staged();
  const [both, backfill] = item.stages;
  assert.deepEqual(item.stages.map((stage) => text(stageLabel(item, stage, AT))), [
    `pushed ${time(both.pushed_at)} · ✓ deploy succeeded`,
    `pushed ${time(backfill.pushed_at)} · waiting: verified by you`,
    "locked until stage 2 is verified · 2 commits",
  ]);
  const list = stageList(item, AT);
  assert.match(text(list), /^Stages · 3 ↑ 1 Add the new column, write to both/);
  assert.deepEqual([...list.matchAll(/class="step st-([a-z]+)"/g)].map((match) => match[1]), ["met", "waiting", "locked"]);
  const verified = caseItem("verified: the third stage opens and its accepted commits can go out");
  assert.equal(text(stageLabel(verified, verified.stages[2], AT)), "open · 2 commits to push");
  assert.equal(text(stageLabel(verified, verified.stages[1], AT)), `pushed ${time(backfill.pushed_at)} · ✓ verified by you`);
  const failed = caseItem("a stage that failed says why and keeps the next locked");
  assert.equal(text(stageLabel(failed, failed.stages[1], AT)), `pushed ${time(backfill.pushed_at)} · ✕ something is wrong`);
  assert.match(text(stageList(failed, AT)), /The backfill stopped at row 40 000: a null email/);
  const check = caseItem("a requirement checked by a command after the deploy");
  assert.equal(text(stageLabel(check, check.stages[0], AT)), `pushed ${time(both.pushed_at)} · ✓ check passed`);
  check.stages[0] = { ...check.stages[0], state: "waiting" };
  delete check.stages[0].met_at;
  assert.equal(text(stageLabel(check, check.stages[0], AT)), `pushed ${time(both.pushed_at)} · waiting: check: Every new row has an email in both columns`);
  assert.equal(stageList({ subtasks: [] }), "", "a task without stages shows none");
});

test("the board row and the you line say which stage is out and what it waits for", () => {
  assert.equal(text(stageLine(staged())), "stage 2 waits: verified by you");
  assert.equal(text(stageLine(caseItem("verified: the third stage opens and its accepted commits can go out"))), "✓ stage 2 done · stage 3 open");
  assert.equal(text(stageLine(caseItem("a stage that failed says why and keeps the next locked"))), "stage 2: ✕ something is wrong");
  assert.equal(stepsWanted(staged(), "mac"), "verify stage 2 on mac");
  assert.equal(stepsWanted(caseItem("verified: the third stage opens and its accepted commits can go out"), "mac"), "push 2 commits on mac");
  assert.match(stepsWanted(caseItem("a manual check and a close, without stages"), "mac"), /^verify a check or /);
});

test("the push order marks where each stage starts, and accepted commits behind a locked stage wait for the stage before it", () => {
  const item = staged();
  const order = text(pushOrder(item));
  assert.match(order, /^stage 3 · Read the new column, drop the old ✓ 1 Read the email from the new column Read the new column · waits for stage 2/);
  assert.equal(text(subtaskLabel(item, item.subtasks[2])), "accepted · waits for stage 2");
  const fix = caseItem("a fix in the failed stage goes out while the next stays locked");
  assert.match(text(pushOrder(fix)), /Ready to push: 1 commit .* stage 2 · Fill the old rows ✓ 1 Skip rows without an email .* stage 3 · Read the new column, drop the old ✓ 2/);
});

test("the push button goes only as far as the open stage and says which stage it pushes", () => {
  const item = caseItem("verified: the third stage opens and its accepted commits can go out");
  assert.deepEqual(pushableOf(item.order, item.stages), item.pushable);
  assert.equal(pushStage(item).number, 3);
  assert.deepEqual(text(pushBlock({ item, machineName: "mac", ui: idle(), sent: null, state: null })), "Push 2 commits · stage 3");
  const fix = caseItem("a fix in the failed stage goes out while the next stays locked");
  const asking = text(pushBlock({ item: fix, machineName: "mac", ui: { ...idle(), confirming: true }, sent: null, state: null }));
  assert.match(asking, /1 commit of stage 2, Fill the old rows; stage 3 stays locked until this one is verified\./);
  assert.equal(pushStage(staged()), null, "nothing is pushable while stage 3 is locked");
});

test("kebbie's question asks you without the task asking, and the questions tab and notices count it", () => {
  const item = staged();
  assert.equal(asksYou(item), true);
  assert.equal(shownQuestion(item).id, "verify-backfill");
  item.question = { id: "q1", text: "Keep the old column's index?", options: [], recommend: null, kept_for_user: false, plan: null, at: AT };
  assert.equal(shownQuestion(item).id, "q1", "the task's own question comes first");
  const board = { ...vectors.board, items: [staged()] };
  const opened = [{ machine: "mch_1", name: "mac", board, lastSeen: AT, problem: null, expired: false }];
  assert.equal(arrange(opened, AT).questions.length, 1);
  const data = { category: "questions", machine: "mch_1", open: { to: "task", address: "email-column@api" } };
  assert.equal(isSettled(data, opened), false);
  const answered = [{ ...opened[0], board: { ...board, items: [caseItem("verified: the third stage opens and its accepted commits can go out")] } }];
  assert.equal(isSettled(data, answered), true);
});

test("the verify card offers its three answers, and an answer to it is kept for a week while the machine sleeps", () => {
  const question = staged().kebbie_questions[0];
  const html = answerBlock({ key: "k", question, ui: idle(), sent: null, state: null, describe: kebbieAnswerStatus });
  assert.equal(text(html), "Verified Stage 3 unlocks and can be pushed Not yet Stage 3 stays locked and this question waits Something is wrong Tell the task what you see; stage 3 stays locked Write your own answer");
  assert.doesNotMatch(html, /Recommended/);
  const wrong = answerBlock({ key: "k", question, ui: { ...idle(), picked: 2 }, sent: null, state: null, describe: kebbieAnswerStatus });
  assert.match(text(wrong), /Note for the agent \(needed for this answer\)/);
  const sent = { id: "cmd_1", kind: "answer", machineName: "mac", address: "email-column@api", question: question.id, choice: 0, label: "Verified", at: AT, exp: AT + KEPT_ANSWER_TTL, result: null };
  const day = new Date((AT + KEPT_ANSWER_TTL) * 1000).toLocaleDateString("en-GB", { weekday: "short", day: "numeric", month: "short" });
  assert.match(text(kebbieAnswerStatus(sent, "waiting")), new RegExp(`^Waiting for mac The hub keeps your answer "Verified" until mac wakes up\\. Sent from this device at .* · kept until ${day}$`));
  assert.match(text(kebbieAnswerStatus(sent, "expired")), /^Not delivered: mac stayed asleep for a week/);
  const done = { ...sent, result: { outcome: "done", reason: null, text: "Verified Fill the old rows of email-column@api." } };
  assert.match(text(kebbieAnswerStatus(done, "done")), /^✓ Answered: Verified Verified Fill the old rows of email-column@api\./);
});
