import { acceptRequired, asksYou } from "./board.js";
import { connectBlock, connectBlockText } from "./connect.js";
import { CATEGORY_GROUPS, switchable } from "./notify.js";

export const esc = (value) => String(value ?? "").replace(/[&<>"']/g, (char) => `&#${char.charCodeAt(0)};`);

export const brandMark = `<svg class="brand-mark" viewBox="0 0 32 32" aria-hidden="true"><rect class="mark-bg" width="32" height="32" rx="9" fill="#a8501f"/><g transform="translate(16 16) scale(0.66) translate(-16 -16)"><path d="M23 18V12.5A7.5 7.5 0 0 0 8 12.5A4.6 4.6 0 0 0 17.2 12.5" fill="none" stroke="#ffffff" stroke-width="3.8" stroke-linecap="round" stroke-linejoin="round"/><path d="M14.41 22.55L17.30 25.20L14.41 27.85" fill="none" stroke="#ffffff" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"/><rect class="mark-dot" x="20.5" y="20.8" width="5" height="8.8" fill="#ffffff"/></g></svg>`;

export const brand = `<header class="brand">${brandMark}<b>kebbie hub</b></header>`;
export const heading = (title, lead) => `<h1 class="title">${esc(title)}</h1><p class="lead">${esc(lead)}</p>`;
export const warn = (text) => (text ? `<div class="center warn" role="alert">${esc(text)}</div>` : "");
export const disabled = (off) => (off ? " disabled" : "");
const art = `<div class="auth-art" aria-hidden="true"><div class="art">${brandMark}<div class="art-rows">${`<div class="art-row"><i></i><b></b><u></u></div>`.repeat(3)}</div></div></div>`;

// The intro reads from the top, the form sits low where the thumb is, and the art fills what lies between.
export function authScreen({ intro, fields = "", actions, alt = "", notice = null }) {
  const form = `${fields ? `<div class="auth-fields">${fields}</div>` : ""}<div class="auth-actions">${actions}${warn(notice)}${alt ? `<div class="auth-alt">${alt}</div>` : ""}</div>`;
  return `<div class="auth-intro">${brand}${intro}</div>${art}<div class="auth-form">${form}</div>`;
}

export const isAuthScreen = (html) => html.startsWith('<div class="auth-intro">');
const status = (text, tone = "") => `<div class="center${tone ? ` ${tone}` : ""}">${text}</div>`;
const MACHINE_ICON = `<svg class="icon" viewBox="0 0 24 24" aria-hidden="true"><rect x="4" y="5" width="16" height="11" rx="1.5"/><path d="M2 19h20"/></svg>`;

export function lastSeenText(seconds, now = Date.now() / 1000) {
  if (!seconds) return "never";
  const date = new Date(seconds * 1000);
  const time = date.toLocaleTimeString("en-GB", { hour: "2-digit", minute: "2-digit" });
  const today = new Date(now * 1000);
  if (date.toDateString() === today.toDateString()) return time;
  if (now - seconds < 6 * 86_400) return `${date.toLocaleDateString("en-GB", { weekday: "short" })} ${time}`;
  return `${date.toLocaleDateString("en-GB", { day: "numeric", month: "short" })} ${time}`;
}

export function sleepingRow(machine, now) {
  const working = machine.working ? ` · ${machine.working} ${machine.working === 1 ? "task was" : "tasks were"} working then` : "";
  return `<div class="task asleep"><div class="name">${esc(machine.name)}</div><div class="meta">asleep · last seen ${esc(lastSeenText(machine.lastSeen, now))}${working}</div></div>`;
}

export const STEP_MARKS = { planned: "○", "in-progress": "◐", ready: "●", accepted: "✓", pushed: "↑" };
export const OPENABLE_STEPS = ["ready", "accepted"];
const SHOWN_PLANNED = 2;
const CHEV_RIGHT = `<svg class="chev" viewBox="0 0 24 24" aria-hidden="true"><path d="M9 6l6 6-6 6"/></svg>`;
const CHEV_DOWN = `<svg class="chev" viewBox="0 0 24 24" aria-hidden="true"><path d="M7 10l5 5 5-5"/></svg>`;
const LOCK = `<svg class="lock" viewBox="0 0 24 24" aria-hidden="true"><rect x="5" y="11" width="14" height="9" rx="2"/><path d="M8 11V8a4 4 0 0 1 8 0v3"/></svg>`;
const CHEV_UP = `<svg class="chev" viewBox="0 0 24 24" aria-hidden="true"><path d="M7 14l5-5 5 5"/></svg>`;

export function waited(seconds) {
  if (seconds < 60) return "now";
  if (seconds < 3600) return `${Math.floor(seconds / 60)} min`;
  if (seconds < 86_400) return `${Math.floor(seconds / 3600)} h`;
  return `${Math.floor(seconds / 86_400)} d`;
}

export const taskName = (item, task) => item?.title || task;

export function agentChip(agent, withModel = false) {
  if (!agent?.kind) return "";
  const said = withModel && agent.model ? `${agent.kind} · ${agent.model}` : agent.kind;
  return `<span class="chip ${esc(agent.kind)}">${esc(said)}</span>`;
}

const recipeText = (item) => (item.recipe ? `recipe: ${item.recipe}` : "");

const NEXT_PARTS = [
  { key: "stopped", title: "Stopped on you" },
  { key: "push", title: "Push, in this order" },
  { key: "read", title: "To read" },
];
const NEXT_ICONS = { push: "↑", read: "◆" };
const nextIcon = (row) => NEXT_ICONS[row.part] ?? (asksYou(row.item) ? "?" : "!");

export function countsLine(counts) {
  if (!counts) return "";
  return [counts.working && `${counts.working} working`, counts.queued && `${counts.queued} queued`, counts.parked && `${counts.parked} parked`].filter(Boolean).join(" · ");
}

function nextRows(list, now, selected) {
  const parts = NEXT_PARTS.map((part) => ({ ...part, rows: list.rows.filter((row) => row.part === part.key) })).filter((part) => part.rows.length > 0);
  return parts
    .map((part) => {
      const rows = part.rows.map((row) => {
        const href = `#/task/${encodeURIComponent(row.machine)}/${encodeURIComponent(row.address)}`;
        const age = row.since ? waited(Math.max(0, now - row.since)) : "";
        const on = selected === taskKey(row.machine, row.address);
        return `<a class="nrow${on ? " sel" : ""}" href="${href}"${on ? ' aria-current="true"' : ""}><span class="n">${esc(row.rank)}</span><span aria-hidden="true">${nextIcon(row)}</span><span class="t"><b>${esc(taskName(row.item, row.task))}</b> ${agentChip(row.item?.agent)}<small>${esc(row.line)}</small></span><span class="w">${esc(age)}</span></a>`;
      });
      return `<div class="nsub">${esc(part.title)}</div>${rows.join("")}`;
    })
    .join("");
}

export function nextList(lists, now, selected = null) {
  if (lists.length === 0) return "";
  const many = lists.length > 1;
  const blocks = lists.map((list) => {
    const name = many ? `<div class="nmachine">${esc(list.name)}</div>` : "";
    const body = list.rows.length ? nextRows(list, now, selected) : `<div class="meta">Nothing needs you.</div>`;
    const foot = [countsLine(list.counts), list.rows.length ? "same order as N on the board" : ""].filter(Boolean).join(" · ");
    return `${name}${body}${foot ? `<div class="meta nfoot">${esc(foot)}</div>` : ""}`;
  });
  return `<section class="next" aria-label="Next">${blocks.join("")}</section>`;
}

export function laterRow(row, now) {
  const { later } = row;
  const until = later.until ? `until ${lastSeenText(later.until, now)}` : "";
  const meta = [row.repo, later.line, until, later.question ? "a question waits" : ""].filter(Boolean).map(esc).join(" · ");
  return `<div class="task"><div class="head"><span class="name">${esc(later.title || row.task)}</span> <span class="machine">${esc(row.name)}</span></div><div class="meta">${meta}</div></div>`;
}

export function laterGroups(groups, now) {
  return groups.map((group) => `<section class="col"><h2 class="group g-muted">${esc(group.title)} · ${group.rows.length}</h2>${group.rows.map((row) => laterRow(row, now)).join("")}</section>`).join("");
}

export function routineLines(routines, now) {
  const lines = routines.map(({ name, routine }) => {
    const parts = [routine.title, routine.repo, routine.next_run ? `next ${lastSeenText(routine.next_run, now)}` : "not scheduled", routine.last_outcome ? `last run ${routine.last_outcome}` : "", name];
    return `<div class="meta">${parts.filter(Boolean).map(esc).join(" · ")}</div>`;
  });
  return `<div class="task">${lines.join("")}</div>`;
}

export function routinesBlock(routines, now) {
  if (routines.length === 0) return "";
  return `<section class="col"><h2 class="group g-muted">Routines · ${routines.length}</h2>${routineLines(routines, now)}</section>`;
}

export function awayLine(away, now) {
  if (away.length === 0) return "";
  const said = away.map((machine) => `${machine.name} is away since ${lastSeenText(machine.since, now)}`).join(" · ");
  return `<div class="center warn">${esc(said)}</div>`;
}

export function assessmentText(assessment) {
  if (!assessment) return "";
  if (typeof assessment === "string") return assessment;
  if (assessment.word === "recommended") return "full review recommended";
  if (assessment.word === "not-needed") return "no full review needed";
  return "assessing the review";
}

export function taskFacts(item) {
  const agent = agentChip(item.agent, true);
  const profile = item.profile ? `profile ${esc(item.profile)}` : "";
  const parts = [agent, profile, esc(recipeText(item))].filter(Boolean);
  return parts.length ? `<div class="meta">${parts.join(" · ")}</div>` : "";
}

export function reviewLines(item) {
  const lines = [];
  if (item.assessment && item.assessment.word !== "recommended") lines.push(`<div>${esc(assessmentText(item.assessment))}</div>`);
  const verdict = item.verdict;
  const said = [];
  if (verdict) {
    const text = verdict.running ? "reviewer running" : verdict.verdict === "ok" ? `reviewer <span class="ok">✓ ok</span>` : verdict.verdict === "changes" ? `reviewer: ${esc(plural(verdict.findings ?? 0, "finding", "findings"))}` : "reviewer gave no verdict";
    said.push(`${text}${verdict.stale ? " · stale" : ""}`);
  }
  const second = item.second_opinion;
  if (second) said.push(`second opinion ${esc(second.by)}: ${second.running ? "running" : esc(plural(second.points ?? 0, "point", "points"))}`);
  if (said.length) lines.push(`<div>${said.join(" · ")}</div>`);
  const held = verdict?.held;
  const holds = held === true ? "the push waits for the reviewer's pass" : held === false ? "the push does not wait for the reviewer" : "";
  const note = [holds, second ? "the second opinion never holds it" : ""].filter(Boolean).join("; ");
  if (note) lines.push(`<div class="meta">${esc(note)}</div>`);
  return lines.length ? `<h3 class="sec">Review</h3><div class="plan">${lines.join("")}</div>` : "";
}

const plural = (count, one, many) => `${count} ${count === 1 ? one : many}`;

export const cardAge = (card) => (card.current ? "" : `<div class="meta warn">Written before the last commit; the code changed since.</div>`);

const OVERRIDDEN_BY = { user: "you", lead: "the lead" };

export function rulesLines(rules, { expandable = false } = {}) {
  if (!rules || rules.state === "ok") return "";
  const count = `breaks ${esc(rules.breaks)} project ${rules.breaks === 1 ? "rule" : "rules"}`;
  const names = expandable && rules.names?.length ? `<details class="rules-names"><summary class="warn">${count}</summary><ul>${rules.names.map((name) => `<li>${esc(name)}</li>`).join("")}</ul></details>` : `<div class="warn">${count}</div>`;
  const broken = rules.state === "breaks" ? names : "";
  const by = OVERRIDDEN_BY[rules.override?.by];
  const said = by ? `<div class="meta">pushed past the rules check by ${by}</div>` : rules.state === "checking" ? `<div class="meta">checking project rules</div>` : "";
  return `<div class="rules">${broken}${said}</div>`;
}

const commitCount = (part) => plural(part.commits.length, "commit", "commits");
const titleOf = (item, id) => item.subtasks?.find((one) => one.id === id)?.title ?? id;
const dim = (html) => `<span class="dim">${html}</span>`;

export const runCount = (item) => item.pushable?.count ?? 0;
export const pushedCount = (item) => Math.max(0, (item.subtasks ?? []).reduce((sum, one) => sum + one.commits.length, 0) - (item.order ?? []).length);

const stageAt = (item, id) => (item.stages ?? []).findIndex((stage) => stage.id === id);
export function heldBy(item, row) {
  const at = stageAt(item, row?.stage);
  return at < 0 ? null : (item.stages.slice(0, at).find((stage) => stage.state !== "met") ?? null);
}
const stageName = (item, stage) => `stage ${stageAt(item, stage.id) + 1}`;
const HOLD_VERB = { deploy: "has deployed", check: "passes its check", verified: "is verified" };

export function blockerOf(item) {
  const row = (item.order ?? []).slice(runCount(item)).find((one) => !one.accepted);
  const owner = row ? item.subtasks?.find((one) => one.id === row.subtask) : null;
  return owner && owner.state !== "accepted" ? owner : null;
}

const rowsOf = (item, subtask) => (item.order ?? []).filter((row) => row.subtask === subtask.id);
export function acceptedCommits(item, subtask) {
  const rows = new Map(rowsOf(item, subtask).map((row) => [row.sha, row]));
  return subtask.commits.filter((sha) => !rows.has(sha) || rows.get(sha).accepted === true).length;
}
const partlyAccepted = (item, subtask) => {
  const accepted = acceptedCommits(item, subtask);
  return accepted > 0 && accepted < subtask.commits.length ? `${accepted} of ${commitCount(subtask)} accepted` : "";
};

export function inRun(item, subtask) {
  const order = item.order ?? [];
  const rows = order.map((row, index) => [row, index]).filter(([row]) => row.subtask === subtask.id);
  return rows.length > 0 && rows.every(([, index]) => index < runCount(item));
}

export function subtaskLabel(item, subtask) {
  const partly = partlyAccepted(item, subtask);
  if (subtask.state === "ready") return esc(`${partly || commitCount(subtask)} · to review`);
  if (subtask.state === "pushed") return "pushed";
  if (subtask.state === "in-progress") return esc(subtask.commits.length ? `in progress · ${partly || commitCount(subtask)}` : "in progress");
  if (subtask.state !== "accepted") return "";
  if (inRun(item, subtask)) return "accepted · ready to push";
  const blocker = blockerOf(item);
  const waiting = (item.order ?? []).some((row) => row.subtask === subtask.id);
  const held = heldBy(item, rowsOf(item, subtask)[0]);
  if (waiting && held) return `accepted ${dim(`· waits for ${esc(stageName(item, held))}`)}`;
  return waiting && blocker ? `accepted ${dim(`· waits for ${esc(blocker.title)}`)}` : "accepted";
}

const addedLine = (subtask) => (subtask.added ? `<span class="added">added ${esc(lastSeenText(subtask.added.at))} · ${esc(subtask.added.reason)}</span>` : "");

function subtaskRow(item, subtask, href, on) {
  const label = subtaskLabel(item, subtask);
  const what = `<span class="what"><span class="mark">${STEP_MARKS[subtask.state] ?? ""}</span><span class="tt"><span class="step-title">${esc(subtask.title)}</span>${label ? `<span class="label">${label}</span>` : ""}${addedLine(subtask)}</span></span>`;
  if (href && OPENABLE_STEPS.includes(subtask.state)) return `<a class="step s-${esc(subtask.state)}${on ? " on" : ""}" href="${esc(href(subtask.id))}"${on ? ' aria-current="true"' : ""}>${what}${CHEV_RIGHT}</a>`;
  return `<div class="step s-${esc(subtask.state)}">${what}</div>`;
}

export function subtaskList(item, { href = null, selected = null, more = false, expandable = false } = {}) {
  const subtasks = item.subtasks ?? [];
  if (!subtasks.length) return "";
  let planned = 0;
  let folded = 0;
  const rows = [];
  for (const subtask of subtasks) {
    if (subtask.state === "planned") planned += 1;
    if (subtask.state === "planned" && planned > SHOWN_PLANNED && !more) {
      folded += 1;
      continue;
    }
    rows.push(subtaskRow(item, subtask, href, subtask.id === selected) + rulesLines(subtask.rules, { expandable }));
  }
  const extra = Math.max(0, planned - SHOWN_PLANNED);
  const fold = folded ? `<button type="button" class="more" data-action="steps-more" aria-expanded="false">${esc(folded)} more planned${CHEV_DOWN}</button>` : more && extra ? `<button type="button" class="more" data-action="steps-more" aria-expanded="true">Fewer planned${CHEV_UP}</button>` : "";
  const optional = acceptRequired(item) ? "" : `<div class="meta">accepting each subtask is optional: push takes what is committed</div>`;
  return `<div class="steps go">${rows.join("")}${fold}${optional}</div>`;
}

const TESTS_WORDS = { passed: "tests ✓", failed: "tests ✕", running: "tests running", "not-run": "tests –" };
const TESTS_TITLES = { passed: "tests passed", failed: "tests failed", running: "tests running", "not-run": "not tested" };
const testsMark = (tests) => (tests && TESTS_WORDS[tests.state] ? ` · <span class="tests t-${esc(tests.state)}" title="${TESTS_TITLES[tests.state]}">${esc(TESTS_WORDS[tests.state])}</span>` : "");
const rowState = (item, row) => (row.accepted ? "accepted" : (item.subtasks.find((one) => one.id === row.subtask)?.state ?? "planned"));

function orderLabel(item, row, index) {
  const owner = item.subtasks.find((one) => one.id === row.subtask);
  const title = esc(owner?.title ?? row.subtask);
  const order = item.order ?? [];
  const count = runCount(item);
  const blocking = order.findIndex((one, at) => at >= count && !one.accepted);
  const held = heldBy(item, row);
  if (row.accepted && index >= count && held) return `${title} ${dim(`· waits for ${esc(stageName(item, held))}`)}`;
  if (row.accepted && index >= count && blocking >= 0) return `${title} ${dim(`· waits for no. ${blocking + 1}`)}`;
  if (row.accepted) return owner?.state === "accepted" ? title : `${title} · accepted`;
  if (owner?.state === "ready") return `${title} · to review`;
  if (owner?.state === "in-progress") return `${title} · in progress`;
  return title;
}

function orderRow(item, row, index) {
  const state = rowState(item, row);
  const moved = row.moved ? `<span class="moved">moved up · ${esc(row.moved.reason)}</span>` : "";
  return `<div class="step s-${esc(state)}"><span class="what"><span class="mark">${STEP_MARKS[state] ?? ""}</span><span class="number">${index + 1}</span><span class="tt"><span class="step-title">${esc(row.subject)}</span><span class="label">${orderLabel(item, row, index)}${testsMark(row.tests)}</span>${moved}</span></span><span class="mono meta">${esc(row.sha.slice(0, 7))}</span></div>`;
}

function stageHead(item, row) {
  const stage = (item.stages ?? [])[stageAt(item, row.stage)];
  return stage ? `<div class="stage-h">${esc(stageName(item, stage))} · ${esc(stage.title)}${stage.state === "locked" ? ` ${LOCK}` : ""}</div>` : "";
}

function orderRows(item, rows, from) {
  const html = [];
  for (let at = 0; at < rows.length; ) {
    const before = at === 0 ? item.order[from - 1] : rows[at - 1];
    if (rows[at].stage && rows[at].stage !== before?.stage) html.push(stageHead(item, rows[at]));
    const group = rows[at].group;
    let end = at + 1;
    while (group && end < rows.length && rows[end].group === group) end += 1;
    const shown = rows.slice(at, end).map((row, offset) => orderRow(item, row, from + at + offset)).join("");
    html.push(group ? `<div class="together" role="group" aria-label="goes out together"><span class="together-h">goes out together</span>${shown}</div>` : shown);
    at = end;
  }
  return html.join("");
}

export function pushOrder(item) {
  const order = item.order ?? [];
  if (!item.subtasks || !order.length) return "";
  const count = runCount(item);
  const names = (item.pushable?.subtasks ?? []).map((id) => titleOf(item, id));
  const run = count ? `<div class="run"><div class="run-h"><b>Ready to push: ${esc(plural(count, "commit", "commits"))}</b><span>${esc(`${names.length === 1 ? "subtask" : "subtasks"} ${names.join(", ")}`)}</span></div>${orderRows(item, order.slice(0, count), 0)}</div>` : "";
  return `<div class="steps go order">${run}${orderRows(item, order.slice(count), count)}</div>`;
}

const OBSTACLE_WORDS = { investigating: "obstacle: being investigated", asked: "obstacle: the investigator asks you", "with-lead": "obstacle: with the lead" };
const DECIDED_BY = { investigator: "Investigator", lead: "The lead", you: "You" };
const DECISION = { subtask: "add a preparatory subtask first", workaround: "work around it", task: "a separate task takes it", ask: "asks you" };

export function obstacleBlock(item, now = Date.now() / 1000) {
  const obstacle = item.obstacle;
  if (!obstacle) return "";
  const said = `<div class="said">${esc(obstacle.text)}<div class="meta">raised by the task · ${esc(lastSeenText(obstacle.at, now))}</div></div>`;
  const decision = obstacle.decision;
  let verdict = "";
  if (decision) verdict = `<div class="sent ${obstacle.state === "decided" ? "done" : "wait"}" role="status"><b>${esc(DECIDED_BY[decision.by])}: ${esc(DECISION[decision.kind])}</b><span>${esc(decision.text)}</span></div>`;
  else if (obstacle.state === "investigating") verdict = `<div class="sent wait" role="status"><b><span class="spinner"></span>&nbsp; The investigator is looking into it</b><span>The task waits for its ruling.</span></div>`;
  else if (obstacle.state === "with-lead") verdict = `<div class="sent wait" role="status"><b>With the lead</b><span>The lead decides how the task goes on.</span></div>`;
  const added = obstacle.state === "decided" && decision?.kind === "subtask" ? (item.subtasks ?? []).filter((one) => one.added && one.added.at >= obstacle.at).sort((a, b) => b.added.at - a.added.at)[0] : null;
  const change = added ? `<div class="change"><span class="mark">+</span><span>Subtask added: <b>${esc(added.title)}</b></span></div>` : "";
  return `<h3 class="sec">Obstacle</h3><div class="obs">${said}${verdict}${change}</div>`;
}

const at = (seconds, now) => {
  const date = new Date(seconds * 1000);
  const time = date.toLocaleTimeString("en-GB", { hour: "2-digit", minute: "2-digit" });
  return date.toDateString() === new Date(now * 1000).toDateString() ? time : `${date.toLocaleDateString("en-GB", { day: "numeric", month: "short" })} ${time}`;
};
const MET = { deploy: "✓ deploy succeeded", check: "✓ check passed", verified: "✓ verified by you" };
const WAITS = { deploy: "deploy", check: "check", verified: "verified by you" };
const FAILED = { deploy: "✕ deploy failed", check: "✕ check failed", verified: "✕ something is wrong" };
const STAGE_MARKS = { open: "→", pushed: "↑", waiting: "?", met: "↑", failed: "!" };

export function stageLabel(item, stage, now = Date.now() / 1000) {
  const kind = stage.requirement.kind;
  const commits = (item.order ?? []).filter((row) => row.stage === stage.id).length;
  const left = commits ? plural(commits, "commit", "commits") : "";
  const pushed = stage.pushed_at ? `pushed ${at(stage.pushed_at, now)}` : "";
  const detail = kind === "check" ? `: ${stage.requirement.detail}` : "";
  if (stage.state === "locked") {
    const held = item.stages.slice(0, stageAt(item, stage.id)).find((one) => one.state !== "met");
    return esc([`locked until ${stageName(item, held)} ${HOLD_VERB[held.requirement.kind]}`, left].filter(Boolean).join(" · "));
  }
  if (stage.state === "open") return esc(["open", left ? `${left} to push` : ""].filter(Boolean).join(" · "));
  if (stage.state === "met") return `${esc([pushed, left ? `${left} more` : ""].filter(Boolean).join(" · "))} · <span class="ok">${esc(MET[kind])}</span>`;
  if (stage.state === "failed") return `${esc(pushed)} · <span class="warn">${esc(FAILED[kind])}</span>`;
  if (stage.state === "waiting") return esc(`${pushed} · waiting: ${WAITS[kind]}${detail}`);
  return esc(`${pushed} · next: ${WAITS[kind]}${detail}`);
}

export function stageList(item, now = Date.now() / 1000) {
  const stages = item.stages ?? [];
  if (!stages.length) return "";
  const rows = stages.map((stage, index) => {
    const mark = stage.state === "locked" ? LOCK : esc(STAGE_MARKS[stage.state] ?? "");
    const note = stage.note ? `<span class="note${stage.state === "failed" ? " warn" : ""}">${esc(stage.note)}</span>` : "";
    return `<div class="step st-${esc(stage.state)}"><span class="what"><span class="mark">${mark}</span><span class="number">${index + 1}</span><span class="tt"><span class="step-title">${esc(stage.title)}</span><span class="label">${stageLabel(item, stage, now)}</span>${note}</span></span></div>`;
  });
  return `<h3 class="sec">Stages · ${esc(stages.length)}</h3><div class="steps go stages">${rows.join("")}</div>`;
}

export function stageLine(item) {
  const stages = item.stages ?? [];
  const last = stages.findLastIndex((stage) => stage.through);
  const stage = stages[last];
  const next = stages[last + 1];
  if (!stage) return stages[0]?.state === "open" ? "stage 1 open" : "";
  const name = `stage ${last + 1}`;
  if (stage.state === "waiting") return `<span class="accept">${esc(name)} waits: ${esc(WAITS[stage.requirement.kind])}</span>`;
  if (stage.state === "failed") return `<span class="warn">${esc(name)}: ${esc(FAILED[stage.requirement.kind])}</span>`;
  if (stage.state === "met" && next) return `<span class="ok">✓ ${esc(name)} done</span> · stage ${esc(last + 2)} ${next.state === "open" ? "open" : "locked"}`;
  if (stage.state === "met") return `<span class="ok">✓ every stage done</span>`;
  return `${esc(name)} pushed`;
}

export function stepSummary(item) {
  const subtasks = item.subtasks ?? [];
  const of = (...states) => subtasks.filter((one) => states.includes(one.state)).length;
  const [accepted, ready, building] = [of("accepted", "pushed"), of("ready"), of("in-progress")];
  const total = subtasks.length;
  const parts = [];
  if (accepted) parts.push(`<span class="ok">✓ ${esc(accepted)} of ${esc(total)} subtasks accepted</span>`);
  if (ready) parts.push(`<span class="accept">◆ ${accepted ? `${esc(ready)} to review` : `${esc(ready)} of ${esc(total)} subtasks to review`}</span>`);
  if (building) parts.push(`<span class="prog">${esc(building)} in progress</span>`);
  if (total && !ready && !building && pushedCount(item)) parts.push(`↑ ${esc(plural(pushedCount(item), "commit", "commits"))} pushed`);
  const stage = stageLine(item);
  if (stage) parts.push(stage);
  if (OBSTACLE_WORDS[item.obstacle?.state]) parts.push(`<span class="warn">${OBSTACLE_WORDS[item.obstacle.state]}</span>`);
  return parts.length ? `<div class="step-line">${parts.join(" · ")}</div>` : "";
}

export function stepsWanted(item, machineName) {
  const ready = (item.subtasks ?? []).find((one) => one.state === "ready");
  const kebbie = item.kebbie_questions?.[0];
  const verify = kebbie?.stage ? `verify ${stageName(item, item.stages[stageAt(item, kebbie.stage)])}` : kebbie?.kind === "verify" ? "verify a check" : kebbie ? "close the task" : "";
  const asks = [verify, runCount(item) ? `push ${plural(runCount(item), "commit", "commits")}` : "", ready ? `review ${ready.title}` : ""].filter(Boolean);
  return asks.length ? `${asks.join(" or ")} on ${machineName}` : null;
}

export const acceptCount = (count) => (count ? `<span class="accept">◆ ${esc(count)} to accept</span>` : "");

export const taskKey = (machine, address) => `${machine}|${address}`;

export function taskRow(row, selected = null) {
  const href = `#/task/${encodeURIComponent(row.machine)}/${encodeURIComponent(row.item.address)}`;
  const on = selected === taskKey(row.machine, row.item.address);
  const meta = [row.repo && esc(row.repo), agentChip(row.item.agent, true), esc(recipeText(row.item)), esc(assessmentText(row.item.assessment)), esc(row.item.summary)].filter(Boolean).join(" · ");
  const wanted = row.item.next_for_you ?? stepsWanted(row.item, row.name);
  const you = wanted ? `<div class="you"><b>you:</b> ${esc(wanted)}</div>` : "";
  return `<a class="task${on ? " sel" : ""}" href="${href}"${on ? ' aria-current="true"' : ""}><div class="head"><span class="name">${esc(taskName(row.item, row.task))}</span> <span class="machine">${esc(row.name)}</span></div><div class="meta">${meta}</div>${stepSummary(row.item)}${you}</a>`;
}

export const ICONS = {
  board: `<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M9 4v16M15 4v16"/>`,
  backlog: `<rect x="4" y="3.5" width="16" height="17" rx="2"/><path d="M8 8.5h8M8 12h8M8 15.5h5"/>`,
  questions: `<path d="M4 4h16a1 1 0 0 1 1 1v11a1 1 0 0 1-1 1h-9l-5 4v-4H4a1 1 0 0 1-1-1V5a1 1 0 0 1 1-1z"/><path d="M10 9a2 2 0 1 1 2.8 1.8c-.5.3-.8.7-.8 1.4"/><path d="M12 14.6v.1"/>`,
  lead: `<path d="M4 4.5h11a1 1 0 0 1 1 1v7a1 1 0 0 1-1 1H9l-4 3v-3H4a1 1 0 0 1-1-1v-7a1 1 0 0 1 1-1z"/><path d="M16 8.5h4a1 1 0 0 1 1 1v7a1 1 0 0 1-1 1h-1v3l-4-3h-4a1 1 0 0 1-1-1v-.5"/>`,
  machines: `<rect x="5" y="5" width="14" height="10" rx="1.5"/><path d="M3 19h18"/>`,
  back: `<path d="M15 5l-7 7 7 7"/>`,
  sections: `<rect x="3.5" y="4.5" width="17" height="15" rx="2.5"/><path d="M9.5 4.5v15M5.8 8.5h1.6M5.8 11.5h1.6"/>`,
  card: `<rect x="3.5" y="5" width="17" height="14" rx="2"/><path d="M3.5 9h17M7 12.5h7M7 15.5h4"/>`,
};

export const icon = (name) => `<svg class="tab-icon" viewBox="0 0 24 24" aria-hidden="true">${ICONS[name]}</svg>`;

export const backButton = (href) => `<a class="back-btn round" href="${esc(href)}" aria-label="Back">${icon("back")}</a>`;

const taskPath = (machine, address) => `${encodeURIComponent(machine)}/${encodeURIComponent(address)}`;
export const questionHref = (machine, address) => `#/task/${taskPath(machine, address)}`;
export const cardHref = (machine, address) => `#/card/${taskPath(machine, address)}`;

export const backRow = (back, button) => `<div class="back-row">${back}${button}</div>`;

export const cardButton = (href) => `<a class="round switch-btn" href="${esc(href)}" aria-label="Open the task's card">${icon("card")}</a>`;

export function questionButton(href, waiting = 0) {
  const said = waiting ? `Back to the question, ${waiting} waiting` : "Back to the question";
  return `<a class="round switch-btn" href="${esc(href)}" aria-label="${esc(said)}">${icon("questions")}${waiting ? `<em class="badge">${esc(waiting)}</em>` : ""}</a>`;
}

export function questionLine(item) {
  const text = item.question?.text ?? (item.prompt ? promptParts(item.prompt.question).question : (item.kebbie_questions?.[0]?.text ?? ""));
  return text.split("\n").map((line) => line.trim()).find(Boolean) ?? "";
}

export const questionWaits = (item, href) => `<div class="sent wait qwait" role="status"><span class="eyebrow">A question waits for you</span><span class="first">${esc(questionLine(item))}</span><a class="btn fit primary" href="${esc(href)}">Answer</a></div>`;

export const youLine = (item) => (item.next_for_you && !asksYou(item) ? `<div class="you"><b>you:</b> ${esc(item.next_for_you)}</div>` : "");

export function nav({ active, questions = 0, replies = 0, deskOnly = false }) {
  const tab = (href, label, key, said = label) => `<a href="${href}" aria-label="${esc(said)}" class="${active === key ? "on" : ""}"${active === key ? ' aria-current="page"' : ""}>${label}${icon(key)}</a>`;
  const badge = questions ? ` <em class="badge">${esc(questions)}</em>` : "";
  const account = `<a href="/account" class="account${active === "account" ? " on" : ""}"${active === "account" ? ' aria-current="page"' : ""}>Account</a>`;
  return `<nav class="tabbar${deskOnly ? " desk-nav" : ""}"><div class="side-brand brand">${brand.replace(/^<header class="brand">|<\/header>$/g, "")}</div>${tab("/#/board", "Board", "board")}${tab("/#/backlog", "Backlog", "backlog")}${tab("/#/questions", `Questions${badge}`, "questions", questions ? `Questions, ${questions} waiting` : "Questions")}${tab("/#/lead", `Lead${replies ? ` <em class="badge">${esc(replies)}</em>` : ""}`, "lead", replies ? `Lead, ${replies} new ${replies === 1 ? "reply" : "replies"}` : "Lead")}${tab("/#/machines", "Machines", "machines")}${account}</nav>`;
}

export function optionCards(options, recommend = null) {
  const cards = options.map((option, index) => {
    const recommended = Boolean(recommend) && index === recommend - 1;
    return `<div class="opt${recommended ? " rec" : ""}">${recommended ? `<span class="tag">Recommended</span>` : ""}<b>${esc(option.label)}</b>${option.meaning ? `<small>${esc(option.meaning)}</small>` : ""}</div>`;
  });
  return `<div class="options">${cards.join("")}</div>`;
}

export const machineAnswerNote = () => `<div class="pro">Answer at the machine: N on the board, or ask the lead.</div>`;

export function promptParts(text) {
  const lines = text.split("\n").map((line) => line.trim()).filter(Boolean);
  const question = lines.find((line) => line.endsWith("?")) ?? lines[0] ?? "";
  const command = [];
  const details = [];
  let inCommand = false;
  for (const line of lines) {
    if (line === question) inCommand = false;
    else if (line.startsWith("$ ")) {
      inCommand = true;
      command.push(line.slice(2));
    } else if (inCommand) command.push(line);
    else details.push(line.replace(/^Reason:\s*/, ""));
  }
  return { question, command: command.join(" "), details };
}

export const promptCards = (prompt) => optionCards(prompt.options.map((option) => ({ label: option.label, meaning: option.takes_text ? "with text" : "" })));

export function promptBlock({ item, repo }, answer = promptCards(item.prompt) + machineAnswerNote()) {
  const { question, command, details } = promptParts(item.prompt.question);
  const context = [esc(repo), agentChip(item.agent)].filter(Boolean).join(" · ");
  const detail = details.map((line) => `<div class="meta pad">${esc(line)}</div>`).join("");
  const cmd = command ? `<div class="cmd">${esc(command)}</div>` : "";
  return `<p class="q">${esc(question)}</p>${context ? `<div class="meta pad">${context}</div>` : ""}${detail}${cmd}${answer}`;
}

export function machineRow({ machine, name, awake, facts }) {
  const remove = `<button type="button" class="btn small" data-action="remove-machine" data-id="${esc(machine)}" data-name="${esc(name)}">Remove</button>`;
  return `<div class="mrow${awake ? "" : " asleep"}"><span class="dot${awake ? " awake" : ""}" aria-hidden="true"></span><div class="about"><b>${esc(name)}</b><small>${esc([awake ? "awake" : "asleep", ...facts].join(" · "))}</small></div>${remove}</div>`;
}

export function machineDots(machines) {
  const dots = machines.map(({ name, awake, lastSeen }) => `<span><i class="dot${awake ? " awake" : ""}" aria-hidden="true"></i>${esc(name)}${!awake && lastSeen ? ` · last seen ${esc(lastSeen)}` : ""}</span>`);
  return `<span class="dmach desk-only">${dots.join("")}</span>`;
}

export function githubHref({ next = null, connect = null, invite = null, setup = null, link = false } = {}) {
  const query = new URLSearchParams();
  if (link) query.set("link", "1");
  if (connect) query.set("connect", connect);
  if (invite) query.set("invite", invite);
  if (setup) query.set("setup", setup);
  if (next) query.set("next", next);
  const said = query.toString();
  return said ? `/auth/github?${said}` : "/auth/github";
}

const githubButton = (label, href) => `<div class="or"><span>or</span></div><a class="btn" href="${esc(href)}">${esc(label)}</a>`;

const GITHUB_ERRORS = {
  expired: "The GitHub sign-in expired or was already used. Try again.",
  denied: "GitHub sign-in was cancelled.",
  failed: "GitHub sign-in did not work. Try again.",
  "no-account": "No account on this hub uses this GitHub account. New to kebbie? Start in your terminal: kebbie setup",
  "signup-closed": "This hub makes new accounts by invite only.",
  unauthorized: "That link is used or expired. Ask for a new one.",
  "github-taken": "That GitHub account already belongs to another kebbie account.",
  "signed-out": "Sign in first, then connect GitHub.",
  deactivated: "This account is deactivated.",
};

export const githubErrorText = (code) => (code ? (GITHUB_ERRORS[code] ?? GITHUB_ERRORS.failed) : null);

export function connectWelcome({ name, code = "", signupOpen, github, email = false, notice = null, busy = false }) {
  const lead = signupOpen
    ? `The hub shows ${name}'s board on your phone and in any browser, end-to-end encrypted. Create a passkey to start: it signs you in and unlocks your boards wherever it's saved.`
    : `The hub shows ${name}'s board on your phone and in any browser, end-to-end encrypted. Sign in with your passkey to connect it.`;
  const back = `/connect?code=${encodeURIComponent(code)}`;
  const gh = github ? githubButton(signupOpen ? "Continue with GitHub" : "Sign in with GitHub", githubHref({ connect: signupOpen ? code : null, next: back })) : "";
  const intro = heading(`Connect ${name} to kebbie hub`, lead);
  if (!signupOpen) return authScreen({ intro, actions: `<button type="button" class="btn primary" data-action="sign-in"${disabled(busy)}>Sign in with passkey</button>`, alt: gh, notice });
  return authScreen({
    intro,
    fields: email ? emailField() : "",
    actions: `<button type="button" class="btn primary" data-action="create-passkey"${disabled(busy)}>Create passkey</button>`,
    alt: `${gh}<p class="foot">Already have a kebbie account? <button type="button" class="link inline" data-action="sign-in">Sign in</button></p>`,
    notice,
  });
}

export function connectAsk({ name, code, machines = [], blocked = null, typed = false, notice = null, busy = false }) {
  const already = machines.length ? `<section class="task small-card"><div class="meta">Already connected</div><div>${esc(machines.join(" · "))}</div></section>` : "";
  const typedLine = typed ? `<div class="center">You typed the code, so this page can't check which machine it is. Its board stays unreadable until you connect it from the link kebbie opens.</div>` : "";
  const reason = blocked ? `<div class="center warn" role="alert">${esc(blocked)}</div>` : "";
  return authScreen({
    intro: heading(`Connect ${name}?`, "Check that your terminal shows the same code. If it doesn't, cancel: another machine is asking."),
    fields: `<section class="task machine-card"><div class="row-start">${MACHINE_ICON}<div><div class="name">${esc(name)}</div><div class="meta">waiting in its terminal</div></div></div><div class="code peach" aria-live="polite">${esc(code)}</div></section>` + already + typedLine + reason,
    actions: `<div class="pair-answer"><button type="button" class="btn" data-action="cancel"${disabled(busy)}>Cancel</button><button type="button" class="btn primary" data-action="connect"${disabled(busy || blocked)}>Connect ${esc(name)}</button></div>`,
    notice,
  });
}

export function connectDone({ name, host }) {
  return `<section class="done"><div class="ok title-ok">✓ ${esc(name)} is connected</div><p class="lead">Its board shows here now. On your phone, open ${esc(host)} and sign in with the same passkey.</p></section>`;
}

export function connectTyped({ name }) {
  return authScreen({ intro: "", actions: status(`${esc(name)} is connected. Its board stays unreadable until you connect it from the link kebbie opens on that computer.`, "ok") });
}

export function connectCancelled() {
  return authScreen({ intro: "", actions: status("Cancelled. The terminal shows it too.") });
}

export function signInScreen({ github, notice = null, next = null }) {
  const gh = github ? `<a class="btn" href="${esc(githubHref({ next }))}">Sign in with GitHub</a>` : "";
  return authScreen({
    intro: heading("See your boards", "Your machines' boards, end-to-end encrypted. Sign in with the passkey you made for kebbie."),
    actions: `<button type="button" class="btn primary" data-action="sign-in">Sign in with passkey</button>${gh}`,
    alt: `<p class="foot">New to kebbie? Start in your terminal: <code>kebbie setup</code></p>`,
    notice,
  });
}

export function unlockScreen({ notice = null } = {}) {
  return authScreen({ intro: heading("Unlock your boards", "You're signed in. Unlock with your passkey to read your boards. They stay unlocked on this device for an hour."), actions: `<button type="button" class="btn primary" data-action="sign-in">Unlock with passkey</button>`, notice });
}

export function unlockingScreen() {
  return authScreen({ intro: "", actions: status(`<span class="spinner" aria-hidden="true"></span> Unlocking your boards…`) });
}

const CANT_UNLOCK = {
  "no-prf": "You're signed in, but this browser can't unlock your boards with its passkey.",
  "not-wrapped": "You're signed in, but this passkey can't unlock your boards yet.",
  github: "You're signed in with GitHub, but GitHub can't unlock your boards.",
};

export function addPasskeyScreen({ login, notice = null, busy = false }) {
  return authScreen({
    intro: `<div class="center ok">✓ Signed in with GitHub as ${esc(login)}</div>` + heading("Add a passkey to unlock your boards", "GitHub signs you in. A passkey also unlocks your boards on every phone and browser, so you never approve one by hand."),
    actions: `<button type="button" class="btn primary" data-action="add-passkey"${disabled(busy)}>Add passkey</button><button type="button" class="link" data-action="not-now"${disabled(busy)}>Not now</button>`,
    notice,
  });
}

export const needsFirstPasskey = (me, session) => Boolean(me?.github && !me.key && (!session || session.state === "locked"));

export const EMAIL_LABEL = "Email, so we can tell who you are (optional)";

const emailInput = (label, value) => `<div class="note-field"><label for="email">${esc(label)}</label><input id="email" name="email" type="email" autocomplete="email" inputmode="email" maxlength="254" value="${esc(value)}"></div>`;

export const emailField = (value = "") => emailInput(EMAIL_LABEL, value);

function emailState(address) {
  if (!address?.email) return "No email yet";
  if (address.verified) return `${address.email} · confirmed`;
  return address.sent ? `${address.email} · not confirmed yet. We sent a link; open it to confirm.` : `${address.email} · not confirmed`;
}

export function emailSection(address, { busy = false, codes = false, changing = false } = {}) {
  const shown = address?.email ? { ...address, sent: codes ? false : address.sent } : address;
  const state = `<div class="meta${address?.verified ? " ok" : ""}">${esc(emailState(shown))}</div>`;
  if (address?.email && !changing) return `<section class="task"><div class="row"><div><div class="name">Email</div>${state}</div><button type="button" class="btn small" data-action="change-email"${disabled(busy)}>Change</button></div></section>`;
  const field = codes ? emailInput(address?.email ? "New email" : "Email", "") : address?.email ? emailInput("New email", "") : emailField("");
  const send = `<button type="button" class="btn small" data-action="save-email"${disabled(busy)}>${codes ? "Send code" : "Save email"}</button>`;
  const remove = !codes && address?.email ? `<button type="button" class="link" data-action="remove-email"${disabled(busy)}>Remove email</button>` : "";
  const cancel = address?.email ? `<button type="button" class="link" data-action="keep-email"${disabled(busy)}>Cancel</button>` : "";
  return `<section class="task"><div class="name">Email</div>${state}${field}${send}${remove}${cancel}</section>`;
}

export function thisDevice(allowed) {
  const state = allowed ? "Changes go to each machine that takes them." : "View only";
  return `<h2 class="group g-muted">This device</h2><section class="task"><label class="row switch-row"><div><div class="name" id="allow-name">Allow changes from this device</div><div class="meta" id="allow-state">${state}</div></div><input type="checkbox" role="switch" class="switch" name="allow-changes" aria-labelledby="allow-name" aria-describedby="allow-state" aria-checked="${allowed ? "true" : "false"}"${allowed ? " checked" : ""}></label></section>`;
}

function switchRow({ id, name, meta, on = false, disabled: off = false, field = id, value = null }) {
  const valued = value === null ? "" : ` value="${esc(value)}"`;
  return `<label class="row switch-row"><div><div class="name" id="${id}-name">${esc(name)}</div><div class="meta" id="${id}-state">${esc(meta)}</div></div><input type="checkbox" role="switch" class="switch" name="${field}"${valued} aria-labelledby="${id}-name" aria-describedby="${id}-state" aria-checked="${on ? "true" : "false"}"${on ? " checked" : ""}${off ? " disabled" : ""}></label>`;
}

export function notifySection({ phase, text, categories = [], failure = null, senders = "" }) {
  const on = phase === "on" || phase === "asking";
  const from = phase === "on" && senders ? `<div class="meta" id="notify-senders">${esc(senders)}</div>` : "";
  const why = failure ? `<div class="meta warn" id="notify-failure">${esc(failure)}</div>` : "";
  const main = `<section class="task" id="notify-card">${switchRow({ id: "notify", name: "Notifications", meta: text, on, disabled: !switchable(phase) })}${from}${why}</section>`;
  if (phase !== "on") return main;
  const cards = CATEGORY_GROUPS.map((group) => `<h2 class="group g-muted">${esc(group.title)}</h2><section class="task">${group.rows.map((row) => switchRow({ id: row.id, name: row.name, meta: row.meta, on: categories.includes(row.category), field: "notice-category", value: row.category })).join("")}</section>`).join("");
  return main + cards;
}

export function notifyRow({ phase }) {
  const on = phase === "on" || phase === "asking";
  return `<a class="task row-link" href="#notifications" data-action="notifications"><span class="name">Notifications</span><span class="meta${on ? " ok" : ""}">${on ? "On" : "Off"}</span><svg class="chev" viewBox="0 0 24 24" aria-hidden="true"><path d="M9 6l6 6-6 6"/></svg></a>`;
}

export function notificationsScreen({ notify, notice = null }) {
  return `${brand}<h1 class="title">Notifications</h1>${notifySection(notify)}${warn(notice)}<a class="btn back-link" href="/account" data-action="notifications-back">Back to Account</a><a class="back-btn round" href="/account" data-action="notifications-back" aria-label="Back">${icon("back")}</a>`;
}

export function accountScreen({ me, github, email = false, codes = false, changing = false, address = null, notice = null, busy = false, allowed = null, devices = null, notify = null, now = Date.now() / 1000 }) {
  const linked = me.github
    ? `<section class="task"><div class="row"><div><div class="name">GitHub</div><div class="meta">Signed in with GitHub as ${esc(me.github)}</div></div><button type="button" class="btn small" data-action="remove-github">Remove</button></div></section>`
    : github
      ? `<section class="task"><div class="row"><div><div class="name">GitHub</div><div class="meta">Not connected</div></div><a class="btn small" href="${esc(githubHref({ link: true, next: "/account" }))}">Connect GitHub</a></div></section>`
      : "";
  return brand + `<h1 class="title">Account</h1>` + linked + (email ? emailSection(address, { busy, codes, changing }) : "") + (allowed === null ? "" : thisDevice(allowed) + (notify ? notifyRow(notify) : "")) + (devices ? devicesSection(devices, now) : "") + warn(notice) + `<a class="btn back-link" href="/">Back to your boards</a><button type="button" class="btn" data-action="sign-out"${disabled(busy)}>Sign out</button>` + backButton("/");
}

export function cantUnlockScreen({ state = "no-prf", notice = null }) {
  return authScreen({
    intro: heading("This browser can't unlock your boards", CANT_UNLOCK[state] ?? CANT_UNLOCK["no-prf"]),
    fields: `<section class="task"><div class="meta">What works today: sign in on a device whose passkey can unlock them, such as an iPhone or Mac with iCloud Keychain, Google Password Manager or 1Password.</div></section>`,
    actions: `<button type="button" class="btn primary" data-action="other-passkey">Use a passkey instead</button>`,
    notice,
  });
}

export const INSTALL_HINT = "Add kebbie to your Home Screen — Share → Add to Home Screen.";

export function installHint(show) {
  return show ? `<div class="hint"><div>${esc(INSTALL_HINT)}</div><button type="button" class="close" data-action="dismiss-hint" aria-label="Close">×</button></div>` : "";
}

export function setupScreen({ phase, needsSetup, token, github = false, notice = null }) {
  const top = brand + `<h1 class="title">Set up this hub</h1>`;
  if (phase === "done") return top + `<div class="center ok">You own this hub.</div><a class="btn primary" href="/">Open kebbie</a>`;
  if (!needsSetup) return top + `<div class="center">This hub has an owner already.</div><a class="btn" href="/">Sign in</a>`;
  if (!token) return top + `<div class="center">Open the setup link the hub printed in its log when it started.</div>`;
  return top + `<p class="lead">Create a passkey to become this hub's owner. It signs you in; there is no password.</p><button type="button" class="btn primary" data-action="create-passkey">Create passkey</button>${github ? githubButton("Continue with GitHub", githubHref({ setup: token })) : ""}${warn(notice)}`;
}

export function signupScreen({ signedIn, next, invite, github = false, email = false, notice = null }) {
  const top = brand + `<h1 class="title">Sign up</h1>`;
  if (signedIn) return top + `<div class="center ok">You're signed in.</div><a class="btn primary" href="${esc(next ?? "/")}">Continue</a>`;
  if (invite) return top + `<p class="lead">You're invited to this hub. Create a passkey to start: it signs you in and unlocks your boards.</p>${email ? emailField() : ""}<button type="button" class="btn primary" data-action="create-passkey">Create passkey</button>${github ? githubButton("Continue with GitHub", githubHref({ invite, next })) : ""}${warn(notice)}`;
  return top + `<p class="foot">New to kebbie? Start in your terminal: <code>kebbie setup</code></p><a class="btn" href="/">Sign in</a>`;
}

export function connectScreen({ supported = true, phase = "start", view = null, me = null, session = null, fragment = null, signupOpen = false, github = false, email = false, machines = [], notice = null, busy = false, code = "" }) {
  if (!supported) return authScreen({ intro: "", actions: status("This browser cannot sign in with a passkey.") });
  const name = view?.name ?? "this machine";
  if (phase === "connected") return connectTyped({ name });
  if (phase === "cancelled") return connectCancelled();
  if (phase === "pairing") return authScreen({ intro: "", actions: status(`Handing ${esc(name)} your account's key …`, "ok") });
  if (!view) return authScreen({ intro: "", actions: notice ? "" : status("Loading …"), notice });
  if (!me) return connectWelcome({ name, code: view.code || code, signupOpen, github, email, notice, busy });
  if (needsFirstPasskey(me, session)) return addPasskeyScreen({ login: me.github, notice, busy });
  if (session?.state === "github") return cantUnlockScreen({ state: "github", notice });
  const blocked = session?.state === "locked" ? null : connectBlock({ fragment, session });
  return connectAsk({ name, code: view.code || code, machines, blocked: blocked ? connectBlockText(blocked, name) : null, typed: !fragment, notice, busy });
}

const KIND_TEXT = { web: "web app" };
const day = (seconds, now) => (new Date(seconds * 1000).toDateString() === new Date(now * 1000).toDateString() ? "today" : new Date(seconds * 1000).toLocaleDateString("en-GB", { day: "numeric", month: "short" }));
const hoursLeft = (seconds) => `${Math.max(1, Math.floor(seconds / 3600))} h`;

export function standingText(standing, now) {
  if (standing.state === "approved") return `<span class="ok">✓ Approved on ${esc(standing.machine)}</span>`;
  if (standing.state === "waiting") return `<span class="warn">⧗ Waiting on ${esc(standing.machine)}${standing.until ? ` · ${esc(hoursLeft(standing.until - now))}` : ""}</span>`;
  return `<span class="g-yellow">✗ Denied on ${esc(standing.machine)}</span>`;
}

function deviceFacts(device, now) {
  if (device.removed) return [KIND_TEXT[device.kind] ?? device.kind, `added ${day(device.added, now)}`, `removed ${day(device.removed, now)}`];
  const seen = device.self ? "now" : device.seen ? lastSeenText(device.seen, now) : "not yet";
  return [KIND_TEXT[device.kind] ?? device.kind, `added ${day(device.added, now)}`, `last seen ${seen}`];
}

export function deviceRow(device, now) {
  const facts = deviceFacts(device, now).map(esc).join(" · ");
  const standing = device.standing.map((one) => standingText(one, now)).join(" · ");
  return `<a class="task" href="#device/${esc(device.id)}" data-action="device" data-id="${esc(device.id)}"><div class="head"><span class="name">${esc(device.name)}</span> ${device.self ? `<span class="machine">this device</span>` : ""}</div><div class="meta">${facts}</div>${standing ? `<div class="meta">${standing}</div>` : ""}</a>`;
}

export function devicesSection(devices, now) {
  const kept = devices.filter((device) => !device.removed);
  const removed = devices.filter((device) => device.removed);
  const gone = removed.length ? `<h2 class="group g-muted">Removed · ${removed.length}</h2>${removed.map((device) => deviceRow(device, now)).join("")}` : "";
  return `<h2 class="group g-muted">Devices · ${kept.length}</h2>${kept.map((device) => deviceRow(device, now)).join("")}<div class="meta pad">Each machine decides which devices make changes on it. Approve or deny a device in kebbie on that machine, or open it here from a device that machine approved.</div>${gone}`;
}

const NOUNS = { answer: "Answer to", push: "Push of", "prompt-answer": "Choice on", start: "Start in", routine: "Routine", idea: "Idea for", "item-answer": "Answer on", "start-item": "Start of", "item-edit": "Edit of", "device-decision": "Decision on", "review-open": "Review of", "review-comments": "Review comments on", "review-read": "Read mark on", "accept-step": "Accept on", "accept-subtask": "Accept on", "found-start": "Start from", "found-add": "Adding of", "found-dismiss": "Dismissal of", "found-keep": "Keeping of", "review-send": "Full review of" };
const clip = (text, length = 60) => (text.length > length ? `${text.slice(0, length - 1)}…` : text);
const itemName = (entry) => entry.label ?? entry.payload.item.split("/").at(-1).replace(/^(vault|local|kept|found):/, "").replace(/\.md$/, "");

function said(entry) {
  const { payload } = entry;
  if (payload.type === "answer") return entry.label ? `"${entry.label}"` : payload.choice === null ? `"${payload.comment}"` : `option ${payload.choice + 1}`;
  if (payload.type === "prompt-answer") return `"${entry.label ?? payload.key}"`;
  return null;
}

function subject(entry) {
  const { payload } = entry;
  if (payload.type === "routine") return payload.id;
  if (payload.type === "start") return `${payload.name}@${payload.repo}`;
  if (payload.type === "idea") return `${entry.machineName}: ${clip(payload.text)}`;
  if (["item-answer", "start-item", "item-edit", "found-start", "found-add", "found-dismiss", "found-keep"].includes(payload.type)) return itemName(entry);
  if (payload.type === "device-decision") return entry.targetName ?? payload.device.slice(-6);
  return payload.address;
}

export function historyEntry(entry, now) {
  const machine = entry.machineName;
  const kind = entry.payload.type;
  const what = subject(entry);
  const words = said(entry);
  const on = [words, `on ${machine}`].filter(Boolean).join(" · ");
  const reason = entry.result?.reason;
  if (entry.result?.outcome === "done") {
    if (kind === "answer") return { icon: "✓", what: `Answered ${what}`, said: on, at: entry.at };
    if (kind === "push") return { icon: "↑", what: `Pushed ${what}`, said: [entry.short, `on ${machine}`].filter(Boolean).join(" · "), at: entry.at };
    if (kind === "prompt-answer") return { icon: "?", what: `Chose ${words} on ${what}'s prompt`, said: `on ${machine}`, at: entry.at };
    if (kind === "routine") return { icon: "✓", what: `Ran routine ${what}`, said: `on ${machine}`, at: entry.at };
    if (kind === "idea") return { icon: "✓", what: `Idea for ${what}`, said: "filed in its backlog", at: entry.at };
    if (kind === "device-decision") return { icon: "✓", what: `${entry.payload.approve ? "Approved" : "Denied"} ${what}`, said: `on ${machine}`, at: entry.at };
    if (kind === "item-answer") return { icon: "✓", what: `Answered a question on ${what}`, said: `"${clip(entry.payload.answer)}" · on ${machine}`, at: entry.at };
    if (kind === "item-edit") return { icon: "✓", what: `Edited ${entry.payload.section === null || entry.payload.section === undefined ? "the title" : `“${entry.heading ?? entry.payload.section}”`} of ${what}`, said: `on ${machine}`, at: entry.at };
    if (kind === "review-open") return { icon: "✓", what: `Opened the review of ${what}`, said: `on ${machine}`, at: entry.at };
    if (kind === "review-comments") return { icon: "✓", what: `Sent ${plural(entry.payload.comments.length, "review comment", "review comments")} on ${what}`, said: `on ${machine}`, at: entry.at };
    if (kind === "review-read") return { icon: "✓", what: `Marked ${entry.payload.path} ${entry.payload.read ? "read" : "unread"} on ${what}`, said: `on ${machine}`, at: entry.at };
    if (kind === "accept-step") return { icon: "✓", what: `Accepted step ${entry.payload.step} of ${what}`, said: [(entry.payload.head ?? entry.payload.sha).slice(0, 7), `on ${machine}`].join(" · "), at: entry.at };
    if (kind === "accept-subtask") return { icon: "✓", what: `Accepted a subtask of ${what}`, said: [entry.payload.head.slice(0, 7), `on ${machine}`].join(" · "), at: entry.at };
    if (kind === "found-start") return { icon: "✓", what: `Started a task from ${what}`, said: `on ${machine}`, at: entry.at };
    if (kind === "found-add") return { icon: "✓", what: `Added ${what} to ${entry.payload.task}`, said: `on ${machine}`, at: entry.at };
    if (kind === "found-dismiss") return { icon: "✓", what: `Dismissed ${what}`, said: `on ${machine}`, at: entry.at };
    if (kind === "found-keep") return { icon: "✓", what: `Kept ${what} in the backlog`, said: `on ${machine}`, at: entry.at };
    if (kind === "review-send") return { icon: "✓", what: `Sent ${what} to a full review`, said: `on ${machine}`, at: entry.at };
    return { icon: "✓", what: `Started ${what}`, said: `on ${machine}`, at: entry.at };
  }
  const noun = `${NOUNS[kind] ?? kind} ${what}`;
  if (reason === "not-approved") return { icon: "✗", what: `${noun} not delivered`, said: `${machine} didn't approve this device within the hour`, at: entry.at };
  if (reason === "denied") return { icon: "✗", what: `${noun} refused`, said: `denied on ${machine}`, at: entry.at };
  if (kind === "device-decision" && reason === "not-allowed") return { icon: "✗", what: `${noun} refused`, said: `${machine} takes approvals only at the machine`, at: entry.at };
  if (entry.result) return { icon: "✗", what: `${noun} refused`, said: entry.result.text || `on ${machine}`, at: entry.at };
  if (now >= entry.exp) return { icon: "✗", what: `${noun} not delivered`, said: `expired before ${machine} took it`, at: entry.at };
  return { icon: "⧗", what: `${noun} waiting`, said: on, at: entry.at };
}

const TONES = { "✓": "ok", "↑": "ok", "?": "warn", "⧗": "warn", "✗": "g-yellow" };

function removeArea(device, { removing, busy }) {
  if (device.removed) return `<div class="meta pad">Removed from your account. It sends nothing through this hub and gets no notifications; what it sent stays listed here.</div>`;
  const off = busy ? " disabled" : "";
  if (!removing) return `<button type="button" class="btn" data-action="device-remove-ask">Remove ${esc(device.name)}</button>`;
  const self = device.self ? " This browser makes a new device key the next time you send something from it." : "";
  return `<div class="confirm" role="group" aria-label="Confirm remove"><p><b>Remove ${esc(device.name)}?</b></p><p class="meta">It can't send through this hub any more and gets no notifications. What it sent stays listed. Each machine keeps its own approval of it; deny it in kebbie on a machine to stop it there too.${self}</p><div class="row2"><button type="button" class="btn" data-action="device-remove-cancel"${off}>Cancel</button><button type="button" class="btn primary" data-action="device-remove"${off}>Remove</button></div></div>`;
}

export function deviceScreen({ device, history, loading = false, older = false, removing = false, busy = false, notice = null, decisions = new Map(), now = Date.now() / 1000 }) {
  const facts = [...deviceFacts(device, now), `key ${device.key}`].map(esc).join(" · ");
  const standing = `<section class="task"><div class="meta">${facts}</div>${device.standing.map((one) => `<div>${standingText(one, now)}</div>${decisions.get(one.machineId) ?? ""}`).join("")}</section>`;
  const entries = history.map((entry) => historyEntry(entry, now));
  const days = new Map();
  for (const entry of [...entries].sort((a, b) => b.at - a.at)) {
    const date = new Date(entry.at * 1000);
    const heading = date.toDateString() === new Date(now * 1000).toDateString() ? "Today" : date.toLocaleDateString("en-GB", { weekday: "long", day: "numeric", month: "short" });
    if (!days.has(heading)) days.set(heading, []);
    days.get(heading).push(entry);
  }
  const list = [...days].map(([heading, shown]) => `<div class="nsub">${esc(heading)}</div>${shown.map((entry) => `<div class="nrow"><span class="n"></span><span class="${TONES[entry.icon] ?? ""}" aria-hidden="true">${esc(entry.icon)}</span><span class="t"><b>${esc(entry.what)}</b><small>${esc(entry.said)}</small></span><span class="w">${esc(new Date(entry.at * 1000).toLocaleTimeString("en-GB", { hour: "2-digit", minute: "2-digit" }))}</span></div>`).join("")}`).join("");
  const more = older && !loading ? `<button type="button" class="btn" data-action="device-older"${busy ? " disabled" : ""}>Show older</button>` : "";
  const sent = loading ? `<div class="center">Loading …</div>` : entries.length ? `<section class="next" aria-label="Sent from ${esc(device.name)}">${list}</section>${more}` : `<div class="meta pad">Nothing sent from ${esc(device.name)} in the last 90 days.</div>`;
  const count = loading ? "" : ` · ${entries.length}${older ? "+" : ""}`;
  return `${brand}<h1 class="title">${esc(device.name)}</h1>${standing}<h2 class="group g-muted">Sent from ${esc(device.name)}${count}</h2>${sent}${removeArea(device, { removing, busy })}${warn(notice)}<a class="btn back-link" href="/account" data-action="devices-back">Back to Account</a><a class="back-btn round" href="/account" data-action="devices-back" aria-label="Back">${icon("back")}</a>`;
}

