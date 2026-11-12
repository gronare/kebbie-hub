import { failedLine, loadingLine } from "./blobs.js";
import { isAsleep } from "./board.js";
import { APPROVAL_STATES, approvalStatus, block, clock, from, hours, refusal, settled, takes, viewOnlyBlock } from "./decide.js";
import { esc, waited } from "./screens.js";

export const PRIORITIES = [
  { key: "none", title: "New", tone: "teal" },
  { key: "high", title: "High", tone: "peach" },
  { key: "medium", title: "Medium", tone: "blue" },
  { key: "low", title: "Low", tone: "muted" },
];

const plural = (count, one, many) => `${count} ${count === 1 ? one : many}`;
const waits = (count) => (count === 1 ? "1 question waits" : `${count} questions wait`);
const KEEP_FAILED_IDEA = 2 * 3600;

export const entryHref = (id) => `#/item/${encodeURIComponent(id)}`;

const STATE_RANK = { "to-write": 0, drafting: 1, ready: 2 };

export function mergeBacklog(opened, now) {
  const entries = new Map();
  for (const machine of opened) {
    for (const row of machine.board?.backlog ?? []) {
      const vault = row.home.kind === "vault";
      const id = vault ? row.item : `${machine.machine}|${row.item}`;
      const seen = { machine: machine.machine, name: machine.name, accepts: machine.board.accepts ?? null, asleep: isAsleep({ lastSeen: machine.lastSeen }, now), repos: (machine.board.checkouts ?? []).map((checkout) => checkout.repo), row };
      const held = entries.get(id);
      if (!held) entries.set(id, { id, key: row.item, vault, row, machines: [seen] });
      else {
        held.machines.push(seen);
        if (STATE_RANK[row.state] > STATE_RANK[held.row.state] || (row.state === held.row.state && row.blocking < held.row.blocking)) held.row = row;
      }
    }
  }
  return [...entries.values()];
}

export function ideaEntries(sentList, opened, now, stateOf) {
  return sentList
    .filter((sent) => sent.kind === "idea")
    .map((sent) => ({ sent, shown: stateOf(sent) }))
    .filter(({ sent, shown }) => {
      if (shown === "done") return (opened.find((machine) => machine.machine === sent.machine)?.storedAt ?? 0) < (sent.result.at ?? 0);
      if (settled(shown)) return now < sent.at + KEEP_FAILED_IDEA;
      return true;
    })
    .map(({ sent, shown }) => ({ id: `idea:${sent.id}`, key: null, vault: false, idea: { sent, state: shown }, row: { title: sent.text, priority: "none", repo: sent.repo ?? null, state: "to-write", filed: sent.at, questions: 0, blocking: 0, goal: null, body: null, home: { kind: "kept", label: "" } }, machines: [{ machine: sent.machine, name: sent.machineName, accepts: null, asleep: false, repos: [], row: null }] }));
}

export const chip = (entry) => (entry.vault ? "vault" : entry.machines[0].name);

function ideaLine(entry) {
  const { sent, state } = entry.idea;
  const machine = esc(sent.machineName);
  if (state === "waiting") return `waits in the hub for ${machine} · goes out when it wakes`;
  if (state === "approval") return `waits for ${machine} to approve this device`;
  if (state === "done" || state === "sending") return `the lead on ${machine} writes it out`;
  return `not filed: ${esc(sent.result?.text || (state === "expired" ? `${sent.machineName} stayed asleep past the hour` : "not approved"))}`;
}

function rowLines(entry, now) {
  const { row } = entry;
  const age = esc(waited(Math.max(0, now - row.filed)));
  const repo = row.repo ? `${esc(row.repo)} · ` : "";
  if (entry.idea) return `<div class="meta">${repo}to write · ${age}</div><div class="meta">${ideaLine(entry)}</div>`;
  if (row.state === "to-write") return `<div class="meta">${repo}to write · ${age}</div><div class="meta">the lead writes it out</div>`;
  if (row.state === "drafting" && row.blocking > 0) return `<div class="meta">${repo}drafting · <span class="g-yellow">${plural(row.blocking, "question", "questions")}</span> · ${age}</div><div class="you"><b>you:</b> answer ${row.blocking === 1 ? "it" : "them"} on its card</div>`;
  if (row.state === "drafting") return `<div class="meta">${repo}drafting · ${age}</div><div class="meta">the lead is writing it out from the code, the notes and your chat</div>`;
  return `<div class="meta">${repo}${age}</div>${row.goal ? `<div>${esc(row.goal)}</div>` : ""}`;
}

export function backlogRow(entry, selected, now) {
  const on = selected === entry.id;
  return `<a class="task${on ? " sel" : ""}" href="${entryHref(entry.id)}"${on ? ' aria-current="true"' : ""}><div class="head"><span class="name">${esc(entry.row.title)}</span> <span class="machine">${esc(chip(entry))}</span></div>${rowLines(entry, now)}</a>`;
}

export const proLine = (what, url) => `<div class="pro"><b>Hub Pro</b>${what}${url ? ` <a class="link inline" href="${esc(url)}">Details</a>` : ""}</div>`;

const newest = (a, b) => Number(Boolean(b.idea)) - Number(Boolean(a.idea)) || b.row.filed - a.row.filed || a.row.title.localeCompare(b.row.title);

export function backlogGroups(entries) {
  return PRIORITIES.map((priority) => ({ ...priority, rows: entries.filter((entry) => (entry.row.priority ?? "none") === priority.key).sort(newest) })).filter((group) => group.rows.length > 0);
}

export function listAction({ access, refusedUrl, ideaMachines, composing }) {
  if (composing || access === "off") return "";
  if (access === "refused") return `<div class="thumb-end">${proLine("Adding ideas and starting them from this device come with Hub Pro.", refusedUrl)}</div>`;
  if (access === "view-only") return `<div class="thumb-end">${viewOnlyBlock()}</div>`;
  if (ideaMachines.length === 0) return "";
  return `<div class="thumb"><a class="btn primary" href="#/idea">New idea</a></div>`;
}

export function backlogList({ entries, selected = null, now, top, action = "" }) {
  const waiting = entries.reduce((sum, entry) => sum + (entry.row.blocking ?? 0), 0);
  const aside = [plural(entries.length, "item", "items"), waiting ? waits(waiting) : ""].filter(Boolean).join(" · ");
  const groups = backlogGroups(entries);
  const body = groups.length ? groups.map((group) => `<h2 class="group g-${group.tone}">${esc(group.title)} · ${group.rows.length}</h2>${group.rows.map((entry) => backlogRow(entry, selected, now)).join("")}`).join("") : `<div class="center">No machine has a backlog on its board yet.</div>`;
  return `<div class="narrow">${top("Backlog", esc(entries.length ? aside : ""))}${body}</div>${action}`;
}

export function chooseMachine(candidates, { repo = null, lastUsed = null, chosen = null } = {}) {
  if (candidates.length === 0) return null;
  const picked = candidates.find((one) => one.machine === chosen);
  if (picked) return { ...picked, why: "you chose it" };
  const withRepo = repo ? candidates.find((one) => one.repos.includes(repo)) : null;
  if (withRepo) return { ...withRepo, why: `it has ${repo}` };
  const last = candidates.find((one) => one.machine === lastUsed);
  if (last) return { ...last, why: "you used it last" };
  const awake = candidates.find((one) => !one.asleep) ?? candidates[0];
  return { ...awake, why: awake.asleep ? "asleep now" : "awake now" };
}

export function machinePicker(candidates, chosen, { label, choosing }) {
  const change = candidates.length > 1 ? `<button type="button" class="btn small" data-action="backlog-choose">${choosing ? "Done" : "Change"}</button>` : "";
  const list = choosing ? `<div class="options">${candidates.map((one) => `<div class="opt pick${one.machine === chosen.machine ? " picked" : ""}" role="button" tabindex="0" data-action="backlog-machine" data-machine="${esc(one.machine)}" aria-pressed="${one.machine === chosen.machine}"><b>${esc(one.name)}</b><small>${one.asleep ? "asleep" : "awake"}</small></div>`).join("")}</div>` : "";
  return `<section class="task"><div class="row"><div><div class="meta">${esc(label)}</div><div class="name">${esc(chosen.name)}</div><div class="meta">${esc(chosen.why)}</div></div>${change}</div></section>${list}`;
}

export function ideaForm({ text = "", candidates, chosen, choosing = false, busy = false, error = null }) {
  const field = `<div class="note-field"><label for="idea">What you want, in a line. The lead writes it out from the code, the notes and your chats.</label><textarea id="idea" data-draft="idea" rows="2" maxlength="2000">${esc(text)}</textarea></div>`;
  if (!chosen) return field + `<div class="pro">No machine takes ideas from the hub. Allow them in kebbie's settings on a machine.</div>`;
  const ready = text.trim() !== "" && !busy;
  const failed = error ? `<div class="center warn" role="alert">${esc(error)}</div>` : "";
  return field + machinePicker(candidates, chosen, { label: "Goes to the lead on", choosing }) + failed + `<div class="thumb"><button type="button" class="btn primary" data-action="add-idea"${ready ? "" : " disabled"}>Add to the backlog</button></div>`;
}

const dayOf = (seconds, now) => (new Date(seconds * 1000).toDateString() === new Date(now * 1000).toDateString() ? "today" : new Date(seconds * 1000).toLocaleDateString("en-GB", { day: "numeric", month: "short" }));
const HOME_TERMS = { vault: "note", local: "file", kept: "kept" };

export function questionsLeft(body, sentFor) {
  const questions = body?.questions ?? [];
  const withLead = questions.filter((question, index) => question.blocks && (question.pending_answer !== null && question.pending_answer !== undefined || sentFor(index)?.live)).length;
  const left = questions.filter((question) => question.blocks).length - withLead;
  return { left, withLead };
}

function stateText(row, { left, withLead }) {
  if (row.state === "ready") return "ready";
  if (row.state === "to-write") return "to write: the lead writes it out";
  const lead = withLead ? `${plural(withLead, "answer", "answers")} with the lead` : "";
  if (left > 0) return `drafting: <span class="g-yellow">${waits(left)}</span> on you${lead ? ` · ${lead}` : ""}`;
  return `drafting: ${lead || "the lead writes it out"}`;
}

export function itemFacts(entry, counts, now) {
  const { row } = entry;
  const names = entry.machines.map((one) => one.name);
  const seen = names.length > 1 ? `seen on ${names.slice(0, -1).join(", ")} and ${names.at(-1)}` : `on ${names[0]}`;
  const rows = [["priority", esc(row.priority)], ["repo", esc(row.repo ?? "none")], ["state", stateText(row, counts)], ["filed", esc(dayOf(row.filed, now))], [HOME_TERMS[row.home.kind], `${esc(row.home.label)}<span class="meta"> · ${esc(seen)}</span>`]];
  return `<section class="task"><dl class="facts">${rows.map(([label, value]) => `<dt>${label}</dt><dd>${value}</dd>`).join("")}</dl></section>`;
}

const section = (title, text) => `<h3 class="sec">${esc(title)}</h3><div class="md" data-markdown>${esc(text)}</div>`;

export function answerStatus(sent, state, { standing = null, now }) {
  const machine = esc(sent.machineName);
  const said = `"${esc(sent.text)}"`;
  if (APPROVAL_STATES.includes(state)) return approvalStatus(sent, state, { standing, now, what: { goes: `Your answer ${said} goes to the lead`, again: "answer again" } });
  if (state === "done") return withLead(sent.text, `${from(sent)} · on ${machine}`);
  if (state === "refused") return block("refused", ...refusal("Not answered", sent.result.text), from(sent));
  if (state === "expired") return block("refused", `Not delivered: ${machine} stayed asleep past the hour`, `Answer again when ${machine} is awake.`, `Sent ${from(sent)} · expired ${esc(clock(sent.exp))}`);
  if (state === "waiting") return block("wait", `Waiting for ${machine}`, `Your answer ${said} goes to the lead when ${machine} wakes up.`, `Sent ${from(sent)} · expires ${esc(clock(sent.exp))}`);
  return block("wait", `Sent to ${machine}`, `Your answer ${said} is on its way to the lead.`, `Sent ${from(sent)} · expires ${esc(clock(sent.exp))}`);
}

const withLead = (text, meta) => block("wait", `With the lead: "${esc(text)}"`, "The lead folds your answer into the item and takes the question out.", meta);

function questionCard(question, index, { pickable, picked, ui, sent, now }) {
  const pending = question.pending_answer !== null && question.pending_answer !== undefined;
  const live = sent && !settled(sent.state);
  const answered = pending || (live && sent.state === "done");
  const mark = question.blocks ? `<span class="qmark${answered ? "" : " warn"}" aria-hidden="true">?</span>` : `<span class="qmark" aria-hidden="true">·</span>`;
  const press = pickable && !pending && !live;
  const classes = ["opt", "bq", question.blocks ? "" : "plain", press ? "pick" : "", picked ? "picked" : "", answered ? "answered" : ""].filter(Boolean).join(" ");
  const attributes = press ? ` role="button" tabindex="0" data-action="pick-question" data-index="${index}" aria-pressed="${picked}"` : "";
  const card = `<div class="${classes}"${attributes}><b>${mark}${esc(question.text)}</b>${question.blocks ? "" : "<small>the work can settle it</small>"}</div>`;
  if (sent && (live || !pending)) {
    const status = answerStatus(sent.sent, sent.state, { standing: sent.standing, now });
    if (live || !picked) return card + status;
    return card + status + answerRow(index, ui);
  }
  if (pending) return card + withLead(question.pending_answer, "with the lead on its machine");
  if (picked) return card + answerRow(index, ui);
  return card;
}

function answerRow(index, ui) {
  const ready = ui.text.trim() !== "" && !ui.busy;
  const failed = ui.error ? `<div class="center warn" role="alert">${esc(ui.error)}</div>` : "";
  return `<div class="answer-row"><div class="note-field"><label for="answer-${index}">Your answer</label><textarea id="answer-${index}" data-draft="item-answer" rows="2" maxlength="2000">${esc(ui.text)}</textarea></div><button type="button" class="btn primary" data-action="send-item-answer"${ready ? "" : " disabled"}>Send to the lead</button></div>${failed}`;
}

function questionsBlock(body, { access, takesAnswers, refusedUrl, ui, sentFor, now, counts }) {
  const questions = body.questions;
  const head = counts.left > 0 ? `${counts.left === 1 ? "1 waits" : `${counts.left} wait`} on you before it starts` : "none blocks the start";
  const pickable = access === "ready" && takesAnswers;
  const cards = questions.map((question, index) => questionCard(question, index, { pickable, picked: ui.picked === index, ui, sent: sentFor(index), now })).join("");
  const gate = access === "refused" ? proLine("Answering its questions from this device comes with Hub Pro.", refusedUrl) : access === "view-only" && questions.length ? viewOnlyBlock() : "";
  if (questions.length === 0) return `<h3 class="sec">Open questions</h3><div class="meta pad">None.</div>`;
  return `<h3 class="sec">Open questions</h3><div class="meta pad">${head}</div><div class="options qlist">${cards}</div>${gate}`;
}

export function startStatus(sent, state, { title, standing = null, now, taskHref = "#/board" }) {
  const machine = esc(sent.machineName);
  if (APPROVAL_STATES.includes(state)) return approvalStatus(sent, state, { standing, now, what: { goes: "Your start goes out", again: "press Start again" } });
  if (state === "done") return block("done", `✓ Started ${esc(title)}`, esc(sent.result.text), `${from(sent)} · on ${machine}`) + `<a class="btn" href="${esc(taskHref)}">Open the task</a>`;
  if (state === "refused" && sent.result?.reason === "questions-open") return block("refused", `Not started: ${waits(sent.result.blocking ?? 1)} on you.`, "Answer them on its card, or press Start again to start anyway.", from(sent));
  if (state === "refused") return block("refused", ...refusal("Not started", sent.result.text), from(sent));
  if (state === "expired") return block("refused", `Not delivered: ${machine} stayed asleep for a day`, `Press Start again when ${machine} is awake.`, `Sent ${from(sent)} · expired ${esc(clock(sent.exp))}`);
  if (state === "waiting") return block("wait", `Waiting for ${machine}`, `The lead starts ${esc(title)} when ${machine} wakes up.`, `Sent ${from(sent)} · goes out when it wakes · expires in ${hours(sent.exp - now)}`);
  return block("wait", `Sent to ${machine}`, `The lead on ${machine} picks the recipe and queues ${esc(title)}.`, `Sent ${from(sent)} · expires in ${hours(sent.exp - now)}`);
}

function startArea(entry, { access, refusedUrl, candidates, chosen, ui, start, now, taskHref }) {
  if (entry.row.state === "to-write") return "";
  if (access === "off") return "";
  if (access === "refused") return proLine("Starting it from this device comes with Hub Pro.", refusedUrl);
  if (access === "view-only") return viewOnlyBlock();
  if (!chosen) return `<div class="pro">Not started from here: ${esc(entry.machines.map((one) => one.name).join(" and "))} ${entry.machines.length > 1 ? "take" : "takes"} no starts from the backlog. Start it in kebbie, or allow it in its kebbie settings.</div>`;
  const status = start ? startStatus(start.sent, start.state, { title: entry.row.title, standing: start.standing, now, taskHref }) : "";
  if (start && !settled(start.state)) return status;
  const failed = ui.error ? `<div class="center warn" role="alert">${esc(ui.error)}</div>` : "";
  const picker = candidates.length > 1 ? machinePicker(candidates, chosen, { label: "Starts on", choosing: ui.choosing }) : "";
  return `${status}${picker}<div class="pair-answer"><a class="btn" href="#/backlog">Save to backlog</a><button type="button" class="btn primary" data-action="start-item"${ui.busy ? " disabled" : ""}>Start</button></div><div class="meta pad">The lead on ${esc(chosen.name)} picks the recipe and queues it.</div>${failed}`;
}

function ideaCard(entry, now) {
  const { sent, state } = entry.idea;
  const machine = sent.machineName;
  const said = state === "waiting" ? `It waits in the hub for ${machine} and goes out when it wakes. The lead then writes it out, and its sections show here.` : `The lead on ${machine} writes it out from the code, the notes and your chats. Its sections show here when it is saved.`;
  const shown = state === "waiting" ? `to write: waits for ${machine}` : `to write: the lead on ${machine} writes it out`;
  const status = APPROVAL_STATES.includes(state) ? approvalStatus(sent, state, { standing: entry.idea.standing ?? null, now, what: { goes: "Your idea goes to the lead", again: "add it again" } }) : settled(state) ? block("refused", ...refusal("Not filed", sent.result?.text ?? `${machine} stayed asleep past the hour`), from(sent)) : "";
  return `<section class="task"><dl class="facts"><dt>state</dt><dd>${esc(shown)}</dd><dt>filed</dt><dd>${esc(dayOf(sent.at, now))}, from ${esc(sent.deviceName ?? "this device")}</dd></dl></section><div class="md"><p>${esc(said)}</p></div>${status}`;
}

export function itemBody(entry, { body = null, bodyState = null, access = "off", refusedUrl = null, ui, sentFor = () => null, start = null, candidates = [], chosen = null, now, taskHref = "#/board" }) {
  if (entry.idea) return ideaCard(entry, now);
  if (entry.gone) return `<div class="decide start-area">${start ? startStatus(start.sent, start.state, { title: entry.row.title, standing: start.standing, now, taskHref }) : ""}</div><div class="meta pad">It left the backlog when it started.</div>`;
  const counts = body ? questionsLeft(body, sentFor) : { left: entry.row.blocking, withLead: 0 };
  const facts = itemFacts(entry, counts, now);
  const area = `<div class="decide start-area" data-item="${esc(entry.id)}">${startArea(entry, { access, refusedUrl, candidates, chosen, ui, start, now, taskHref })}</div>`;
  if (!entry.row.body && entry.row.state === "ready") return facts + (entry.row.goal ? section("Goal", entry.row.goal) : "") + area;
  if (!entry.row.body) {
    const said = entry.row.state === "drafting" ? "The lead is writing it out from the code, the notes and your chat. Its sections show here when it is saved." : `The lead on ${entry.machines[0].name} writes it out from the code, the notes and your chats. Its sections show here when it is saved.`;
    return facts + `<div class="md"><p>${esc(said)}</p></div>` + area;
  }
  if (!body) return facts + `<div class="plan"><div>${esc(bodyState === "failed" ? failedLine("item") : loadingLine("item", entry.row.body))}</div></div>` + area;
  const takesAnswers = Boolean(chosen && takes(chosen.accepts, "item-answer")) || entry.machines.some((one) => takes(one.accepts, "item-answer"));
  return facts + section("Background", body.background) + section("Goal", body.goal) + section("What we know", body.what_we_know) + `<div class="decide" data-item="${esc(entry.id)}">${questionsBlock(body, { access, takesAnswers, refusedUrl, ui, sentFor, now, counts })}</div>` + section("Out of scope", body.out_of_scope) + section("Source", body.source) + area;
}

export function startedEntry(id, sent) {
  const kind = sent.address.slice(0, sent.address.indexOf(":"));
  const row = { title: sent.label ?? sent.address, priority: "none", repo: null, state: "ready", filed: sent.at, questions: 0, blocking: 0, goal: null, body: null, home: { kind, label: sent.address.slice(kind.length + 1) } };
  return { id, key: sent.address, vault: kind === "vault", gone: true, row, machines: [{ machine: sent.machine, name: sent.machineName, accepts: null, asleep: false, repos: [], row: null }] };
}

export function startsAnyway(start, visit) {
  return Boolean(start?.sent.result?.reason === "questions-open" && start.sent.at >= visit);
}
