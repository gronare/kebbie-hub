import assert from "node:assert/strict";
import { test } from "node:test";

import { currentPlan, endingChip, machineNeedsPro, needsPro, planGroup, proCard, trialLine } from "./pro.js";

const NOW = 1_795_000_000;
const DAY = 86_400;
const PRO = ["push", "accept-subtask", "review-comments", "lead-message"];
const BILLING = "https://hub.kebbie.dev/v1/a/billing/checkout";
const plan = (state, extra = {}) => ({ state, pro: PRO, machines: state === "pro" || state === "trial" ? null : 1, trial_days: 14, billing: BILLING, ...extra });

test("only a free or ended plan asks before a Pro kind, and a hub without plans never asks", () => {
  assert.equal(needsPro(plan("free"), "push"), true);
  assert.equal(needsPro(plan("expired", { until: NOW - DAY, trial: true }), "lead-message"), true);
  assert.equal(needsPro(plan("free"), "answer"), false, "an answer is free");
  for (const state of ["trial", "pro"]) assert.equal(needsPro(plan(state, { until: NOW + DAY }), "push"), false);
  assert.equal(needsPro(null, "push"), false);
  assert.equal(machineNeedsPro(plan("free"), 0), false, "the first machine is free");
  assert.equal(machineNeedsPro(plan("free"), 1), true);
  assert.equal(machineNeedsPro(plan("trial", { until: NOW + DAY }), 5), false);
  assert.equal(machineNeedsPro(null, 3), false);
});

test("a trial whose end has passed reads as ended before the hub says so", () => {
  const lapsed = currentPlan(plan("trial", { until: NOW - 1 }), NOW);
  assert.deepEqual([lapsed.state, lapsed.trial, lapsed.until], ["expired", true, NOW - 1]);
  assert.equal(needsPro(lapsed, "push"), true);
  assert.equal(currentPlan(plan("trial", { until: NOW + 1 }), NOW).state, "trial");
});

test("the card is named after the action and offers the trial, with Not now beside it", () => {
  const html = proCard({ plan: plan("free"), kind: "push" });
  assert.match(html, /<h2 class="pro-title" id="pro-title">Push is part of Pro<\/h2>/);
  assert.match(html, /Push, accept and comment from your phone.*Start tasks and talk to the lead.*Every machine you run, 90 days of history/);
  assert.match(html, /Free for 14 days, then monthly or yearly\. Your push goes ahead as soon as the trial starts\./);
  assert.match(html, /<button type="button" class="btn fit" data-action="pro-close">Not now<\/button><button type="button" class="btn primary fit" data-action="pro-trial">Try Pro free for 14 days<\/button>/);
  assert.doesNotMatch(html, /ended on|Continue with Pro/);
  assert.match(proCard({ plan: plan("free"), kind: "review-comments" }), /Commenting on a review is part of Pro.*Your comments go ahead/);
  assert.match(proCard({ plan: plan("free"), kind: "push", busy: true }), /data-action="pro-trial" disabled>/);
  assert.match(proCard({ plan: plan("free"), kind: "push", error: "That is part of Pro." }), /role="alert">That is part of Pro\.<\/p>/);
});

test("after the trial the card says when it ended and opens the purchase in the browser, never in the app", () => {
  const html = proCard({ plan: plan("expired", { until: Date.UTC(2026, 10, 28, 12) / 1000, trial: true }), kind: "push" });
  assert.match(html, /<p class="pro-sub">Your Pro trial ended on 28 November\.<\/p>/);
  assert.match(html, /Pro is bought on hub\.kebbie\.dev in your browser, monthly or yearly\. Come back and push\./);
  assert.match(html, /<a class="btn primary fit" href="https:\/\/hub\.kebbie\.dev\/v1\/a\/billing\/checkout" target="_blank" rel="noopener" data-action="pro-continue">Continue with Pro<svg/);
  assert.doesNotMatch(html, /pro-trial/);
  assert.match(proCard({ plan: plan("expired", { until: NOW, trial: false }), kind: "push" }), /Your Pro plan ended on/);
});

test("the second machine has its own card that names the machine waiting to connect", () => {
  const html = proCard({ plan: plan("free"), kind: "machine", machine: "Devbox" });
  assert.match(html, /A second machine is part of Pro/);
  assert.match(html, /<li><svg[^]*?<span>Every machine you run, on one board<\/span>/);
  assert.match(html, /90 days of history instead of 7/);
  assert.match(html, /Devbox connects as soon as the trial starts\./);
  assert.match(proCard({ plan: plan("expired", { until: NOW, trial: true }), kind: "machine", machine: "Devbox" }), /Come back and connect Devbox\./);
});

test("the action that started the trial carries a quiet line with the days left", () => {
  assert.equal(trialLine({ kind: "push", until: NOW + 14 * DAY, now: NOW }), `<div class="trial-line"><span class="chip trial">Pro trial · 14 days left</span><span>Your trial started with this push.</span></div>`);
});

test("the board shows a Buy chip only in the trial's last three days", () => {
  assert.equal(endingChip(plan("trial", { until: NOW + 4 * DAY }), NOW), "");
  assert.equal(endingChip(plan("trial", { until: NOW + 3 * DAY }), NOW), `<a class="chip trial" href="${BILLING}" target="_blank" rel="noopener">Trial · 3 days left · Buy</a>`);
  assert.match(endingChip(plan("trial", { until: NOW + 3600 }), NOW), /Trial · 1 day left · Buy/);
  for (const state of ["free", "pro", "expired"]) assert.equal(endingChip(plan(state, { until: NOW + DAY }), NOW), "");
  assert.equal(endingChip(null, NOW), "");
});

test("Account's Plan group shows the days left and the end date, and Buy Pro towards the end", () => {
  const ending = Date.UTC(2026, 10, 28, 12) / 1000;
  const html = planGroup(plan("trial", { until: ending }), ending - 3 * DAY + 60);
  assert.match(html, /^<h2 class="group g-muted">Plan<\/h2><section class="task plan-row"><div class="head"><span class="name">Hub Pro<\/span><span class="chip trial">Trial · 3 days left<\/span><\/div><div class="meta">Ends 28 November\. After that the board stays readable, and pushing and the other Pro actions ask again\.<\/div><\/section>/);
  assert.match(html, /<a class="task row-link" href="https:\/\/hub\.kebbie\.dev\/v1\/a\/billing\/checkout" target="_blank" rel="noopener"><span class="name">Buy Pro<\/span><span class="meta">monthly or yearly<\/span><svg class="chev"/);
  assert.doesNotMatch(planGroup(plan("trial", { until: ending }), ending - 10 * DAY), /Buy Pro/, "early in the trial Account only counts down");
  assert.match(planGroup(plan("expired", { until: ending, trial: true }), ending + DAY), /Trial ended.*Buy Pro/);
  assert.match(planGroup(plan("free"), NOW), /<span class="chip">Free<\/span>.*first Pro action starts a 14-day trial/);
  assert.match(planGroup(plan("pro"), NOW), /<span class="chip">Pro<\/span>/);
  assert.equal(planGroup(null, NOW), "", "a hub without plans has no Plan group");
});
