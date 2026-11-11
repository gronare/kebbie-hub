import { acceptRequired, toAccept } from "./board.js";
import { connectBlock, connectBlockText } from "./connect.js";

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

const STEP_MARKS = { planned: "○", "in-progress": "◐", committed: "●", accepted: "✓" };
const STEP_LABELS = { planned: "", "in-progress": "in progress", committed: "committed", accepted: "accepted" };

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
const nextIcon = (row) => NEXT_ICONS[row.part] ?? (row.item?.question || row.item?.prompt ? "?" : "!");

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

export function taskFacts(item) {
  const agent = agentChip(item.agent, true);
  const profile = item.profile ? `profile ${esc(item.profile)}` : "";
  const parts = [agent, profile, esc(recipeText(item))].filter(Boolean);
  return parts.length ? `<div class="meta">${parts.join(" · ")}</div>` : "";
}

export function reviewLines(item) {
  const lines = [];
  if (item.assessment) lines.push(`<div>${esc(item.assessment)}</div>`);
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

export function stepLines(item) {
  const steps = item.steps;
  if (!steps || (steps.pushed === 0 && steps.items.length === 0)) return "";
  const pushed = steps.pushed ? `<div class="pushed">↑ ${esc(steps.pushed)} pushed</div>` : "";
  const rules = item.git?.approved ? " · rules ✓" : "";
  const lines = steps.items.map((step) => {
    const label = step.state === "committed" ? `committed${rules}` : STEP_LABELS[step.state];
    return `<div class="step s-${esc(step.state)}"><span class="what"><span class="mark">${STEP_MARKS[step.state]}</span><span class="number">${esc(step.number)}</span><span class="step-title">${esc(step.title)}</span></span>${label ? `<span class="label">${esc(label)}</span>` : ""}</div>`;
  });
  const optional = acceptRequired(item) || steps.items.length === 0 ? "" : `<div class="meta">accepting each step is optional: push takes what is committed</div>`;
  return `<div class="steps">${pushed}${lines.join("")}${optional}</div>`;
}

export const acceptCount = (count) => (count ? `<span class="accept">◆ ${esc(count)} to accept</span>` : "");

export const taskKey = (machine, address) => `${machine}|${address}`;

export function taskRow(row, selected = null) {
  const href = `#/task/${encodeURIComponent(row.machine)}/${encodeURIComponent(row.item.address)}`;
  const on = selected === taskKey(row.machine, row.item.address);
  const waiting = toAccept(row.item);
  const meta = [row.repo && esc(row.repo), agentChip(row.item.agent, true), esc(recipeText(row.item)), esc(row.item.assessment ?? ""), waiting ? `<span class="accept">${waiting} to accept</span>` : "", esc(row.item.summary)].filter(Boolean).join(" · ");
  const next = acceptRequired(row.item) ? row.item.steps.items.find((step) => step.state === "committed") : null;
  const wanted = row.item.next_for_you ?? (next ? `accept ${next.number} on ${row.name}` : null);
  const you = wanted ? `<div class="you"><b>you:</b> ${esc(wanted)}</div>` : "";
  return `<a class="task${on ? " sel" : ""}" href="${href}"${on ? ' aria-current="true"' : ""}><div class="head"><span class="name">${esc(taskName(row.item, row.task))}</span> <span class="machine">${esc(row.name)}</span></div><div class="meta">${meta}</div>${stepLines(row.item)}${you}</a>`;
}

export const ICONS = {
  board: `<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M9 4v16M15 4v16"/>`,
  questions: `<path d="M4 4h16a1 1 0 0 1 1 1v11a1 1 0 0 1-1 1h-9l-5 4v-4H4a1 1 0 0 1-1-1V5a1 1 0 0 1 1-1z"/><path d="M10 9a2 2 0 1 1 2.8 1.8c-.5.3-.8.7-.8 1.4"/><path d="M12 14.6v.1"/>`,
  machines: `<rect x="5" y="5" width="14" height="10" rx="1.5"/><path d="M3 19h18"/>`,
  back: `<path d="M15 5l-7 7 7 7"/>`,
  sections: `<rect x="3.5" y="4.5" width="17" height="15" rx="2.5"/><path d="M9.5 4.5v15M5.8 8.5h1.6M5.8 11.5h1.6"/>`,
};

export const icon = (name) => `<svg class="tab-icon" viewBox="0 0 24 24" aria-hidden="true">${ICONS[name]}</svg>`;

export const backButton = (href) => `<a class="back-btn round" href="${esc(href)}" aria-label="Back">${icon("back")}</a>`;

export function nav({ active, questions = 0, deskOnly = false }) {
  const tab = (href, label, key, said = label) => `<a href="${href}" aria-label="${esc(said)}" class="${active === key ? "on" : ""}"${active === key ? ' aria-current="page"' : ""}>${label}${icon(key)}</a>`;
  const badge = questions ? ` <em class="badge">${esc(questions)}</em>` : "";
  const account = `<a href="/account" class="account${active === "account" ? " on" : ""}"${active === "account" ? ' aria-current="page"' : ""}>Account</a>`;
  return `<nav class="tabbar${deskOnly ? " desk-nav" : ""}"><div class="side-brand brand">${brand.replace(/^<header class="brand">|<\/header>$/g, "")}</div>${tab("/#/board", "Board", "board")}${tab("/#/questions", `Questions${badge}`, "questions", questions ? `Questions, ${questions} waiting` : "Questions")}${tab("/#/machines", "Machines", "machines")}${account}</nav>`;
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

export const emailField = (value = "") => `<div class="note-field"><label for="email">${esc(EMAIL_LABEL)}</label><input id="email" name="email" type="email" autocomplete="email" inputmode="email" maxlength="254" value="${esc(value)}"></div>`;

function emailState(address) {
  if (!address?.email) return "No email yet";
  if (address.verified) return `${address.email} · confirmed`;
  return address.sent ? `${address.email} · not confirmed yet. We sent a link; open it to confirm.` : `${address.email} · not confirmed`;
}

export function emailSection(address, busy = false, codes = false) {
  if (codes) return `<section class="task"><div class="name">Email</div><div class="meta${address?.verified ? " ok" : ""}">${esc(emailState({ ...address, sent: false }))}</div><div class="note-field"><label for="email">Change email</label><input id="email" name="email" type="email" autocomplete="email" inputmode="email" maxlength="254" value="${esc(address?.email ?? "")}"></div><button type="button" class="btn small" data-action="save-email"${disabled(busy)}>Send code</button></section>`;
  const remove = address?.email ? `<button type="button" class="link" data-action="remove-email"${disabled(busy)}>Remove email</button>` : "";
  return `<section class="task"><div class="name">Email</div><div class="meta${address?.verified ? " ok" : ""}">${esc(emailState(address))}</div>${emailField(address?.email ?? "")}<button type="button" class="btn small" data-action="save-email"${disabled(busy)}>Save email</button>${remove}</section>`;
}

const accountId = (id) => `<div class="row"><span class="mono">${esc(id)}</span><button type="button" class="btn small" data-action="copy-account" data-copy="${esc(id)}" aria-label="Copy account id">Copy</button></div>`;

export function accountScreen({ me, github, email = false, codes = false, address = null, notice = null, busy = false }) {
  const linked = me.github
    ? `<section class="task"><div class="row"><div><div class="name">GitHub</div><div class="meta">Signed in with GitHub as ${esc(me.github)}</div></div><button type="button" class="btn small" data-action="remove-github">Remove</button></div></section>`
    : github
      ? `<section class="task"><div class="row"><div><div class="name">GitHub</div><div class="meta">Not connected</div></div><a class="btn small" href="${esc(githubHref({ link: true, next: "/account" }))}">Connect GitHub</a></div></section>`
      : "";
  return brand + `<h1 class="title">Account</h1>` + accountId(me.account) + linked + (email ? emailSection(address, busy, codes) : "") + warn(notice) + `<a class="btn back-link" href="/">Back to your boards</a><button type="button" class="btn" data-action="sign-out"${disabled(busy)}>Sign out</button>` + backButton("/");
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
