import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import { MAX_REVIEW_COMMENT_CHARS, MAX_REVIEW_COMMENTS } from "../js/protocol.js";
import { againOf, cleanComment, foldCommitsOn, commentProblem, commentsProblem, commentWhere, commitPage, diffHref, fetchBlock, filesOf, freshReviewUi, keptTaskView, readKey, readOf, reviewEntry, reviewPage, sendControl, settleReview, shaOf, subtaskEnd, subtaskHref, subtaskPage, taskHref, taskViewOf, wholePage } from "./review.js";

const text = (html) => html.replace(/<span[^>]*aria-hidden="true"[^>]*>[^<]*<\/span>/g, " ").replace(/<[^>]+>/g, " ").replace(/&#(\d+);/g, (_, code) => String.fromCharCode(Number(code))).replace(/\s+/g, " ").trim();
const top = (title, aside) => `<h1>${title}</h1><span>${aside}</span>`;
const NOW = 1_761_070_000;
const ALL = ["review-open", "review-comments", "review-read", "accept-subtask", "push"];
const load = (name) => JSON.parse(readFileSync(new URL(`../vectors/${name}`, import.meta.url), "utf8"));
const { review } = load("review.json");
const planned = load("envelopes.json").cases.find((vector) => vector.name === "board-with-subtasks-opened-by-the-account").expect.payload.items[0];
const [first, last] = review.subtask.commits;
const ID = review.subtask.id;
const item = { ...planned, title: "Charge through the gateway", subtasks: planned.subtasks.filter((one) => one.id !== "remove-old") };
const boardSubtask = item.subtasks.find((one) => one.id === ID);
const page = (kind = "subtask", commit = null) => ({ subtask: ID, kind, commit });
const view = (fields = {}) => ({ machine: "mch_a", name: "MacBook", address: item.address, task: "payments", title: item.title, item, accepts: ALL, subtask: ID, page: page(), boardSubtask, review, fetch: { state: "fetching", sent: null }, ui: freshReviewUi(), sentComments: [], readSent: [], acceptSent: null, push: null, pushUi: { confirming: false, busy: false, error: null }, access: "ready", canComment: true, canRead: true, refusedUrl: null, now: NOW, ...fields });
const allRead = (fields = {}) => {
  const v = view(fields);
  for (const file of [...review.subtask.files, ...review.subtask.commits.flatMap((commit) => commit.files)]) v.ui.read.set(readKey(file), true);
  return v;
};
const accepted = (more = {}) => {
  const subtasks = item.subtasks.map((one) => (one.id === ID ? { ...one, state: "accepted" } : one));
  return { ...item, subtasks, order: item.order.map((row) => ({ ...row, accepted: true })), pushable: { count: 4, through: last.sha, subtasks: ["currency", ID, "tests"] }, group: "READY TO PUSH", ...more };
};
const acceptedView = (fields = {}) => {
  const next = fields.item ?? accepted();
  return view({ item: next, boardSubtask: next.subtasks.find((one) => one.id === ID), ...fields });
};

test("a review leads back to the page it was opened from: the card when the card opened it, the question page otherwise", () => {
  const walk = (hops) => hops.reduce((kept, [name, ...params]) => keptTaskView(kept, name, params), null);
  const back = (kept) => taskHref("mch_a", "payments@api", taskViewOf(kept, "mch_a", "payments@api"));
  const fromCard = walk([["task", "mch_a", "payments@api"], ["card", "mch_a", "payments@api"], ["review", "mch_a", "payments@api", "gateway"], ["diff", "mch_a", "payments@api", "gateway", "1"]]);
  assert.equal(back(fromCard), "#/card/mch_a/payments%40api");
  assert.equal(back(walk([["card", "mch_a", "payments@api"], ["task", "mch_a", "payments@api"], ["review", "mch_a", "payments@api", "gateway"]])), "#/task/mch_a/payments%40api");
  assert.equal(back(walk([["card", "mch_a", "other@api"], ["review", "mch_a", "payments@api", "gateway"]])), "#/task/mch_a/payments%40api", "another task's card does not decide this one's way back");
  assert.equal(back(null), "#/task/mch_a/payments%40api");
  assert.match(subtaskEnd(acceptedView({ taskView: "card" })), /<a class="link" href="#\/card\/mch_a\/payments%40api">Back to the task<\/a>/);
  assert.match(subtaskEnd(acceptedView()), /<a class="link" href="#\/task\/mch_a\/payments%40api">Back to the task<\/a>/);
});

test("a subtask opens by its id, its commits each read on their own page, and the whole subtask on one", () => {
  assert.deepEqual([subtaskHref("mch_a", "payments@api", "gateway"), diffHref("mch_a", "payments@api", "gateway", 1), diffHref("mch_a", "payments@api", "gateway"), diffHref("mch_a", "payments@api"), subtaskHref("m", "a@b", "a b")], ["#/review/mch_a/payments%40api/gateway", "#/diff/mch_a/payments%40api/gateway/1", "#/diff/mch_a/payments%40api/gateway/all", "#/diff/mch_a/payments%40api", "#/review/m/a%40b/a%20b"]);
  assert.deepEqual([filesOf(view()), filesOf(view({ page: page("commit", 0) })), filesOf(view({ page: page("whole") }))], [review.subtask.files, first.files, review.subtask.files]);
  assert.deepEqual([shaOf(view()), shaOf(view({ page: page("commit", 0) })), shaOf(view({ page: page("commit", 1) }))], [last.sha, first.sha, last.sha]);
});

test("the subtask page names the subtask, places each commit in the push order with what lies between, folds each commit to its subject and sha, and lists the step's files without their diffs", () => {
  const html = subtaskPage(view(), { top });
  assert.match(text(html), /^Subtask MacBook Charge through a PaymentGateway 2 commits · 3 files · \+4 −1 Guide from Claude · 2 notes/);
  assert.match(text(html), /Commits · 2 Add the PaymentGateway trait a41c9e0 1 file · ✓ 1 of 1 read no\. 2 of 4 in the push order · ✓ accepted no\. 3 between them belongs to Run the payment tests on CI: Run the payment tests on CI Charge orders through the gateway in cents 9f3c2a1 Read the diff · 3 files 1 of 3 read no\. 4 of 4 in the push order Files of the subtask · 1 of 3 read/);
  assert.equal((text(subtaskPage(acceptedView(), { top })).match(/✓ accepted/g) ?? []).length, 1, "once the whole subtask is accepted only its header says so, no commit is singled out");
  assert.match(html, /<div class="cm shut"><div class="cm-h" role="button" tabindex="0" data-action="rv-commit" data-commit="0" aria-expanded="false">/);
  assert.match(html, /<a class="btn fit" href="#\/diff\/mch_a\/payments%40api\/gateway\/2">Read the diff · 3 files<\/a>/);
  assert.doesNotMatch(html, /rv-diff|rv-line|A float drops cents/, "neither diffs nor the folded messages show");
  assert.match(html, /<a class="btn fit" href="#\/diff\/mch_a\/payments%40api\/gateway\/all">Read the whole subtask · 3 files<\/a>/);
  assert.match(html, /<a class="rv-fh-name" href="#\/diff\/mch_a\/payments%40api\/gateway\/all" data-open-file="0">/);
  assert.doesNotMatch(html, /rv-end|Accept step/, "nothing to accept while a file is unread");
  const opened = view();
  opened.ui.commits.add(last.sha);
  assert.match(text(subtaskPage(opened, { top })), /9f3c2a1 A float drops cents on large orders; the gateway takes whole cents\. Read the diff/);
});

test("a step of one commit shows that commit as its subject and sha alone, and reads its diff from the bottom", () => {
  const one = { ...review, subtask: { ...review.subtask, commits: [{ ...last, files: review.subtask.files }] } };
  const html = subtaskPage(view({ review: one }), { top });
  assert.match(text(html), /Commits · 1 Charge orders through the gateway in cents 9f3c2a1 no\. 4 of 4 in the push order Files of the subtask · 1 of 3 read/);
  assert.match(html, /<div class="cm one shut"><div class="cm-h" role="button" tabindex="0" data-action="rv-commit" data-commit="0" aria-expanded="false">/);
  assert.doesNotMatch(html, /of 1 read|Read the whole subtask|A float drops cents/);
  assert.match(html, /<div class="whole"><a class="btn fit" href="#\/diff\/mch_a\/payments%40api\/gateway\/all">Read the diff · 3 files<\/a><\/div>/);
  assert.match(subtaskPage(view(), { top }), />Read the whole subtask · 3 files<\/a>/, "a step of two commits keeps its own reading");
});

test("a commit message unfolded on one page starts folded on the next, so a message is folded wherever a page opens", () => {
  const ui = freshReviewUi();
  foldCommitsOn(ui, "#/diff/mch_a/payments%40api/gateway/gateway");
  ui.commits.add(last.sha);
  assert.match(text(commitPage(view({ ui, page: page("commit", 1) }), { top })), /A float drops cents/);
  foldCommitsOn(ui, "#/diff/mch_a/payments%40api/gateway/gateway");
  assert.equal(ui.commits.has(last.sha), true, "a render of the same page keeps what was unfolded");
  foldCommitsOn(ui, "#/review/mch_a/payments%40api/gateway");
  assert.doesNotMatch(subtaskPage(view({ ui }), { top }), /A float drops cents|aria-expanded="true"/);
  foldCommitsOn(ui, "#/diff/mch_a/payments%40api/gateway/gateway");
  assert.doesNotMatch(commitPage(view({ ui, page: page("commit", 1) }), { top }), /A float drops cents/, "coming back to a page opens it folded");
});

test("a file read in one commit counts for the step, unless a later commit changed it again", () => {
  const v = view();
  const [gateway, orders] = review.subtask.files;
  assert.equal(readOf(v, gateway), false);
  assert.deepEqual(againOf(v, gateway), { read: first.sha, again: last.sha });
  assert.match(text(subtaskPage(v, { top })), /src\/gateway\.rs \+3 −0 · read in a41c9e0 · changed again in 9f3c2a1 · new file/);
  assert.equal(readOf(v, orders), true);
  v.ui.read.set(readKey(last.files[0]), true);
  assert.equal(readOf(v, gateway), true, "read at the step's last content is read for the step");
  assert.equal(againOf(view({ ui: Object.assign(freshReviewUi(), { read: new Map([[readKey(first.files[0]), false]]) }) }), gateway), null);
  assert.equal(readOf(view({ readSent: [{ path: "src/gateway.rs", content: gateway.content, read: true }] }), gateway), true);
  assert.equal(readOf(view({ readSent: [{ path: "src/gateway.rs", content: "0".repeat(40), read: true }] }), gateway), false, "a mark holds only for the content it was given on");
});

test("the commit page shows that commit's message folded, its facts and only its diff, then the next commit", () => {
  const html = commitPage(view({ page: page("commit", 0) }), { top });
  assert.match(text(html), /^Commit 1 of 2 MacBook Add the PaymentGateway trait a41c9e0 Charge through a PaymentGateway · a41c9e0 · 1 file · \+3 −0 · 1 of 1 read/);
  assert.equal((html.match(/class="rv-fsec"/g) ?? []).length, 1);
  assert.match(text(html), /End of commit 1\. Next: commit 2 · 3 files Back to the subtask$/);
  assert.match(html, /<a class="btn primary" href="#\/diff\/mch_a\/payments%40api\/gateway\/2">Next: commit 2 · 3 files<\/a>/);
  const final = commitPage(view({ page: page("commit", 1) }), { top });
  assert.match(text(final), /^Commit 2 of 2 .* Charge through a PaymentGateway · 9f3c2a1 · 3 files · \+2 −2 · 1 of 3 read/);
  assert.match(final, /<a class="btn primary" href="#\/review\/mch_a\/payments%40api\/gateway">Back to the subtask<\/a>/);
  assert.match(text(final), /Not shown here: the diff is binary or too large\./);
  assert.match(final, /data-action="rv-line" data-file="0" data-side="new" data-line="2"/);
  assert.doesNotMatch(commitPage(view({ page: page("commit", 1), canComment: false }), { top }), /data-action="rv-line"/);
  assert.match(text(commitPage(view({ page: page("commit", 4) }), { top })), /Charge through a PaymentGateway has no commit 5\./);
});

test("the whole step reads every file on one page and ends on the step's next action", () => {
  const html = wholePage(view({ page: page("whole") }), { top });
  assert.match(text(html), /^Subtask MacBook Charge through a PaymentGateway · 2 commits · 3 files · \+4 −1 · 1 of 3 read/);
  assert.match(html, /<header class="rv-fh is-read">.*aria-pressed="true" aria-label="Mark unread"/);
  assert.match(text(html), /End of the subtask\.$/, "nothing follows the end while a file is unread");
  assert.match(text(wholePage(allRead({ page: page("whole") }), { top })), /End of the subtask\. Accept subtask$/);
  const branch = view({ subtask: null, page: { subtask: null, kind: "whole", commit: null }, review: { ...review, subtask: null, files: review.subtask.files }, boardSubtask: null });
  assert.match(text(wholePage(branch, { top })), /^Diff MacBook Since main · 3 files · \+4 −1 · 1 of 3 read Guide from Claude/);
});

test("before the diff arrives the page says it is fetching, asleep or not ready, and asks nothing of a device that can't send", () => {
  assert.match(text(fetchBlock(view({ review: null }))), /^&nbsp; Fetching the diff from MacBook… It comes sealed to your account and opens here\.$/);
  assert.match(text(fetchBlock(view({ review: null, fetch: { state: "asleep", sent: null, lastSeen: NOW - 7200 } }))), /^MacBook is asleep The diff comes from MacBook\./);
  assert.match(fetchBlock(view({ review: null, fetch: { state: "view-only", sent: null } })), /View only on this device/);
  assert.equal(text(fetchBlock(view({ review: null, fetch: { state: "not-ready", sent: null } }))), "Charge through a PaymentGateway isn't ready to review yet.");
  assert.match(text(subtaskPage(view({ review: null }), { top })), /^Subtask MacBook Charge through a PaymentGateway &nbsp; Fetching the diff/, "the step's title shows while the diff comes");
  assert.equal(text(subtaskPage(view({ review: null }), { top, left: true })), "Subtask MacBook Charge through a PaymentGateway");
});

test("side by side, the left column holds the step with the open commit marked and no actions", () => {
  const left = subtaskPage(allRead(), { top, left: true, selected: 0 });
  assert.match(left, /<div class="cm shut on">/);
  assert.doesNotMatch(left, /Read the whole subtask|rv-end|Accept/);
});

test("a fully read step offers Accept in the thumb zone, and a comment waiting on Claude holds it", () => {
  assert.equal(text(subtaskEnd(allRead())), "Accept subtask");
  assert.match(subtaskPage(allRead(), { top }), /<div class="rv-end"><button type="button" class="btn primary" data-action="rv-accept">Accept subtask<\/button><\/div>$/);
  assert.equal(text(subtaskEnd(allRead({ sentComments: [{ path: "src/orders.rs", state: "done" }] }))), "Waiting for Claude's changes");
  assert.equal(text(subtaskEnd(allRead({ sentComments: [{ path: "src/orders.rs", state: "refused" }] }))), "Accept subtask", "a comment that never arrived waits on nothing");
  assert.match(text(subtaskEnd(allRead({ accepts: ["review-open"] }))), /MacBook takes accepts only at the machine\. Accept Charge through a PaymentGateway in kebbie there\./);
  assert.equal(subtaskEnd(allRead({ access: "view-only" })), "");
});

test("after the accept come the receipt with what is now ready to push, Push N commits, the next subtask and the way back to the task", () => {
  const sent = { head: last.sha, subtask: ID, at: NOW, exp: NOW + 3600, deviceName: "iPhone", machineName: "MacBook", result: { outcome: "done" } };
  const done = subtaskEnd(allRead({ item: accepted(), acceptSent: { sent, state: "done" } }));
  assert.match(text(done), /^✓ Accepted Charge through a PaymentGateway Ready to push: 4 commits\. Run the payment tests on CI waited for this subtask and goes out with it\. from iPhone at \d\d:\d\d Push 4 commits · 9f3c2a1 Add the new gateway is being built Back to the task$/);
  assert.match(done, /<div class="row-btns"><div class="decide" data-machine="mch_a" data-address="payments@api"><button type="button" class="btn primary" data-action="push-ask">Push 4 commits · 9f3c2a1<\/button><\/div><\/div>/);
  assert.equal(text(subtaskEnd(acceptedView())), "Push 4 commits · 9f3c2a1 Add the new gateway is being built Back to the task", "an accepted subtask offers the push even before it is read again");
  const confirming = subtaskEnd(acceptedView({ pushUi: { confirming: true, busy: false, error: null } }));
  assert.match(text(confirming), /^Push 4 commits · 9f3c2a1 to main\? 4 commits, reviewer passed|^Push 4 commits · 9f3c2a1 to main\? 4 commits\. /);
  const pushed = { head: last.sha, short: "9f3c2a1", count: 4, machineName: "MacBook", deviceName: "iPhone", at: NOW, exp: NOW + 3600, result: { outcome: "done", text: "Pushed 4 commits." } };
  assert.match(text(subtaskEnd(acceptedView({ push: { sent: pushed, state: "done" } }))), /^✓ Pushed 9f3c2a1 to main Pushed 4 commits\. from iPhone at \d\d:\d\d Add the new gateway is being built Back to the task$/);
  const gone = item.subtasks.map((one) => (one.id === ID ? { ...one, state: "pushed" } : one));
  assert.equal(text(subtaskEnd(view({ item: { ...item, subtasks: gone }, boardSubtask: { ...boardSubtask, state: "pushed" } }))), "↑ Charge through a PaymentGateway is pushed. Add the new gateway is being built Back to the task");
  assert.match(text(subtaskEnd(acceptedView({ accepts: ["review-open"] }))), /MacBook takes pushes only at the machine\. Push in kebbie there\./);
  const ready = { id: "new-gateway", title: "Add the new gateway", state: "ready", commits: ["b".repeat(40)] };
  const next = accepted({ subtasks: accepted().subtasks.map((one) => (one.id === "new-gateway" ? ready : one)) });
  assert.match(subtaskEnd(acceptedView({ item: next })), /<a class="btn" href="#\/review\/mch_a\/payments%40api\/new-gateway">Next: Add the new gateway · 1 commit<\/a>/);
});

test("a subtask accepted but behind one still to review waits for it, and offers no push of its own", () => {
  const behind = item.subtasks.find((one) => one.id === "tests");
  const v = view({ subtask: "tests", boardSubtask: behind, review: { ...review, subtask: { ...review.subtask, id: "tests", title: behind.title } } });
  assert.equal(text(subtaskEnd(v)), "Waits for Charge through a PaymentGateway before it goes out. Back to the task".replace(" Back", " Next: Charge through a PaymentGateway · 2 commits Back"));
  assert.doesNotMatch(subtaskEnd(v), /push-ask/);
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
  ui.read.set(readKey(review.subtask.files[0]), true);
  settleReview(ui, review);
  assert.equal(ui.drafts.length, 1, "the same review keeps its drafts");
  const amended = { ...review, head: "1".repeat(40), subtask: { ...review.subtask, commits: [first, { ...last, sha: "1".repeat(40) }] } };
  settleReview(ui, amended);
  assert.deepEqual([ui.drafts, ui.read.size], [[], 0]);
  assert.deepEqual(ui.stale.comments, [{ path: "src/orders.rs", side: "old", line: 41, text: "Why the cast?" }]);
  assert.match(text(subtaskPage(view({ review: amended, ui }), { top })), /Not sent: 1 comment on Charge through a PaymentGateway · 9f3c2a1 The diff changed since you wrote it, so it may point at other lines\. .* src\/orders.rs:41 \(before\) Why the cast\? Dismiss/);
});

test("a comment loses its control characters, and what is too long is named before anything is sealed", () => {
  assert.equal(cleanComment(" Clear it\u001b[2J\nand\tthis\u0007 "), "Clear it[2J\nand\tthis");
  assert.equal(commentProblem("x".repeat(MAX_REVIEW_COMMENT_CHARS)), null);
  assert.equal(commentProblem("x".repeat(MAX_REVIEW_COMMENT_CHARS + 1)), "This comment is 2,001 characters; a comment holds at most 2,000.");
  const comment = { path: "src/orders.rs", side: "old", line: 41, sha: null, text: "Why?" };
  const payload = (comments) => ({ address: item.address, subtask: "gateway", comments });
  assert.equal(commentsProblem(payload([comment])), null);
  assert.match(commentsProblem(payload(Array(MAX_REVIEW_COMMENTS + 1).fill(comment))), /^51 comments wait, and at most 50 go together\./);
  assert.match(commentsProblem(payload([{ ...comment, text: "x".repeat(MAX_REVIEW_COMMENT_CHARS + 1) }])), /^The comment on src\/orders.rs:41 \(before\) is too long: This comment is 2,001 characters/);
  assert.match(commentsProblem(payload(Array(5).fill({ ...comment, text: "€".repeat(MAX_REVIEW_COMMENT_CHARS) }))), /^These comments come to 30 kB, and at most 24 kB go together\./);
  const v = view();
  v.ui.drafts.push({ id: 1, ...comment });
  assert.equal(sendControl({ ...v, review: null }), "", "nothing is sent before the review it belongs to is open");
});

test("the task card offers a Review button only for a task without subtasks", () => {
  const found = { machine: "mch_a", item: { ...item, review: { subtask: ID, files: 3 } }, accepts: ALL };
  assert.equal(reviewEntry(found), "", "subtasks are the way in");
  const plain = { ...found, item: { ...item, subtasks: undefined, review: { subtask: null, files: 1 } } };
  assert.equal(reviewEntry(plain), `<div class="lead-act"><a class="btn primary fit" href="#/diff/mch_a/payments%40api">Review · 1 file</a></div>`);
  assert.equal(reviewEntry({ ...plain, accepts: ["answer"] }), "");
});

test("a named subtask's draft is kept under the subtask the page shows, on the review and on its diff", () => {
  assert.deepEqual(reviewPage(["mch_a", "t@r", "gateway"], false), { subtask: "gateway", kind: "subtask", commit: null });
  assert.deepEqual(reviewPage(["mch_a", "t@r", "gateway", "2"], true), { subtask: "gateway", kind: "commit", commit: 1 });
  assert.deepEqual(reviewPage(["mch_a", "t@r", "3"], false).subtask, "3", "a numbered step stays the string the page keys on");
  assert.deepEqual(reviewPage(["mch_a", "t@r"], false), { subtask: null, kind: "whole", commit: null });
});
