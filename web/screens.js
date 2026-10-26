import { connectBlock, connectBlockText } from "./connect.js";

export const esc = (value) => String(value ?? "").replace(/[&<>"']/g, (char) => `&#${char.charCodeAt(0)};`);

const brand = `<header class="brand"><span class="brand-mark" aria-hidden="true">k</span><b>kebbie hub</b></header>`;
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
  if (phase === "done") return top + `<div class="center ok">You own this hub.</div><a class="btn primary" href="/admin">Open admin</a><a class="btn" href="/">Open kebbie</a>`;
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
