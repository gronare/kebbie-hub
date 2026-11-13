import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import { MAX_REVIEW_COMMENT_CHARS, MAX_REVIEW_COMMENTS } from "../js/protocol.js";
import { againOf, cleanComment, foldCommitsOn, commentProblem, commentsProblem, commentWhere, commitPage, diffHref, fetchBlock, filesOf, freshReviewUi, readKey, readOf, reviewEntry, sendControl, settleReview, shaOf, stepEnd, stepHref, stepPage, wholePage } from "./review.js";

const text = (html) => html.replace(/<span[^>]*aria-hidden="true"[^>]*>[^<]*<\/span>/g, " ").replace(/<[^>]+>/g, " ").replace(/&#(\d+);/g, (_, code) => String.fromCharCode(Number(code))).replace(/\s+/g, " ").trim();
const top = (title, aside) => `<h1>${title}</h1><span>${aside}</span>`;
const NOW = 1_761_070_000;
const ALL = ["review-open", "review-comments", "review-read", "accept-step", "push"];
const { review } = JSON.parse(readFileSync(new URL("../vectors/review.json", import.meta.url), "utf8"));
const [first, last] = review.step.commits;
const boardStep = { number: 2, title: "Charge through a PaymentGateway", state: "ready", commits: [{ sha: first.sha, subject: first.subject }, { sha: last.sha, subject: last.subject }] };
const building = { number: 3, title: "Add the new gateway", state: "in-progress", commits: [] };
const item = { address: "payments@api", title: "Charge through the gateway", head: review.head, steps: { pushed: 1, items: [boardStep, building] } };
const page = (kind = "step", commit = null) => ({ step: 2, kind, commit });
const view = (fields = {}) => ({ machine: "mch_a", name: "MacBook", address: item.address, task: "payments", title: item.title, item, accepts: ALL, step: 2, page: page(), boardStep, review, fetch: { state: "fetching", sent: null }, ui: freshReviewUi(), sentComments: [], readSent: [], acceptSent: null, stepPush: null, taskPush: null, pushUi: { confirming: false, busy: false, error: null }, access: "ready", canComment: true, canRead: true, refusedUrl: null, now: NOW, ...fields });
const allRead = (fields = {}) => {
  const v = view(fields);
  for (const file of [...review.step.files, ...review.step.commits.flatMap((commit) => commit.files)]) v.ui.read.set(readKey(file), true);
  return v;
};
const withState = (state, more = {}) => ({ ...item, ...more, steps: { ...item.steps, items: [{ ...boardStep, state }, building] } });

test("a step opens by its number, its commits each read on their own page, and the whole step on one", () => {
  assert.deepEqual([stepHref("mch_a", "payments@api", 2), diffHref("mch_a", "payments@api", 2, 1), diffHref("mch_a", "payments@api", 2), diffHref("mch_a", "payments@api")], ["#/review/mch_a/payments%40api/2", "#/diff/mch_a/payments%40api/2/1", "#/diff/mch_a/payments%40api/2/all", "#/diff/mch_a/payments%40api"]);
  assert.deepEqual([filesOf(view()), filesOf(view({ page: page("commit", 0) })), filesOf(view({ page: page("whole") }))], [review.step.files, first.files, review.step.files]);
  assert.deepEqual([shaOf(view()), shaOf(view({ page: page("commit", 0) })), shaOf(view({ page: page("commit", 1) }))], [last.sha, first.sha, last.sha]);
});

test("the step page names the step, folds each commit to its subject and sha, and lists the step's files without their diffs", () => {
  const html = stepPage(view(), { top });
  assert.match(text(html), /^Step 2 MacBook Charge through a PaymentGateway 2 commits · 3 files · \+4 −1 Guide from Claude · 2 notes/);
  assert.match(text(html), /Commits · 2 Add the PaymentGateway trait a41c9e0 1 file · ✓ 1 of 1 read Charge orders through the gateway in cents 9f3c2a1 Read the diff · 3 files 1 of 3 read Files of the step · 1 of 3 read/);
  assert.match(html, /<div class="cm shut"><div class="cm-h" role="button" tabindex="0" data-action="rv-commit" data-commit="0" aria-expanded="false">/);
  assert.match(html, /<a class="btn fit" href="#\/diff\/mch_a\/payments%40api\/2\/2">Read the diff · 3 files<\/a>/);
  assert.doesNotMatch(html, /rv-diff|rv-line|A float drops cents/, "neither diffs nor the folded messages show");
  assert.match(html, /<a class="btn fit" href="#\/diff\/mch_a\/payments%40api\/2\/all">Read the whole step · 3 files<\/a>/);
  assert.match(html, /<a class="rv-fh-name" href="#\/diff\/mch_a\/payments%40api\/2\/all" data-open-file="0">/);
  assert.doesNotMatch(html, /rv-end|Accept step/, "nothing to accept while a file is unread");
  const opened = view();
  opened.ui.commits.add(last.sha);
  assert.match(text(stepPage(opened, { top })), /9f3c2a1 A float drops cents on large orders; the gateway takes whole cents\. Read the diff/);
});

test("a step of one commit shows that commit as its subject and sha alone, and reads its diff from the bottom", () => {
  const one = { ...review, step: { ...review.step, commits: [{ ...last, files: review.step.files }] } };
  const html = stepPage(view({ review: one }), { top });
  assert.match(text(html), /Commits · 1 Charge orders through the gateway in cents 9f3c2a1 Files of the step · 1 of 3 read/);
  assert.match(html, /<div class="cm one shut"><div class="cm-h" role="button" tabindex="0" data-action="rv-commit" data-commit="0" aria-expanded="false">/);
  assert.doesNotMatch(html, /cm-foot|of 1 read|Read the whole step|A float drops cents/);
  assert.match(html, /<div class="whole"><a class="btn fit" href="#\/diff\/mch_a\/payments%40api\/2\/all">Read the diff · 3 files<\/a><\/div>/);
  assert.match(stepPage(view(), { top }), />Read the whole step · 3 files<\/a>/, "a step of two commits keeps its own reading");
});

test("a commit message unfolded on one page starts folded on the next, so a message is folded wherever a page opens", () => {
  const ui = freshReviewUi();
  foldCommitsOn(ui, "#/diff/mch_a/payments%40api/2/2");
  ui.commits.add(last.sha);
  assert.match(text(commitPage(view({ ui, page: page("commit", 1) }), { top })), /A float drops cents/);
  foldCommitsOn(ui, "#/diff/mch_a/payments%40api/2/2");
  assert.equal(ui.commits.has(last.sha), true, "a render of the same page keeps what was unfolded");
  foldCommitsOn(ui, "#/review/mch_a/payments%40api/2");
  assert.doesNotMatch(stepPage(view({ ui }), { top }), /A float drops cents|aria-expanded="true"/);
  foldCommitsOn(ui, "#/diff/mch_a/payments%40api/2/2");
  assert.doesNotMatch(commitPage(view({ ui, page: page("commit", 1) }), { top }), /A float drops cents/, "coming back to a page opens it folded");
});

test("a file read in one commit counts for the step, unless a later commit changed it again", () => {
  const v = view();
  const [gateway, orders] = review.step.files;
  assert.equal(readOf(v, gateway), false);
  assert.deepEqual(againOf(v, gateway), { read: first.sha, again: last.sha });
  assert.match(text(stepPage(v, { top })), /src\/gateway\.rs \+3 −0 · read in a41c9e0 · changed again in 9f3c2a1 · new file/);
  assert.equal(readOf(v, orders), true);
  v.ui.read.set(readKey(last.files[0]), true);
  assert.equal(readOf(v, gateway), true, "read at the step's last content is read for the step");
  assert.equal(againOf(view({ ui: Object.assign(freshReviewUi(), { read: new Map([[readKey(first.files[0]), false]]) }) }), gateway), null);
  assert.equal(readOf(view({ readSent: [{ path: "src/gateway.rs", content: gateway.content, read: true }] }), gateway), true);
  assert.equal(readOf(view({ readSent: [{ path: "src/gateway.rs", content: "0".repeat(40), read: true }] }), gateway), false, "a mark holds only for the content it was given on");
});

test("the commit page shows that commit's message folded, its facts and only its diff, then the next commit", () => {
  const html = commitPage(view({ page: page("commit", 0) }), { top });
  assert.match(text(html), /^Commit 1 of 2 MacBook Add the PaymentGateway trait a41c9e0 Step 2 · a41c9e0 · 1 file · \+3 −0 · 1 of 1 read/);
  assert.equal((html.match(/class="rv-fsec"/g) ?? []).length, 1);
  assert.match(text(html), /End of commit 1\. Next: commit 2 · 3 files Back to the step$/);
  assert.match(html, /<a class="btn primary" href="#\/diff\/mch_a\/payments%40api\/2\/2">Next: commit 2 · 3 files<\/a>/);
  const final = commitPage(view({ page: page("commit", 1) }), { top });
  assert.match(text(final), /^Commit 2 of 2 .* Step 2 · 9f3c2a1 · 3 files · \+2 −2 · 1 of 3 read/);
  assert.match(final, /<a class="btn primary" href="#\/review\/mch_a\/payments%40api\/2">Back to the step<\/a>/);
  assert.match(text(final), /Not shown here: the diff is binary or too large\./);
  assert.match(final, /data-action="rv-line" data-file="0" data-side="new" data-line="2"/);
  assert.doesNotMatch(commitPage(view({ page: page("commit", 1), canComment: false }), { top }), /data-action="rv-line"/);
  assert.match(text(commitPage(view({ page: page("commit", 4) }), { top })), /Step 2 has no commit 5\./);
});

test("the whole step reads every file on one page and ends on the step's next action", () => {
  const html = wholePage(view({ page: page("whole") }), { top });
  assert.match(text(html), /^Step 2 MacBook The whole step · 2 commits · 3 files · \+4 −1 · 1 of 3 read/);
  assert.match(html, /<header class="rv-fh is-read">.*aria-pressed="true" aria-label="Mark unread"/);
  assert.match(text(html), /End of step 2\.$/, "nothing follows the end while a file is unread");
  assert.match(text(wholePage(allRead({ page: page("whole") }), { top })), /End of step 2\. Accept step 2$/);
  const branch = view({ step: null, page: { step: null, kind: "whole", commit: null }, review: { ...review, step: null, files: review.step.files }, boardStep: null });
  assert.match(text(wholePage(branch, { top })), /^Diff MacBook Since main · 3 files · \+4 −1 · 1 of 3 read Guide from Claude/);
});

test("before the diff arrives the page says it is fetching, asleep or not ready, and asks nothing of a device that can't send", () => {
  assert.match(text(fetchBlock(view({ review: null }))), /^&nbsp; Fetching the diff from MacBook… It comes sealed to your account and opens here\.$/);
  assert.match(text(fetchBlock(view({ review: null, fetch: { state: "asleep", sent: null, lastSeen: NOW - 7200 } }))), /^MacBook is asleep The diff comes from MacBook\./);
  assert.match(fetchBlock(view({ review: null, fetch: { state: "view-only", sent: null } })), /View only on this device/);
  assert.equal(text(fetchBlock(view({ review: null, fetch: { state: "not-ready", sent: null } }))), "Step 2 isn't ready to review yet.");
  assert.match(text(stepPage(view({ review: null }), { top })), /^Step 2 MacBook Charge through a PaymentGateway &nbsp; Fetching the diff/, "the step's title shows while the diff comes");
  assert.equal(text(stepPage(view({ review: null }), { top, left: true })), "Step 2 MacBook Charge through a PaymentGateway");
});

test("side by side, the left column holds the step with the open commit marked and no actions", () => {
  const left = stepPage(allRead(), { top, left: true, selected: 0 });
  assert.match(left, /<div class="cm shut on">/);
  assert.doesNotMatch(left, /Read the whole step|rv-end|Accept/);
});

test("a fully read step offers Accept in the thumb zone, and a comment waiting on Claude holds it", () => {
  assert.equal(text(stepEnd(allRead())), "Accept step 2");
  assert.match(stepPage(allRead(), { top }), /<div class="rv-end"><button type="button" class="btn primary" data-action="rv-accept">Accept step 2<\/button><\/div>$/);
  assert.equal(text(stepEnd(allRead({ sentComments: [{ path: "src/orders.rs", state: "done" }] }))), "Waiting for Claude's changes");
  assert.equal(text(stepEnd(allRead({ sentComments: [{ path: "src/orders.rs", state: "refused" }] }))), "Accept step 2", "a comment that never arrived waits on nothing");
  assert.match(text(stepEnd(allRead({ accepts: ["review-open"] }))), /MacBook takes accepts only at the machine\. Accept step 2 in kebbie there\./);
  assert.equal(stepEnd(allRead({ access: "view-only" })), "");
});

test("after the accept come the receipt, Push step N, the next step and the way back to the task", () => {
  const sent = { head: last.sha, step: 2, at: NOW, exp: NOW + 3600, deviceName: "iPhone", machineName: "MacBook", result: { outcome: "done" } };
  const ready = { number: 3, title: "Add the new gateway", state: "ready", commits: [{ sha: "b".repeat(40), subject: "Add it" }] };
  const next = { ...item, steps: { ...item.steps, items: [boardStep, ready] } };
  const done = stepEnd(allRead({ item: next, acceptSent: { sent, state: "done" } }));
  assert.match(text(done), /^✓ Accepted step 2 Push it now, or read step 3 first and push both later\. from iPhone at \d\d:\d\d Push step 2 Next: step 3 · 1 commit Back to the task$/);
  assert.match(done, /<div class="row-btns"><div class="decide" data-machine="mch_a" data-address="payments@api" data-step="2"><button type="button" class="btn primary" data-action="push-ask">Push step 2<\/button><\/div><a class="btn" href="#\/review\/mch_a\/payments%40api\/3">Next: step 3 · 1 commit<\/a><\/div><a class="link" href="#\/task\/mch_a\/payments%40api">Back to the task<\/a>/);
  assert.equal(text(stepEnd(view({ item: withState("accepted"), boardStep: { ...boardStep, state: "accepted" } }))), "Push step 2 Step 3 is being built Back to the task", "an accepted step offers its push even before it is read again");
  const confirming = stepEnd(view({ item: withState("accepted"), boardStep: { ...boardStep, state: "accepted" }, pushUi: { confirming: true, busy: false, error: null } }));
  assert.match(text(confirming), /^Push step 2 · 9f3c2a1 to main\? The task's commits up to and including step 2 go out/);
  assert.match(confirming, /data-action="push-step"/);
  const pushed = { head: last.sha, short: "9f3c2a1", step: 2, machineName: "MacBook", deviceName: "iPhone", at: NOW, exp: NOW + 3600, result: { outcome: "done", text: "Pushed 2 commits." } };
  assert.match(text(stepEnd(view({ item: withState("accepted"), boardStep: { ...boardStep, state: "accepted" }, stepPush: { sent: pushed, state: "done" } }))), /^✓ Pushed 9f3c2a1 to main Pushed 2 commits\. from iPhone at \d\d:\d\d Step 3 is being built Back to the task$/);
  assert.equal(text(stepEnd(view({ item: withState("pushed"), boardStep: { ...boardStep, state: "pushed" } }))), "↑ Step 2 is pushed. Step 3 is being built Back to the task");
  assert.match(text(stepEnd(view({ item: withState("accepted"), boardStep: { ...boardStep, state: "accepted" }, accepts: ["review-open"] }))), /MacBook takes pushes only at the machine\. Push step 2 in kebbie there\./);
});

test("the last step, accepted, offers the task's own push", () => {
  const lastStep = { ...boardStep, state: "accepted" };
  const ready = { ...item, group: "READY TO PUSH", git: { upstream: "main", ahead: 2, behind: 0, uncommitted: 0 }, steps: { pushed: 1, items: [lastStep] } };
  assert.match(stepEnd(view({ item: ready, boardStep: lastStep })), /data-action="push-ask">Push 2 commits · 9f3c2a1<\/button>/);
});

test("Send shows only once a comment waits, names each comment's commit, and sent comments sit on the lines of the page they were written on", () => {
  const v = view({ page: page("commit", 1) });
  assert.equal(sendControl(v), "");
  v.ui.drafts.push({ id: 1, path: "src/orders.rs", side: "old", line: 41, sha: last.sha, text: "Did anything else pass a float?" }, { id: 2, path: "src/gateway.rs", side: "new", line: 2, sha: first.sha, text: "Name the currency." });
  assert.match(text(sendControl(v)), /^Send 2 comments$/);
  v.ui.tray = true;
  assert.match(text(sendControl(v)), /^To send · 2 Close src\/orders.rs:41 \(before\) in 9f3c2a1 Did anything else pass a float\? Show Edit Remove src\/gateway.rs:2 in a41c9e0/);
  v.ui.receipt = { count: 2, at: NOW, device: "iPhone" };
  assert.match(text(sendControl(v)), /^✓ Sent 2 comments They reach payments's Claude in one message/);
  assert.equal(commentWhere({ path: "a.rs", line: 3, side: "new", sha: null }), "a.rs:3");
  const said = { path: "src/orders.rs", side: "old", line: 41, sha: last.sha, text: "Did anything else pass a float?", at: NOW, state: "done" };
  const sent = view({ page: page("commit", 1), sentComments: [said] });
  sent.ui.open.set("src/orders.rs", true);
  assert.match(text(commitPage(sent, { top })), /41 − client::charge\(self.total as f64\) ✓ sent to Claude at \d\d:\d\d Did anything else pass a float\?/);
  const elsewhere = view({ page: page("commit", 1), sentComments: [{ ...said, sha: first.sha }] });
  elsewhere.ui.open.set("src/orders.rs", true);
  assert.doesNotMatch(text(commitPage(elsewhere, { top })), /sent to Claude/, "a comment written on another commit stays there");
});

test("comments written on one diff are never sent against another: a changed review sets them aside, named, with its read marks cleared", () => {
  const ui = freshReviewUi();
  settleReview(ui, review);
  ui.drafts.push({ id: 1, path: "src/orders.rs", side: "old", line: 41, sha: last.sha, text: "Why the cast?" });
  ui.read.set(readKey(review.step.files[0]), true);
  settleReview(ui, review);
  assert.equal(ui.drafts.length, 1, "the same review keeps its drafts");
  const amended = { ...review, head: "1".repeat(40), step: { ...review.step, commits: [first, { ...last, sha: "1".repeat(40) }] } };
  settleReview(ui, amended);
  assert.deepEqual([ui.drafts, ui.read.size], [[], 0]);
  assert.deepEqual(ui.stale.comments, [{ path: "src/orders.rs", side: "old", line: 41, text: "Why the cast?" }]);
  assert.match(text(stepPage(view({ review: amended, ui }), { top })), /Not sent: 1 comment on step 2 · 9f3c2a1 The diff changed since you wrote it, so it may point at other lines\. .* src\/orders.rs:41 \(before\) Why the cast\? Dismiss/);
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

test("the task card offers a Review button only for a task without steps", () => {
  const found = { machine: "mch_a", item: { ...item, review: { step: 2, files: 3 } }, accepts: ALL };
  assert.equal(reviewEntry(found), "", "steps are the way in");
  const plain = { ...found, item: { ...item, steps: undefined, review: { step: null, files: 1 } } };
  assert.equal(reviewEntry(plain), `<div class="lead-act"><a class="btn primary fit" href="#/diff/mch_a/payments%40api">Review · 1 file</a></div>`);
  assert.equal(reviewEntry({ ...plain, accepts: ["answer"] }), "");
});
