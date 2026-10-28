import { toAccept } from "./board.js";
import { connectBlock, connectBlockText } from "./connect.js";

export const esc = (value) => String(value ?? "").replace(/[&<>"']/g, (char) => `&#${char.charCodeAt(0)};`);

export const brand = `<header class="brand"><svg class="brand-mark" viewBox="0 0 32 32" aria-hidden="true"><rect class="mark-bg" width="32" height="32" rx="9" fill="#a8501f"/><g transform="translate(16 16) scale(0.66) translate(-16 -16)"><path d="M23 18V12.5A7.5 7.5 0 0 0 8 12.5A4.6 4.6 0 0 0 17.2 12.5" fill="none" stroke="#ffffff" stroke-width="3.8" stroke-linecap="round" stroke-linejoin="round"/><path d="M14.41 22.55L17.30 25.20L14.41 27.85" fill="none" stroke="#ffffff" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"/><rect class="mark-dot" x="20.5" y="20.8" width="5" height="8.8" fill="#ffffff"/></g></svg><b>kebbie hub</b></header>`;
const heading = (title, lead) => `<h1 class="title">${esc(title)}</h1><p class="lead">${esc(lead)}</p>`;
const warn = (text) => (text ? `<div class="center warn" role="alert">${esc(text)}</div>` : "");
const disabled = (off) => (off ? " disabled" : "");
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

export function stepLines(item) {
  const steps = item.steps;
  if (!steps || (steps.pushed === 0 && steps.items.length === 0)) return "";
  const pushed = steps.pushed ? `<div class="pushed">↑ ${esc(steps.pushed)} pushed</div>` : "";
  const rules = item.git?.approved ? " · rules ✓" : "";
  const lines = steps.items.map((step) => {
    const label = step.state === "committed" ? `committed${rules}` : STEP_LABELS[step.state];
    return `<div class="step s-${esc(step.state)}"><span class="what"><span class="mark">${STEP_MARKS[step.state]}</span><span class="number">${esc(step.number)}</span><span class="step-title">${esc(step.title)}</span></span>${label ? `<span class="label">${esc(label)}</span>` : ""}</div>`;
  });
  return `<div class="steps">${pushed}${lines.join("")}</div>`;
}

export const acceptCount = (count) => (count ? `<span class="accept">◆ ${esc(count)} to accept</span>` : "");

export const taskKey = (machine, address) => `${machine}|${address}`;

export function taskRow(row, selected = null) {
  const href = `#/task/${encodeURIComponent(row.machine)}/${encodeURIComponent(row.item.address)}`;
  const on = selected === taskKey(row.machine, row.item.address);
  const waiting = toAccept(row.item);
  const meta = [row.repo && esc(row.repo), waiting ? `<span class="accept">${waiting} to accept</span>` : "", esc(row.item.summary)].filter(Boolean).join(" · ");
  const next = row.item.steps?.items.find((step) => step.state === "committed");
  const wanted = row.item.next_for_you ?? (next ? `accept ${next.number} on ${row.name}` : null);
  const you = wanted ? `<div class="you"><b>you:</b> ${esc(wanted)}</div>` : "";
  return `<a class="task${on ? " sel" : ""}" href="${href}"${on ? ' aria-current="true"' : ""}><div class="head"><span class="name">${esc(row.task)}</span> <span class="machine">${esc(row.name)}</span></div><div class="meta">${meta}</div>${stepLines(row.item)}${you}</a>`;
}

export function nav({ active, questions = 0, deskOnly = false }) {
  const tab = (href, label, key) => `<a href="${href}" class="${active === key ? "on" : ""}"${active === key ? ' aria-current="page"' : ""}>${label}</a>`;
  const badge = questions ? ` <em class="badge">${esc(questions)}</em>` : "";
  const account = `<a href="/account" class="account${active === "account" ? " on" : ""}"${active === "account" ? ' aria-current="page"' : ""}>Account</a>`;
  return `<nav class="tabbar${deskOnly ? " desk-nav" : ""}"><div class="side-brand brand">${brand.replace(/^<header class="brand">|<\/header>$/g, "")}</div>${tab("/#/board", "Board", "board")}${tab("/#/questions", `Questions${badge}`, "questions")}${tab("/#/machines", "Machines", "machines")}${account}</nav>`;
}

export function optionCards(options, recommend = null) {
  const cards = options.map((option, index) => {
    const recommended = Boolean(recommend) && index === recommend - 1;
    return `<div class="opt${recommended ? " rec" : ""}">${recommended ? `<span class="tag">Recommended</span>` : ""}<b>${esc(option.label)}</b>${option.meaning ? `<small>${esc(option.meaning)}</small>` : ""}</div>`;
  });
  return `<div class="options">${cards.join("")}</div>`;
}

export const hubProNote = () => `<div class="pro"><b>Coming soon · Hub Pro</b><span class="phone-only">Answer from your phone.</span><span class="desk-only">Answer here.</span> Today you answer at the machine with i on the board.</div>`;

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

export function connectWelcome({ name, code = "", signupOpen, github, notice = null, busy = false }) {
  const lead = signupOpen
    ? `The hub shows ${name}'s board on your phone and in any browser, end-to-end encrypted. Create a passkey to start: it signs you in and unlocks your boards wherever it's saved.`
    : `The hub shows ${name}'s board on your phone and in any browser, end-to-end encrypted. Sign in with your passkey to connect it.`;
  const back = `/connect?code=${encodeURIComponent(code)}`;
  const gh = github ? githubButton(signupOpen ? "Continue with GitHub" : "Sign in with GitHub", githubHref({ connect: signupOpen ? code : null, next: back })) : "";
  const start = signupOpen
    ? `<button type="button" class="btn primary" data-action="create-passkey"${disabled(busy)}>Create passkey</button>${gh}${warn(notice)}<p class="foot">Already have a kebbie account? <button type="button" class="link inline" data-action="sign-in">Sign in</button></p>`
    : `<button type="button" class="btn primary" data-action="sign-in"${disabled(busy)}>Sign in with passkey</button>${gh}${warn(notice)}`;
  return brand + heading(`Connect ${name} to kebbie hub`, lead) + start;
}

export function connectAsk({ name, code, machines = [], blocked = null, typed = false, plan = "", notice = null, busy = false }) {
  const already = machines.length ? `<section class="task small-card"><div class="meta">Already connected</div><div>${esc(machines.join(" · "))}</div></section>` : "";
  const typedLine = typed ? `<div class="center">You typed the code, so this page can't check which machine it is. Its board stays unreadable until you connect it from the link kebbie opens.</div>` : "";
  const reason = blocked ? `<div class="center warn" role="alert">${esc(blocked)}</div>` : "";
  return (
    brand +
    heading(`Connect ${name}?`, "Check that your terminal shows the same code. If it doesn't, cancel: another machine is asking.") +
    `<section class="task machine-card"><div class="row-start">${MACHINE_ICON}<div><div class="name">${esc(name)}</div><div class="meta">waiting in its terminal</div></div></div><div class="code peach" aria-live="polite">${esc(code)}</div></section>` +
    already +
    typedLine +
    reason +
    plan +
    `<div class="pair-answer"><button type="button" class="btn" data-action="cancel"${disabled(busy)}>Cancel</button><button type="button" class="btn primary" data-action="connect"${disabled(busy || blocked)}>Connect ${esc(name)}</button></div>` +
    warn(notice)
  );
}

export function connectDone({ name, host }) {
  return `<section class="done"><div class="ok title-ok">✓ ${esc(name)} is connected</div><p class="lead">Its board shows here now. On your phone, open ${esc(host)} and sign in with the same passkey.</p></section>`;
}

export function connectTyped({ name }) {
  return brand + `<div class="center ok">${esc(name)} is connected. Its board stays unreadable until you connect it from the link kebbie opens on that computer.</div>`;
}

export function connectCancelled() {
  return brand + `<div class="center">Cancelled. The terminal shows it too.</div>`;
}

export function signInScreen({ github, notice = null, next = null }) {
  const gh = github ? `<a class="btn" href="${esc(githubHref({ next }))}">Sign in with GitHub</a>` : "";
  return (
    brand +
    heading("See your boards", "Your machines' boards, end-to-end encrypted. Sign in with the passkey you made for kebbie.") +
    `<button type="button" class="btn primary" data-action="sign-in">Sign in with passkey</button>${gh}${warn(notice)}` +
    `<p class="foot">New to kebbie? Start in your terminal: <code>kebbie setup</code></p>`
  );
}

export function unlockingScreen() {
  return brand + `<div class="center"><span class="spinner" aria-hidden="true"></span> Unlocking your boards…</div>`;
}

const CANT_UNLOCK = {
  "no-prf": "You're signed in, but this browser can't unlock your boards with its passkey.",
  "not-wrapped": "You're signed in, but this passkey can't unlock your boards yet.",
  github: "You're signed in with GitHub, but GitHub can't unlock your boards.",
};

export function addPasskeyScreen({ login, notice = null, busy = false }) {
  return (
    brand +
    `<div class="center ok">✓ Signed in with GitHub as ${esc(login)}</div>` +
    heading("Add a passkey to unlock your boards", "GitHub signs you in. A passkey also unlocks your boards on every phone and browser, so you never approve one by hand.") +
    `<button type="button" class="btn primary" data-action="add-passkey"${disabled(busy)}>Add passkey</button><button type="button" class="link" data-action="not-now"${disabled(busy)}>Not now</button>${warn(notice)}`
  );
}

export const needsFirstPasskey = (me, session) => Boolean(me?.github && !me.key && (!session || session.state === "locked"));

export function accountScreen({ me, github, notice = null }) {
  const linked = me.github
    ? `<section class="task"><div class="row"><div><div class="name">GitHub</div><div class="meta">Signed in with GitHub as ${esc(me.github)}</div></div><button type="button" class="btn small" data-action="remove-github">Remove</button></div></section>`
    : github
      ? `<section class="task"><div class="row"><div><div class="name">GitHub</div><div class="meta">Not connected</div></div><a class="btn small" href="${esc(githubHref({ link: true, next: "/account" }))}">Connect GitHub</a></div></section>`
      : "";
  return brand + `<h1 class="title">Account</h1><div class="center mono">${esc(me.account)}</div>` + linked + warn(notice) + `<a class="btn" href="/">Back to your boards</a>`;
}

export function cantUnlockScreen({ state = "no-prf", notice = null }) {
  return (
    brand +
    heading("This browser can't unlock your boards", CANT_UNLOCK[state] ?? CANT_UNLOCK["no-prf"]) +
    `<section class="task"><div class="meta">What works today: sign in on a device whose passkey can unlock them, such as an iPhone or Mac with iCloud Keychain, Google Password Manager or 1Password.</div></section>` +
    `<button type="button" class="btn primary" data-action="other-passkey">Use a passkey instead</button>${warn(notice)}`
  );
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

export function signupScreen({ signedIn, next, invite, github = false, notice = null }) {
  const top = brand + `<h1 class="title">Sign up</h1>`;
  if (signedIn) return top + `<div class="center ok">You're signed in.</div><a class="btn primary" href="${esc(next ?? "/")}">Continue</a>`;
  if (invite) return top + `<p class="lead">You're invited to this hub. Create a passkey to start: it signs you in and unlocks your boards.</p><button type="button" class="btn primary" data-action="create-passkey">Create passkey</button>${github ? githubButton("Continue with GitHub", githubHref({ invite, next })) : ""}${warn(notice)}`;
  return top + `<p class="foot">New to kebbie? Start in your terminal: <code>kebbie setup</code></p><a class="btn" href="/">Sign in</a>`;
}

export function connectScreen({ supported = true, phase = "start", view = null, me = null, session = null, fragment = null, signupOpen = false, github = false, machines = [], plan = "", notice = null, busy = false, code = "" }) {
  if (!supported) return brand + `<div class="center">This browser cannot sign in with a passkey.</div>`;
  const name = view?.name ?? "this machine";
  if (phase === "connected") return connectTyped({ name });
  if (phase === "cancelled") return connectCancelled();
  if (phase === "pairing") return brand + `<div class="center ok">Handing ${esc(name)} your account's key …</div>`;
  if (!view) return brand + (notice ? warn(notice) : `<div class="center">Loading …</div>`);
  if (!me) return connectWelcome({ name, code: view.code || code, signupOpen, github, notice, busy });
  if (needsFirstPasskey(me, session)) return addPasskeyScreen({ login: me.github, notice, busy });
  if (session?.state === "github") return cantUnlockScreen({ state: "github", notice });
  const blocked = session?.state === "locked" ? null : connectBlock({ fragment, session });
  return connectAsk({ name, code: view.code || code, machines, blocked: blocked ? connectBlockText(blocked, name) : null, typed: !fragment, plan, notice, busy });
}
