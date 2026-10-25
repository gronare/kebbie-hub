import { arrange, emptyMarks, findTask, isAsleep, mergeMarks, openBoards, readMarks } from "./board.js";
import { connectText, listWithout, PairingFailed, publishList, withReauth } from "./connect.js";
import { proposedName } from "./device-name.js";
import { autofillAvailable, NoPasskeyOffered, OFFER_TEXT, signInText, signInWith, signUpText, signUpWith } from "./signin.js";
import { account, connectCalls, follow, HubError } from "./hub.js";
import { forgetDeviceKeys, get, put } from "./store.js";
import { addUnlockingPasskey, rememberedCredential, STATE_TEXT, unlockAfter } from "./unlock.js";
import { PAGES, startPage } from "./pages.js";
import { supported as passkeysSupported } from "./passkey.js";

const root = document.getElementById("app");
const onPage = PAGES.includes(location.pathname);
const nowSeconds = () => Math.floor(Date.now() / 1000);
const state = { me: null, session: null, opened: [], marks: emptyMarks(), storeFailed: false, offline: false, fetchedAt: null, notice: null, byName: false, offerPasskey: false, selected: {}, hint: false };
const hub = { account, credentials: globalThis.navigator?.credentials };
const lists = { boards: account.boards, putMachines: connectCalls.putMachines };

const esc = (value) => String(value ?? "").replace(/[&<>"']/g, (char) => `&#${char.charCodeAt(0)};`);
const plural = (count, one, many) => `${count} ${count === 1 ? one : many}`;
const short = (sha) => (sha ?? "").slice(0, 7);
const kb = (bytes) => `${Math.max(1, Math.round(bytes / 1024))} kB`;

function when(seconds) {
  if (!seconds) return "never";
  const date = new Date(seconds * 1000);
  const time = date.toLocaleTimeString("en-GB", { hour: "2-digit", minute: "2-digit" });
  return date.toDateString() === new Date().toDateString() ? time : `${date.toLocaleDateString("en-GB", { day: "numeric", month: "short" })} ${time}`;
}

const isIos = () => /iPhone|iPad|iPod/.test(navigator.userAgent) || (navigator.platform === "MacIntel" && navigator.maxTouchPoints > 1);
const isStandalone = () => navigator.standalone === true || matchMedia("(display-mode: standalone)").matches;

function cryptoReady() {
  return typeof Uint8Array.fromBase64 === "function" && typeof Uint8Array.prototype.toBase64 === "function" && typeof indexedDB !== "undefined" && Boolean(crypto?.subtle);
}

function route() {
  const [, name = "board", ...rest] = location.hash.replace(/^#/, "").split("/");
  return { name, params: rest.map(decodeURIComponent) };
}

const go = (hash) => {
  if (location.hash === hash) render();
  else location.hash = hash;
};

function tabbar(active) {
  const questions = arrange(state.opened, nowSeconds()).questions.length;
  const tab = (hash, label, key) => `<a href="${hash}" class="${active === key ? "on" : ""}" ${active === key ? 'aria-current="page"' : ""}>${label}</a>`;
  return `<nav class="tabbar">${tab("#/board", "Board", "board")}${tab("#/questions", `Questions${questions ? ` ${questions}` : ""}`, "questions")}${tab("#/machines", "Machines", "machines")}</nav>`;
}

function top(title, aside = "") {
  return `<header class="top"><b>${esc(title)}</b><span>${aside}</span></header>`;
}

function taskRow(row) {
  const href = `#/task/${encodeURIComponent(row.machine)}/${encodeURIComponent(row.item.address)}`;
  const meta = [row.repo, row.item.summary].filter(Boolean).join(" · ");
  const you = row.item.next_for_you ? `<div class="you"><b>you:</b> ${esc(row.item.next_for_you)}</div>` : "";
  return `<a class="task" href="${href}"><div class="name">${esc(row.task)} <span class="machine">${esc(row.name)}</span></div><div class="meta">${esc(meta)}</div>${you}</a>`;
}

const PROBLEMS = {
  "unknown-machine": "not confirmed by your account; connect it again",
  "not-sealed-to-this-account": "the board is not sealed to your account yet",
  "revision-differs": "the hub's revision differs from the machine's",
  "bad-signature": "the signature does not match; not shown",
  expired: "the board is too old",
  "not-yet-valid": "this device's clock is wrong",
  "rolled-back": "older than the board you last read",
  "rolled-back-list": "the hub served an older list of your machines than this device has seen; not shown",
  "not-a-recipient": "the board is not sealed to your account",
  undecryptable: "does not decrypt",
};
const problemText = (code) => PROBLEMS[code] ?? `does not open (${code})`;

function installHint() {
  if (!state.hint) return "";
  return `<div class="hint"><div>Add kebbie to your home screen: tap <b>Share</b>, then <b>Add to Home Screen</b>. It then opens as its own app.</div><button type="button" class="close" data-action="dismiss-hint" aria-label="Close">×</button></div>`;
}

function freshness() {
  const machines = plural(state.opened.length, "machine", "machines");
  return state.offline ? "can't reach the hub right now" : machines;
}

function boardScreen() {
  const view = arrange(state.opened, nowSeconds());
  const parts = [top("kebbie", esc(freshness())), installHint(), passkeyOffer(), storeWarning()];
  if (state.opened.length === 0) {
    parts.push(`<div class="center">No machine is connected yet. Run <b>kebbie hub connect</b> on your computer.</div>`);
  } else if (!state.opened.some((machine) => machine.board || machine.problem)) {
    parts.push(`<div class="center">No connected machine has sent a board yet.</div>`);
  }
  for (const group of view.groups) {
    parts.push(`<h2 class="group g-${group.tone}">${esc(group.title)} · ${group.rows.length}</h2>`, ...group.rows.map(taskRow));
  }
  if (view.sleeping.length > 0) {
    parts.push(`<h2 class="group g-muted">Asleep</h2>`);
    for (const machine of view.sleeping) {
      const working = machine.working ? `${plural(machine.working, "task", "tasks")} were working then` : "nothing was working then";
      parts.push(`<div class="task asleep"><div class="name">${esc(machine.name)}</div><div class="meta">last seen ${esc(when(machine.lastSeen))} · ${working}</div></div>`);
    }
  }
  if (view.problems.length > 0) {
    parts.push(`<h2 class="group g-muted">Can't be read</h2>`);
    for (const machine of view.problems) parts.push(`<div class="task asleep"><div class="name">${esc(machine.name)}</div><div class="meta">${esc(problemText(machine.problem))}</div></div>`);
  }
  return parts.join("") + tabbar("board");
}

function questionsScreen() {
  const { questions } = arrange(state.opened, nowSeconds());
  const list = questions.length ? questions.map(taskRow).join("") : `<div class="center">No questions wait for you.</div>`;
  return top("Questions", questions.length ? `${questions.length} waiting` : "") + list + tabbar("questions");
}

function chips(key, options, recommend) {
  const selected = state.selected[key] ?? (recommend ? recommend - 1 : 0);
  const row = options.map((option, index) => `<button type="button" class="chip${index === selected ? " on" : ""}" data-action="choose" data-key="${esc(key)}" data-index="${index}" aria-pressed="${index === selected}">${esc(option.label)}</button>`).join("");
  const chosen = options[selected];
  const meaning = chosen ? `${esc(chosen.meaning ?? "")}${recommend && selected === recommend - 1 ? " · recommended" : ""}` : "";
  return `<div class="chips pad">${row}</div><div class="center">${meaning}</div>`;
}

function questionBlock(found) {
  const { item } = found;
  if (item.question) {
    const question = item.question;
    const plan = question.plan ? `<h3 class="sec">Plan</h3><div class="plan"><div>The plan (${kb(question.plan.bytes)}) shows here in a later version. It stays encrypted on the machine.</div></div>` : "";
    const context = [found.repo, item.summary].filter(Boolean).join(" · ");
    return `<section class="task"><div class="name">${esc(question.text)}</div><div class="meta">${esc(context)} · asked ${esc(when(question.at))}</div>${plan}</section>
      <h3 class="sec pad">Answer</h3>${chips(`${found.machine}|${item.address}|${question.id}`, question.options, question.recommend)}
      <button type="button" class="btn primary off" disabled>Answer · coming in a later version</button>`;
  }
  const prompt = item.prompt;
  return `<section class="task"><div class="name">${esc(prompt.question)}</div></section>
    <h3 class="sec pad">Answer</h3>${chips(`${found.machine}|${item.address}|${prompt.id}`, prompt.options.map((option) => ({ label: option.label, meaning: option.takes_text ? "with text" : "" })), null)}
    <button type="button" class="btn primary off" disabled>Answer · coming in a later version</button>`;
}

function cardBlock(found) {
  const { item } = found;
  const lines = [];
  if (item.card) {
    lines.push(item.card.current ? `<div class="meta ok">✓ current at ${esc(short(item.card.sha))}</div>` : `<div class="meta warn">the card is for ${esc(short(item.card.sha))}; HEAD has moved on</div>`);
    lines.push(`<h3 class="sec">Card</h3><div class="plan"><div>The card (${kb(item.card.blob.bytes)}) shows here in a later version. It stays encrypted on the machine.</div></div>`);
  }
  if (item.next_for_you) lines.push(`<div class="you"><b>you:</b> ${esc(item.next_for_you)}</div>`);
  const progress = [item.summary, item.report?.summary].filter(Boolean);
  if (progress.length) lines.push(`<h3 class="sec">Progress</h3><div class="plan">${progress.map((line) => `<div>${esc(line)}</div>`).join("")}</div>`);
  if (item.git) {
    const git = item.git;
    const facts = [`${git.ahead} ahead of ${git.upstream}`, git.behind ? `${git.behind} behind` : null, git.uncommitted ? `${git.uncommitted} uncommitted changes` : null, git.approved ? "approved for push" : null].filter(Boolean);
    lines.push(`<h3 class="sec">Commits · ${git.ahead}</h3><div class="plan"><div>${esc(facts.join(" · "))}</div>${item.head ? `<div class="mono">HEAD ${esc(short(item.head))}</div>` : ""}</div>`);
  }
  if (item.verdict) {
    const verdict = item.verdict;
    const text = verdict.running ? "review running" : verdict.verdict === "ok" ? `reviewed, ok${verdict.sha ? ` at ${short(verdict.sha)}` : ""}` : verdict.verdict === "changes" ? `reviewed, ${plural(verdict.findings ?? 0, "finding", "findings")}` : "not reviewed";
    lines.push(`<h3 class="sec">Review</h3><div class="plan"><div>${esc(text)}${verdict.stale ? " · stale" : ""}</div></div>`);
  }
  if (item.ci) lines.push(`<h3 class="sec">CI</h3><div class="plan"><div>${item.ci.state === "running" ? "CI running" : `CI failed${item.ci.failure ? `: ${esc(item.ci.failure)}` : ""}`}</div></div>`);
  if (item.push_waits) lines.push(`<div class="meta">${esc(item.push_waits)}</div>`);
  const card = lines.length ? `<section class="task">${lines.join("")}</section>` : "";
  const pushable = item.group === "READY TO PUSH" || item.card;
  const push = pushable && item.head ? `<button type="button" class="btn primary off" disabled>Push ${esc(short(item.head))} · coming in a later version</button>` : "";
  return card + push;
}

function taskScreen([machine, address]) {
  const found = findTask(state.opened, machine, address);
  if (!found) return top("Task") + `<div class="center">This task is no longer on the board.</div>` + tabbar("board");
  const asks = found.item.question || found.item.prompt;
  return top(found.task, esc(found.name)) + (asks ? questionBlock(found) : cardBlock(found)) + tabbar(asks ? "questions" : "board");
}

function storeWarning() {
  return state.storeFailed ? `<div class="center warn" role="alert">This browser can't remember which boards and machines it has seen, so an older board replayed by the hub is only caught during this visit.</div>` : "";
}

function machinesScreen() {
  const now = nowSeconds();
  const rows = state.opened.map((opened) => {
    const awake = !isAsleep({ lastSeen: opened.lastSeen }, now);
    const facts = opened.board ? [`last heard ${when(opened.lastSeen)}`, `kebbie ${opened.board.kelpie}`, `board rev ${opened.rev}`] : opened.problem ? [problemText(opened.problem)] : ["no board yet"];
    const remove = `<button type="button" class="btn small" data-action="remove-machine" data-id="${esc(opened.machine)}" data-name="${esc(opened.name)}">Remove</button>`;
    return `<div class="task${awake ? "" : " asleep"}"><div class="row"><div class="name">${esc(opened.name)} <span class="machine">${awake ? "awake" : "asleep"}</span></div>${remove}</div><div class="meta">${esc(facts.join(" · "))}</div></div>`;
  });
  const signedIn = state.me ? `<h3 class="sec">Account</h3><div class="meta pad">${esc(state.me.handle)}</div>` : "";
  const message = state.notice ? `<div class="center warn" role="alert">${esc(state.notice)}</div>` : "";
  return top("Machines", esc(`${state.opened.length} connected`)) + storeWarning() + message + rows.join("") + signedIn + tabbar("machines");
}

function lockedScreen() {
  const message = state.notice ? `<div class="center warn" role="alert">${esc(state.notice)}</div>` : "";
  const status = state.session?.state ?? "locked";
  const again = status === "locked" ? "Unlock with passkey" : "Try another passkey";
  return top("kebbie", esc(state.me?.handle ?? "")) + `<div class="center">${esc(STATE_TEXT[status] ?? STATE_TEXT.locked)}</div><button type="button" class="btn primary" data-action="unlock">${again}</button>${message}`;
}

function loginScreen(signUp) {
  const message = state.notice ? `<div class="center warn" role="alert">${esc(state.notice)}</div>` : "";
  if (!passkeysSupported()) return top("kebbie") + `<div class="center">This browser can't sign in with a passkey.</div>`;
  if (signUp) {
    return (
      top("Sign up") +
      `<div class="center">Enter your invite and pick an account name. Then this device adds a passkey.</div>
      <form data-form="signup" class="stack"><label class="task field"><span class="meta">Invite</span><input name="invite" class="code-input" autocomplete="off" autocapitalize="characters" spellcheck="false" placeholder="XXXX-XXXX" required></label>
      <label class="task field"><span class="meta">Account name</span><input name="handle" autocomplete="username" autocapitalize="none" spellcheck="false" pattern="[a-zA-Z][a-zA-Z0-9\\-]*" maxlength="32" required></label>
      <button class="btn primary">Create account with passkey</button></form>${message}<a class="center link" href="#/login">Have an account? Sign in</a>`
    );
  }
  const fallback = state.byName
    ? `<form data-form="login" class="stack"><label class="task field"><span class="meta">Account name</span><input name="handle" autocomplete="username webauthn" autocapitalize="none" spellcheck="false" required></label><button class="btn">Sign in with this account</button></form>`
    : `<button type="button" class="center link" data-action="by-name">Use an account name instead</button>`;
  return (
    top("kebbie") +
    `<div class="center">Sign in with your passkey. It unlocks your account's key on this device, which reads the board itself; the hub never sees what it says.</div>
    <button type="button" class="btn primary" data-action="sign-in">Sign in with passkey</button>${message}${fallback}<a class="center link" href="/signup">No account yet? Sign up</a>`
  );
}

function unsupportedScreen() {
  return top("kebbie") + `<div class="center">This browser lacks the cryptography the app needs (Ed25519, X25519, HKDF and base64 in Uint8Array). Update to iOS 18.2 or later, or a current Chrome or Firefox.</div>`;
}

function render() {
  if (onPage) return;
  const { name, params } = route();
  let html;
  if (!cryptoReady()) html = unsupportedScreen();
  else if (!state.me) html = loginScreen(name === "signup");
  else if (state.session?.state !== "unlocked") html = lockedScreen();
  else if (name === "questions") html = questionsScreen();
  else if (name === "machines") html = machinesScreen();
  else if (name === "task") html = taskScreen(params);
  else html = boardScreen();
  root.innerHTML = `<main class="screen">${html}</main>`;
  document.title = name === "task" && params[1] ? `${params[1].split("@")[0]} · kebbie` : "kebbie";
}

let retrying = null;

const marksKey = () => `marks:${state.me?.account}`;

async function storedMarks() {
  const { marks, failed } = await readMarks(get, marksKey());
  if (failed) state.storeFailed = true;
  return marks;
}

async function keepMarks() {
  try {
    await put(marksKey(), state.marks);
  } catch {
    state.storeFailed = true;
  }
}

async function cachedListing() {
  try {
    return (await get("listing")) ?? null;
  } catch {
    state.storeFailed = true;
    return null;
  }
}

async function refresh() {
  if (state.session?.state !== "unlocked") return;
  let listing;
  try {
    listing = await account.boards();
    state.offline = false;
    state.fetchedAt = nowSeconds();
    await put("listing", { listing, at: state.fetchedAt }).catch(() => (state.storeFailed = true));
  } catch (error) {
    if (error instanceof HubError && error.status === 401) return signedOut("Your session ended. Sign in again.");
    clearTimeout(retrying);
    retrying = setTimeout(refresh, 15_000);
    state.offline = true;
    const cached = await cachedListing();
    if (!cached?.listing) return render();
    listing = cached.listing;
    state.fetchedAt = cached.at;
  }
  state.marks = mergeMarks(state.marks, await storedMarks());
  const result = await openBoards(listing, state.session.identity, nowSeconds(), state.marks);
  state.opened = result.opened;
  state.marks = result.marks;
  await keepMarks();
  if (result.gone.length && !state.offline) {
    try {
      const pruned = await publishList({ hub: lists, identity: state.session.identity, marks: state.marks, change: (list) => result.gone.reduce((next, machine) => listWithout(machine)(next), list) });
      state.marks = mergeMarks(state.marks, { list: pruned.counter });
      await keepMarks();
      return refresh();
    } catch (error) {
      state.notice = error instanceof PairingFailed ? connectText(error.reason) : hubMessage(error);
    }
  }
  render();
}

async function removeMachine(machine, name) {
  if (!confirm(`Remove ${name}? Its token stops working and your account stops trusting it.`)) return;
  state.notice = null;
  try {
    const list = await publishList({ hub: lists, identity: state.session.identity, marks: state.marks, change: listWithout(machine) });
    state.marks = mergeMarks(state.marks, { list: list.counter });
    await keepMarks();
    await withReauth(
      () => connectCalls.removeMachine(machine),
      async () => {
        state.session = await unlockAfter(hub, await signingIn.withPasskey(rememberedCredential()));
        state.me = state.session.me;
      },
    );
  } catch (error) {
    state.notice = error instanceof PairingFailed ? connectText(error.reason) : hubMessage(error);
  }
  await refresh();
}

function signedOut(message) {
  following?.abort();
  following = null;
  state.me = null;
  state.session = null;
  state.opened = [];
  state.marks = emptyMarks();
  state.notice = message;
  go("#/login");
}

let following = null;
function startFeed() {
  if (state.session?.state !== "unlocked" || following) return;
  const controller = new AbortController();
  following = controller;
  let delay = 2000;
  const loop = async () => {
    while (!controller.signal.aborted) {
      try {
        await follow((name) => {
          delay = 2000;
          if (name === "board" || name === "machine-removed" || name === "machines") refresh();
        }, controller.signal);
      } catch (error) {
        if (error instanceof HubError && error.status === 401) return signedOut("Your session ended. Sign in again.");
      }
      await new Promise((resolve) => setTimeout(resolve, delay));
      delay = Math.min(delay * 2, 60_000);
    }
  };
  loop();
}

const deviceName = () => proposedName(navigator.userAgent);

function hubMessage(error) {
  if (error instanceof HubError) return `The hub answered ${error.status} (${error.code}).`;
  if (error?.name === "NotAllowedError") return "The passkey was cancelled.";
  return error?.message ?? String(error);
}

const signingIn = signInWith(hub);

async function unlocked(ceremony, byName) {
  signingIn.stop();
  state.byName = false;
  const previous = state.me?.account;
  state.session = await unlockAfter(hub, ceremony);
  state.me = state.session.me;
  if (previous !== state.me.account) state.marks = emptyMarks();
  state.offerPasskey = byName && state.session.state === "unlocked";
  state.notice = null;
  if (state.session.state !== "unlocked") return render();
  go("#/board");
  await refresh();
  startFeed();
}

async function startAutofill() {
  if (state.me || !(await autofillAvailable())) return;
  try {
    await unlocked(await signingIn.autofill(), false);
  } catch (error) {
    if (error?.name === "AbortError") return;
    state.notice = signInText(error);
    render();
  }
}

async function signUp(form) {
  await unlocked(await signUpWith(hub, { invite: form.invite.value.trim() }, form.handle.value, deviceName()), false);
}

async function signInWithPasskey() {
  try {
    await unlocked(await signingIn.withPasskey(rememberedCredential()), false);
  } catch (error) {
    if (error instanceof NoPasskeyOffered) state.byName = true;
    state.notice = signInText(error);
    render();
    if (state.byName) startAutofill();
  }
}

async function unlock() {
  const remembered = state.session?.state === "locked" ? rememberedCredential() : null;
  try {
    await unlocked(await signingIn.withPasskey(remembered), false);
  } catch (error) {
    state.notice = signInText(error);
    render();
  }
}

async function logIn(form) {
  await unlocked(await signingIn.byName(form.handle.value), true);
}

async function offerPasskey(yes) {
  state.offerPasskey = false;
  if (yes) {
    try {
      await addUnlockingPasskey(hub, state.session, deviceName());
    } catch (error) {
      state.notice = signUpText(error) ?? hubMessage(error);
    }
  }
  render();
}

const passkeyOffer = () =>
  state.offerPasskey ? `<section class="task"><div class="meta">${esc(OFFER_TEXT)}</div><div class="pair-answer"><button type="button" class="btn" data-action="passkey-later">Not now</button><button type="button" class="btn primary" data-action="passkey-add">Add passkey</button></div></section>` : "";

root.addEventListener("submit", async (event) => {
  const form = event.target.closest("form[data-form]");
  if (!form) return;
  event.preventDefault();
  const button = form.querySelector("button");
  if (button) button.disabled = true;
  try {
    if (form.dataset.form === "login") await logIn(form).catch((error) => {
      state.notice = signInText(error);
      render();
    });
    if (form.dataset.form === "signup") await signUp(form).catch((error) => {
      state.notice = signUpText(error) ?? hubMessage(error);
      render();
    });
  } catch (error) {
    state.notice = hubMessage(error);
    render();
  }
});

root.addEventListener("click", (event) => {
  const target = event.target.closest("[data-action]");
  if (!target) return;
  const action = target.dataset.action;
  if (action === "choose") {
    state.selected[target.dataset.key] = Number(target.dataset.index);
    render();
  } else if (action === "dismiss-hint") {
    state.hint = false;
    try {
      localStorage.setItem("kebbie-hint-dismissed", "1");
    } catch {}
    render();
  } else if (action === "sign-in") signInWithPasskey();
  else if (action === "unlock") unlock();
  else if (action === "remove-machine") removeMachine(target.dataset.id, target.dataset.name);
  else if (action === "by-name") {
    state.byName = true;
    state.notice = null;
    render();
    startAutofill();
  } else if (action === "passkey-add") offerPasskey(true);
  else if (action === "passkey-later") offerPasskey(false);
});

addEventListener("hashchange", () => {
  render();
  window.scrollTo(0, 0);
});

document.addEventListener("visibilitychange", () => {
  if (document.visibilityState === "visible") refresh();
});

async function start() {
  if (onPage) return startPage(root);
  if (!cryptoReady()) return render();
  try {
    state.hint = isIos() && !isStandalone() && localStorage.getItem("kebbie-hint-dismissed") !== "1";
  } catch {
    state.hint = isIos() && !isStandalone();
  }
  if ("serviceWorker" in navigator) navigator.serviceWorker.register("/sw.js", { type: "module", scope: "/" }).catch(() => {});
  await forgetDeviceKeys();
  state.me = await account.me().catch(() => null);
  state.session = state.me ? { state: "locked", me: state.me } : null;
  render();
  if (!state.me) return startAutofill();
  setInterval(() => document.visibilityState === "visible" && refresh(), 60_000);
}

start();
