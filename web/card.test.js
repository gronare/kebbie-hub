import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import { assessmentBlock, cardChanges, cardParts, changedNotice, inline, noticedArea } from "./card.js";
import { assessmentText, reviewLines } from "./screens.js";

const cards = JSON.parse(readFileSync(new URL("../vectors/cards.json", import.meta.url), "utf8"));
const card = cards.card;
const item = cards.board.items[0];
const ALL = ["found-add", "found-start", "found-keep", "review-send"];
const NOW = card.changed_at + 60;
const options = (more = {}) => ({ access: "ready", accepts: ALL, now: NOW, ...more });

test("the card shows what was built, each decision with who took it, and what was noticed", () => {
  const html = cardParts(card, { noticed: (line) => noticedArea(line, options()) });
  assert.match(html, /<h3 class="sec">Built<\/h3>/);
  assert.match(html, /See it: <code>pay refund --dry-run 1042<\/code> prints the rounded amount\./);
  assert.match(html, /<h3 class="sec">Decisions · 5<\/h3>/);
  assert.deepEqual([...html.matchAll(/<span class="who who-([a-z]+)">([a-z]+)<\/span>/g)].map((match) => match[2]), ["lead", "you", "investigator", "you", "task"]);
  assert.match(html, /<span class="who who-investigator">investigator<\/span> <span class="meta">obstacle · /);
  assert.match(html, /<span class="who who-you">you<\/span> <span class="meta">approved with the plan · /);
  assert.match(html, /<div class="dec replaced"><div class="dec-text">Round VAT per line<\/div>.*replaced by: Round VAT per invoice, as the accountant books it/s);
  assert.match(html, /<h3 class="sec">Also noticed · 2<\/h3>/);
  assert.doesNotMatch(html, /confirm/i);
});

test("a filed noticed line offers Add as a subtask, New task and Keep in backlog, and only what the machine takes", () => {
  const [open, kept] = card.noticed;
  const html = noticedArea(open, options());
  assert.deepEqual([...html.matchAll(/data-action="([a-z-]+)" data-item="found:f3">([^<]+)</g)].map((match) => [match[1], match[2]]), [["card-subtask", "Add as a subtask"], ["card-task", "New task"], ["card-keep", "Keep in backlog"]]);
  assert.deepEqual([...noticedArea(open, options({ accepts: ["found-keep"] })).matchAll(/data-action="([a-z-]+)"/g)].map((match) => match[1]), ["card-keep"]);
  assert.doesNotMatch(noticedArea(open, options({ access: "view-only" })), /<button/);
  assert.equal(noticedArea({ text: "Not filed" }, options()), "", "a line not filed has nothing to press");
  assert.match(noticedArea(kept, options()), /Kept in the backlog/);
  assert.doesNotMatch(noticedArea(kept, options()), /<button/, "a line already chosen offers nothing more");
  assert.match(noticedArea(open, options({ recurring: 2 })), /seen by 2 tasks/);
  assert.doesNotMatch(noticedArea(open, options({ recurring: 1 })), /seen by/);
});

test("a choice on its way shows its state instead of the buttons, and a refused one offers them again", () => {
  const line = card.noticed[0];
  const sent = { kind: "found-keep", machineName: "MacBook", deviceName: "iPhone", at: NOW - 10, exp: NOW + 3590, result: null };
  const waiting = noticedArea(line, options({ sent: { sent, state: "sending" } }));
  assert.match(waiting, /It stays in the backlog/);
  assert.doesNotMatch(waiting, /<button/);
  const refused = noticedArea(line, options({ sent: { sent: { ...sent, result: { outcome: "refused", reason: "unknown-item", text: "Not in the backlog." } }, state: "refused" } }));
  assert.match(refused, /data-action="card-keep"/);
});

test("a changed card says when and what changed until you have seen this version", () => {
  assert.deepEqual(cardChanges(card).map((one) => [one.part, one.mark]), [["Built", "−"], ["Built", "+"], ["Built", "+"], ["Decisions", "+"], ["Decisions", "+"], ["Decisions", "+"], ["Decisions", "+"], ["Also noticed", "+"]]);
  const notice = changedNotice(card, 0);
  assert.match(notice, /The card changed at \d\d:\d\d · version 1 → 2/);
  assert.match(notice, /<span>Built: 2 new, 1 gone · Decisions: 4 new · Also noticed: 1 new<\/span><details class="card-diff"><summary>What changed<\/summary>/);
  assert.match(notice, /<div class="change removed"><span class="mark">−<\/span><span><small>Built<\/small> Payments go through the gateway; a refund rounds per invoice\.<\/span><\/div>/);
  assert.match(notice, /data-action="card-seen" data-version="2"/);
  assert.equal(changedNotice(card, 2), "", "seen");
  assert.equal(changedNotice({ ...card, previous: undefined }, 0), "", "a first card has nothing to compare");
});

test("a recommended full review says why and offers Send to review until the review runs", () => {
  const status = () => "<i>status</i>";
  const html = assessmentBlock(item, options({ status }));
  assert.match(html, /<b>A full review is recommended<\/b><span>Changes how refunds are rounded and booked, in 4 commits<\/span><div class="meta">assessed \d\d:\d\d<\/div>/);
  assert.match(html, /data-action="review-send">Send to review</);
  assert.doesNotMatch(assessmentBlock(item, options({ accepts: [] })), /<button/);
  assert.match(assessmentBlock({ ...item, verdict: { verdict: null, findings: null, sha: null, stale: false, running: true } }, options()), /The full review is running/);
  assert.doesNotMatch(assessmentBlock({ ...item, verdict: { running: true, stale: false } }, options()), /<button/);
  const sent = { kind: "review-send", at: NOW, exp: NOW + 3600, result: null };
  assert.doesNotMatch(assessmentBlock(item, options({ sent: { sent, state: "sending" }, status })), /<button/);
  assert.equal(assessmentBlock({ ...item, assessment: "review suggested: touches money" }, options()), "", "a machine not yet updated keeps its one line in Review");
  assert.equal(assessmentBlock({ ...item, assessment: { word: "not-needed" } }, options()), "");
});

test("an assessment reads as one line in a row and in Review, whichever machine wrote it", () => {
  assert.equal(assessmentText("review suggested: touches money"), "review suggested: touches money");
  assert.equal(assessmentText({ word: "not-needed" }), "no full review needed");
  assert.equal(assessmentText({ word: "assessing" }), "assessing the review");
  assert.match(reviewLines({ assessment: { word: "not-needed" } }), /no full review needed/);
  assert.equal(reviewLines({ assessment: item.assessment }), "", "a recommendation has its own box");
});

test("inline code in a card line is code and nothing else is markup", () => {
  assert.equal(inline("Run `a<b` <i>now</i>"), "Run <code>a&#60;b</code> &#60;i&#62;now&#60;/i&#62;");
});

test("while a full review is recommended the push is the second action, and the main one again once the review runs", async () => {
  const { pushBlock } = await import("./decide.js");
  const ui = { confirming: false, busy: false, error: null };
  const pushOf = (shown) => pushBlock({ item: shown, machineName: "MacBook", ui, sent: null, state: null }).match(/class="([^"]+)" data-action="push-ask"/)[1];
  assert.equal(pushOf(item), "btn");
  assert.equal(pushOf({ ...item, verdict: { running: true, stale: false } }), "btn primary");
  assert.equal(pushOf({ ...item, assessment: { word: "not-needed" } }), "btn primary");
});

test("a review sent for an earlier recommendation gives way to Send to review for a newer one", () => {
  const status = () => "<i>status</i>";
  const at = item.assessment.at;
  const done = { kind: "review-send", at: NOW, exp: NOW + 3600, assessed: at, result: { outcome: "done" } };
  assert.doesNotMatch(assessmentBlock(item, options({ sent: { sent: done, state: "done" }, status })), /<button/, "the recommendation it answered");
  const newer = { ...item, assessment: { ...item.assessment, at: at + 600 } };
  const html = assessmentBlock(newer, options({ sent: { sent: done, state: "done" }, status }));
  assert.match(html, /data-action="review-send">Send to review</);
  assert.doesNotMatch(html, /<i>status<\/i>/);
  const going = { ...done, result: null };
  assert.doesNotMatch(assessmentBlock(newer, options({ sent: { sent: going, state: "sending" }, status })), /<button/, "one still on its way holds the button");
});
