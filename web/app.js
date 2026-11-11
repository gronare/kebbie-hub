import { blobReader, blobSlot, failedLine, kb, loadingLine, planHref } from "./blobs.js";
import { markdown } from "./markdown.js";
import { arrange, emptyMarks, findTask, isAsleep, mergeMarks, openBoards, readMarks } from "./board.js";
import { connectText, listWithout, PairingFailed, publishList, safeNext, withReauth } from "./connect.js";
import { autofillAvailable, signInText, signInWith } from "./signin.js";
import { account, connectCalls, deviceCalls, follow, HubError } from "./hub.js";
import * as deviceStore from "./store.js";
import { collectResults, DeviceListProblem, enableDevice, forgetDevice, keepSent, loadDevice, loadSent, removeListedDevice, sendCommand, trustedDeviceList } from "./device.js";
import { answerBlock, answerReady, COMMAND_TEXT, deviceRows, gate, latestSent, pushBlock, pushReady, refusedBlock, sentState, settled, setupBlock } from "./decide.js";
import { forgetDeviceKeys, get, put } from "./store.js";
import { addFirstPasskey, forgetUnlock, keptUnlock, lockWhenDue, rememberedCredential, signedInAccount, STATE_TEXT, unlockAfter } from "./unlock.js";
import { proposedName } from "./device-name.js";
import { PAGES, startPage } from "./pages.js";
import { supported as passkeysSupported } from "./passkey.js";
import { outOfRangeScroll } from "./scroll.js";
import { choosePose, overviewPose, POSES, railSide } from "./pose.js";
import { backHref, boardSections, firstTaskOf, sectionPills, sectionSheet, sectionsButton, sectionView } from "./sections.js";
import { acceptCount, addPasskeyScreen, awayLine, brand, cantUnlockScreen, connectDone, githubErrorText, machineAnswerNote, installHint, lastSeenText, laterGroups, machineDots, machineRow, nav, needsFirstPasskey, nextList, optionCards, promptBlock, backButton, reviewLines, routinesBlock, signInScreen, sleepingRow, stepLines, taskFacts, taskKey, taskName, taskRow, unlockingScreen, unlockScreen } from "./screens.js";

const root = document.getElementById("app");
let pageMode = PAGES.includes(location.pathname);
const onward = safeNext(new URLSearchParams(location.search).get("next"));
const nowSeconds = () => Math.floor(Date.now() / 1000);
const state = { info: null, me: null, session: null, opened: [], marks: emptyMarks(), storeFailed: false, offline: false, fetchedAt: null, notice: null, hint: false, unlocking: false, connected: null, from: "board", sheet: false, device: null, kept: { cursor: 0, sent: [] }, ui: new Map(), keys: new Map(), devices: null, pro: { busy: false, error: null }, refused: null };
const hub = { account, credentials: globalThis.navigator?.credentials };

const cameraAtBottom = () => globalThis.screen?.orientation?.angle === (Math.max(innerWidth, innerHeight) > 800 ? 90 : 270);

function markStrip() {
  const probe = document.createElement("div");
  probe.style.cssText = "position: fixed; visibility: hidden; padding-left: env(safe-area-inset-left); padding-right: env(safe-area-inset-right)";
  document.body.append(probe);
  const { paddingLeft, paddingRight } = getComputedStyle(probe);
  probe.remove();
  const left = parseFloat(paddingLeft) >= 60;
  const right = parseFloat(paddingRight) >= 60;
  const pending = !left && !right && stripExpected();
  const strip = left || right || pending;
  const classes = document.documentElement.classList;
  classes.toggle("strip-left", left);
  classes.toggle("strip-right", right);
  classes.toggle("strip-pending", pending);
  classes.toggle("strip-cam-bottom", (left || right) && cameraAtBottom());
  classes.toggle("fold", strip && Math.max(innerWidth, innerHeight) <= 800);
  const next = choosePose({ width: innerWidth, height: innerHeight, strip });
  for (const name of POSES) classes.toggle(`pose-${name}`, name === next);
  const rail = next === "land" ? railSide(globalThis.screen?.orientation?.angle) : null;
  classes.toggle("rail-left", rail === "left");
  classes.toggle("rail-right", rail === "right");
  const shown = `${next}:${overviewPose(next, innerHeight > innerWidth)}`;
  const changed = layout !== null && layout !== shown;
  pose = next;
  layout = shown;
  if (changed && !pageMode) render();
}

const DUO_SCREEN = [466, 678];
let resizedAt = -Infinity;

const duoApp = () => isStandalone() && Math.min(screen.width, screen.height) === DUO_SCREEN[0] && Math.max(screen.width, screen.height) === DUO_SCREEN[1];

function stripExpected() {
  if (performance.now() - resizedAt > 1200 || !duoApp()) return false;
  return Math.max(innerWidth, innerHeight) <= 800 || innerWidth > innerHeight;
}
let pose = null;
let layout = null;


const splitLike = () => pose === "split" || pose === "land";
const twoColumns = () => splitLike() || pose === "desk";
const overview = () => overviewPose(pose, innerHeight > innerWidth);
const lists = { boards: account.boards, putMachines: connectCalls.putMachines };
const unlockEnds = lockWhenDue(lockBoards);

const esc = (value) => String(value ?? "").replace(/[&<>"']/g, (char) => `&#${char.charCodeAt(0)};`);
const plural = (count, one, many) => `${count} ${count === 1 ? one : many}`;
const short = (sha) => (sha ?? "").slice(0, 7);
const blobs = blobReader(account.blob);
const blobRefs = new Map();

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

function boardCounts(view) {
  const needs = view.next.reduce((sum, list) => sum + list.rows.length, 0);
  return [needs ? `${needs} need you` : "", acceptCount(view.toAccept), esc(freshness())].filter(Boolean).join(" · ");
}

function boardHeader(view) {
  const now = nowSeconds();
  const phone = `<span class="phone-only">${boardCounts(view)}</span>`;
  const dots = state.offline ? `<span class="desk-only">${esc(freshness())}</span>` : machineDots(awakeFirst(state.opened, now).map((opened) => ({ name: opened.name, awake: awakeNow(opened, now), lastSeen: opened.lastSeen || opened.storedAt ? lastSeenText(opened.lastSeen ?? opened.storedAt, now) : null })));
  const accept = view.toAccept ? `<span class="desk-only">${acceptCount(view.toAccept)}</span>` : "";
  return top("Board", phone + dots + accept);
}

function boardNotices(view, now) {
  const parts = [state.connected ? connectDone({ name: state.connected, host: location.host }) : "", storeWarning(), awayLine(view.away, now)];
  if (state.opened.length === 0) {
    parts.push(`<div class="center">No machine is connected yet. Run <b>kebbie hub connect</b> on your computer.</div>`);
  } else if (!state.opened.some((machine) => machine.board || machine.problem || machine.expired)) {
    parts.push(`<div class="center">No connected machine has sent a board yet.</div>`);
  }
  return parts.join("");
}

function boardColumns(view, selected, now) {
  const columns = view.groups.map((group) => `<section class="col"><h2 class="group g-${group.tone}">${esc(group.title)} · ${group.rows.length}</h2>${group.rows.map((row) => taskRow(row, selected)).join("")}</section>`);
  const later = laterGroups(view.later, now);
  if (later) columns.push(later);
  const routines = routinesBlock(view.routines, now);
  if (routines) columns.push(routines);
  if (view.sleeping.length > 0) {
    columns.push(`<section class="col phone-only"><h2 class="group g-muted">Asleep</h2>${view.sleeping.map((machine) => sleepingRow(machine, now)).join("")}</section>`);
  }
  return columns.length ? `<div class="cols">${columns.join("")}</div>` : "";
}

function problemsBlock(view) {
  if (view.problems.length === 0) return "";
  return `<h2 class="group g-muted">Can't be read</h2>` + view.problems.map((machine) => `<div class="task asleep"><div class="name">${esc(machine.name)}</div><div class="meta">${esc(problemText(machine.problem))}</div></div>`).join("");
}

function boardScreen(selected = null) {
  const view = arrange(state.opened, nowSeconds());
  const now = nowSeconds();
  return boardHeader(view) + boardNotices(view, now) + nextList(view.next, now) + boardColumns(view, selected, now) + problemsBlock(view) + installHint(state.hint);
}

function boardOverview() {
  const now = nowSeconds();
  const view = arrange(state.opened, now);
  const sections = boardSections(view);
  if (sections.length === 0) return boardScreen();
  const body = view.next.length ? nextList(view.next, now) : boardColumns(view, null, now);
  return `<h1 class="big-title">Board</h1><p class="lead">${boardCounts(view)}</p>` + boardNotices(view, now) + sectionPills(sections) + body + problemsBlock(view) + installHint(state.hint);
}

function sectionList(view, key, selected) {
  return sectionView(view, key, nowSeconds(), { selected, nextLine: boardCounts(view) });
}

function splitScreen(key, params = null, panel = taskPanel) {
  const view = arrange(state.opened, nowSeconds());
  const sections = boardSections(view);
  if (sections.length === 0) return shell("board", boardScreen());
  const current = sections.some((section) => section.key === key) ? key : sections[0].key;
  const chosen = params ?? firstTaskOf(view, current);
  const selected = chosen ? taskKey(...chosen) : null;
  const sheet = state.sheet ? sectionSheet(sections, current) : "";
  return shell("board", `<div class="bar">${sectionsButton(state.sheet)}</div>${sectionList(view, current, selected)}${sheet}`, chosen ? panel(chosen) : "");
}

const sectionOf = (from) => (from.startsWith("section/") ? from.slice("section/".length) : "next");

function questionsScreen(selected = null) {
  const { questions } = arrange(state.opened, nowSeconds());
  const list = questions.length ? questions.map((row) => taskRow(row, selected)).join("") : `<div class="center">No questions wait for you.</div>`;
  return `<div class="narrow">${top("Questions", questions.length ? `${questions.length} waiting` : "")}${list}</div>`;
}

function slot(ref, options) {
  blobRefs.set(ref.id, ref);
  return blobSlot(ref, options);
}

function fillSlot(element) {
  const ref = blobRefs.get(element.dataset.blob);
  const entry = ref ? blobs.peek(ref.id) : null;
  if (entry?.text !== undefined) {
    element.className = element.dataset.look;
    element.replaceChildren(...markdown(entry.text));
    return;
  }
  const line = document.createElement("div");
  line.textContent = entry?.failed || !ref ? failedLine(element.dataset.kind) : loadingLine(element.dataset.kind, ref);
  element.className = element.dataset.wait;
  element.replaceChildren(line);
  if (!entry && ref) blobs.load(ref).then((loaded) => loaded && fillBlobs(ref.id));
}

function fillBlobs(id = null) {
  for (const element of root.querySelectorAll("[data-blob]")) if (id === null || element.dataset.blob === id) fillSlot(element);
}

function questionBlock(found) {
  const { item } = found;
  if (item.question) {
    const question = item.question;
    const plan = question.plan ? `<section class="task"><h3 class="sec">Plan</h3>${slot(question.plan, { kind: "plan", look: "md md-peek" })}<a class="btn" href="${planHref(found.machine, item.address)}">Read the whole plan</a></section>` : "";
    const context = [found.repo, item.summary].filter(Boolean).join(" · ");
    return `<p class="q">${esc(question.text)}</p><div class="meta pad">${esc(context)} · asked ${esc(when(question.at))}</div>${plan}${answerArea(found)}`;
  }
  return promptBlock(found, state.info?.commands ? `<div class="pro">Answer Codex prompts at the machine.</div>` : machineAnswerNote());
}

function cardBlock(found) {
  const { item } = found;
  const lines = [];
  if (item.card) {
    lines.push(item.card.current ? `<div class="meta ok">✓ current at ${esc(short(item.card.sha))}</div>` : `<div class="meta warn">the card is for ${esc(short(item.card.sha))}; HEAD has moved on</div>`);
    lines.push(`<h3 class="sec">Card</h3>${slot(item.card.blob, { kind: "card", look: "md" })}`);
  }
  lines.push(taskFacts(item));
  if (item.next_for_you) lines.push(`<div class="you"><b>you:</b> ${esc(item.next_for_you)}</div>`);
  const steps = stepLines(item);
  if (steps) lines.push(`<h3 class="sec">Steps</h3>${steps}`);
  lines.push(reviewLines(item));
  const progress = [item.summary, item.report?.summary].filter(Boolean);
  if (progress.length) lines.push(`<h3 class="sec">Progress</h3><div class="plan">${progress.map((line) => `<div>${esc(line)}</div>`).join("")}</div>`);
  if (item.git) {
    const git = item.git;
    const facts = [`${git.ahead} ahead of ${git.upstream}`, git.behind ? `${git.behind} behind` : null, git.uncommitted ? `${git.uncommitted} uncommitted changes` : null, git.approved ? "approved for push" : null].filter(Boolean);
    lines.push(`<h3 class="sec">Commits · ${git.ahead}</h3><div class="plan"><div>${esc(facts.join(" · "))}</div>${item.head ? `<div class="mono">HEAD ${esc(short(item.head))}</div>` : ""}</div>`);
  }
  if (item.ci) lines.push(`<h3 class="sec">CI</h3><div class="plan"><div>${item.ci.state === "running" ? "CI running" : `CI failed${item.ci.failure ? `: ${esc(item.ci.failure)}` : ""}`}</div></div>`);
  if (item.push_waits) lines.push(`<div class="meta">${esc(item.push_waits)}</div>`);
  const card = lines.some(Boolean) ? `<section class="task">${lines.join("")}</section>` : "";
  return card + pushArea(found);
}

const uiKey = (machine, address) => `${machine}|${address}`;
const freshUi = () => ({ picked: null, free: false, note: "", confirming: false, busy: false, error: null });

function uiOf(machine, address) {
  const key = uiKey(machine, address);
  if (!state.ui.has(key)) state.ui.set(key, freshUi());
  return state.ui.get(key);
}

const commandGate = () => gate({ info: state.info, refused: state.refused, device: state.device });
const decide = (found, html) => `<div class="decide" data-machine="${esc(found.machine)}" data-address="${esc(found.item.address)}">${html}</div>`;

function sentFor(found, kind, question = null) {
  const sent = latestSent(state.kept.sent, { machine: found.machine, address: found.item.address, kind, question });
  if (!sent) return { sent: null, state: null };
  const opened = state.opened.find((machine) => machine.machine === found.machine);
  return { sent, state: sentState(sent, { now: nowSeconds(), asleep: isAsleep({ lastSeen: opened?.lastSeen ?? null }, nowSeconds()) }) };
}

function answerArea(found) {
  const question = found.item.question;
  const access = commandGate();
  if (access === "off") return optionCards(question.options, question.recommend) + machineAnswerNote();
  if (access === "refused") return optionCards(question.options, question.recommend) + refusedBlock(state.refused.url);
  if (access === "setup") return optionCards(question.options, question.recommend) + setupBlock(state.pro);
  const { sent, state: shown } = sentFor(found, "answer", question.id);
  return decide(found, answerBlock({ key: uiKey(found.machine, found.item.address), question, ui: uiOf(found.machine, found.item.address), sent, state: shown }));
}

function pushArea(found) {
  const { item } = found;
  const pushable = item.group === "READY TO PUSH" || item.card;
  if (!pushable || !item.head) return "";
  const access = commandGate();
  if (access === "off" || !pushReady(item)) return `<button type="button" class="btn primary off" disabled>Push ${esc(short(item.head))} · coming in a later version</button>`;
  if (access === "refused") return refusedBlock(state.refused.url);
  if (access === "setup") return setupBlock(state.pro);
  const { sent, state: shown } = sentFor(found, "push");
  const current = sent && sent.head !== item.head && settled(shown) ? null : sent;
  return decide(found, pushBlock({ item, machineName: found.name, ui: uiOf(found.machine, item.address), sent: current, state: current ? shown : null }));
}

function taskBody(found) {
  return found.item.question || found.item.prompt ? questionBlock(found) : cardBlock(found);
}

function taskScreen([machine, address]) {
  const back = overview() ? backButton(backHref("task", state.from)) : "";
  const found = findTask(state.opened, machine, address);
  if (!found) return shell("board", back + top("Task") + `<div class="center">This task is no longer on the board.</div>`);
  const asks = found.item.question || found.item.prompt;
  return shell(asks ? "questions" : "board", back + top(taskName(found.item, found.task), esc(found.name)) + taskBody(found));
}

function taskPanel([machine, address]) {
  const found = findTask(state.opened, machine, address);
  if (!found) return `<h2>Task</h2><div class="center left">This task is no longer on the board.</div>`;
  return `<span class="machine">${esc(found.name)}</span><h2>${esc(taskName(found.item, found.task))}</h2>${taskBody(found)}`;
}

function planParts([machine, address]) {
  const found = findTask(state.opened, machine, address);
  const question = found?.item.question;
  const plan = question?.plan;
  if (!plan) return null;
  const meta = ["Plan", kb(plan.bytes), question.plan_at ? `written ${when(question.plan_at)}` : null].filter(Boolean).join(" · ");
  const back = `#/task/${encodeURIComponent(machine)}/${encodeURIComponent(address)}`;
  return { found, back, meta: `<div class="meta pad">${esc(meta)}</div>`, body: `${slot(plan, { kind: "plan", tag: "article", look: "task md md-read", wait: "task plan" })}<a class="btn primary" href="${back}">Back to the question</a>` };
}

function planScreen(params) {
  const parts = planParts(params);
  const back = parts?.back ?? `#/task/${params.map(encodeURIComponent).join("/")}`;
  const round = overview() ? backButton(back) : "";
  if (!parts) return shell("questions", round + top("Plan") + `<div class="center">This plan is no longer on the board.</div>`);
  return shell("questions", round + top(taskName(parts.found.item, parts.found.task), esc(parts.found.name)) + parts.meta + parts.body);
}

function planPanel(params) {
  const parts = planParts(params);
  if (!parts) return `<h2>Plan</h2><div class="center left">This plan is no longer on the board.</div>`;
  return `<span class="machine">${esc(parts.found.name)}</span><h2>${esc(taskName(parts.found.item, parts.found.task))}</h2>${parts.meta}${parts.body}`;
}

function wideTaskScreen(params, panel = taskPanel) {
  if (splitLike() && state.from !== "questions") return splitScreen(sectionOf(state.from), params, panel);
  const selected = taskKey(params[0], params[1]);
  const list = state.from === "questions" ? questionsScreen(selected) : state.from.startsWith("section/") ? sectionList(arrange(state.opened, nowSeconds()), sectionOf(state.from), selected) : boardScreen(selected);
  return shell(state.from === "questions" ? "questions" : "board", list, panel(params));
}

function boardRoute() {
  if (splitLike()) return splitScreen("next");
  return shell("board", overview() ? boardOverview() : boardScreen());
}

function sectionRoute(key) {
  if (splitLike()) return splitScreen(key);
  const view = arrange(state.opened, nowSeconds());
  const list = sectionList(view, key, null);
  return list ? shell("board", backButton(backHref("section", state.from)) + list) : boardRoute();
}

function storeWarning() {
  return state.storeFailed ? `<div class="center warn" role="alert">This browser can't remember which boards and machines it has seen, so an older board replayed by the hub is only caught during this visit.</div>` : "";
}

function machinesScreen() {
  const now = nowSeconds();
  const rows = awakeFirst(state.opened, now).map((opened) => {
    const awake = !opened.expired && !isAsleep({ lastSeen: opened.lastSeen }, now);
    const facts = opened.problem ? [problemText(opened.problem)] : awake && opened.board ? [`last heard ${when(opened.lastSeen)}`, `kebbie ${opened.board.kebbie}`, `board rev ${opened.rev}`] : opened.board || opened.expired ? [`last seen ${lastSeenText(opened.lastSeen ?? opened.storedAt, now)}`] : ["no board yet"];
    return machineRow({ machine: opened.machine, name: opened.name, awake, facts });
  });
  const message = state.notice ? `<div class="center warn" role="alert">${esc(state.notice)}</div>` : "";
  return `<div class="narrow">${top("Machines", esc(`${state.opened.length} connected`))}${storeWarning()}${message}${rows.join("")}${devicesSection()}<a class="btn phone-only" href="/account">Account</a></div>`;
}

function devicesSection() {
  if (!state.info?.commands) return "";
  const view = state.devices;
  const body = !view ? `<div class="center">Loading the devices that answer and push…</div>` : view.problem ? `<div class="center warn" role="alert">${esc(COMMAND_TEXT[view.problem] ?? view.problem)}</div>` : deviceRows(view.list, state.device?.id ?? null);
  const add = commandGate() === "setup" ? setupBlock(state.pro) : state.pro.error ? `<div class="center warn" role="alert">${esc(state.pro.error)}</div>` : "";
  return `<h2 class="group g-muted">Devices that answer and push</h2>${body}${add}`;
}

function lockedScreen() {
  const status = state.session?.state ?? "locked";
  if (needsFirstPasskey(state.me, state.session)) return addPasskeyScreen({ login: state.me.github, notice: state.notice, busy: state.busy });
  if (status === "locked") return unlockScreen({ notice: state.notice });
  if (status === "no-prf" || status === "not-wrapped" || status === "github") return cantUnlockScreen({ state: status, notice: state.notice });
  const message = state.notice ? `<div class="center warn" role="alert">${esc(state.notice)}</div>` : "";
  return top("kebbie") + `<div class="center">${esc(STATE_TEXT[status] ?? STATE_TEXT.locked)}</div><button type="button" class="btn primary" data-action="other-passkey">Try another passkey</button>${message}`;
}

function unsupportedScreen() {
  return top("kebbie") + `<div class="center">This browser lacks the cryptography the app needs (Ed25519, X25519, HKDF and base64 in Uint8Array). Update to iOS 18.2 or later, or a current Chrome or Firefox.</div>`;
}

function render() {
  if (pageMode) return;
  unlockEnds.watch(state.session);
  const { name, params } = route();
  let html;
  if (!cryptoReady()) html = unsupportedScreen();
  else if (state.unlocking) html = unlockingScreen();
  else if (!state.me) html = signInScreen({ github: Boolean(state.info?.github), notice: state.notice, next: onward });
  else if (state.session?.state !== "unlocked") html = lockedScreen();
  else if (name === "questions") html = shell("questions", questionsScreen());
  else if (name === "machines") html = shell("machines", machinesScreen());
  else if (name === "task") html = twoColumns() ? wideTaskScreen(params) : taskScreen(params);
  else if (name === "plan") html = twoColumns() ? wideTaskScreen(params, planPanel) : planScreen(params);
  else if (name === "section") html = sectionRoute(params[0]);
  else html = boardRoute();
  const focused = document.activeElement?.id && root.contains(document.activeElement) ? { id: document.activeElement.id, start: document.activeElement.selectionStart, end: document.activeElement.selectionEnd } : null;
  root.innerHTML = html.startsWith('<div class="app') ? html : `<main class="screen">${html}</main>`;
  const refocus = focused ? document.getElementById(focused.id) : null;
  if (refocus) {
    refocus.focus({ preventScroll: true });
    if (focused.start !== null && focused.start !== undefined) refocus.setSelectionRange(focused.start, focused.end);
  }
  fillBlobs();
  document.title = (name === "task" || name === "plan") && params[1] ? `${params[1].split("@")[0]} · kebbie` : "kebbie";
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
  const session = state.session;
  if (session?.state !== "unlocked") return;
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
  const result = await openBoards(listing, session.identity, nowSeconds(), state.marks);
  if (state.session !== session) return render();
  state.opened = result.opened;
  state.marks = result.marks;
  state.keys = new Map((result.list?.machines ?? []).map((listed) => [listed.machine, listed.keys]));
  await refreshDevices(session);
  blobs.retryFailed();
  await keepMarks();
  if (result.gone.length && !state.offline) {
    try {
      const pruned = await publishList({ hub: lists, identity: session.identity, marks: state.marks, change: (list) => result.gone.reduce((next, machine) => listWithout(machine)(next), list) });
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

function commandText(error) {
  if (error instanceof DeviceListProblem) return COMMAND_TEXT[error.reason] ?? error.reason;
  if (error instanceof HubError && COMMAND_TEXT[error.code]) return COMMAND_TEXT[error.code];
  return hubMessage(error);
}

async function loadDeviceState(account) {
  state.device = await loadDevice(deviceStore, account);
  state.kept = await loadSent(deviceStore, account, nowSeconds());
}

async function refreshDevices(session) {
  if (!state.info?.commands || session?.state !== "unlocked") return;
  if (state.device?.account !== session.identity.id) await loadDeviceState(session.identity.id);
  try {
    const trusted = await trustedDeviceList(await account.deviceList(), session.identity, state.device?.listed ?? 0);
    state.devices = trusted;
    if (trusted.list && state.device?.listed && !trusted.list.devices.some((listed) => listed.device === state.device.id)) {
      await forgetDevice(deviceStore, session.identity.id);
      state.device = null;
    }
  } catch (error) {
    state.devices = { problem: error instanceof HubError ? error.code : "unreachable" };
  }
  startDeviceFeed();
  collect();
}

const keysOf = (machine) => state.keys.get(machine) ?? null;
let collecting = null;

function collect() {
  const device = state.device;
  if (!device?.listed || collecting) return collecting;
  collecting = (async () => {
    try {
      const kept = await collectResults({ calls: deviceCalls, device, kept: state.kept, keysOf, now: nowSeconds() });
      if (state.device !== device) return;
      state.kept = kept;
      await keepSent(deviceStore, device.account, kept);
      render();
    } catch (error) {
      if (error instanceof HubError && error.status === 401) await dropDevice();
    } finally {
      collecting = null;
    }
  })();
  return collecting;
}

async function dropDevice() {
  if (!state.device) return;
  await forgetDevice(deviceStore, state.device.account);
  state.device = null;
  deviceFeed?.abort();
  deviceFeed = null;
  render();
}

let deviceFeed = null;
function startDeviceFeed() {
  const device = state.device;
  if (!device?.listed || deviceFeed) return;
  const controller = new AbortController();
  deviceFeed = controller;
  (async () => {
    let delay = 2000;
    while (!controller.signal.aborted) {
      try {
        await follow((name) => {
          delay = 2000;
          if (name === "result") collect();
        }, controller.signal, { path: "/v1/d/events", token: device.token, lastEventId: state.kept.cursor || null });
      } catch (error) {
        if (error instanceof HubError && error.status === 401) return dropDevice();
      }
      await new Promise((resolve) => setTimeout(resolve, delay));
      delay = Math.min(delay * 2, 60_000);
    }
  })();
}

async function freshPasskey() {
  const session = await unlockAfter(hub, await signingIn.withPasskey(rememberedCredential()));
  if (session.state !== "unlocked") throw new Error(STATE_TEXT[session.state] ?? "Your account did not unlock.");
  state.session = session;
  state.me = session.me;
  return session.identity;
}

async function enableThisDevice() {
  state.pro = { busy: true, error: null };
  render();
  try {
    const { device, list } = await enableDevice({ hub: account, store: deviceStore, identity: state.session.identity, name: proposedName(navigator.userAgent), reauth: freshPasskey, seen: state.devices?.list?.counter ?? 0 });
    state.device = device;
    state.devices = { list };
    state.pro = { busy: false, error: null };
    startDeviceFeed();
  } catch (error) {
    state.pro = { busy: false, error: commandText(error) };
  }
  render();
}

async function removeDevice(device, name) {
  if (!confirm(`Remove ${name}? It stops answering and pushing, and your machines stop trusting it.`)) return;
  state.pro = { busy: false, error: null };
  try {
    const list = await removeListedDevice({ hub: account, store: deviceStore, identity: state.session.identity, device, reauth: freshPasskey, seen: state.devices?.list?.counter ?? 0 });
    state.devices = { list };
    if (state.device?.id === device) await dropDevice();
  } catch (error) {
    state.pro = { busy: false, error: commandText(error) };
  }
  render();
}

async function send(target, payload, meta) {
  const { machine, address } = target.dataset;
  const ui = uiOf(machine, address);
  const keys = keysOf(machine);
  ui.busy = true;
  ui.error = null;
  render();
  try {
    if (!keys) throw new Error("Your account doesn't list this machine's keys.");
    const sent = await sendCommand({ calls: deviceCalls, device: state.device, machine: { id: machine, keys }, payload, at: nowSeconds(), meta: { ...meta, device: state.device.name } });
    state.kept = { ...state.kept, sent: [...state.kept.sent, sent] };
    await keepSent(deviceStore, state.device.account, state.kept);
    state.ui.set(uiKey(machine, address), freshUi());
  } catch (error) {
    ui.busy = false;
    ui.error = commandText(error);
    if (error instanceof HubError && error.status === 402) state.refused = { url: error.url };
    if (error instanceof HubError && error.status === 401) await dropDevice();
  }
  render();
  collect();
}

function sendAnswer(target) {
  const { machine, address } = target.dataset;
  const found = findTask(state.opened, machine, address);
  const question = found?.item.question;
  const ui = uiOf(machine, address);
  const option = ui.free ? undefined : question?.options[ui.picked];
  if (!question || !answerReady(option, ui)) return;
  const choice = ui.free ? null : ui.picked;
  send(target, { type: "answer", address, question: question.id, choice, comment: ui.note.trim() }, { question: question.id, choice, label: option?.label ?? null, machineName: found.name });
}

function sendPush(target) {
  const { machine, address } = target.dataset;
  const found = findTask(state.opened, machine, address);
  if (!found || !pushReady(found.item)) return;
  const { item } = found;
  send(target, { type: "push", address, head: item.head, card_sha: item.card?.sha ?? null }, { head: item.head, short: short(item.head), machineName: found.name });
}

function decideAction(action, target, element) {
  const { machine, address } = target.dataset;
  const ui = uiOf(machine, address);
  if (ui.busy) return;
  if (action === "pick") {
    const index = Number(element.dataset.index);
    if (ui.picked !== index) ui.note = "";
    Object.assign(ui, { picked: index, free: false, error: null });
  } else if (action === "write-own") Object.assign(ui, { picked: null, free: true, note: "", error: null });
  else if (action === "pick-instead") Object.assign(ui, { free: false, note: "", error: null });
  else if (action === "push-ask") Object.assign(ui, { confirming: true, error: null });
  else if (action === "push-cancel") Object.assign(ui, { confirming: false, error: null });
  else if (action === "answer") return sendAnswer(target);
  else if (action === "push") return sendPush(target);
  render();
}

function stopDevice() {
  deviceFeed?.abort();
  deviceFeed = null;
  state.device = null;
  state.devices = null;
  state.kept = { cursor: 0, sent: [] };
  state.ui = new Map();
  state.keys = new Map();
}

function lockBoards() {
  blobs.forget();
  stopDevice();
  following?.abort();
  following = null;
  state.opened = [];
  state.session = { state: "locked", me: state.me };
  render();
}

function signedOut(message) {
  blobs.forget();
  stopDevice();
  forgetUnlock();
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

function closeSheet(after = render) {
  const parts = [...root.querySelectorAll(".sheet, .scrim")];
  const sheet = root.querySelector(".sheet");
  state.sheet = false;
  if (!sheet || matchMedia("(prefers-reduced-motion: reduce)").matches) return after();
  if (sheet.classList.contains("closing")) return;
  for (const part of parts) {
    part.classList.add("closing");
    part.inert = true;
  }
  let done = false;
  const finish = () => {
    if (done) return;
    done = true;
    after();
  };
  sheet.addEventListener("animationend", finish, { once: true });
  setTimeout(finish, 400);
}

root.addEventListener("click", (event) => {
  if (pageMode) return;
  const picked = event.target.closest(".sheet a.item");
  if (picked) {
    event.preventDefault();
    return closeSheet(() => go(picked.getAttribute("href")));
  }
  const target = event.target.closest("[data-action]");
  if (!target) return;
  const action = target.dataset.action;
  const decided = target.closest(".decide");
  if (decided) return decideAction(action, decided, target);
  if (action === "enable-device") return enableThisDevice();
  if (action === "remove-device") return removeDevice(target.dataset.id, target.dataset.name);
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
  else if (action === "close-sections" || (action === "sections" && state.sheet)) closeSheet();
  else if (action === "sections") {
    state.sheet = true;
    render();
  }
});

root.addEventListener("input", (event) => {
  const field = event.target.closest("textarea[data-draft]");
  const decided = field?.closest(".decide");
  if (!decided) return;
  const ui = uiOf(decided.dataset.machine, decided.dataset.address);
  ui.note = field.value;
  const found = findTask(state.opened, decided.dataset.machine, decided.dataset.address);
  const option = ui.free ? undefined : found?.item.question?.options[ui.picked];
  const button = decided.querySelector('[data-action="answer"]');
  if (button) button.disabled = ui.busy || !answerReady(option, ui);
});

root.addEventListener("keydown", (event) => {
  const pick = event.target.closest?.('[data-action="pick"]');
  if (!pick || (event.key !== "Enter" && event.key !== " ")) return;
  event.preventDefault();
  pick.click();
});

addEventListener("hashchange", () => {
  if (pageMode) return;
  const { name } = route();
  if (name !== "board") state.connected = null;
  if (name === "board" || name === "questions") state.from = name;
  if (name === "section") state.from = `section/${route().params[0]}`;
  state.sheet = false;
  render();
  if (!(twoColumns() && (name === "task" || name === "plan"))) window.scrollTo(0, 0);
});

let settleUntil = 0;
function clampScroll() {
  const target = outOfRangeScroll(scrollY, document.documentElement.scrollHeight, innerHeight);
  if (target !== null) scrollTo(0, target);
}

// iOS can hand a home-screen app its strip inset after the first frame, at launch and when it returns from the background, without a resize event.
function remarkStrip(settle = 1300) {
  resizedAt = performance.now();
  const polling = settleUntil > resizedAt;
  settleUntil = Math.max(settleUntil, resizedAt + settle);
  markStrip();
  if (polling) return;
  requestAnimationFrame(function poll() {
    markStrip();
    clampScroll();
    if (performance.now() < settleUntil) requestAnimationFrame(poll);
  });
}
for (const event of ["resize", "orientationchange"]) addEventListener(event, () => remarkStrip());
addEventListener("pageshow", () => remarkStrip(3000));
addEventListener("scroll", () => performance.now() < settleUntil && clampScroll(), { passive: true });
globalThis.screen?.orientation?.addEventListener?.("change", () => remarkStrip());

addEventListener("keydown", (event) => {
  if (pageMode || event.key !== "Escape") return;
  if (state.sheet) closeSheet();
  else if (twoColumns() && route().name === "plan") go(`#/task/${route().params.map(encodeURIComponent).join("/")}`);
  else if (twoColumns() && route().name === "task") go(`#/${state.from}`);
});

document.addEventListener("visibilitychange", () => {
  if (document.visibilityState !== "visible") return;
  remarkStrip(3000);
  refresh();
});

function showHint() {
  try {
    state.hint = isIos() && !isStandalone() && localStorage.getItem("kebbie-hint-dismissed") !== "1";
  } catch {
    state.hint = isIos() && !isStandalone();
  }
}

async function resumeBoards() {
  try {
    await refresh();
  } catch (error) {
    state.notice = hubMessage(error);
  } finally {
    state.unlocking = false;
  }
  render();
  startFeed();
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
  state.me = await signedInAccount(account);
  if (state.me && onward) return location.assign(onward);
  const kept = await keptUnlock(state.me);
  state.session = kept ?? (state.me ? { state: "locked", me: state.me } : null);
  state.unlocking = Boolean(kept);
  render();
  if (!state.me) return startAutofill();
  if (kept) await resumeBoards();
  setInterval(() => document.visibilityState === "visible" && refresh(), 60_000);
}

markStrip();
remarkStrip(3000);
start();
