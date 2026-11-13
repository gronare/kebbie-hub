import { MAX_REVIEW_COMMENT_CHARS, MAX_REVIEW_COMMENTS, MAX_REVIEW_COMMENTS_BYTES } from "../js/protocol.js";
import { failedLine } from "./blobs.js";
import { commitBody } from "./commit.js";
import { APPROVAL_STATES, approvalStatus, block, clock, from, pushBlock, pushReady, refusal, settled, stepPushBlock, takes, until, viewOnlyBlock } from "./decide.js";
import { esc, lastSeenText, rulesLines, STEP_MARKS } from "./screens.js";

const plural = (count, one, many) => `${count} ${count === 1 ? one : many}`;
const short = (sha) => (sha ?? "").slice(0, 7);
const encoded = (machine, address) => `${encodeURIComponent(machine)}/${encodeURIComponent(address)}`;
const ICON_LIST = `<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M5 7h14M5 12h14M5 17h9"/></svg>`;
const ICON_CHEVRON = `<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M7 10l5 5 5-5"/></svg>`;
const ICON_CHECK = `<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M6 12.5l4 4 8-9"/></svg>`;
const BAND = `<div class="rv-band" aria-hidden="true"></div>`;
const STEP_LABELS = { planned: "", "in-progress": "in progress", committed: "to accept", accepted: "accepted" };

export const reviewHref = (machine, address) => `#/review/${encoded(machine, address)}`;
export const diffHref = (machine, address, file = null) => `#/diff/${encoded(machine, address)}${file === null ? "" : `/${file}`}`;
export const freshReviewUi = () => ({ read: new Map(), open: new Map(), drafts: [], composing: null, tray: false, receipt: null, confirming: false, busy: false, error: null, requested: null, nextId: 1, on: null, onLabel: null, stale: null });
export const offerKey = (item) => `${item.head ?? ""}|${item.review?.step ?? ""}`;
export const opensOffer = (item) => (sent) => (sent.head ?? null) === (item.head ?? null) && (sent.step ?? null) === (item.review?.step ?? null);
const reviewIdentity = (review) => (review.step ? review.step.sha : `${review.since}..${review.head}`);
const reviewLabel = (review) => (review.step ? `step ${review.step.number} · ${short(review.step.sha)}` : `the changes up to ${short(review.head)}`);

export function settleReview(ui, review) {
  if (!review || ui.on === reviewIdentity(review)) return;
  const unsent = [...ui.drafts, ...(ui.composing && !ui.composing.id && ui.composing.text?.trim() ? [ui.composing] : [])];
  if (ui.on !== null && unsent.length) ui.stale = { label: ui.onLabel, comments: unsent.map(({ path, side, line, text }) => ({ path, side, line, text })) };
  Object.assign(ui, { read: new Map(), open: new Map(), drafts: [], composing: null, tray: false, receipt: null, confirming: false, error: null, on: reviewIdentity(review), onLabel: reviewLabel(review) });
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
export const offered = (found) => Boolean(found?.item.review) && takes(found.accepts, "review-open");

export function reviewEntry(found) {
  if (!offered(found)) return "";
  const { step, files } = found.item.review;
  const what = step ? `Review step ${esc(step)}` : "Review";
  return `<div class="lead-act"><a class="btn primary fit" href="${reviewHref(found.machine, found.item.address)}">${what} · ${plural(files, "file", "files")}</a></div>`;
}

export const lineTarget = (line) => (line.kind === "del" ? { side: "old", line: line.old } : { side: "new", line: line.new });
export const onLine = (comment, line) => comment.side === lineTarget(line).side && comment.line === lineTarget(line).line;
export const readKey = (file) => `${file.path}\n${file.content}`;
export function readOf(v, file) {
  if (v.ui.read.has(readKey(file))) return v.ui.read.get(readKey(file));
  const marked = v.readSent?.find((sent) => sent.path === file.path && sent.content === file.content);
  return marked ? marked.read : file.read;
}
export const openOf = (v, file) => (v.ui.open.has(file.path) ? v.ui.open.get(file.path) : !readOf(v, file));
export const commentWhere = (comment) => `${comment.path}:${comment.line}${comment.side === "old" ? " (before)" : ""}${comment.sha ? ` in ${short(comment.sha)}` : ""}`;
const readCount = (v) => v.review.files.filter((file) => readOf(v, file)).length;
const stat = (file) => `<span class="rv-stat"><span class="rv-add">+${esc(file.add)}</span> <span class="rv-del">−${esc(file.del)}</span></span>`;
const chip = (v) => `<span class="machine">${esc(v.name)}</span>`;
const sentHere = (v, path) => v.sentComments.filter((comment) => comment.path === path);

function counts(item) {
  const steps = item.steps?.items ?? [];
  const waiting = steps.filter((step) => step.state === "committed").length;
  const accepted = steps.filter((step) => step.state === "accepted").length;
  if (waiting) return `<span class="accept">◆ ${waiting} to accept</span>`;
  return accepted ? `<span class="ok">✓ ${accepted} accepted</span>` : "";
}

function stepsCard(v) {
  const reviewed = v.review?.step?.number ?? v.item.review?.step ?? null;
  const steps = v.item.steps?.items ?? (v.review?.step ? [{ number: v.review.step.number, title: v.review.step.title, state: "committed" }] : []);
  if (!steps.length) return "";
  const rows = steps.map((step) => {
    const on = step.number === reviewed ? " rv-on" : "";
    const label = STEP_LABELS[step.state] ?? "";
    return `<div class="step s-${esc(step.state)} rv-step${on}"><span class="what"><span class="mark">${STEP_MARKS[step.state] ?? ""}</span><span class="number">${esc(step.number)}</span><span class="step-title">${esc(step.title)}</span></span>${label ? `<span class="label">${esc(label)}</span>` : ""}</div>${rulesLines(step.rules, { expandable: true })}`;
  });
  return `<h3 class="sec">${steps.length === 1 ? "Step" : "Steps"}</h3><section class="task rv-steps steps">${rows.join("")}</section>`;
}

function guideCard(review) {
  if (!review.guide.length && !review.left_out) return "";
  const notes = review.guide.map((note) => `<li><span class="mono">${esc(note.path)}</span><div class="md rv-text" data-markdown>${esc(note.text)}</div></li>`).join("");
  const left = review.left_out ? `<div class="meta">${esc(review.left_out)}</div>` : "";
  return `<h3 class="sec">Guide from Claude</h3><section class="task">${notes ? `<ol class="rv-guide">${notes}</ol>` : ""}${left}</section>`;
}

function messageCard(step) {
  if (!step) return "";
  const body = step.body.trim() ? `<div class="rv-body">${commitBody(step.body)}</div>` : "";
  return `<h3 class="sec">Commit</h3><section class="task rv-msg"><div class="head"><b class="name">${esc(step.subject)}</b><span class="mono meta">${esc(short(step.sha))}</span></div>${body}</section>`;
}

function fileRow(v, file, index) {
  const drafts = v.ui.drafts.filter((comment) => comment.path === file.path).length;
  const sent = sentHere(v, file.path).length;
  const said = [readOf(v, file) ? `<span class="ok">✓ read</span>` : "unread", file.status === "added" ? "new file" : file.status === "deleted" ? "deleted" : file.status === "renamed" ? `renamed from ${esc(file.old_path)}` : "", drafts ? `<span class="accept">${plural(drafts, "comment", "comments")} to send</span>` : "", sent ? `${plural(sent, "comment", "comments")} sent` : ""].filter(Boolean).join(" · ");
  return `<a class="task rv-file" href="${diffHref(v.machine, v.address, index)}" data-file="${index}"><div class="head"><span class="name"><span class="rv-n">${index + 1}</span>${esc(file.path)}</span>${stat(file)}</div><div class="meta">${said}</div></a>`;
}

export function fetchBlock(v) {
  const name = esc(v.name);
  const { state, sent } = v.fetch;
  const again = `<button type="button" class="btn" data-action="rv-retry">Ask ${name} again</button>`;
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

function acceptStatus(v, sent, state, step) {
  const machine = esc(v.name);
  const what = `step ${esc(step.number)} · ${esc(short(step.sha))}`;
  if (APPROVAL_STATES.includes(state)) return approvalStatus(sent, state, { standing: v.acceptSent.standing, now: v.now, what: { goes: `Step ${esc(step.number)} is accepted`, again: "accept again" } });
  if (state === "done") {
    const next = (v.item.steps?.items ?? []).find((one) => one.number > step.number && one.state !== "accepted");
    const later = next ? `Step ${esc(next.number)} is ${next.state === "committed" ? "next to accept" : esc(STEP_LABELS[next.state] || "planned")}. ` : "";
    return block("done", `✓ Accepted ${what}`, `${later}herdr shows step ${esc(step.number)} accepted too.`, from(sent));
  }
  if (state === "refused") return block("refused", ...refusal("Not accepted", sent.result.text), from(sent));
  if (state === "expired") return block("refused", `Not delivered: ${machine} stayed asleep past the hour`, `Accept again when ${machine} is awake.`, `Sent ${from(sent)} · expired ${esc(clock(sent.exp))}`);
  if (state === "waiting") return block("wait", `Waiting for ${machine}`, `Step ${esc(step.number)} is accepted when ${machine} wakes up.`, `Sent ${from(sent)} · expires ${esc(until(sent.exp, v.now))}`);
  return block("wait", `Sent to ${machine}`, `${machine} accepts ${what}.`, `Sent ${from(sent)} · expires ${esc(until(sent.exp, v.now))}`);
}

export function acceptArea(v) {
  const step = v.review?.step;
  if (!step) return "";
  const onBoard = (v.item.steps?.items ?? []).find((one) => one.number === step.number);
  const sent = v.acceptSent?.sent ?? null;
  const shown = v.acceptSent?.state ?? null;
  if (sent && !settled(shown)) return acceptStatus(v, sent, shown, step);
  if (onBoard?.state === "accepted") return `<div class="meta ok">✓ Step ${esc(step.number)} is accepted.</div>`;
  if (onBoard && onBoard.state !== "committed") return "";
  const status = sent ? acceptStatus(v, sent, shown, step) : "";
  if (v.access !== "ready") return status;
  if (!takes(v.accepts, "accept-step")) return status + `<div class="pro">${esc(v.name)} takes accepts only at the machine. Accept step ${esc(step.number)} in kebbie there.</div>`;
  const error = v.ui.error ? `<div class="center warn" role="alert">${esc(v.ui.error)}</div>` : "";
  const busy = v.ui.busy ? " disabled" : "";
  if (v.ui.confirming) {
    const sentCount = v.sentComments.length;
    const facts = `${readCount(v)} of ${v.review.files.length} files read, ${plural(sentCount, "comment", "comments")} sent. ${esc(v.name)} records it as <code>a</code> does in herdr.`;
    return status + `<div class="confirm" role="group" aria-label="Confirm accept"><p><b>Accept step ${esc(step.number)} · ${esc(short(step.sha))}?</b></p><p>${esc(step.subject)}</p><p class="meta">${facts}</p><div class="row2"><button type="button" class="btn" data-action="rv-accept-cancel"${busy}>Cancel</button><button type="button" class="btn primary" data-action="rv-accept"${busy}>Accept</button></div></div>` + error;
  }
  return status + `<button type="button" class="btn primary" data-action="rv-accept-ask"${busy}>Accept step ${esc(step.number)} · ${esc(short(step.sha))}</button>` + error;
}

const decideOn = (v, html, step = null) => `<div class="decide" data-machine="${esc(v.machine)}" data-address="${esc(v.address)}"${step === null ? "" : ` data-step="${esc(step)}"`}>${html}</div>`;
const GONE = ["refused", "expired", "not-approved", "denied"];

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

function laterStep(v, step) {
  const later = (v.item.steps?.items ?? []).find((one) => one.number > step.number && one.state !== "accepted");
  if (!later) return "";
  if (later.state !== "committed") return `<div class="meta">Step ${esc(later.number)} is being built</div>`;
  const files = v.item.review?.step === later.number ? ` · ${plural(v.item.review.files, "file", "files")}` : "";
  return `<div class="lead-act"><a class="btn fit" href="${reviewHref(v.machine, v.address)}">Review step ${esc(later.number)}${files}</a></div>`;
}

export function nextAction(v) {
  const step = v.review?.step ?? null;
  const waits = v.sentComments.some((comment) => !GONE.includes(comment.state));
  const waiting = `<div class="meta">Waiting for Claude's changes</div>`;
  if (!step) return waits ? waiting : wholePush(v);
  if ((v.item.steps?.pushed ?? 0) >= step.number && !v.stepPush) return laterStep(v, step);
  const onBoard = (v.item.steps?.items ?? []).find((one) => one.number === step.number);
  const justAccepted = v.acceptSent?.state === "done" && onBoard?.state === "committed";
  const accepted = onBoard?.state === "accepted" || v.acceptSent?.state === "done" || Boolean(v.stepPush);
  if (!accepted) return waits ? waiting : acceptArea(v);
  const receipt = justAccepted ? acceptArea(v) : "";
  const later = (v.item.steps?.items ?? []).some((one) => one.number > step.number && one.state !== "accepted");
  if (!later && pushReady(v.item) && !v.stepPush) return receipt + wholePush(v);
  const shown = v.stepPush;
  const push = pushArea(v, `step ${esc(step.number)}`, () => decideOn(v, stepPushBlock({ step, machineName: v.name, ui: v.pushUi, sent: shown?.sent ?? null, state: shown?.state ?? null, standing: shown?.standing ?? null }), step.number));
  return receipt + push + laterStep(v, step);
}

export function staleBlock(v) {
  const { stale } = v.ui;
  if (!stale) return "";
  const rows = stale.comments.map((comment) => `<div class="opt rv-c"><span class="mono meta">${esc(commentWhere(comment))}</span><span>${esc(comment.text)}</span></div>`).join("");
  return `<section class="task rv-stale" role="status"><div class="name">Not sent: ${plural(stale.comments.length, "comment", "comments")} on ${esc(stale.label)}</div><div class="meta">The diff changed since you wrote ${stale.comments.length === 1 ? "it" : "them"}, so ${stale.comments.length === 1 ? "it" : "they"} may point at other lines. Copy what still holds into a new comment.</div>${rows}<button type="button" class="btn" data-action="rv-stale-dismiss">Dismiss</button></section>`;
}

export function reviewOverview(v, { split, top }) {
  const since = v.review ? ` · since ${esc(v.review.since)}` : "";
  const head = top("Review", chip(v)) + `<div class="meta pad">${[esc(v.title) + since, counts(v.item)].filter(Boolean).join(" · ")}</div>`;
  if (!v.review) {
    const wait = split ? `<div class="meta pad">The guide and the files come with the diff.</div>` : fetchBlock(v);
    return `<div class="narrow">${head}${stepsCard(v)}${wait}</div>`;
  }
  const files = v.review.files.map((file, index) => fileRow(v, file, index)).join("");
  const read = split ? "" : `<a class="btn" href="${diffHref(v.machine, v.address)}">Read the diff · ${plural(v.review.files.length, "file", "files")}</a>`;
  const end = split && readCount(v) === v.review.files.length ? "" : nextAction(v);
  return `<div class="narrow">${head}${staleBlock(v)}${guideCard(v.review)}${stepsCard(v)}${messageCard(v.review.step)}<h3 class="sec">Files · ${readCount(v)} of ${v.review.files.length} read</h3>${files}${read}</div>${end ? `<div class="rv-end">${end}</div>` : ""}`;
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
  const drafts = v.ui.drafts.filter((comment) => comment.path === file.path && comment.id !== composing?.id);
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
  const notes = v.ui.drafts.filter((comment) => comment.path === file.path).length + sentHere(v, file.path).length;
  const meta = [stat(file), read ? `<span class="ok">read</span>` : "", notes ? plural(notes, "comment", "comments") : "", open ? "" : "collapsed"].filter(Boolean).join(" · ");
  const fold = `<button type="button" class="rv-icon rv-fold${open ? " is-open" : ""}" data-action="rv-fold" data-file="${index}" aria-expanded="${open}" aria-label="${open ? "Collapse" : "Expand"} ${esc(file.path)}">${ICON_CHEVRON}</button>`;
  const name = `<button type="button" class="rv-fh-name" data-action="rv-fold" data-file="${index}"><span class="mono">${esc(file.path)}</span><span class="meta">${meta}</span></button>`;
  const check = `<button type="button" class="rv-icon rv-check" data-action="rv-read" data-file="${index}" aria-pressed="${read}" aria-label="${read ? "Mark unread" : "Mark read"}"><span class="box">${ICON_CHECK}</span></button>`;
  return `<section class="rv-fsec" id="rv-f-${index}"><header class="rv-fh${read ? " is-read" : ""}">${fold}${name}${check}</header>${open ? diffCard(v, file, index) : ""}</section>`;
}

export function reviewDiff(v, { inPanel, top }) {
  const title = v.review?.step?.title ?? v.title;
  if (!v.review) return inPanel ? `${chip(v)}<h2>${esc(title)}</h2>${fetchBlock(v)}` : `${top("Diff", chip(v))}${fetchBlock(v)}`;
  const { files, step } = v.review;
  const add = files.reduce((sum, file) => sum + file.add, 0);
  const del = files.reduce((sum, file) => sum + file.del, 0);
  const which = step ? `Step ${esc(step.number)} · ${esc(short(step.sha))}` : `Since ${esc(v.review.since)}`;
  const facts = `<div class="meta pad">${which} · ${plural(files.length, "file", "files")} · <span class="rv-add">+${add}</span> <span class="rv-del">−${del}</span> · ${readCount(v)} of ${files.length} read</div>`;
  const head = (inPanel ? `${chip(v)}<h2>${esc(title)}</h2>${facts}` : `${top("Diff", chip(v))}${facts}`) + (inPanel ? "" : staleBlock(v));
  const next = readCount(v) === files.length ? nextAction(v) : "";
  const end = `<div class="meta pad rv-endnote">End of ${step ? `step ${esc(step.number)}` : "the changes"}.</div>${next ? `<div class="rv-end">${next}</div>` : ""}`;
  return head + files.map((file, index) => fileSection(v, file, index)).join("") + end;
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
    ? `<div class="rv-traypop" role="dialog" aria-label="Comments to send"><div class="row"><b>To send · ${drafts.length}</b><button type="button" class="sub-btn rv-tap" data-action="rv-tray">Close</button></div>${drafts.map((comment) => `<div class="opt rv-c"><span class="mono meta">${esc(commentWhere({ ...comment, sha: v.review?.step?.sha ?? null }))}</span><span>${esc(comment.text)}</span><div class="rv-acts"><button type="button" class="sub-btn rv-tap" data-action="rv-show" data-id="${esc(comment.id)}">Show</button><button type="button" class="sub-btn rv-tap" data-action="rv-edit" data-id="${esc(comment.id)}">Edit</button><button type="button" class="sub-btn rv-tap rv-remove" data-action="rv-remove" data-id="${esc(comment.id)}">Remove</button></div></div>`).join("")}<div class="meta">They go to ${esc(v.task)}'s Claude in one message.</div></div>`
    : "";
  const error = v.ui.error && !v.ui.confirming ? `<div class="center warn rv-senderror" role="alert">${esc(v.ui.error)}</div>` : "";
  const busy = v.ui.busy ? " disabled" : "";
  return `${BAND}<div class="rv-sendbar">${tray}${error}<div class="rv-pill"><button type="button" class="rv-pill-list" data-action="rv-tray" aria-expanded="${v.ui.tray}" aria-label="Show the comments to send">${ICON_LIST}</button><button type="button" class="rv-pill-send" data-action="rv-send"${busy}>Send ${plural(drafts.length, "comment", "comments")}</button></div></div>`;
}
