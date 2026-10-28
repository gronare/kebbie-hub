import { arrange, emptyMarks, findTask, isAsleep, mergeMarks, openBoards, readMarks } from "./board.js";
import { connectText, listWithout, PairingFailed, publishList, safeNext, withReauth } from "./connect.js";
import { autofillAvailable, signInText, signInWith } from "./signin.js";
import { account, connectCalls, follow, HubError } from "./hub.js";
import { forgetDeviceKeys, get, put } from "./store.js";
import { addFirstPasskey, rememberedCredential, STATE_TEXT, unlockAfter } from "./unlock.js";
import { proposedName } from "./device-name.js";
import { PAGES, startPage } from "./pages.js";
import { supported as passkeysSupported } from "./passkey.js";
import { acceptCount, addPasskeyScreen, brand, cantUnlockScreen, connectDone, githubErrorText, hubProNote, installHint, lastSeenText, machineDots, machineRow, nav, needsFirstPasskey, optionCards, signInScreen, sleepingRow, taskKey, taskRow, unlockingScreen } from "./screens.js";

const root = document.getElementById("app");
let pageMode = PAGES.includes(location.pathname);
const onward = safeNext(new URLSearchParams(location.search).get("next"));
const nowSeconds = () => Math.floor(Date.now() / 1000);
const state = { info: null, me: null, session: null, opened: [], marks: emptyMarks(), storeFailed: false, offline: false, fetchedAt: null, notice: null, hint: false, unlocking: false, connected: null, from: "board" };
const wide = globalThis.matchMedia?.("(min-width: 1024px)") ?? { matches: false, addEventListener() {} };
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

function shell(active, main, panel = "") {
  const questions = arrange(state.opened, nowSeconds()).questions.length;
  const side = panel ? `<aside class="panel" aria-label="Selected task">${panel}</aside>` : "";
  return `<div class="app${panel ? " with-panel" : ""}">${nav({ active, questions })}<main class="screen in-app">${brand}${main}</main>${side}</div>`;
}

function top(title, aside = "") {
  return `<header class="top"><h1>${esc(title)}</h1><span>${aside}</span></header>`;
}

const awakeNow = (opened, now) => Boolean(opened.board) && !opened.expired && !isAsleep({ lastSeen: opened.lastSeen }, now);
const awakeFirst = (opened, now) => [...opened].sort((a, b) => Number(awakeNow(b, now)) - Number(awakeNow(a, now)));

const PROBLEMS = {
  "unknown-machine": "not confirmed by your account; connect it again",
  "not-sealed-to-this-account": "the board is not sealed to your account yet",
  "revision-differs": "the hub's revision differs from the machine's",
  "bad-signature": "the signature does not match; not shown",
  "not-yet-valid": "this device's clock is wrong",
  "rolled-back": "older than the board you last read",
  "rolled-back-list": "the hub served an older list of your machines than this device has seen; not shown",
  "not-a-recipient": "the board is not sealed to your account",
  undecryptable: "does not decrypt",
};
const problemText = (code) => PROBLEMS[code] ?? `does not open (${code})`;

function freshness() {
  const machines = plural(state.opened.length, "machine", "machines");
  return state.offline ? "can't reach the hub right now" : machines;
}

function boardHeader(view) {
  const now = nowSeconds();
  const phone = `<span class="phone-only">${[acceptCount(view.toAccept), esc(freshness())].filter(Boolean).join(" · ")}</span>`;
  const dots = state.offline ? `<span class="desk-only">${esc(freshness())}</span>` : machineDots(awakeFirst(state.opened, now).map((opened) => ({ name: opened.name, awake: awakeNow(opened, now), lastSeen: opened.lastSeen || opened.storedAt ? lastSeenText(opened.lastSeen ?? opened.storedAt, now) : null })));
  const accept = view.toAccept ? `<span class="desk-only">${acceptCount(view.toAccept)}</span>` : "";
  return top("Board", phone + dots + accept);
}

function boardScreen(selected = null) {
  const view = arrange(state.opened, nowSeconds());
  const parts = [boardHeader(view), state.connected ? connectDone({ name: state.connected, host: location.host }) : "", storeWarning()];
  if (state.opened.length === 0) {
    parts.push(`<div class="center">No machine is connected yet. Run <b>kebbie hub connect</b> on your computer.</div>`);
  } else if (!state.opened.some((machine) => machine.board || machine.problem || machine.expired)) {
    parts.push(`<div class="center">No connected machine has sent a board yet.</div>`);
  }
  const columns = view.groups.map((group) => `<section class="col"><h2 class="group g-${group.tone}">${esc(group.title)} · ${group.rows.length}</h2>${group.rows.map((row) => taskRow(row, selected)).join("")}</section>`);
  if (view.sleeping.length > 0) {
    columns.push(`<section class="col phone-only"><h2 class="group g-muted">Asleep</h2>${view.sleeping.map((machine) => sleepingRow(machine, nowSeconds())).join("")}</section>`);
  }
  if (columns.length) parts.push(`<div class="cols">${columns.join("")}</div>`);
  if (view.problems.length > 0) {
    parts.push(`<h2 class="group g-muted">Can't be read</h2>`);
    for (const machine of view.problems) parts.push(`<div class="task asleep"><div class="name">${esc(machine.name)}</div><div class="meta">${esc(problemText(machine.problem))}</div></div>`);
  }
  return parts.join("") + installHint(state.hint);
}

function questionsScreen(selected = null) {
  const { questions } = arrange(state.opened, nowSeconds());
  const list = questions.length ? questions.map((row) => taskRow(row, selected)).join("") : `<div class="center">No questions wait for you.</div>`;
  return `<div class="narrow">${top("Questions", questions.length ? `${questions.length} waiting` : "")}${list}</div>`;
}

function questionBlock(found) {
  const { item } = found;
  if (item.question) {
    const question = item.question;
    const plan = question.plan ? `<section class="task"><h3 class="sec">Plan</h3><div class="plan"><div>The plan (${kb(question.plan.bytes)}) shows here in a later version. It stays encrypted on the machine.</div></div></section>` : "";
    const context = [found.repo, item.summary].filter(Boolean).join(" · ");
    return `<p class="q">${esc(question.text)}</p><div class="meta pad">${esc(context)} · asked ${esc(when(question.at))}</div>${plan}${optionCards(question.options, question.recommend)}${hubProNote()}`;
  }
  const prompt = item.prompt;
  return `<p class="q">${esc(prompt.question)}</p>${optionCards(prompt.options.map((option) => ({ label: option.label, meaning: option.takes_text ? "with text" : "" })))}${hubProNote()}`;
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

function taskBody(found) {
  return found.item.question || found.item.prompt ? questionBlock(found) : cardBlock(found);
}

function taskScreen([machine, address]) {
  const found = findTask(state.opened, machine, address);
  if (!found) return shell("board", top("Task") + `<div class="center">This task is no longer on the board.</div>`);
  const asks = found.item.question || found.item.prompt;
  return shell(asks ? "questions" : "board", top(found.task, esc(found.name)) + taskBody(found));
}

function taskPanel([machine, address]) {
  const found = findTask(state.opened, machine, address);
  if (!found) return `<h2>Task</h2><div class="center left">This task is no longer on the board.</div>`;
  return `<span class="machine">${esc(found.name)}</span><h2>${esc(found.task)}</h2>${taskBody(found)}`;
}

function wideTaskScreen(params) {
  const selected = taskKey(params[0], params[1]);
  const list = state.from === "questions" ? questionsScreen(selected) : boardScreen(selected);
  return shell(state.from, list, taskPanel(params));
}

function storeWarning() {
  return state.storeFailed ? `<div class="center warn" role="alert">This browser can't remember which boards and machines it has seen, so an older board replayed by the hub is only caught during this visit.</div>` : "";
}

function machinesScreen() {
  const now = nowSeconds();
  const rows = awakeFirst(state.opened, now).map((opened) => {
    const awake = !opened.expired && !isAsleep({ lastSeen: opened.lastSeen }, now);
    const facts = opened.problem ? [problemText(opened.problem)] : awake && opened.board ? [`last heard ${when(opened.lastSeen)}`, `kebbie ${opened.board.kelpie}`, `board rev ${opened.rev}`] : opened.board || opened.expired ? [`last seen ${lastSeenText(opened.lastSeen ?? opened.storedAt, now)}`] : ["no board yet"];
    return machineRow({ machine: opened.machine, name: opened.name, awake, facts });
  });
  const message = state.notice ? `<div class="center warn" role="alert">${esc(state.notice)}</div>` : "";
  return `<div class="narrow">${top("Machines", esc(`${state.opened.length} connected`))}${storeWarning()}${message}${rows.join("")}<a class="btn phone-only" href="/account">Account</a></div>`;
}

function lockedScreen() {
  const status = state.session?.state ?? "locked";
  if (needsFirstPasskey(state.me, state.session)) return addPasskeyScreen({ login: state.me.github, notice: state.notice, busy: state.busy });
  if (status === "locked") return signInScreen({ github: false, notice: state.notice });
  if (status === "no-prf" || status === "not-wrapped" || status === "github") return cantUnlockScreen({ state: status, notice: state.notice });
  const message = state.notice ? `<div class="center warn" role="alert">${esc(state.notice)}</div>` : "";
  return top("kebbie") + `<div class="center">${esc(STATE_TEXT[status] ?? STATE_TEXT.locked)}</div><button type="button" class="btn primary" data-action="other-passkey">Try another passkey</button>${message}`;
}

function unsupportedScreen() {
  return top("kebbie") + `<div class="center">This browser lacks the cryptography the app needs (Ed25519, X25519, HKDF and base64 in Uint8Array). Update to iOS 18.2 or later, or a current Chrome or Firefox.</div>`;
}

function render() {
  if (pageMode) return;
  const { name, params } = route();
  let html;
  if (!cryptoReady()) html = unsupportedScreen();
  else if (state.unlocking) html = unlockingScreen();
  else if (!state.me) html = signInScreen({ github: Boolean(state.info?.github), notice: state.notice, next: onward });
  else if (state.session?.state !== "unlocked") html = lockedScreen();
  else if (name === "questions") html = shell("questions", questionsScreen());
  else if (name === "machines") html = shell("machines", machinesScreen());
  else if (name === "task") html = wide.matches ? wideTaskScreen(params) : taskScreen(params);
  else html = shell("board", boardScreen());
  root.innerHTML = html.startsWith('<div class="app') ? html : `<main class="screen">${html}</main>`;
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

function hubMessage(error) {
  if (error instanceof HubError) return `The hub answered ${error.status} (${error.code}).`;
  if (error?.name === "NotAllowedError") return "The passkey was cancelled.";
  return error?.message ?? String(error);
}

const signingIn = signInWith(hub);

const unlocked = (ceremony) => entered(() => unlockAfter(hub, ceremony));

async function entered(unlock) {
  signingIn.stop();
  const previous = state.me?.account;
  state.unlocking = true;
  render();
  try {
    state.session = await unlock();
  } finally {
    state.unlocking = false;
  }
  state.me = state.session.me;
  if (onward) return location.assign(onward);
  if (previous !== state.me.account) state.marks = emptyMarks();
  state.notice = null;
  if (state.session.state !== "unlocked") return render();
  state.unlocking = true;
  go("#/board");
  await refresh();
  state.unlocking = false;
  render();
  startFeed();
}

async function startAutofill() {
  if (state.me || !(await autofillAvailable())) return;
  try {
    await unlocked(await signingIn.autofill());
  } catch (error) {
    if (error?.name === "AbortError") return;
    state.notice = signInText(error);
    render();
  }
}

async function signInWithPasskey(remembered) {
  try {
    await unlocked(await signingIn.withPasskey(remembered));
  } catch (error) {
    state.unlocking = false;
    state.notice = signInText(error);
    render();
  }
}

async function addPasskey() {
  state.busy = true;
  state.notice = null;
  const me = state.me;
  try {
    await entered(() => addFirstPasskey(hub, me, proposedName(navigator.userAgent)));
  } catch (error) {
    state.me = me;
    state.session = { state: "locked", me };
    state.notice = hubMessage(error);
    render();
  } finally {
    state.busy = false;
  }
}

function showBoard(session, name) {
  pageMode = false;
  history.replaceState(null, "", "/#/board");
  state.session = session;
  state.me = session.me;
  state.connected = name;
  state.marks = emptyMarks();
  showHint();
  render();
  refresh();
  startFeed();
  setInterval(() => document.visibilityState === "visible" && refresh(), 60_000);
}

root.addEventListener("click", (event) => {
  if (pageMode) return;
  const target = event.target.closest("[data-action]");
  if (!target) return;
  const action = target.dataset.action;
  if (action === "dismiss-hint") {
    state.hint = false;
    try {
      localStorage.setItem("kebbie-hint-dismissed", "1");
    } catch {}
    render();
  } else if (action === "sign-in") signInWithPasskey(rememberedCredential());
  else if (action === "other-passkey") signInWithPasskey(null);
  else if (action === "add-passkey") addPasskey();
  else if (action === "not-now") {
    state.session = { state: "github", me: state.me };
    render();
  }
  else if (action === "remove-machine") removeMachine(target.dataset.id, target.dataset.name);
});

addEventListener("hashchange", () => {
  if (pageMode) return;
  const { name } = route();
  if (name !== "board") state.connected = null;
  if (name === "board" || name === "questions") state.from = name;
  render();
  if (!(wide.matches && name === "task")) window.scrollTo(0, 0);
});

wide.addEventListener("change", () => !pageMode && render());

addEventListener("keydown", (event) => {
  if (!pageMode && event.key === "Escape" && wide.matches && route().name === "task") go(`#/${state.from}`);
});

document.addEventListener("visibilitychange", () => {
  if (document.visibilityState === "visible") refresh();
});

function showHint() {
  try {
    state.hint = isIos() && !isStandalone() && localStorage.getItem("kebbie-hint-dismissed") !== "1";
  } catch {
    state.hint = isIos() && !isStandalone();
  }
}

async function start() {
  if (pageMode) return startPage(root, { showBoard });
  if (!cryptoReady()) return render();
  showHint();
  if ("serviceWorker" in navigator) navigator.serviceWorker.register("/sw.js", { type: "module", scope: "/" }).catch(() => {});
  await forgetDeviceKeys();
  const params = new URLSearchParams(location.search);
  state.notice = githubErrorText(params.get("github_error"));
  if (params.has("github_error")) history.replaceState(null, "", location.pathname + location.hash);
  state.info = await account.hub().catch(() => null);
  state.me = await account.me().catch(() => null);
  if (state.me && onward) return location.assign(onward);
  state.session = state.me ? { state: "locked", me: state.me } : null;
  render();
  if (!state.me) return startAutofill();
  setInterval(() => document.visibilityState === "visible" && refresh(), 60_000);
}

start();
