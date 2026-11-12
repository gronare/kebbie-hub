import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import { MAX_REVIEW_COMMENT_CHARS, MAX_REVIEW_COMMENTS } from "../js/protocol.js";
import { acceptArea, cleanComment, commentProblem, commentsProblem, commentWhere, diffHref, fetchBlock, freshReviewUi, offerKey, opensOffer, readKey, readOf, reviewDiff, reviewEntry, reviewHref, reviewOverview, sendControl, settleReview } from "./review.js";

const text = (html) => html.replace(/<span[^>]*aria-hidden="true"[^>]*>[^<]*<\/span>/g, " ").replace(/<[^>]+>/g, " ").replace(/&#(\d+);/g, (_, code) => String.fromCharCode(Number(code))).replace(/\s+/g, " ").trim();
const top = (title, aside) => `<h1>${title}</h1><span>${aside}</span>`;
const NOW = 1_761_070_000;
const ALL = ["review-open", "review-comments", "review-read", "accept-step"];
const { review } = JSON.parse(readFileSync(new URL("../vectors/review.json", import.meta.url), "utf8"));
const item = { address: "payments@api", title: "Charge through the gateway", head: review.head, review: { step: 2, files: 3 }, steps: { pushed: 1, items: [{ number: 2, title: "Charge through a PaymentGateway", state: "committed", sha: review.head }, { number: 3, title: "Add the new gateway", state: "in-progress", sha: null }] } };
const view = (fields = {}) => ({ machine: "mch_a", name: "MacBook", address: item.address, task: "payments", title: item.title, item, accepts: ALL, review, fetch: { state: "fetching", sent: null }, ui: freshReviewUi(), sentComments: [], readSent: [], acceptSent: null, access: "ready", canComment: true, canRead: true, refusedUrl: null, now: NOW, ...fields });

test("the task card offers the review only when the machine offers it and takes review-open", () => {
  const found = { machine: "mch_a", item, accepts: ALL };
  assert.equal(reviewEntry(found), `<div class="thumb"><a class="btn primary" href="#/review/mch_a/payments%40api">Review step 2 · 3 files</a></div>`);
  assert.equal(reviewEntry({ ...found, accepts: ["answer", "push"] }), "");
  assert.equal(reviewEntry({ ...found, item: { ...item, review: undefined } }), "");
  assert.match(reviewEntry({ ...found, item: { ...item, review: { step: null, files: 1 } } }), />Review · 1 file</);
  assert.deepEqual([reviewHref("mch_a", "payments@api"), diffHref("mch_a", "payments@api", 2)], ["#/review/mch_a/payments%40api", "#/diff/mch_a/payments%40api/2"]);
});

test("the overview shows the guide, the steps with the one to accept marked, the commit message, the files and Accept in the flow", () => {
  const phone = reviewOverview(view(), { split: false, top });
  assert.match(text(phone), /Review MacBook Charge through the gateway · since main · ◆ 1 to accept Guide from Claude src\/gateway.rs/);
  assert.match(phone, /class="step s-committed rv-step rv-on">.*Charge through a PaymentGateway.*to accept/);
  assert.match(text(phone), /Charge through a PaymentGateway 9f3c2a1 Orders charge through the gateway trait/);
  assert.match(text(phone), /Files · 1 of 3 read 1 src\/gateway.rs \+3 −0 unread · new file 2 src\/orders.rs \+1 −1 ✓ read 3 assets\/logo.png/);
  assert.match(phone, /<a class="btn" href="#\/diff\/mch_a\/payments%40api">Read the diff · 3 files<\/a><\/div><div class="rv-end"><button type="button" class="btn primary" data-action="rv-accept-ask">Accept step 2 · 9f3c2a1<\/button><\/div>$/);
  assert.doesNotMatch(reviewOverview(view(), { split: true, top }), /Read the diff/);
  const confirming = view();
  confirming.ui.confirming = true;
  assert.match(text(acceptArea({ ...confirming, sentComments: [{ path: "src/orders.rs" }] })), /^Accept step 2 · 9f3c2a1\? Charge through a PaymentGateway 1 of 3 files read, 1 comment sent\. MacBook records it as a does in herdr\. Cancel Accept$/);
  assert.equal(acceptArea(view({ accepts: ["review-open"] })).includes("takes accepts only at the machine"), true);
  assert.equal(acceptArea(view({ access: "view-only" })), "");
  const accepted = { ...item, steps: { ...item.steps, items: [{ ...item.steps.items[0], state: "accepted" }, item.steps.items[1]] } };
  assert.match(text(acceptArea(view({ item: accepted }))), /✓ Step 2 is accepted\./);
  const sent = { sha: review.head, at: NOW, exp: NOW + 3600, deviceName: "iPhone", machineName: "MacBook", result: { outcome: "done" } };
  assert.match(text(acceptArea(view({ item: accepted, acceptSent: { sent, state: "done" } }))), /^✓ Accepted step 2 · 9f3c2a1 Step 3 is in progress\. herdr shows step 2 accepted too\. from iPhone at/);
});

test("before the diff arrives the page says it is fetching, or that the machine is asleep, and asks nothing of a device that can't send", () => {
  assert.match(text(fetchBlock(view({ review: null }))), /^&nbsp; Fetching the diff from MacBook… It comes sealed to your account and opens here\.$/);
  assert.match(text(fetchBlock(view({ review: null, fetch: { state: "asleep", sent: null, lastSeen: NOW - 7200 } }))), /^MacBook is asleep The diff comes from MacBook\. Review opens here when it wakes, while this page is open\. last seen/);
  assert.match(fetchBlock(view({ review: null, fetch: { state: "view-only", sent: null } })), /View only on this device/);
  assert.match(text(fetchBlock(view({ review: null, fetch: { state: "not-taken", sent: null } }))), /MacBook doesn't send its diffs to the hub/);
  const refused = { at: NOW, exp: NOW + 3600, deviceName: "iPhone", result: { outcome: "refused", reason: "already-settled", text: "Already settled." } };
  assert.match(fetchBlock(view({ review: null, fetch: { state: "rejected", sent: refused } })), /data-action="rv-retry"/);
  const waiting = reviewOverview(view({ review: null }), { split: true, top });
  assert.match(text(waiting), /Steps .* The guide and the files come with the diff\.$/);
});

test("the diff runs every file on one page, a read file folds away, and lines take comments only where the machine takes them", () => {
  const v = view();
  const page = reviewDiff(v, { inPanel: false, top });
  assert.equal((page.match(/class="rv-fsec"/g) ?? []).length, 3);
  assert.match(text(page), /^Diff MacBook Step 2 · 9f3c2a1 · 3 files · \+4 −1 · 1 of 3 read/);
  assert.match(page, /<header class="rv-fh is-read">.*aria-pressed="true" aria-label="Mark unread"/);
  assert.doesNotMatch(page.split('id="rv-f-1"')[1].split('id="rv-f-2"')[0], /rv-diff/, "a read file starts folded");
  assert.match(page, /data-action="rv-line" data-file="0" data-side="new" data-line="2"/);
  v.ui.open.set("src/orders.rs", true);
  assert.match(reviewDiff(v, { inPanel: true, top }), /data-action="rv-line" data-file="1" data-side="old" data-line="41"/);
  assert.match(text(page), /Not shown here: the diff is binary or too large\./);
  assert.match(text(page), /End of step 2\. Read marks are shared with herdr and hold until a file changes\.$/);
  assert.doesNotMatch(reviewDiff(view({ canComment: false }), { inPanel: false, top }), /data-action="rv-line"/);
  v.ui.read.set(readKey(review.files[0]), true);
  assert.equal(readOf(v, review.files[0]), true);
  assert.equal(readOf(v, { ...review.files[0], read: false, content: "0".repeat(40) }), false, "a local mark holds only for the content it was set on");
  assert.equal(readOf(view({ readSent: [{ path: "src/gateway.rs", content: review.files[0].content, read: true }] }), review.files[0]), true);
  assert.equal(readOf(view({ readSent: [{ path: "src/gateway.rs", content: "0".repeat(40), read: true }] }), review.files[0]), false, "a mark holds only for the content it was given on");
});

test("Send shows only once a comment waits, lists them with Show, Edit and Remove, and sent comments sit on their lines", () => {
  const v = view();
  assert.equal(sendControl(v), "");
  v.ui.drafts.push({ id: 1, path: "src/orders.rs", side: "old", line: 41, text: "Did anything else pass a float?" }, { id: 2, path: "src/gateway.rs", side: "new", line: 2, text: "Name the currency." });
  assert.match(text(sendControl(v)), /^Send 2 comments$/);
  v.ui.tray = true;
  assert.match(text(sendControl(v)), /^To send · 2 Close src\/orders.rs:41 \(before\) in 9f3c2a1 Did anything else pass a float\? Show Edit Remove src\/gateway.rs:2 in 9f3c2a1/);
  v.ui.receipt = { count: 2, at: NOW, device: "iPhone" };
  assert.match(text(sendControl(v)), /^✓ Sent 2 comments They reach payments's Claude in one message/);
  assert.equal(commentWhere({ path: "a.rs", line: 3, side: "new", sha: null }), "a.rs:3");
  const sent = view({ sentComments: [{ path: "src/orders.rs", side: "old", line: 41, text: "Did anything else pass a float?", at: NOW, state: "done" }] });
  sent.ui.open.set("src/orders.rs", true);
  assert.match(text(reviewDiff(sent, { inPanel: true, top })), /41 − client::charge\(self.total as f64\) ✓ sent to Claude at \d\d:\d\d Did anything else pass a float\?/);
});

test("the review opens again once the offered step moves, even when the task's head stays", () => {
  const opener = { head: review.head, step: 2 };
  assert.equal(opensOffer(item)(opener), true);
  const next = { ...item, review: { step: 3, files: 1 } };
  assert.equal(opensOffer(next)(opener), false, "accepting step 2 offers step 3 on the same head");
  assert.notEqual(offerKey(next), offerKey(item));
  assert.equal(opensOffer({ ...item, head: "0".repeat(40) })(opener), false);
});

test("comments written on one diff are never sent against another: a changed review sets them aside, named, with its read marks cleared", () => {
  const ui = freshReviewUi();
  settleReview(ui, review);
  ui.drafts.push({ id: 1, path: "src/orders.rs", side: "old", line: 41, text: "Why the cast?" });
  ui.read.set(readKey(review.files[0]), true);
  settleReview(ui, review);
  assert.equal(ui.drafts.length, 1, "the same review keeps its drafts");
  const amended = { ...review, step: { ...review.step, sha: "1".repeat(40) } };
  settleReview(ui, amended);
  assert.deepEqual([ui.drafts, ui.read.size], [[], 0]);
  assert.deepEqual(ui.stale.comments, [{ path: "src/orders.rs", side: "old", line: 41, text: "Why the cast?" }]);
  const page = reviewOverview(view({ review: amended, ui }), { split: false, top });
  assert.match(text(page), /Not sent: 1 comment on step 2 · 9f3c2a1 The diff changed since you wrote it, so it may point at other lines\. .* src\/orders.rs:41 \(before\) Why the cast\? Dismiss/);
  assert.match(text(reviewDiff(view({ review: amended, ui }), { inPanel: false, top })), /Not sent: 1 comment/);
});

test("a comment loses its control characters, and what is too long is named before anything is sealed", () => {
  assert.equal(cleanComment(" Clear it\u001b[2J\nand\tthis\u0007 "), "Clear it[2J\nand\tthis");
  assert.equal(commentProblem("x".repeat(MAX_REVIEW_COMMENT_CHARS)), null);
  assert.equal(commentProblem("x".repeat(MAX_REVIEW_COMMENT_CHARS + 1)), "This comment is 2,001 characters; a comment holds at most 2,000.");
  const comment = { path: "src/orders.rs", side: "old", line: 41, sha: null, text: "Why?" };
  const payload = (comments) => ({ address: item.address, step: 2, comments });
  assert.equal(commentsProblem(payload([comment])), null);
  assert.match(commentsProblem(payload(Array(MAX_REVIEW_COMMENTS + 1).fill(comment))), /^51 comments wait, and at most 50 go together\./);
  assert.match(commentsProblem(payload([{ ...comment, text: "x".repeat(MAX_REVIEW_COMMENT_CHARS + 1) }])), /^The comment on src\/orders.rs:41 \(before\) is too long: This comment is 2,001 characters/);
  assert.match(commentsProblem(payload(Array(5).fill({ ...comment, text: "€".repeat(MAX_REVIEW_COMMENT_CHARS) }))), /^These comments come to 30 kB, and at most 24 kB go together\./);
  const v = view();
  v.ui.drafts.push({ id: 1, ...comment });
  assert.equal(sendControl({ ...v, review: null }), "", "nothing is sent before the review it belongs to is open");
});
