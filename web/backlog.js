import { MAX_ITEM_EDIT_BYTES, MAX_ITEM_TITLE_CHARS, MAX_SECTION_CHARS } from "../js/protocol.js";
import { failedLine, loadingLine } from "./blobs.js";
import { isAsleep } from "./board.js";
import { APPROVAL_STATES, approvalStatus, block, clock, from, hours, refusal, settled, takes, viewOnlyBlock } from "./decide.js";
import { esc, waited } from "./screens.js";
import { chipsFor, emptyFind, filtering, findIn, marked, snippet, textsOf } from "./find.js";

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

// A vault note is one row across the machines of one account; the same note in another account is that account's own row, so an answer never reaches the other account's machines.
export const vaultPrefix = (opened, account) => (account === undefined || account === (opened.find((machine) => machine.account !== undefined)?.account ?? account) ? "" : `${account}|`);

export function mergeBacklog(opened, now) {
  const entries = new Map();
  for (const machine of opened) {
    for (const row of machine.board?.backlog ?? []) {
      const vault = row.home.kind === "vault";
      const prefix = vaultPrefix(opened, machine.account);
      const id = vault ? `${prefix}${row.item}` : `${machine.machine}|${row.item}`;
      const seen = { machine: machine.machine, name: machine.name, accepts: machine.board.accepts ?? null, asleep: isAsleep({ lastSeen: machine.lastSeen }, now), repos: (machine.board.checkouts ?? []).map((checkout) => checkout.repo), row };
      const held = entries.get(id);
      if (!held) entries.set(id, { id, key: row.item, vault, prefix, account: machine.account ?? null, row, machines: [seen] });
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

export const waitingOf = (entry) => entry.waiting ?? entry.row.blocking ?? 0;
const toneOf = (key) => PRIORITIES.find((one) => one.key === (key ?? "none")) ?? PRIORITIES[0];
const showsGoal = (entry) => !entry.idea && waitingOf(entry) === 0 && entry.row.state === "ready" && !entry.row.found && Boolean(entry.row.goal);

function rowLines(entry, now, { q = "", sort = "priority" } = {}) {
  const { row } = entry;
  const age = esc(waited(Math.max(0, now - row.filed)));
  const priority = sort === "newest" && !entry.idea && !isFound(entry) ? `<span class="g-${toneOf(row.priority).tone}">${esc(toneOf(row.priority).title.toLowerCase())}</span> · ` : "";
  const repo = `${priority}${row.repo ? `${esc(row.repo)} · ` : ""}`;
  const waiting = waitingOf(entry);
  if (entry.idea) return `<div class="meta">${repo}to write · ${age}</div><div class="meta">${ideaLine(entry)}</div>`;
  if (waiting > 0) return `<div class="meta">${repo}${esc(row.state === "to-write" ? "to write" : row.state)} · <span class="g-yellow">${plural(waiting, "question", "questions")}</span> · ${age}</div><div class="you"><b>you:</b> answer ${waiting === 1 ? "it" : "them"} on its card</div>`;
  if (row.state === "to-write") return `<div class="meta">${repo}to write · ${age}</div><div class="meta">the lead writes it out</div>`;
  if (row.state === "drafting") return `<div class="meta">${repo}${esc(writingWord(row, now))} · ${age}</div><div class="meta">the lead is writing it out from the code, the notes and your chat</div>`;
  if (row.found) return `<div class="meta">${repo}${esc(foundBy(row.found))} · ${age}</div>`;
  return `<div class="meta">${repo}${age}</div>${row.goal ? `<div>${marked(row.goal, q)}</div>` : ""}`;
}

const taskOf = (address) => address.slice(0, address.indexOf("@"));
export const foundBy = (found) => `${found.count > 1 ? `found ${found.count}× · ` : ""}by ${found.sources.map(taskOf).join(", ")}`;
export const isFound = (entry) => entry.row.home?.kind === "found";

// A row kept after its idea was merged into a vault note: its label is the note's path.
export const mergedInto = (row) => (row.home?.kind === "kept" && / · merged into /.test(row.title) && /\.md$/.test(row.home.label) ? `vault:${row.home.label}` : null);
export const FOUND = { key: "found", title: "Found by tasks", tone: "muted" };

export function backlogRow(entry, selected, now, look = {}) {
  const on = selected === entry.id;
  const q = look.q ?? "";
  const titled = q && marked(entry.row.title, q) !== esc(entry.row.title);
  const texts = q && !titled && look.textOf ? look.textOf(entry).filter((text) => !(showsGoal(entry) && text === entry.row.goal)) : [];
  const hit = texts.length ? snippet(texts, q) : null;
  return `<a class="task${on ? " sel" : ""}" href="${esc(entry.opens ?? entryHref(entry.id))}"${on ? ' aria-current="true"' : ""}><div class="head"><span class="name">${marked(entry.row.title, q)}</span> <span class="machine">${esc(chip(entry))}</span></div>${rowLines(entry, now, look)}${hit ? `<div class="hit">${hit}</div>` : ""}</a>`;
}

export const proLine = (what, url) => `<div class="pro"><b>Hub Pro</b>${what}${url ? ` <a class="link inline" href="${esc(url)}">Details</a>` : ""}</div>`;

const newest = (a, b) => Number(Boolean(b.idea)) - Number(Boolean(a.idea)) || b.row.filed - a.row.filed || a.row.title.localeCompare(b.row.title);

export const NEEDS_YOU = { key: "needs", title: "Needs you", tone: "yellow" };
export const NEWEST = { key: "newest", title: "Newest first", tone: "muted" };

export function backlogGroups(entries, sort = "priority") {
  const needs = entries.filter((entry) => waitingOf(entry) > 0).sort(newest);
  const others = entries.filter((entry) => waitingOf(entry) === 0);
  const filed = others.filter((entry) => !isFound(entry));
  const rest = sort === "newest"
    ? [{ ...NEWEST, rows: [...others].sort(newest) }]
    : [...PRIORITIES.map((priority) => ({ ...priority, rows: filed.filter((entry) => (entry.row.priority ?? "none") === priority.key).sort(newest) })), { ...FOUND, rows: others.filter(isFound).sort(newest) }];
  return [{ ...NEEDS_YOU, rows: needs }, ...rest].filter((group) => group.rows.length > 0);
}

export function answersWithLead(entry, sentList, { shown, storedAt, body = null }) {
  if (!entry.key || !entry.row.blocking) return 0;
  const machines = new Set(entry.machines.map((one) => one.machine));
  const indexes = new Set();
  for (const sent of answerSents(sentList)) {
    if (sent.kind !== "item-answer" || sent.address !== entry.key || !machines.has(sent.machine)) continue;
    const question = body?.questions[body.questions.findIndex((_, index) => answers(sent, body.questions, index))];
    if (body && !(question?.blocks && (question.pending_answer ?? null) === null)) continue;
    const { state, live } = shown(sent);
    if (live && !(state === "done" && storedAt(sent.machine) >= (sent.result?.at ?? 0))) indexes.add(sent.question === undefined ? `#${sent.index}` : keyOf(sent.question, sent.nth ?? 0));
  }
  return indexes.size;
}

export const needsYouCount = (entries) => entries.filter((entry) => waitingOf(entry) > 0).length;

export function listAction({ access, refusedUrl, ideaMachines, composing }) {
  if (composing || access === "off") return "";
  if (access === "refused") return `<div class="thumb-end">${proLine("Adding ideas and starting them from this device come with Hub Pro.", refusedUrl)}</div>`;
  if (access === "view-only") return `<div class="thumb-end">${viewOnlyBlock()}</div>`;
  if (ideaMachines.length === 0) return "";
  return `<a class="fab" href="#/idea" aria-label="New idea"><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 5v14M5 12h14"/></svg></a>`;
}

const SEARCH_ICON = `<svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="11" cy="11" r="6.5"/><path d="M16 16l4.5 4.5"/></svg>`;
const CROSS = `<svg class="x" viewBox="0 0 24 24" aria-hidden="true"><path d="M6 6l12 12M18 6L6 18"/></svg>`;

const pill = (one) => `<button type="button" class="pill" data-action="find-chip" data-kind="${esc(one.kind)}" data-value="${esc(one.value)}" aria-pressed="${one.on}">${esc(one.label)} <span class="n">${one.n}</span></button>`;
const SEPARATOR = `<span class="sep" aria-hidden="true"></span>`;

export function chipRow(groups) {
  const active = groups.flat().filter((one) => one.on);
  const rest = groups.map((group) => group.filter((one) => !one.on)).filter((group) => group.length > 0);
  const lead = active.length ? `<button type="button" class="pill clear" data-action="find-clear" aria-label="Clear filters">${CROSS}Clear</button>${active.map(pill).join("")}${SEPARATOR}` : "";
  return `<div class="pills" role="group" aria-label="Filter">${lead}${rest.map((group) => group.map(pill).join("")).join(SEPARATOR)}</div>`;
}

export function findFilters({ chips, shown, total, find }) {
  const count = filtering(find) ? `<b>${shown}</b> of ${total}` : `All <b>${total}</b>`;
  const sort = (key, label) => `<button type="button" data-action="find-sort" data-sort="${key}" aria-pressed="${find.sort === key}">${label}</button>`;
  return `<div class="bl-filters">${chipRow(chips)}<div class="bl-count"><span class="n">${count}</span><span class="bl-sort" role="group" aria-label="Sort">${sort("priority", "Priority")}${sort("newest", "Newest")}</span></div></div>`;
}

const searchField = (q) => `<label class="bl-search">${SEARCH_ICON}<input type="search" id="bl-q" placeholder="Search titles and text" enterkeyhint="search" autocomplete="off" autocapitalize="off" spellcheck="false" value="${esc(q)}" aria-label="Search the backlog"><button type="button" class="clear" data-action="find-q-clear" aria-label="Clear search"${q ? "" : " hidden"}>${CROSS}</button></label>`;

export function backlogView(entries, find = emptyFind(), textOf = textsOf(() => null)) {
  const shown = findIn(entries, find, textOf);
  const groups = backlogGroups(shown, find.sort);
  return { shown, groups, first: groups[0]?.rows[0] ?? null, chips: chipsFor(entries, find, textOf, PRIORITIES) };
}

export function backlogResults(view, { entries, selected = null, now, find = emptyFind(), textOf = textsOf(() => null) }) {
  if (entries.length === 0) return `<div class="bl-results"><div class="center">No machine has a backlog on its board yet.</div></div>`;
  if (view.shown.length === 0) return `<div class="bl-results"><div class="center">Nothing in the backlog matches.</div></div>`;
  const look = { q: find.q, sort: find.sort, textOf };
  return `<div class="bl-results">${view.groups.map((group) => `<h2 class="group g-${group.tone}">${esc(group.title)} · ${group.rows.length}</h2>${group.rows.map((entry) => backlogRow(entry, selected, now, look)).join("")}`).join("")}</div>`;
}

export function backlogList({ entries, selected = null, now, top, action = "", find = emptyFind(), textOf = textsOf(() => null), view = backlogView(entries, find, textOf) }) {
  const waiting = entries.reduce((sum, entry) => sum + waitingOf(entry), 0);
  const aside = [plural(entries.length, "item", "items"), waiting ? waits(waiting) : ""].filter(Boolean).join(" · ");
  const tools = entries.length ? `<div class="bl-tools">${searchField(find.q)}${findFilters({ chips: view.chips, shown: view.shown.length, total: entries.length, find })}</div>` : "";
  return `<div class="narrow">${top("Backlog", esc(entries.length ? aside : ""))}${tools}${backlogResults(view, { entries, selected, now, find, textOf })}</div>${action}`;
}

export const ideaMachine = (candidates) => (candidates.find((one) => one.ideas && !one.asleep) ?? candidates.find((one) => one.ideas))?.machine ?? null;

export function chooseMachine(candidates, { repo = null, lastUsed = null, chosen = null, main = null, ideas = null } = {}) {
  if (candidates.length === 0) return null;
  const picked = candidates.find((one) => one.machine === chosen);
  if (picked) return { ...picked, why: "you chose it" };
  const home = candidates.find((one) => one.machine === ideas);
  if (home && !home.asleep) return { ...home, why: "ideas machine" };
  if (home) {
    const other = chooseMachine(candidates.filter((one) => one !== home && !one.asleep), { repo, lastUsed, main });
    return other ? { ...other, why: `ideas machine is asleep, sent to ${other.name}` } : { ...home, why: "ideas machine, asleep now" };
  }
  const head = candidates.find((one) => one.machine === main);
  if (head && !head.asleep) return { ...head, why: "main machine" };
  const others = head ? candidates.filter((one) => one !== head && !one.asleep) : candidates;
  if (head && others.length === 0) return { ...head, why: "main machine, asleep now" };
  const fallback = (one, why) => ({ ...one, why: head ? `main machine is offline, sent to ${one.name}` : why });
  const withRepo = repo ? others.find((one) => one.repos.includes(repo)) : null;
  if (withRepo) return fallback(withRepo, `it has ${repo}`);
  const last = others.find((one) => one.machine === lastUsed);
  if (last) return fallback(last, "you used it last");
  const awake = others.find((one) => !one.asleep) ?? others[0];
  return fallback(awake, awake.asleep ? "asleep now" : "awake now");
}

export function machinePicker(candidates, chosen, { label, choosing, main = null, ideas = null }) {
  const change = candidates.length > 1 ? `<button type="button" class="btn small" data-action="backlog-choose">${choosing ? "Done" : "Change"}</button>` : "";
  const list = choosing ? `<div class="options">${candidates.map((one) => `<div class="opt pick${one.machine === chosen.machine ? " picked" : ""}" role="button" tabindex="0" data-action="backlog-machine" data-machine="${esc(one.machine)}" aria-pressed="${one.machine === chosen.machine}"><b>${esc(one.name)}</b><small>${one.asleep ? "asleep" : "awake"}${one.machine === ideas ? " · ideas machine" : one.machine === main ? " · main machine" : ""}</small></div>`).join("")}</div>` : "";
  return `<section class="task"><div class="row"><div><div class="meta">${esc(label)}</div><div class="name">${esc(chosen.name)}</div><div class="meta">${esc(chosen.why)}</div></div>${change}</div></section>${list}`;
}

export function ideaForm({ text = "", candidates, chosen, choosing = false, busy = false, error = null, main = null, ideas = null, picker = "", account = null }) {
  const field = `<div class="note-field"><label for="idea">${account ? "What do you want done? " : ""}Describe what you want. The lead gives it a title and writes it out from the code, the notes and your chats.</label><textarea id="idea" data-draft="idea" rows="5" maxlength="2000">${esc(text)}</textarea></div>`;
  if (!chosen) return picker + field + `<div class="pro">No machine${account ? ` on ${esc(account)}` : ""} takes ideas from the hub. Allow them in kebbie's settings on a machine.</div>`;
  const ready = text.trim() !== "" && !busy;
  const failed = error ? `<div class="center warn" role="alert">${esc(error)}</div>` : "";
  return picker + field + machinePicker(candidates, chosen, { label: "Goes to the lead on", choosing, main, ideas }) + failed + `<div class="thumb-end idea-add"><button type="button" class="btn primary" data-action="add-idea"${ready ? "" : " disabled"}>${account ? `Add to ${esc(account)}'s backlog` : "Add to the backlog"}</button></div>`;
}

const dayOf = (seconds, now) => (new Date(seconds * 1000).toDateString() === new Date(now * 1000).toDateString() ? "today" : new Date(seconds * 1000).toLocaleDateString("en-GB", { day: "numeric", month: "short" }));
const HOME_TERMS = { vault: "note", local: "file", kept: "kept", found: "found" };

export function questionsLeft(body, sentFor) {
  const questions = body?.questions ?? [];
  const withLead = questions.filter((question, index) => question.blocks && (question.pending_answer !== null && question.pending_answer !== undefined || sentFor(index, questions)?.live)).length;
  const left = questions.filter((question) => question.blocks).length - withLead;
  return { left, withLead };
}

export const WRITE_AGAIN_AFTER = 86_400;

export function writingWord(row, now) {
  if (!row.writing) return "writing…";
  const age = waited(Math.max(0, now - row.writing));
  return age === "now" ? "writing…" : `writing… ${age}`;
}

export const writesAgain = (row, now) => row.state === "drafting" && Boolean(row.writing) && now - row.writing >= WRITE_AGAIN_AFTER;

function stateText(row, { left, withLead }, now) {
  if (row.state === "ready") return "ready";
  if (row.state === "to-write") return "to write: the lead writes it out";
  const lead = withLead ? `${plural(withLead, "answer", "answers")} with the lead` : "";
  if (left > 0) return `drafting: <span class="g-yellow">${waits(left)}</span> on you${lead ? ` · ${lead}` : ""}`;
  return `${esc(writingWord(row, now))}${lead ? ` · ${lead}` : ""}`;
}

export function itemFacts(entry, counts, now, title = "") {
  const { row } = entry;
  const names = entry.machines.map((one) => one.name);
  const seen = names.length > 1 ? `seen on ${names.slice(0, -1).join(", ")} and ${names.at(-1)}` : `on ${names[0]}`;
  const rows = [["priority", esc(row.priority)], ["repo", esc(row.repo ?? "none")], ["state", stateText(row, counts, now)], ["filed", esc(dayOf(row.filed, now))], [HOME_TERMS[row.home.kind], `${esc(row.home.label)}<span class="meta"> · ${esc(seen)}</span>`]];
  return `<section class="task"><dl class="facts">${rows.map(([label, value]) => `<dt>${label}</dt><dd>${value}</dd>`).join("")}</dl>${title}</section>`;
}

const section = (title, text) => `<h3 class="sec">${esc(title)}</h3><div class="md" data-markdown>${esc(text)}</div>`;

export function editStatus(sent, state, { standing = null, now }) {
  const machine = esc(sent.machineName);
  if (APPROVAL_STATES.includes(state)) return approvalStatus(sent, state, { standing, now, what: { goes: "Your edit is saved", again: "edit again" } });
  if (state === "refused" && sent.result?.reason === "section-changed") return block("refused", `${machine} has a newer version of this section; reload and edit again`, "", from(sent));
  if (state === "refused") return block("refused", ...refusal("Not saved", sent.result?.text), from(sent));
  if (state === "expired") return block("refused", `Not saved: ${machine} stayed asleep past the hour`, `Edit again when ${machine} is awake.`, `Sent ${from(sent)} · expired ${esc(clock(sent.exp))}`);
  if (state === "waiting") return block("wait", `Waiting for ${machine}`, `Your edit is saved when ${machine} wakes up.`, `Sent ${from(sent)} · expires ${esc(clock(sent.exp))}`);
  return "";
}

const saving = `<div class="meta saving" role="status"><span class="spinner"></span>Saving…</div>`;

export const cleanEdit = (editing) => (editing.section === null ? editing.text.replace(/\p{Cc}/gu, "") : editing.text.replace(/[^\P{Cc}\n\t]/gu, ""));
const thousands = (count) => count.toLocaleString("en-US");

export function editProblem(payload) {
  const length = [...payload.text].length;
  const what = payload.section === null ? "title" : "section";
  const limit = payload.section === null ? MAX_ITEM_TITLE_CHARS : MAX_SECTION_CHARS;
  if (length > limit) return `This ${what} is ${thousands(length)} characters; a ${what} holds at most ${thousands(limit)}.`;
  const bytes = new TextEncoder().encode(JSON.stringify({ item: payload.item, section: payload.section ?? null, text: payload.text, base: payload.base })).length;
  if (bytes > MAX_ITEM_EDIT_BYTES) return `This ${what} comes to ${Math.ceil(bytes / 1024)} kB, and an edit holds at most ${MAX_ITEM_EDIT_BYTES / 1024} kB. Shorten it before saving.`;
  return null;
}

export function editReady(editing, required) {
  if (!editing || editing.busy || editing.text === editing.original) return false;
  if (editing.section === null || required) return editing.text.trim() !== "";
  return true;
}

function editor(editing, { heading, required }) {
  const key = editing.section ?? "";
  const ready = editReady(editing, required);
  const field = editing.section === null
    ? `<input id="edit-title" data-draft="item-edit" maxlength="200" enterkeyhint="done" value="${esc(editing.text)}">`
    : `<textarea id="edit-${esc(key)}" data-draft="item-edit" rows="8" maxlength="8000">${esc(editing.text)}</textarea>`;
  const hint = required ? `${esc(heading)} can't be empty: the template requires it.` : "Markdown.";
  const failed = editing.error ? `<div class="center warn" role="alert">${esc(editing.error)}</div>` : "";
  return `<div class="sec-edit"><div class="note-field"><label for="edit-${esc(key) || "title"}">${editing.section === null ? "Title" : `${esc(heading)} · markdown`}</label>${field}</div><div class="meta">${hint}</div>${failed}<div class="row2"><button type="button" class="btn" data-action="item-edit-cancel" data-section="${esc(key)}">Cancel</button><button type="button" class="btn primary" data-action="item-edit-save" data-section="${esc(key)}"${ready ? "" : " disabled"}>Save</button></div></div>`;
}

const editButton = (key, label) => `<button type="button" class="sub-btn sec-tap" data-action="item-edit" data-section="${esc(key)}">${label}</button>`;

function titleArea(entry, edit) {
  if (!edit.can && !edit.sentFor(null)) return "";
  const editing = edit.ui?.section === null ? edit.ui : null;
  if (editing) return `<div class="title-edit" data-item="${esc(entry.id)}">${editor(editing, { heading: "Title", required: true })}</div>`;
  const sent = edit.sentFor(null);
  if (sent?.saving) return `<div class="title-edit" data-item="${esc(entry.id)}">${sent.sent.text ? `<div class="meta">title: ${esc(sent.sent.text)}</div>` : ""}${saving}</div>`;
  const status = sent?.recent ? editStatus(sent.sent, sent.state, { standing: sent.standing, now: edit.now }) : "";
  return `<div class="title-edit" data-item="${esc(entry.id)}">${status}${edit.can ? editButton("", "Edit title") : ""}</div>`;
}

function sectionBlock(entry, part, edit, after = "") {
  const editing = edit.ui?.section === part.key ? edit.ui : null;
  const sent = edit.sentFor(part.key);
  const button = edit.can && !editing && !sent?.saving ? editButton(part.key, "Edit") : "";
  const head = `<div class="sec-head"><h3 class="sec">${esc(part.heading)}</h3>${button}</div>`;
  let shown;
  if (editing) shown = editor(editing, part);
  else if (sent?.saving) shown = (sent.sent.text?.trim() ? `<div class="md" data-markdown>${esc(sent.sent.text)}</div>` : "") + saving;
  else {
    const status = sent?.recent ? editStatus(sent.sent, sent.state, { standing: sent.standing, now: edit.now }) : "";
    shown = (part.text.trim() ? `<div class="md" data-markdown>${esc(part.text)}</div>` : after ? "" : `<div class="meta">Empty.</div>`) + status;
  }
  return `<section class="sec-block" data-item="${esc(entry.id)}" data-section="${esc(part.key)}">${head}${shown}</section>${after}`;
}

const stillHere = (count = 1) => (count === 1 ? "Your answer is still here." : "Your answers are still here.");
const NOT_DELIVERED = {
  "question-changed": () => "the item changed while you answered",
  "unknown-item": (machine) => `the item is no longer on ${machine}`,
  "already-settled": () => "the question already has an answer",
  "not-allowed": (machine) => `${machine} takes no answers from the hub`,
  "bad-request": (machine) => `${machine} could not read it`,
};

export function notDelivered(sent, state) {
  const machine = sent.machineName;
  if (state === "expired") return `Not delivered: ${machine} stayed asleep past the hour. ${stillHere()}`;
  if (state === "not-approved") return `Not delivered: ${machine} has not approved this device. ${stillHere()}`;
  if (state === "denied") return `Not delivered: ${machine} turned this device away. ${stillHere()}`;
  const said = NOT_DELIVERED[sent.result?.reason]?.(machine) ?? (sent.result?.outcome === "failed" ? `something went wrong on ${machine}` : `${machine} turned it down`);
  return `Not delivered: ${said}. ${stillHere()}`;
}

const deliveryLine = (line) => `<div class="center left warn" role="alert">${esc(line)}</div>`;

export function answerStatus(sent, state, { standing = null, now }) {
  const machine = esc(sent.machineName);
  const said = `"${esc(sent.text)}"`;
  if (state === "approval") return approvalStatus(sent, state, { standing, now, what: { goes: `Your answer ${said} goes to the lead`, again: "answer again" } });
  if (settled(state)) return deliveryLine(notDelivered(sent, state));
  if (state === "done") return withLead(sent.text, `${from(sent)} · on ${machine}`);
  if (state === "waiting") return block("wait", `Waiting for ${machine}`, `Your answer ${said} goes to the lead when ${machine} wakes up.`, `Sent ${from(sent)} · expires ${esc(clock(sent.exp))}`);
  return block("wait", `Sent to ${machine}`, `Your answer ${said} is on its way to the lead.`, `Sent ${from(sent)} · expires ${esc(clock(sent.exp))}`);
}

const withLead = (text, meta) => block("wait", `With the lead: "${esc(text)}"`, "The lead folds your answer into the item and takes the question out.", meta);

const pendingOf = (question) => question.pending_answer !== null && question.pending_answer !== undefined;
const answerable = (question, sent) => !pendingOf(question) && !(sent && !settled(sent.state));
const retried = (question, sent) => Boolean(sent) && settled(sent.state) && !pendingOf(question);

const nthOf = (questions, index) => questions.slice(0, index).filter((question) => question.text === questions[index].text).length;
const keyOf = (text, nth) => (nth ? `${nth}\u0000${text}` : text);
export const questionKey = (questions, index) => keyOf(questions[index].text, nthOf(questions, index));
export const answers = (sent, questions, index) => (sent.question === undefined ? sent.index === index : sent.question === questions[index].text && (sent.nth ?? 0) === nthOf(questions, index));
export const keyAt = (body, index) => (body?.questions[index] ? questionKey(body.questions, index) : null);
export const sentKey = (sent) => keyOf(sent.question, sent.nth ?? 0);
const textOfKey = (key) => key.slice(key.indexOf("\u0000") + 1);

export const answerSents = (sentList) => sentList.flatMap((sent) => (sent.kind === "item-answers" ? (sent.answers ?? []).map((one) => ({ ...sent, kind: "item-answer", question: one.question, nth: one.nth ?? 0, text: one.text })) : [sent]));

export const answerableQuestions = (body, sentFor) => (body?.questions ?? []).flatMap((question, index, questions) => (answerable(question, sentFor(index, questions)) ? [index] : []));
export const draftOf = (ui, key) => ui.answers?.get(key) ?? "";
export const filledAnswers = (ui, body, indices) => indices.filter((index) => ui.open?.has(keyAt(body, index)) && draftOf(ui, keyAt(body, index)).trim() !== "");
const plainAnswers = (count) => (count === 1 ? "1 answer" : `${count} answers`);
export const sendAllLabel = (count, batch = null) => (batch ? `Sending ${plainAnswers(batch.total)}…` : `Send ${plainAnswers(count)}`);

export function startAnswering(ui, { chosen, answers: going, keys = going.map((one) => one.key) }) {
  if (ui.answering.batch || ui.answering.busy) return null;
  if (!chosen) {
    ui.answering.error = `Not sent: no machine takes answers from the hub right now. ${stillHere(going.length)}`;
    return null;
  }
  if (going.length === 0) {
    ui.answering.error = "Write an answer first, then send it.";
    return null;
  }
  ui.answering.batch = { total: going.length, keys };
  ui.answering.error = null;
  return ui.answering.batch;
}

export function bodyShown(held, shown) {
  if (!held) return shown ? { body: shown, bodyState: "ready" } : { body: null, bodyState: "loading" };
  return held.body ? { body: held.body, bodyState: "ready" } : { body: null, bodyState: "failed" };
}

export const answersToSend = (body, ui, indices) => indices.filter((index) => body?.questions[index]).map((index) => ({ index, key: keyAt(body, index), nth: nthOf(body.questions, index), question: body.questions[index], answer: draftOf(ui, keyAt(body, index)).trim() })).filter((one) => one.answer);
export const answerCommand = (entry, { index, nth, question, answer }) => ({ payload: { type: "item-answer", item: entry.key, index, question: question.text, answer }, meta: { address: entry.key, index, question: question.text, nth, text: answer, label: entry.row.title } });
export const answersCommand = (entry, going) => ({
  payload: { type: "item-answers", item: entry.key, answers: going.map(({ question, answer }) => ({ question: question.text, answer })) },
  meta: { address: entry.key, label: entry.row.title, answers: going.map(({ question, nth, answer }) => ({ question: question.text, nth, text: answer })) },
});
export const takesBatch = (machine, going) => takes(machine?.accepts, "item-answers") && new Set(going.map((one) => one.question.text)).size === going.length;

export async function sendAnswers(entry, going, { send, batch }) {
  if (batch) return (await send(answersCommand(entry, going))) ? [] : going.map((one) => one.key);
  const failed = [];
  for (const one of going) if (!(await send(answerCommand(entry, one)))) failed.push(one.key);
  return failed;
}

const keysOf = (body) => (body?.questions ?? []).map((_, index, questions) => questionKey(questions, index));

export function settleDrafts(ui, body, sentOf) {
  const keys = keysOf(body);
  let changed = false;
  for (const [key] of ui.answers) {
    const index = keys.indexOf(key);
    if (sentOf(key)?.state === "done" || (index >= 0 && pendingOf(body.questions[index]))) changed = ui.answers.delete(key) || changed;
  }
  keys.forEach((key, index) => {
    const sent = sentOf(key);
    if (retried(body.questions[index], sent) && draftOf(ui, key).trim() === "" && sent.sent.text) {
      ui.answers.set(key, sent.sent.text);
      changed = true;
    }
  });
  return changed;
}

export function strayDrafts(ui, body, sentOf) {
  const keys = new Set(keysOf(body));
  return [...ui.answers].filter(([key, text]) => !keys.has(key) && text.trim() !== "" && !(sentOf(key) && !settled(sentOf(key).state))).map(([key, text]) => ({ key, question: textOfKey(key), text }));
}

const draftsKey = (id) => `kebbie-answers:${id}`;
const mainKey = (account) => `kebbie-main-machine:${account}`;

export function readMain(storage, account) {
  try {
    return storage?.getItem(mainKey(account)) || null;
  } catch {
    return null;
  }
}

export function keepMain(storage, account, machine) {
  try {
    if (machine) storage?.setItem(mainKey(account), machine);
    else storage?.removeItem(mainKey(account));
  } catch {}
}

export function readDrafts(storage, id) {
  try {
    const raw = JSON.parse(storage?.getItem(draftsKey(id)) ?? "null");
    return new Map(Object.entries(raw && typeof raw === "object" && !Array.isArray(raw) ? raw : {}).filter(([, text]) => typeof text === "string" && text.trim() !== ""));
  } catch {
    return new Map();
  }
}

export function keepDrafts(storage, id, drafts) {
  try {
    const kept = [...drafts].filter(([, text]) => text.trim() !== "");
    if (kept.length) storage?.setItem(draftsKey(id), JSON.stringify(Object.fromEntries(kept)));
    else storage?.removeItem(draftsKey(id));
  } catch {}
}

function questionCard(question, index, { key, pickable, ui, sent, now }) {
  const pending = pendingOf(question);
  const live = sent && !settled(sent.state);
  const answered = pending || (live && sent.state === "done");
  const again = pickable && retried(question, sent);
  const picked = (again || Boolean(ui.open?.has(key))) && answerable(question, sent);
  const mark = question.blocks ? `<span class="qmark${answered ? "" : " warn"}" aria-hidden="true">?</span>` : `<span class="qmark" aria-hidden="true">·</span>`;
  const press = pickable && !pending && !live && !again;
  const classes = ["opt", "bq", question.blocks ? "" : "plain", press ? "pick" : "", picked ? "picked" : "", answered ? "answered" : ""].filter(Boolean).join(" ");
  const attributes = press ? ` role="button" tabindex="0" data-action="pick-question" data-index="${index}" data-key="${esc(key)}" aria-pressed="${picked}"` : "";
  const card = `<div class="${classes}"${attributes}><b>${mark}${esc(question.text)}</b>${question.blocks ? "" : "<small>the work can settle it</small>"}</div>`;
  if (again) return card + deliveryLine(notDelivered(sent.sent, sent.state)) + answerRow(index, key, ui, { again: true });
  const row = picked && press ? answerRow(index, key, ui) : "";
  if (sent && (live || !pending)) return card + answerStatus(sent.sent, sent.state, { standing: sent.standing, now }) + row;
  if (pending) return card + withLead(question.pending_answer, "with the lead on its machine");
  return card + row;
}

function againButton(key, batch) {
  const sending = Boolean(batch?.keys.includes(key));
  return `<button type="button" class="btn" data-action="send-item-answers" data-key="${esc(key)}"${batch ? " disabled" : ""}>${sending ? "Sending…" : "Send again"}</button>`;
}

function answerRow(index, key, ui, { again = false } = {}) {
  const field = `<div class="note-field"><label for="answer-${index}">Your answer</label><textarea id="answer-${index}" data-draft="item-answer" data-index="${index}" data-key="${esc(key)}"${again ? " data-again" : ""} rows="2" maxlength="2000">${esc(draftOf(ui, key))}</textarea></div>`;
  return `<div class="answer-row">${field}${again ? againButton(key, ui.answering?.batch ?? null) : ""}</div>`;
}

function strayBlock(strays) {
  if (strays.length === 0) return "";
  const one = ({ key, question, text }) => `<div class="opt bq plain"><b>${esc(question)}</b><small>no longer on the item</small></div>${deliveryLine(`Not delivered: the item changed while you answered. ${stillHere()} Copy it into the question as it reads now.`)}<div class="answer-row"><div class="md"><p>${esc(text)}</p></div><button type="button" class="btn" data-action="drop-answer" data-key="${esc(key)}">Remove</button></div>`;
  return `<div class="options qlist strays">${strays.map(one).join("")}</div>`;
}

function questionsBlock(body, { access, takesAnswers, refusedUrl, ui, sentFor, now, counts, strays = [], heading = `<h3 class="sec">Open questions</h3>` }) {
  const questions = body.questions;
  const head = counts.left > 0 ? `${counts.left === 1 ? "1 waits" : `${counts.left} wait`} on you before it starts` : "none blocks the start";
  const pickable = access === "ready" && takesAnswers;
  const fresh = (pickable ? answerableQuestions(body, sentFor) : []).filter((index) => !retried(questions[index], sentFor(index, questions)));
  const filled = filledAnswers(ui, body, fresh).length;
  const cards = questions.map((question, index) => questionCard(question, index, { key: questionKey(questions, index), pickable, ui, sent: sentFor(index, questions), now })).join("");
  const batch = ui.answering?.batch ?? null;
  const freshKeys = new Set(fresh.map((index) => keyAt(body, index)));
  const sending = batch && batch.keys.some((key) => freshKeys.has(key)) ? batch : null;
  const opened = fresh.some((index) => ui.open?.has(keyAt(body, index)));
  const all = opened || sending ? `<div class="answers-send"${filled >= 1 || sending ? "" : " hidden"}><button type="button" class="btn primary" data-action="send-item-answers"${ui.answering?.busy || batch ? " disabled" : ""}>${esc(sendAllLabel(filled, sending))}</button></div>` : "";
  const failed = pickable && ui.answering?.error ? deliveryLine(ui.answering.error) : "";
  const gate = access === "refused" ? proLine("Answering its questions from this device comes with Hub Pro.", refusedUrl) : access === "view-only" && questions.length ? viewOnlyBlock() : "";
  const stray = pickable ? strayBlock(strays) : "";
  if (questions.length === 0) return `${heading}<div class="meta pad">None open.</div>${stray}`;
  return `${heading}<div class="meta pad">${head}</div><div class="options qlist">${cards}</div>${stray}${all}${failed}${gate}`;
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

function startArea(entry, { access, refusedUrl, candidates, chosen, ui, start, now, taskHref, main }) {
  if (entry.row.state === "to-write") return "";
  if (access === "off") return "";
  if (access === "refused") return proLine("Starting it from this device comes with Hub Pro.", refusedUrl);
  if (access === "view-only") return viewOnlyBlock();
  if (!chosen) return `<div class="pro">Not started from here: ${esc(entry.machines.map((one) => one.name).join(" and "))} ${entry.machines.length > 1 ? "take" : "takes"} no starts from the backlog. Start it in kebbie, or allow it in its kebbie settings.</div>`;
  const status = start ? startStatus(start.sent, start.state, { title: entry.row.title, standing: start.standing, now, taskHref }) : "";
  if (start && !settled(start.state)) return status;
  const failed = ui.error ? `<div class="center warn" role="alert">${esc(ui.error)}</div>` : "";
  const picker = candidates.length > 1 ? machinePicker(candidates, chosen, { label: "Starts on", choosing: ui.choosing, main }) : "";
  return `${status}${picker}<div class="pair-answer"><a class="btn" href="#/backlog">Save to backlog</a><button type="button" class="btn primary" data-action="start-item"${ui.busy ? " disabled" : ""}>Start</button></div><div class="meta pad">The lead on ${esc(chosen.name)} picks the recipe and queues it.</div>${failed}`;
}

export function writeStatus(sent, state, { standing = null, now }) {
  const machine = esc(sent.machineName);
  if (APPROVAL_STATES.includes(state)) return approvalStatus(sent, state, { standing, now, what: { goes: "The lead writes it out", again: "ask again" } });
  if (state === "refused") return `<div class="center warn" role="alert">${esc(String(sent.result?.text ?? "Not sent").split("\n")[0].trim())}</div>`;
  if (state === "expired") return block("refused", `Not delivered: ${machine} stayed asleep past the hour`, `Ask again when ${machine} is awake.`, `Sent ${from(sent)} · expired ${esc(clock(sent.exp))}`);
  if (state === "waiting") return block("wait", `Waiting for ${machine}`, `The lead writes it out when ${machine} wakes up.`, `Sent ${from(sent)} · expires ${esc(clock(sent.exp))}`);
  if (state === "done") return "";
  return block("wait", `Sent to ${machine}`, "The lead writes it out in place.", `Sent ${from(sent)} · expires ${esc(clock(sent.exp))}`);
}

// Write it out: on an item to write, or one whose ask the lead left a day; while it is being written the card reads writing….
export function writeArea(entry, { access, write, ui, now }) {
  const { row } = entry;
  if (!write?.chosen || !["to-write", "drafting"].includes(row.state) || access !== "ready") return "";
  const again = writesAgain(row, now);
  const status = write.sent ? writeStatus(write.sent.sent, write.sent.state, { standing: write.sent.standing, now }) : "";
  if (write.sent && !settled(write.sent.state)) return status;
  if (row.state === "drafting" && !again) return status;
  const failed = ui.error ? `<div class="center warn" role="alert">${esc(ui.error)}</div>` : "";
  return `${status}<div class="row-btns"><button type="button" class="btn${row.state === "to-write" ? " primary" : ""}" data-action="write-item"${ui.busy ? " disabled" : ""}>${again ? "Write it out again" : "Write it out"}</button></div>${failed}`;
}

function ideaCard(entry, now) {
  const { sent, state } = entry.idea;
  const machine = sent.machineName;
  const said = state === "waiting" ? `It waits in the hub for ${machine} and goes out when it wakes. The lead then writes it out, and its sections show here.` : `The lead on ${machine} writes it out from the code, the notes and your chats. Its sections show here when it is saved.`;
  const shown = state === "waiting" ? `to write: waits for ${machine}` : `to write: the lead on ${machine} writes it out`;
  const status = APPROVAL_STATES.includes(state) ? approvalStatus(sent, state, { standing: entry.idea.standing ?? null, now, what: { goes: "Your idea goes to the lead", again: "add it again" } }) : settled(state) ? block("refused", ...refusal("Not filed", sent.result?.text ?? `${machine} stayed asleep past the hour`), from(sent)) : "";
  return `<section class="task"><dl class="facts"><dt>state</dt><dd>${esc(shown)}</dd><dt>filed</dt><dd>${esc(dayOf(sent.at, now))}, from ${esc(sent.deviceName ?? "this device")}</dd></dl></section><div class="md"><p>${esc(said)}</p></div>${status}`;
}

const FOUND_DONE = { "found-start": "✓ Started a task from it", "found-add": "✓ Sent to the task as a subtask to add", "found-dismiss": "✓ Dismissed", "found-keep": "✓ Kept in the backlog", "review-send": "✓ Sent to a full review" };
const FOUND_GOES = { "found-start": "The lead starts a task from it", "found-add": "The task adds it as a subtask", "found-dismiss": "It leaves the backlog", "found-keep": "It stays in the backlog", "review-send": "The reviewer starts a full review" };

export function foundStatus(sent, state, { standing = null, now }) {
  const machine = esc(sent.machineName);
  const kind = sent.kind;
  if (APPROVAL_STATES.includes(state)) return approvalStatus(sent, state, { standing, now, what: { goes: FOUND_GOES[kind], again: "press it again" } });
  if (state === "done") return block("done", FOUND_DONE[kind], esc(sent.result.text), `${from(sent)} · on ${machine}`);
  if (state === "refused") return block("refused", ...refusal("Not done", sent.result.text), from(sent));
  if (state === "expired") return block("refused", `Not delivered: ${machine} stayed asleep`, `Press it again when ${machine} is awake.`, `Sent ${from(sent)} · expired ${esc(clock(sent.exp))}`);
  if (state === "waiting") return block("wait", `Waiting for ${machine}`, `${FOUND_GOES[kind]} when ${machine} wakes up.`, `Sent ${from(sent)} · expires ${esc(clock(sent.exp))}`);
  return block("wait", `Sent to ${machine}`, `${FOUND_GOES[kind]}.`, `Sent ${from(sent)} · expires ${esc(clock(sent.exp))}`);
}

function foundArea(entry, { access, refusedUrl, found, ui, now }) {
  if (access === "off") return "";
  if (access === "refused") return proLine("Acting on what tasks found from this device comes with Hub Pro.", refusedUrl);
  if (access === "view-only") return viewOnlyBlock();
  if (!found.chosen) return `<div class="pro">${esc(entry.machines[0].name)} takes no actions on found items from the hub. Settle it in kebbie there.</div>`;
  const status = found.sent ? foundStatus(found.sent.sent, found.sent.state, { standing: found.sent.standing, now }) : "";
  if (found.sent && !settled(found.sent.state)) return status;
  const busy = ui.busy ? " disabled" : "";
  const adds = found.tasks.map((task) => `<button type="button" class="btn" data-action="found-add" data-task="${esc(task.address)}"${busy}>Add to ${esc(task.name)}</button>`).join("");
  const failed = ui.error ? `<div class="center warn" role="alert">${esc(ui.error)}</div>` : "";
  return `${status}<div class="row-btns found-acts"><button type="button" class="btn primary" data-action="found-start"${busy}>Start a task</button>${adds}<button type="button" class="btn" data-action="found-dismiss"${busy}>Dismiss</button></div>${failed}`;
}

function foundBody(entry, facts, options) {
  const { found } = entry.row;
  const sources = `<h3 class="sec">Found by</h3><div class="plan">${found.sources.map((source) => `<div>${esc(source)}</div>`).join("")}<div class="meta">${found.count === 1 ? "found once" : `found ${esc(found.count)} times`} outside their own work</div></div>`;
  return facts + sources + `<div class="decide found-area thumb-end" data-item="${esc(entry.id)}">${foundArea(entry, options)}</div>`;
}

export function itemBody(entry, { body = null, bodyState = null, access = "off", refusedUrl = null, ui, sentFor = () => null, start = null, candidates = [], chosen = null, now, taskHref = "#/board", edits = null, found = null, write = null, main = null, strays = [] }) {
  if (entry.idea) return ideaCard(entry, now);
  if (entry.gone) return `<div class="decide start-area">${start ? startStatus(start.sent, start.state, { title: entry.row.title, standing: start.standing, now, taskHref }) : ""}</div><div class="meta pad">It left the backlog when it started.</div>`;
  const counts = body ? questionsLeft(body, sentFor) : { left: entry.row.blocking, withLead: 0 };
  const edit = { can: access === "ready" && Boolean(edits?.machine), ui: edits?.editing ?? null, sentFor: edits?.sentFor ?? (() => null), now };
  const facts = itemFacts(entry, counts, now, entry.key && !isFound(entry) ? titleArea(entry, edit) : "");
  if (isFound(entry) && found) return foundBody(entry, facts, { access, refusedUrl, found, ui, now });
  const writing = writeArea(entry, { access, write, ui, now });
  const area = `<div class="decide start-area" data-item="${esc(entry.id)}">${writing}${startArea(entry, { access, refusedUrl, candidates, chosen, ui, start, now, taskHref, main })}</div>`;
  if (!entry.row.body && entry.row.state === "ready") return facts + (entry.row.goal ? section("Goal", entry.row.goal) : "") + area;
  if (!entry.row.body) {
    const said = entry.row.state === "drafting" ? "The lead is writing it out from the code, the notes and your chat. Its sections show here when it is saved." : `The lead on ${entry.machines[0].name} writes it out from the code, the notes and your chats. Its sections show here when it is saved.`;
    return facts + `<div class="md"><p>${esc(said)}</p></div>` + area;
  }
  if (!body) return facts + `<div class="plan"><div>${esc(bodyState === "failed" ? failedLine("item") : loadingLine("item", entry.row.body))}</div></div>` + area;
  const takesAnswers = entry.machines.some((one) => takes(one.accepts, "item-answer") || takes(one.accepts, "item-answers"));
  const questions = (heading) => `<div class="decide" data-item="${esc(entry.id)}">${questionsBlock(body, { access, takesAnswers, refusedUrl, ui, sentFor, now, counts, strays, heading })}</div>`;
  const parts = body.sections.map((part) => (part.key === body.questions_key ? sectionBlock(entry, part, edit, questions("")) : sectionBlock(entry, part, edit))).join("");
  const trailing = body.questions_key || body.questions.length === 0 ? "" : questions(undefined);
  return facts + parts + trailing + area;
}

export function startedEntry(id, sent) {
  const kind = sent.address.slice(0, sent.address.indexOf(":"));
  const row = { title: sent.label ?? sent.address, priority: "none", repo: null, state: "ready", filed: sent.at, questions: 0, blocking: 0, goal: null, body: null, home: { kind, label: sent.address.slice(kind.length + 1) } };
  return { id, key: sent.address, vault: kind === "vault", gone: true, row, machines: [{ machine: sent.machine, name: sent.machineName, accepts: null, asleep: false, repos: [], row: null }] };
}

export function startsAnyway(start, visit) {
  return Boolean(start?.sent.result?.reason === "questions-open" && start.sent.at >= visit);
}
