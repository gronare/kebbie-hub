import { MAX_REVIEW_COMMENT_CHARS, MAX_REVIEW_COMMENTS, MAX_REVIEW_COMMENTS_BYTES, stepHead } from "../js/protocol.js";
import { failedLine } from "./blobs.js";
import { commitBody } from "./commit.js";
import { APPROVAL_STATES, approvalStatus, block, clock, from, pushBlock, pushReady, refusal, settled, stepPushBlock, takes, until, viewOnlyBlock } from "./decide.js";
import { esc, lastSeenText } from "./screens.js";

const plural = (count, one, many) => `${count} ${count === 1 ? one : many}`;
const short = (sha) => (sha ?? "").slice(0, 7);
const encoded = (machine, address) => `${encodeURIComponent(machine)}/${encodeURIComponent(address)}`;
const ICON_LIST = `<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M5 7h14M5 12h14M5 17h9"/></svg>`;
const ICON_CHEVRON = `<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M7 10l5 5 5-5"/></svg>`;
const ICON_CHECK = `<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M6 12.5l4 4 8-9"/></svg>`;
const BAND = `<div class="rv-band" aria-hidden="true"></div>`;

export const taskHref = (machine, address) => `#/task/${encoded(machine, address)}`;
export const stepHref = (machine, address, step) => `#/review/${encoded(machine, address)}/${step}`;
export const diffHref = (machine, address, step = null, commit = "all") => `#/diff/${encoded(machine, address)}${step === null ? "" : `/${step}/${commit}`}`;
export const freshReviewUi = () => ({ read: new Map(), open: new Map(), commits: new Set(), drafts: [], composing: null, tray: false, receipt: null, busy: false, error: null, requested: null, nextId: 1, on: null, onLabel: null, stale: null });
const reviewIdentity = (review) => (review.step ? stepHead(review.step) : `${review.since}..${review.head}`);
const reviewLabel = (review) => (review.step ? `step ${review.step.number} · ${short(stepHead(review.step))}` : `the changes up to ${short(review.head)}`);

export function settleReview(ui, review) {
  if (!review || ui.on === reviewIdentity(review)) return;
  const unsent = [...ui.drafts, ...(ui.composing && !ui.composing.id && ui.composing.text?.trim() ? [ui.composing] : [])];
  if (ui.on !== null && unsent.length) ui.stale = { label: ui.onLabel, comments: unsent.map(({ path, side, line, text }) => ({ path, side, line, text })) };
  Object.assign(ui, { read: new Map(), open: new Map(), drafts: [], composing: null, tray: false, receipt: null, error: null, on: reviewIdentity(review), onLabel: reviewLabel(review) });
}

export const cleanComment = (text) => (text ?? "").replace(/[^\P{Cc}\n\t]/gu, "").trim();
const thousands = (count) => count.toLocaleString("en-US");

export function commentProblem(text) {
  const length = [...text].length;
  return length > MAX_REVIEW_COMMENT_CHARS ? `This comment is ${thousands(length)} characters; a comment holds at most ${thousands(MAX_REVIEW_COMMENT_CHARS)}.` : null;
}

export function commentsProblem(payload) {
  const { comments } = payload;
  if (comments.length > MAX_REVIEW_COMMENTS) return `${comments.length} comments wait, and at most ${MAX_REVIEW_COMMENTS} go together. Remove some before sending.`;
  const long = comments.find((comment) => commentProblem(comment.text));
  if (long) return `The comment on ${commentWhere(long)} is too long: ${commentProblem(long.text)}`;
  const bytes = new TextEncoder().encode(JSON.stringify(payload)).length;
  if (bytes > MAX_REVIEW_COMMENTS_BYTES) return `These comments come to ${Math.ceil(bytes / 1024)} kB, and at most ${MAX_REVIEW_COMMENTS_BYTES / 1024} kB go together. Shorten or remove some before sending.`;
  return null;
}

export function reviewEntry(found) {
  if (!found?.item.review || found.item.steps?.items.length || !takes(found.accepts, "review-open")) return "";
  return `<div class="lead-act"><a class="btn primary fit" href="${diffHref(found.machine, found.item.address)}">Review · ${plural(found.item.review.files, "file", "files")}</a></div>`;
}

export function filesOf(v) {
  const step = v.review?.step;
  if (!step) return v.review?.files ?? [];
  return v.page.kind === "commit" ? (step.commits[v.page.commit]?.files ?? []) : step.files;
}

export function shaOf(v) {
  const step = v.review?.step;
  if (!step) return null;
  return v.page.kind === "commit" ? (step.commits[v.page.commit]?.sha ?? null) : stepHead(step);
}

export const lineTarget = (line) => (line.kind === "del" ? { side: "old", line: line.old } : { side: "new", line: line.new });
export const onLine = (comment, line) => comment.side === lineTarget(line).side && comment.line === lineTarget(line).line;
export const readKey = (file) => `${file.path}\n${file.content}`;
export function readOf(v, file) {
  if (v.ui.read.has(readKey(file))) return v.ui.read.get(readKey(file));
  const marked = v.readSent?.find((sent) => sent.path === file.path && sent.content === file.content);
  return marked ? marked.read : file.read;
}
export const openOf = (v, file) => v.ui.open.get(readKey(file)) ?? v.ui.open.get(file.path) ?? !readOf(v, file);
export const commentWhere = (comment) => `${comment.path}:${comment.line}${comment.side === "old" ? " (before)" : ""}${comment.sha ? ` in ${short(comment.sha)}` : ""}`;
const readCount = (v, files) => files.filter((file) => readOf(v, file)).length;
const stat = (file) => `<span class="rv-stat"><span class="rv-add">+${esc(file.add)}</span> <span class="rv-del">−${esc(file.del)}</span></span>`;
const sums = (files) => `<span class="rv-add">+${files.reduce((sum, file) => sum + file.add, 0)}</span> <span class="rv-del">−${files.reduce((sum, file) => sum + file.del, 0)}</span>`;
const chip = (v) => `<span class="machine">${esc(v.name)}</span>`;
const onPage = (v) => (comment) => (comment.sha ?? null) === shaOf(v);
const sentHere = (v, path) => v.sentComments.filter((comment) => comment.path === path && onPage(v)(comment));
const draftsHere = (v, path) => v.ui.drafts.filter((comment) => comment.path === path && onPage(v)(comment));

export function againOf(v, file) {
  const commits = v.review?.step?.commits ?? [];
  let readAt = -1;
  commits.forEach((commit, index) => {
    const there = commit.files.find((one) => one.path === file.path);
    if (there && there.content !== file.content && readOf(v, there)) readAt = index;
  });
  if (readAt < 0) return null;
  const again = commits.slice(readAt + 1).find((commit) => commit.files.some((one) => one.path === file.path));
  return again ? { read: commits[readAt].sha, again: again.sha } : null;
}

function guideCard(review) {
  if (!review.guide.length && !review.left_out) return "";
  const notes = review.guide.map((note) => `<li><span class="mono">${esc(note.path)}</span><div class="md rv-text" data-markdown>${esc(note.text)}</div></li>`).join("");
  const left = review.left_out ? `<div class="meta">${esc(review.left_out)}</div>` : "";
  return `<details class="task rv-guide-fold"><summary>Guide from Claude${review.guide.length ? ` · ${plural(review.guide.length, "note", "notes")}` : ""}</summary>${notes ? `<ol class="rv-guide">${notes}</ol>` : ""}${left}</details>`;
}

const fold = (open) => `<span class="rv-icon rv-fold${open ? " is-open" : ""}">${ICON_CHEVRON}</span>`;
const commitHead = (commit, index, open) => `<div class="cm-h" role="button" tabindex="0" data-action="rv-commit" data-commit="${index}" aria-expanded="${open}">${fold(open)}<span class="t">${esc(commit.subject)}</span><span class="mono meta">${esc(short(commit.sha))}</span></div>`;
const commitText = (commit, open) => (open && commit.body.trim() ? `<div class="rv-body">${commitBody(commit.body)}</div>` : "");

function commitRow(v, commit, index, on) {
  const open = v.ui.commits.has(commit.sha);
  const count = commit.files.length;
  const done = readCount(v, commit.files);
  const said = done === count ? `<span class="ok">✓ ${done} of ${count} read</span>` : `${done} of ${count} read`;
  const foot = done === count ? `<span class="meta">${plural(count, "file", "files")} · ${said}</span>` : `<a class="btn fit" href="${diffHref(v.machine, v.address, v.step, index + 1)}">Read the diff · ${plural(count, "file", "files")}</a><span class="meta">${said}</span>`;
  return `<div class="cm${open ? "" : " shut"}${on ? " on" : ""}">${commitHead(commit, index, open)}${commitText(commit, open)}<div class="cm-foot">${foot}</div></div>`;
}

function stepFileRow(v, file, index) {
  const read = readOf(v, file);
  const again = read ? null : againOf(v, file);
  const notes = v.ui.drafts.filter((comment) => comment.path === file.path).length + v.sentComments.filter((comment) => comment.path === file.path).length;
  const status = file.status === "added" ? "new file" : file.status === "deleted" ? "deleted" : file.status === "renamed" ? `renamed from ${esc(file.old_path)}` : "";
  const meta = [stat(file), read ? `<span class="ok">read</span>` : again ? `<span class="again">read in ${esc(short(again.read))} · changed again in ${esc(short(again.again))}</span>` : "", status, notes ? plural(notes, "comment", "comments") : ""].filter(Boolean).join(" · ");
  const whole = diffHref(v.machine, v.address, v.step);
  const opener = `<a class="rv-icon rv-fold" href="${whole}" data-open-file="${index}" aria-label="Read ${esc(file.path)} in the whole step">${ICON_CHEVRON}</a>`;
  const name = `<a class="rv-fh-name" href="${whole}" data-open-file="${index}"><span class="mono">${esc(file.path)}</span><span class="meta">${meta}</span></a>`;
  const check = `<button type="button" class="rv-icon rv-check" data-action="rv-read" data-file="${index}" aria-pressed="${read}" aria-label="${read ? "Mark unread" : "Mark read"}"><span class="box">${ICON_CHECK}</span></button>`;
  return `<section class="rv-fsec" id="rv-f-${index}"><header class="rv-fh${read ? " is-read" : ""}">${opener}${name}${check}</header></section>`;
}

const stepTitle = (v) => v.boardStep?.title ?? v.review?.step?.title ?? "";
const pageHead = (v, title, top, inPanel) => (inPanel ? `${chip(v)}<h2>${esc(title)}</h2>` : top(title, chip(v)));

export function stepPage(v, { top, inPanel = false, left = false, selected = null }) {
  const heading = pageHead(v, `Step ${v.step}`, top, inPanel);
  const name = stepTitle(v) ? `<p class="step-name">${esc(stepTitle(v))}</p>` : "";
  const step = v.review?.step;
  if (!step) return `${heading}${name}${left ? "" : fetchBlock(v)}`;
  const { commits, files } = step;
  const standing = v.boardStep?.state === "accepted" ? ` · <span class="ok">✓ accepted</span>` : v.boardStep?.state === "pushed" ? " · ↑ pushed" : "";
  const facts = `<div class="meta pad">${plural(commits.length, "commit", "commits")} · ${plural(files.length, "file", "files")} · ${sums(files)}${standing}</div>`;
  const list = `<h3 class="sec">Commits · ${commits.length}</h3><section class="task cm-list">${commits.map((commit, index) => commitRow(v, commit, index, index === selected)).join("")}</section>`;
  const rows = `<h3 class="sec">Files of the step · ${readCount(v, files)} of ${files.length} read</h3>${files.map((file, index) => stepFileRow(v, file, index)).join("")}`;
  if (left) return heading + name + facts + list + rows;
  const whole = `<div class="whole"><a class="btn fit" href="${diffHref(v.machine, v.address, v.step)}">Read the whole step · ${plural(files.length, "file", "files")}</a></div>`;
  const end = stepEnd(v);
  return heading + name + facts + (inPanel ? "" : staleBlock(v)) + guideCard(v.review) + list + rows + whole + (end ? `<div class="rv-end">${end}</div>` : "");
}

export function commitPage(v, { top, inPanel = false }) {
  const step = v.review?.step;
  const index = v.page.commit;
  const total = step?.commits.length ?? v.boardStep?.commits.length ?? 0;
  const heading = pageHead(v, total ? `Commit ${index + 1} of ${total}` : `Commit ${index + 1}`, top, inPanel);
  if (!step) return heading + fetchBlock(v);
  const commit = step.commits[index];
  if (!commit) return heading + `<div class="center left">Step ${esc(v.step)} has no commit ${esc(index + 1)}.</div>`;
  const open = v.ui.commits.has(commit.sha);
  const card = `<section class="task cm-list solo"><div class="cm one${open ? "" : " shut"}">${commitHead(commit, index, open)}${commitText(commit, open)}</div></section>`;
  const { files } = commit;
  const facts = `<div class="meta pad">Step ${esc(v.step)} · <span class="mono">${esc(short(commit.sha))}</span> · ${plural(files.length, "file", "files")} · ${sums(files)} · ${readCount(v, files)} of ${files.length} read</div>`;
  const next = step.commits[index + 1];
  const back = `<a class="${next ? "link" : "btn primary"}" href="${stepHref(v.machine, v.address, v.step)}">Back to the step</a>`;
  const onward = next ? `<div class="row-btns"><a class="btn primary" href="${diffHref(v.machine, v.address, v.step, index + 2)}">Next: commit ${index + 2} · ${plural(next.files.length, "file", "files")}</a></div>${back}` : `<div class="row-btns">${back}</div>`;
  const end = `<div class="meta pad rv-endnote">End of commit ${esc(index + 1)}.</div><div class="rv-end">${onward}</div>`;
  return heading + card + facts + (inPanel ? "" : staleBlock(v)) + files.map((file, at) => fileSection(v, file, at)).join("") + end;
}

export function wholePage(v, { top, inPanel = false }) {
  const step = v.review?.step;
  const heading = pageHead(v, v.step === null ? "Diff" : `Step ${v.step}`, top, inPanel);
  if (!v.review) return heading + fetchBlock(v);
  const files = filesOf(v);
  const which = step ? `The whole step · ${plural(step.commits.length, "commit", "commits")}` : `Since ${esc(v.review.since)}`;
  const facts = `<div class="meta pad">${which} · ${plural(files.length, "file", "files")} · ${sums(files)} · ${readCount(v, files)} of ${files.length} read</div>`;
  const guide = step ? "" : guideCard(v.review);
  const end = step ? stepEnd(v) : readCount(v, files) === files.length ? branchEnd(v) : "";
  const tail = `<div class="meta pad rv-endnote">End of ${step ? `step ${esc(step.number)}` : "the changes"}.</div>${end ? `<div class="rv-end">${end}</div>` : ""}`;
  return heading + facts + (inPanel ? "" : staleBlock(v)) + guide + files.map((file, index) => fileSection(v, file, index)).join("") + tail;
}

export function fetchBlock(v) {
  const name = esc(v.name);
  const { state, sent } = v.fetch;
  const again = `<button type="button" class="btn" data-action="rv-retry">Ask ${name} again</button>`;
  if (state === "not-ready") return `<div class="center left">${v.step === null ? "Nothing waits for review here." : `Step ${esc(v.step)} isn't ready to review yet.`}</div>`;
  if (state === "off") return `<div class="pro">This hub doesn't relay reviews. Review in kebbie on ${name}.</div>`;
  if (state === "refused") return `<div class="pro">Reviewing here is not enabled for your account on this hub.${v.refusedUrl ? ` <a class="link inline" href="${esc(v.refusedUrl)}">Details</a>` : ""}</div>`;
  if (state === "view-only") return viewOnlyBlock();
  if (state === "not-taken") return `<div class="pro">${name} doesn't send its diffs to the hub. Review in kebbie on ${name}.</div>`;
  if (state === "failed") return `<div class="center left">${esc(failedLine("review"))}</div>${again}`;
  if (v.ui.error && !sent) return `<div class="center warn" role="alert">${esc(v.ui.error)}</div>${again}`;
  if (state === "asleep") return block("wait", `${name} is asleep`, `The diff comes from ${name}. Review opens here when it wakes, while this page is open.`, v.fetch.lastSeen ? `last seen ${esc(lastSeenText(v.fetch.lastSeen, v.now))}` : "not seen yet");
  if (APPROVAL_STATES.includes(state)) return approvalStatus(sent, state, { standing: v.fetch.standing, now: v.now, what: { goes: "The diff comes here", again: "open the review again" } }) + (settled(state) ? again : "");
  if (state === "expired") return block("refused", `Not delivered: ${name} stayed asleep past the hour`, `Ask again when ${name} is awake.`, `Sent ${from(sent)} · expired ${esc(clock(sent.exp))}`) + again;
  if (state === "rejected") return block("refused", ...refusal("Not opened", sent.result.text), from(sent)) + again;
  return `<div class="sent wait" role="status"><b><span class="spinner"></span>&nbsp; Fetching the diff from ${name}…</b><span>It comes sealed to your account and opens here.</span></div>`;
}

function acceptStatus(v, sent, state) {
  const machine = esc(v.name);
  const number = esc(v.step);
  if (APPROVAL_STATES.includes(state)) return approvalStatus(sent, state, { standing: v.acceptSent.standing, now: v.now, what: { goes: `Step ${number} is accepted`, again: "accept again" } });
  if (state === "done") return block("done", `✓ Accepted step ${number}`, later(v) ? `Push it now, or read step ${esc(later(v).number)} first and push both later.` : "Push it now.", from(sent));
  if (state === "refused") return block("refused", ...refusal("Not accepted", sent.result.text), from(sent));
  if (state === "expired") return block("refused", `Not delivered: ${machine} stayed asleep past the hour`, `Accept again when ${machine} is awake.`, `Sent ${from(sent)} · expired ${esc(clock(sent.exp))}`);
  if (state === "waiting") return block("wait", `Waiting for ${machine}`, `Step ${number} is accepted when ${machine} wakes up.`, `Sent ${from(sent)} · expires ${esc(until(sent.exp, v.now))}`);
  return block("wait", `Sent to ${machine}`, `${machine} accepts step ${number}.`, `Sent ${from(sent)} · expires ${esc(until(sent.exp, v.now))}`);
}

const later = (v) => (v.item.steps?.items ?? []).find((one) => one.number > v.step && !["accepted", "pushed"].includes(one.state)) ?? null;

function acceptButton(v) {
  const sent = v.acceptSent?.sent ?? null;
  const status = sent ? acceptStatus(v, sent, v.acceptSent.state) : "";
  if (v.access !== "ready") return status;
  if (!takes(v.accepts, "accept-step")) return status + `<div class="pro">${esc(v.name)} takes accepts only at the machine. Accept step ${esc(v.step)} in kebbie there.</div>`;
  const error = v.ui.error ? `<div class="center warn" role="alert">${esc(v.ui.error)}</div>` : "";
  return status + `<button type="button" class="btn primary" data-action="rv-accept"${v.ui.busy ? " disabled" : ""}>Accept step ${esc(v.step)}</button>` + error;
}

const decideOn = (v, html, step = null) => `<div class="decide" data-machine="${esc(v.machine)}" data-address="${esc(v.address)}"${step === null ? "" : ` data-step="${esc(step)}"`}>${html}</div>`;
const GONE = ["refused", "expired", "not-approved", "denied"];
const WAITING = `<div class="meta">Waiting for Claude's changes</div>`;

function pushArea(v, what, html) {
  if (v.access !== "ready") return "";
  if (!takes(v.accepts, "push")) return `<div class="pro">${esc(v.name)} takes pushes only at the machine. Push ${what} in kebbie there.</div>`;
  return html();
}

function wholePush(v) {
  if (!pushReady(v.item)) return "";
  const shown = v.taskPush;
  return pushArea(v, "the task", () => decideOn(v, pushBlock({ item: v.item, machineName: v.name, ui: v.pushUi, sent: shown?.sent ?? null, state: shown?.state ?? null, standing: shown?.standing ?? null })));
}

function onward(v) {
  const next = later(v);
  if (next?.state === "ready") return { link: `<a class="btn" href="${stepHref(v.machine, v.address, next.number)}">Next: step ${esc(next.number)} · ${plural(next.commits.length, "commit", "commits")}</a>`, note: "" };
  return { link: "", note: next?.state === "in-progress" ? `<div class="meta">Step ${esc(next.number)} is being built</div>` : "" };
}

const backToTask = (v) => `<a class="link" href="${taskHref(v.machine, v.address)}">Back to the task</a>`;
const waitsForClaude = (v) => v.sentComments.some((comment) => !GONE.includes(comment.state));

function branchEnd(v) {
  return waitsForClaude(v) ? WAITING : wholePush(v);
}

export function stepEnd(v) {
  const step = v.review?.step;
  if (!step) return "";
  if (waitsForClaude(v)) return WAITING;
  const next = onward(v);
  const pushed = v.boardStep ? v.boardStep.state === "pushed" : (v.item.steps?.pushed ?? 0) >= v.step;
  if (pushed && !v.stepPush) return `<div class="meta">↑ Step ${esc(v.step)} is pushed.</div>${next.link ? `<div class="row-btns">${next.link}</div>` : ""}${next.note}${backToTask(v)}`;
  const accepted = v.boardStep?.state === "accepted" || v.acceptSent?.state === "done" || Boolean(v.stepPush);
  if (!accepted) {
    if (v.acceptSent && !settled(v.acceptSent.state)) return acceptStatus(v, v.acceptSent.sent, v.acceptSent.state);
    return readCount(v, step.files) === step.files.length ? acceptButton(v) : "";
  }
  const receipt = v.acceptSent?.state === "done" ? acceptStatus(v, v.acceptSent.sent, "done") : "";
  const shown = v.stepPush;
  const head = { number: v.step, sha: stepHead(step) };
  const push = !later(v) && pushReady(v.item) && !shown ? wholePush(v) : pushArea(v, `step ${esc(v.step)}`, () => decideOn(v, stepPushBlock({ step: head, machineName: v.name, ui: v.pushUi, sent: shown?.sent ?? null, state: shown?.state ?? null, standing: shown?.standing ?? null }), v.step));
  return `${receipt}<div class="row-btns">${push}${next.link}</div>${next.note}${backToTask(v)}`;
}

export function staleBlock(v) {
  const { stale } = v.ui;
  if (!stale) return "";
  const rows = stale.comments.map((comment) => `<div class="opt rv-c"><span class="mono meta">${esc(commentWhere(comment))}</span><span>${esc(comment.text)}</span></div>`).join("");
  return `<section class="task rv-stale" role="status"><div class="name">Not sent: ${plural(stale.comments.length, "comment", "comments")} on ${esc(stale.label)}</div><div class="meta">The diff changed since you wrote ${stale.comments.length === 1 ? "it" : "them"}, so ${stale.comments.length === 1 ? "it" : "they"} may point at other lines. Copy what still holds into a new comment.</div>${rows}<button type="button" class="btn" data-action="rv-stale-dismiss">Dismiss</button></section>`;
}

function sentNote(comment) {
  const said = comment.state === "done" ? `✓ sent to Claude at ${esc(clock(comment.at))}` : settled(comment.state) || comment.state === "refused" ? "not delivered" : `sending · ${esc(clock(comment.at))}`;
  return `<div class="rv-note sent-c"><div class="meta${comment.state === "done" ? " ok" : ""}">${said}</div>${comment.text ? `<div>${esc(comment.text)}</div>` : ""}</div>`;
}

function draftNote(comment) {
  return `<div class="rv-note" id="rv-c-${esc(comment.id)}"><div class="meta">not sent yet · <button type="button" class="sub-btn rv-tap" data-action="rv-edit" data-id="${esc(comment.id)}">Edit</button> <button type="button" class="sub-btn rv-tap rv-remove" data-action="rv-remove" data-id="${esc(comment.id)}">Remove</button></div><div>${esc(comment.text)}</div></div>`;
}

function composer(composing) {
  const where = composing.side === "old" ? `line ${composing.line} (before)` : `line ${composing.line}`;
  const problem = composing.problem ? `<div class="center warn" role="alert">${esc(composing.problem)}</div>` : "";
  return `<div class="rv-compose"><div class="note-field"><label for="rv-c">Comment on ${esc(where)}</label><textarea id="rv-c" data-draft="review-comment" rows="3" placeholder="What should Claude look at?">${esc(composing.text ?? "")}</textarea></div>${problem}<div class="row2"><button type="button" class="btn" data-action="rv-compose-cancel">Cancel</button><button type="button" class="btn primary" data-action="rv-compose-add">${composing.id ? "Save" : "Add to send"}</button></div></div>`;
}

function diffCard(v, file, index) {
  if (file.omitted) return `<section class="task rv-diff"><div class="meta pad rv-omitted">Not shown here: the diff is binary or too large. Read it in kebbie on ${esc(v.name)}.</div></section>`;
  const composing = v.ui.composing?.path === file.path ? v.ui.composing : null;
  const drafts = draftsHere(v, file.path).filter((comment) => comment.id !== composing?.id);
  const sent = sentHere(v, file.path);
  const hunks = file.hunks.map((hunk) => {
    const rows = hunk.lines.map((line) => {
      const target = lineTarget(line);
      const here = (comment) => onLine(comment, line);
      const mine = drafts.filter(here);
      const theirs = sent.filter(here);
      const writing = composing && here(composing);
      const kind = line.kind === "add" ? " add" : line.kind === "del" ? " del" : "";
      const sign = line.kind === "add" ? "+" : line.kind === "del" ? "−" : "";
      const hang = (line.text.match(/^ */)[0].length) + 2;
      const marked = writing || mine.length || theirs.length ? " on" : "";
      const tap = v.canComment ? ` role="button" tabindex="0" data-action="rv-line" data-file="${index}" data-side="${target.side}" data-line="${esc(target.line)}"` : "";
      const row = `<div class="rv-line${kind}${marked}"${tap}><span class="rv-no">${esc(line.old ?? "")}</span><span class="rv-no">${esc(line.new ?? "")}</span><span class="rv-sign">${sign}</span><span class="rv-code" data-hang="${hang}">${esc(line.text) || " "}</span></div>`;
      return row + theirs.map(sentNote).join("") + mine.map(draftNote).join("") + (writing ? composer(composing) : "");
    });
    return `<div class="rv-hunk">${esc(hunk.head)}</div>${rows.join("")}`;
  });
  return `<section class="task rv-diff">${hunks.join("")}</section>`;
}

function fileSection(v, file, index) {
  const read = readOf(v, file);
  const open = openOf(v, file);
  const notes = draftsHere(v, file.path).length + sentHere(v, file.path).length;
  const meta = [stat(file), read ? `<span class="ok">read</span>` : "", notes ? plural(notes, "comment", "comments") : "", open ? "" : "collapsed"].filter(Boolean).join(" · ");
  const fold = `<button type="button" class="rv-icon rv-fold${open ? " is-open" : ""}" data-action="rv-fold" data-file="${index}" aria-expanded="${open}" aria-label="${open ? "Collapse" : "Expand"} ${esc(file.path)}">${ICON_CHEVRON}</button>`;
  const name = `<button type="button" class="rv-fh-name" data-action="rv-fold" data-file="${index}"><span class="mono">${esc(file.path)}</span><span class="meta">${meta}</span></button>`;
  const check = `<button type="button" class="rv-icon rv-check" data-action="rv-read" data-file="${index}" aria-pressed="${read}" aria-label="${read ? "Mark unread" : "Mark read"}"><span class="box">${ICON_CHECK}</span></button>`;
  return `<section class="rv-fsec" id="rv-f-${index}"><header class="rv-fh${read ? " is-read" : ""}">${fold}${name}${check}</header>${open ? diffCard(v, file, index) : ""}</section>`;
}

export function hangLines(root) {
  for (const code of root.querySelectorAll(".rv-code[data-hang]")) code.style.setProperty("--hang", `${code.dataset.hang}ch`);
}

export function sendControl(v) {
  const { receipt } = v.ui;
  if (receipt) return `${BAND}<div class="rv-sendbar">${block("done", `✓ Sent ${plural(receipt.count, "comment", "comments")}`, `They reach ${esc(v.task)}'s Claude in one message, as Enter sends them in herdr.`, `from ${esc(receipt.device)} at ${esc(clock(receipt.at))}`)}</div>`;
  const drafts = v.ui.drafts;
  if (!drafts.length || !v.review) return "";
  const tray = v.ui.tray
    ? `<div class="rv-traypop" role="dialog" aria-label="Comments to send"><div class="row"><b>To send · ${drafts.length}</b><button type="button" class="sub-btn rv-tap" data-action="rv-tray">Close</button></div>${drafts.map((comment) => `<div class="opt rv-c"><span class="mono meta">${esc(commentWhere(comment))}</span><span>${esc(comment.text)}</span><div class="rv-acts"><button type="button" class="sub-btn rv-tap" data-action="rv-show" data-id="${esc(comment.id)}">Show</button><button type="button" class="sub-btn rv-tap" data-action="rv-edit" data-id="${esc(comment.id)}">Edit</button><button type="button" class="sub-btn rv-tap rv-remove" data-action="rv-remove" data-id="${esc(comment.id)}">Remove</button></div></div>`).join("")}<div class="meta">They go to ${esc(v.task)}'s Claude in one message.</div></div>`
    : "";
  const error = v.ui.error ? `<div class="center warn rv-senderror" role="alert">${esc(v.ui.error)}</div>` : "";
  const busy = v.ui.busy ? " disabled" : "";
  return `${BAND}<div class="rv-sendbar">${tray}${error}<div class="rv-pill"><button type="button" class="rv-pill-list" data-action="rv-tray" aria-expanded="${v.ui.tray}" aria-label="Show the comments to send">${ICON_LIST}</button><button type="button" class="rv-pill-send" data-action="rv-send"${busy}>Send ${plural(drafts.length, "comment", "comments")}</button></div></div>`;
}
