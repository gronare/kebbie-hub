import { blobReader, blobSlot, failedLine, kb, keptBytes, loadingLine, planHref } from "./blobs.js";
import { markdown } from "./markdown.js";
import { answersWithLead, backlogList, backlogView, foundStatus, chip, chooseMachine, isFound, cleanEdit, editProblem, editReady, entryHref, ideaEntries, ideaForm, itemBody, listAction, mergeBacklog, needsYouCount, startedEntry, startsAnyway } from "./backlog.js";
import { cleared, keepFind, loadFind, pacer, textsOf, toggled } from "./find.js";
import { arrange, asksYou, emptyMarks, findTask, isAsleep, mergeMarks, openBoards, readMarks, shownQuestion } from "./board.js";
import { connectText, listWithout, PairingFailed, publishList, safeNext, withReauth } from "./connect.js";
import { autofillAvailable, signInText, signInWith } from "./signin.js";
import { account, connectCalls, follow, HubError } from "./hub.js";
import * as deviceStore from "./store.js";
import { allowsChanges, collectResults, CommandProblem, ensureDevice, forgetDevice, keepSent, loadDevice, loadSent, sendCommand } from "./device.js";
import { CARD_MEDIA_TYPE, KEPT_ANSWER_TTL, textHash } from "../js/protocol.js";
import { assessmentBlock, cardParts, changedNotice, NOTICED_ACTIONS, NOTICED_KINDS, noticedArea } from "./card.js";
import { answerBlock, answerReady, APPROVAL_STATES, kebbieAnswerStatus, choosable, COMMAND_TEXT, gate, latestSent, machinePolicy, notTakenBlock, promptAnswerBlock, pushBlock, pushCount, pushHead, pushReady, refusedBlock, sentState, settled, speaksFor, standingOn, takes, takesPromptAnswers, viewOnlyBlock } from "./decide.js";
import { forgetDeviceKeys, get, put } from "./store.js";
import { addFirstPasskey, forgetUnlock, keptUnlock, LISTING, lockWhenDue, rememberedCredential, signedInAccount, STATE_TEXT, stopNotifications, unlockAfter } from "./unlock.js";
import { proposedName } from "./device-name.js";
import { PAGES, startPage, SUBMIT } from "./pages.js";
import { closeSettled } from "./notify.js";
import { cleanComment, commentProblem, commentsProblem, commitPage, diffHref, filesOf, foldCommitsOn, freshReviewUi, hangLines, openOf, readKey, readOf, reviewEntry, reviewPage, sendControl, settleReview, shaOf, subtaskHref, subtaskPage, wholePage } from "./review.js";
import { addEmailScreen, address, authScreen, codeScreen, createPasskeyScreen, emailSignInScreen, emailText, isAuthScreen, keyboardLift, keylessUnlockScreen, passkeySignInScreen, signedInAs, signedInStep, signedOutStep, typedCode } from "./email.js";
import { supported as passkeysSupported } from "./passkey.js";
import { outOfRangeScroll } from "./scroll.js";
import { awayOn, chosenLead, composerHtml, leadHead, leadHref, leadMachines, panelHead, pendingMessages, pickRow, popHtml, previewOf, shortWhen, threadHtml, threadOf, unseenReplies, awaySwitch, awayNote } from "./lead.js";
import { watchShell, watchUpdate } from "./shell.js";
import { choosePose, overviewPose, POSES, railSide } from "./pose.js";
import { backHref, boardSections, firstTaskOf, sectionPills, sectionSheet, sectionsButton, sectionView } from "./sections.js";
import { acceptCount, addPasskeyScreen, awayLine, brand, cantUnlockScreen, connectDone, githubErrorText, machineAnswerNote, installHint, lastSeenText, laterGroups, machineDots, machineRow, nav, needsFirstPasskey, nextList, optionCards, promptBlock, promptCards, backButton, cardAge, backRow, cardButton, cardHref, obstacleBlock, OPENABLE_STEPS, questionButton, questionHref, questionWaits, youLine, pushOrder, stageList, reviewLines, routinesBlock, rulesLines, signInScreen, sleepingRow, subtaskList, taskFacts, taskKey, taskName, taskRow, unlockingScreen, unlockScreen } from "./screens.js";

const root = document.getElementById("app");
let pageMode = PAGES.includes(location.pathname);
const onward = safeNext(new URLSearchParams(location.search).get("next"));
const nowSeconds = () => Math.floor(Date.now() / 1000);
const state = { info: null, me: null, session: null, opened: [], marks: emptyMarks(), storeFailed: false, offline: false, fetchedAt: null, notice: null, hint: false, unlocking: false, connected: null, from: "board", sheet: false, allowed: false, device: null, keptFor: null, kept: { cursor: 0, sent: [] }, ui: new Map(), keys: new Map(), refused: null, backlogUi: new Map(), idea: { text: "", machine: null, choosing: false, busy: false, error: null }, lastMachine: null, reviewUi: new Map(), signin: { step: "email", email: "", chosen: false }, lead: { machine: null, draft: "", picking: false, busy: false, error: null, depth: new Map(), names: null }, address: null, emailStep: null, busy: false, draft: { email: null, code: "" }, updated: false };
let reloading = false;
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
let rendered = null;


const splitLike = () => pose === "split" || pose === "land";
const twoColumns = () => splitLike() || pose === "desk";
const overview = () => overviewPose(pose, innerHeight > innerWidth);
const lists = { boards: account.boards, putMachines: connectCalls.putMachines };
const unlockEnds = lockWhenDue(lockBoards);

const esc = (value) => String(value ?? "").replace(/[&<>"']/g, (char) => `&#${char.charCodeAt(0)};`);
const plural = (count, one, many) => `${count} ${count === 1 ? one : many}`;
const short = (sha) => (sha ?? "").slice(0, 7);
const blobs = blobReader(keptBytes(account.blob, deviceStore.keptBlobs));
const KEEP_BLOBS_SECONDS = 30 * 86_400;
deviceStore.keptBlobs.prune(Math.floor(Date.now() / 1000) - KEEP_BLOBS_SECONDS).catch(() => undefined);
const deviceStorage = () => {
  try {
    return localStorage;
  } catch {
    return null;
  }
};
let findRefresh = null;
// The hub allows an account a burst of 60 and 120 requests a minute across all its devices; each page's bodies take a quarter of the burst and three eighths of the rate, so two open devices still leave the board and commands room.
const bodies = pacer({ load: (ref) => blobs.load(ref).then((loaded) => (loaded && laterFindRefresh(), loaded)), held: (id) => blobs.peek(id) !== null, burst: 15, perSecond: 0.75, failed: (loaded) => Boolean(loaded?.failed) });
state.find = loadFind(deviceStorage());
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

function shell(active, main, panel = "", { said = "Selected task", bare = false } = {}) {
  const questions = arrange(state.opened, nowSeconds()).questions.length;
  const backlog = needsYouCount(backlogEntries());
  const replies = unseenReplies(leadMachines(state.opened), leadSeen);
  const side = panel ? `<aside class="panel" aria-label="${esc(said)}">${panel}</aside>` : "";
  const toast = state.updated ? `<div class="toast" role="status"><span>New version</span><button type="button" class="toast-act" data-action="reload">Reload</button></div>` : "";
  return `<div class="app${panel ? " with-panel" : ""}">${nav({ active, questions, replies, backlog })}<main class="screen in-app">${bare ? "" : brand}${main}</main>${side}${toast}</div>`;
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
  if (item.prompt) return promptBlock(found, promptArea(found));
  const question = item.kebbie_questions[0];
  const context = [found.repo, item.summary].filter(Boolean).join(" · ");
  const card = [taskFacts(item), stageList(item, nowSeconds())].filter(Boolean).join("");
  return `<p class="q">${esc(question.text)}</p><div class="meta pad">${esc(context)} · asked ${esc(when(question.at))}</div>${card ? `<section class="task">${card}</section>` : ""}${answerArea(found)}`;
}

const seenKey = (machine, address) => `kebbie-card-seen:${machine}|${address}`;

function seenVersion(machine, address) {
  try {
    return Number(localStorage.getItem(seenKey(machine, address))) || 0;
  } catch {
    return 0;
  }
}

function markSeen(machine, address, version) {
  try {
    localStorage.setItem(seenKey(machine, address), String(version));
  } catch {}
}

function noticedSent(found, key) {
  const shown = NOTICED_KINDS.map((kind) => sentFor(found, kind, null, null, key)).filter((one) => one.sent).sort((a, b) => b.sent.at - a.sent.at)[0];
  return shown ? { sent: shown.sent, state: shown.state, standing: shown.standing } : null;
}

function structuredCard(found) {
  const { item } = found;
  const held = blobs.peek(item.card.blob.id);
  if (!held) {
    blobs.load(item.card.blob).then((loaded) => loaded && render());
    return `<div class="plan"><div>${esc(loadingLine("card", item.card.blob))}</div></div>`;
  }
  if (!held.card) return `<div class="plan"><div>${esc(failedLine("card"))}</div></div>`;
  const ui = uiOf(found.machine, item.address);
  const backlog = state.opened.find((machine) => machine.machine === found.machine)?.board?.backlog ?? [];
  const noticed = (line) => noticedArea(line, { access: commandGate(), accepts: found.accepts, sent: line.found ? noticedSent(found, line.found) : null, busy: ui.busy, error: ui.error, now: nowSeconds(), recurring: backlog.find((row) => row.item === line.found)?.found?.sources.length ?? 0 });
  return changedNotice(held.card, seenVersion(found.machine, item.address)) + cardParts(held.card, { noticed });
}

function assessArea(found) {
  const { item } = found;
  const ui = uiOf(found.machine, item.address);
  const shown = sentFor(found, "review-send");
  const sent = shown.sent ? shown : null;
  return assessmentBlock(item, { access: commandGate(), accepts: found.accepts, sent, busy: ui.busy, error: ui.error, status: (one) => foundStatus(one.sent, one.state, { standing: one.standing, now: nowSeconds() }) });
}

function cardBlock(found, selected = null) {
  const { item } = found;
  const lines = [];
  if (item.card?.blob.media_type === CARD_MEDIA_TYPE) {
    lines.push(taskFacts(item));
    lines.push(cardAge(item.card));
    lines.push(structuredCard(found));
  } else {
    if (item.card) {
      lines.push(cardAge(item.card));
      lines.push(`<h3 class="sec">Card</h3>${slot(item.card.blob, { kind: "card", look: "md" })}`);
    }
    lines.push(taskFacts(item));
  }
  lines.push(youLine(item));
  lines.push(obstacleBlock(item, nowSeconds()));
  lines.push(stageList(item, nowSeconds()));
  const subtasks = subtaskList(item, { expandable: true, href: (id) => subtaskHref(found.machine, item.address, id), selected, more: uiOf(found.machine, item.address).moreSteps });
  if (subtasks) lines.push(`<h3 class="sec">Subtasks · ${item.subtasks.length}</h3>${subtasks}`);
  lines.push(reviewLines(item));
  const progress = [item.summary, item.report?.summary].filter(Boolean);
  if (progress.length) lines.push(`<h3 class="sec">Progress</h3><div class="plan">${progress.map((line) => `<div>${esc(line)}</div>`).join("")}</div>`);
  if (item.git) {
    const git = item.git;
    const facts = [`${git.ahead} ahead of ${git.upstream}`, git.behind ? `${git.behind} behind` : null, git.uncommitted ? `${git.uncommitted} uncommitted changes` : null].filter(Boolean);
    const order = pushOrder(item);
    if (order) lines.push(`<h3 class="sec">Push order · ${item.order.length}</h3>${order}<div class="order-foot">${esc(facts.join(" · "))}${rulesLines(git.rules, { expandable: true })}</div>`);
    else lines.push(`<h3 class="sec">Commits · ${git.ahead}</h3><div class="plan"><div>${esc(facts.join(" · "))}</div>${item.head ? `<div class="mono">HEAD ${esc(short(item.head))}</div>` : ""}${rulesLines(git.rules, { expandable: true })}</div>`);
  }
  if (item.ci) lines.push(`<h3 class="sec">CI</h3><div class="plan"><div>${item.ci.state === "running" ? "CI running" : `CI failed${item.ci.failure ? `: ${esc(item.ci.failure)}` : ""}`}</div></div>`);
  if (item.push_waits) lines.push(`<div class="meta">${esc(item.push_waits)}</div>`);
  const card = lines.some(Boolean) ? `<section class="task">${lines.join("")}</section>` : "";
  const structured = item.card?.blob.media_type === CARD_MEDIA_TYPE;
  const assess = assessArea(found);
  const waits = asksYou(item) ? questionWaits(item, questionHref(found.machine, item.address)) : "";
  return waits + reviewEntry(found) + (structured && card ? decide(found, card) : card) + (assess ? decide(found, assess) : "") + pushArea(found);
}

const uiKey = (machine, address) => `${machine}|${address}`;
const freshUi = () => ({ picked: null, free: false, note: "", confirming: false, busy: false, error: null, prompt: null, moreSteps: false });

function uiOf(machine, address) {
  const key = uiKey(machine, address);
  if (!state.ui.has(key)) state.ui.set(key, freshUi());
  return state.ui.get(key);
}

const commandGate = () => gate({ info: state.info, refused: state.refused, allowed: state.allowed });
const decide = (found, html) => `<div class="decide" data-machine="${esc(found.machine)}" data-address="${esc(found.item.address)}">${html}</div>`;

function sentFor(found, kind, question = null, prompt = null, address = found.item.address) {
  const sent = latestSent(state.kept.sent, { machine: found.machine, address, kind, question, prompt });
  if (!sent) return { sent: null, state: null, standing: null };
  const opened = state.opened.find((machine) => machine.machine === found.machine);
  const standing = standingOn(opened?.board, state.device?.id === sent.device ? state.device : null);
  return { sent, standing, state: sentState(sent, { now: nowSeconds(), asleep: isAsleep({ lastSeen: opened?.lastSeen ?? null }, nowSeconds()), standing }) };
}

function answerArea(found) {
  const question = shownQuestion(found.item);
  const access = commandGate();
  if (access === "off") return optionCards(question.options, question.recommend) + machineAnswerNote();
  if (access === "refused") return optionCards(question.options, question.recommend) + refusedBlock(state.refused.url);
  if (access === "view-only") return optionCards(question.options, question.recommend) + viewOnlyBlock();
  if (!takes(found.accepts, "answer")) return optionCards(question.options, question.recommend) + notTakenBlock("answer", found.name, found.accepts);
  const { sent, state: shown, standing } = sentFor(found, "answer", question.id);
  const key = uiKey(found.machine, found.item.address);
  const ui = uiOf(found.machine, found.item.address);
  if (!question.kind) return decide(found, answerBlock({ key, question, ui, sent, state: shown, standing }));
  const waits = question.kind === "verify" && shown === "done" && sent.choice === 1;
  if (waits) return decide(found, kebbieAnswerStatus(sent, shown, { standing }) + answerBlock({ key, question, ui, sent: null, state: null, describe: kebbieAnswerStatus }));
  return decide(found, answerBlock({ key, question, ui, sent, state: shown, standing, describe: kebbieAnswerStatus }));
}

function promptArea(found) {
  const prompt = found.item.prompt;
  const access = commandGate();
  if (access === "off") return promptCards(prompt) + machineAnswerNote();
  if (access === "refused") return promptCards(prompt) + refusedBlock(state.refused.url);
  if (access === "view-only") return promptCards(prompt) + viewOnlyBlock();
  if (!takesPromptAnswers(found)) return promptCards(prompt) + notTakenBlock("prompt-answer", found.name, found.accepts);
  const ui = uiOf(found.machine, found.item.address);
  if (ui.prompt !== prompt.id && !ui.busy) Object.assign(ui, freshUi(), { prompt: prompt.id });
  const { sent, state: shown, standing } = sentFor(found, "prompt-answer", null, prompt.id);
  return decide(found, promptAnswerBlock({ prompt, ui, sent, state: shown, standing }));
}

function pushArea(found) {
  const { item } = found;
  const pushable = item.group === "READY TO PUSH" || item.card || item.pushable;
  if (!pushable || !item.head) return "";
  const access = commandGate();
  if (access === "off" || !pushReady(item)) return `<button type="button" class="btn primary off" disabled>Push ${esc(short(item.head))} · coming in a later version</button>`;
  if (access === "refused") return refusedBlock(state.refused.url);
  if (access === "view-only") return viewOnlyBlock();
  if (!takes(found.accepts, "push")) return notTakenBlock("push", found.name, found.accepts);
  const { sent, state: shown, standing } = sentFor(found, "push");
  const current = speaksFor(sent, shown, (one) => one.head === pushHead(item));
  return `<div class="rv-end thumb-end"><div class="row-btns">${decide(found, pushBlock({ item, machineName: found.name, ui: uiOf(found.machine, item.address), sent: current, state: current ? shown : null, standing }))}</div></div>`;
}

function taskBody(found, selected = null) {
  return asksYou(found.item) ? questionBlock(found) : cardBlock(found, selected);
}

function switchButton(found, card) {
  if (!asksYou(found.item)) return "";
  const { machine, item } = found;
  return card ? questionButton(questionHref(machine, item.address), arrange(state.opened, nowSeconds()).questions.length) : cardButton(cardHref(machine, item.address));
}

function taskScreen([machine, address], card = false) {
  const back = overview() ? backButton(backHref(card ? "card" : "task", state.from)) : "";
  const found = findTask(state.opened, machine, address);
  if (!found) return shell("board", back + top("Task") + `<div class="center">This task is no longer on the board.</div>`);
  const asks = asksYou(found.item);
  const button = switchButton(found, card);
  const body = card ? cardBlock(found) : taskBody(found);
  return shell(asks ? "questions" : "board", (button ? backRow(back, button) : back) + top(taskName(found.item, found.task), esc(found.name)) + body);
}

function taskPanel([machine, address], card = false) {
  const found = findTask(state.opened, machine, address);
  if (!found) return `<h2>Task</h2><div class="center left">This task is no longer on the board.</div>`;
  const button = switchButton(found, card);
  return `${button ? backRow("", button) : ""}<span class="machine">${esc(found.name)}</span><h2>${esc(taskName(found.item, found.task))}</h2>${card ? cardBlock(found) : taskBody(found)}`;
}

const cardPanel = (params) => taskPanel(params, true);

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

function shownOf(sent) {
  const opened = state.opened.find((machine) => machine.machine === sent.machine);
  const standing = standingOn(opened?.board, state.device?.id === sent.device ? state.device : null);
  const shown = sentState(sent, { now: nowSeconds(), asleep: isAsleep({ lastSeen: opened?.lastSeen ?? null }, nowSeconds()), standing });
  return { sent, state: shown, standing, live: !settled(shown) };
}

function backlogEntries() {
  const now = nowSeconds();
  const merged = mergeBacklog(state.opened, now).map((entry) => ({ ...entry, waiting: Math.max(0, entry.row.blocking - answersInFlight(entry)) }));
  return [...ideaEntries(state.kept.sent, state.opened, now, (sent) => shownOf(sent).state), ...merged];
}

function answersInFlight(entry) {
  const storedAt = (machine) => state.opened.find((opened) => opened.machine === machine)?.storedAt ?? 0;
  return answersWithLead(entry, state.kept.sent, { shown: shownOf, storedAt, body: blobs.peek(entry.row.body?.id ?? "")?.body ?? null });
}

const bodyTextOf = textsOf((entry) => blobs.peek(entry.row.body?.id ?? "")?.body ?? null);

function wantBodies(entries) {
  bodies.want(entries.map((entry) => entry.row.body).filter(Boolean));
}

function laterFindRefresh() {
  if (findRefresh || !state.find.q.trim()) return;
  findRefresh = setTimeout(() => {
    findRefresh = null;
    patchBacklogList();
  }, 600);
}

function patchBacklogList() {
  const results = root.querySelector(".bl-results");
  if (!results) return;
  const picked = root.querySelector(".bl-results a.task.sel")?.getAttribute("href") ?? null;
  const selected = picked ? decodeURIComponent(picked.slice("#/item/".length)) : null;
  const shaped = document.createElement("template");
  shaped.innerHTML = backlogListHtml(backlogEntries(), selected);
  root.querySelector(".bl-filters")?.replaceWith(shaped.content.querySelector(".bl-filters"));
  results.replaceWith(shaped.content.querySelector(".bl-results"));
  const clear = root.querySelector(".bl-search .clear");
  if (clear) clear.hidden = state.find.q === "";
}

function setFind(find, { patch = false } = {}) {
  state.find = find;
  keepFind(deviceStorage(), find);
  if (patch) patchBacklogList();
  else render();
}

function backlogUi(id) {
  if (!state.backlogUi.has(id)) state.backlogUi.set(id, { picked: null, text: "", busy: false, error: null, choosing: false, machine: null, visit: nowSeconds(), editing: null });
  return state.backlogUi.get(id);
}

const candidatesOf = (entry, kind) => entry.machines.filter((one) => takes(one.accepts, kind));

function chosenFor(entry, kind) {
  const ui = backlogUi(entry.id);
  return chooseMachine(candidatesOf(entry, kind), { repo: entry.row.repo, lastUsed: state.lastMachine, chosen: ui.machine });
}

function latestFor(entry, kind, match = () => true) {
  const machines = new Set(entry.machines.map((one) => one.machine));
  const mine = state.kept.sent.filter((sent) => sent.kind === kind && sent.address === entry.key && machines.has(sent.machine) && match(sent));
  const sent = mine.sort((a, b) => b.at - a.at)[0];
  return sent ? shownOf(sent) : null;
}

function taskHrefFor(entry, start) {
  const opened = state.opened.find((machine) => machine.machine === start?.sent.machine);
  const task = opened?.board?.items.find((item) => item.title === entry.row.title);
  return task ? `#/task/${encodeURIComponent(opened.machine)}/${encodeURIComponent(task.address)}` : "#/board";
}

function bodyOf(entry) {
  const ref = entry.row.body;
  if (!ref) return { body: null, bodyState: null };
  const held = blobs.peek(ref.id);
  if (!held) {
    blobs.load(ref).then((loaded) => loaded && render());
    return { body: null, bodyState: "loading" };
  }
  return held.body ? { body: held.body, bodyState: "ready" } : { body: null, bodyState: "failed" };
}

function itemCardHtml(entry) {
  const ui = backlogUi(entry.id);
  const candidates = candidatesOf(entry, "start-item");
  const chosen = chosenFor(entry, "start-item");
  const start = entry.key ? latestFor(entry, "start-item") : null;
  const sentFor = (index) => latestFor(entry, "item-answer", (sent) => sent.index === index);
  const idea = entry.idea ? { ...entry.idea, standing: shownOf(entry.idea.sent).standing } : null;
  const edits = entry.key ? { machine: chosenFor(entry, "item-edit"), editing: ui.editing, sentFor: (section) => editFor(entry, section) } : null;
  return itemBody(idea ? { ...entry, idea } : entry, { ...bodyOf(entry), access: commandGate(), refusedUrl: state.refused?.url ?? null, ui, sentFor, start, candidates, chosen, now: nowSeconds(), taskHref: taskHrefFor(entry, start), edits, found: isFound(entry) ? foundOf(entry) : null });
}

const FOUND_KINDS = ["found-start", "found-add", "found-dismiss"];

function foundOf(entry) {
  const chosen = chosenFor(entry, "found-start");
  const board = state.opened.find((machine) => machine.machine === chosen?.machine)?.board;
  const tasks = takes(chosen?.accepts, "found-add") ? (entry.row.found?.sources ?? []).map((address) => board?.items.find((item) => item.address === address)).filter(Boolean).map((item) => ({ address: item.address, name: taskName(item, item.address.slice(0, item.address.indexOf("@"))) })) : [];
  const sent = FOUND_KINDS.map((kind) => latestFor(entry, kind)).filter(Boolean).sort((a, b) => b.sent.at - a.sent.at)[0] ?? null;
  return { chosen, tasks, sent };
}

async function foundAction(entry, action, target) {
  const ui = backlogUi(entry.id);
  const chosen = chosenFor(entry, action === "found-dismiss" ? "found-dismiss" : action === "found-add" ? "found-add" : "found-start");
  if (!chosen || ui.busy) return;
  const task = action === "found-add" ? target.dataset.task : null;
  const payload = { type: action, item: entry.key, ...(task ? { task } : {}) };
  await sendBacklog({ machine: chosen, payload, meta: { address: entry.key, label: entry.row.title, task }, ui });
  render();
}

function editFor(entry, section) {
  const shown = latestFor(entry, "item-edit", (sent) => (sent.section ?? null) === section);
  if (!shown) return null;
  const storedAt = state.opened.find((machine) => machine.machine === shown.sent.machine)?.storedAt ?? 0;
  const saving = shown.live && (shown.state !== "done" || storedAt < (shown.sent.result?.at ?? 0)) && !APPROVAL_STATES.includes(shown.state) && shown.state !== "waiting";
  return { ...shown, saving, recent: shown.sent.at >= backlogUi(entry.id).visit };
}

function findEntry(id) {
  const entries = backlogEntries();
  const found = entries.find((entry) => entry.id === id);
  if (found) return { entries, entry: found };
  const started = state.kept.sent.filter((sent) => sent.kind === "start-item" && sent.result?.outcome === "done" && (sent.address === id || id.endsWith(`|${sent.address}`))).sort((a, b) => b.at - a.at)[0];
  return { entries, entry: started ? startedEntry(id, started) : null };
}

function ideaCandidates() {
  const now = nowSeconds();
  return state.opened.filter((opened) => opened.board && takes(opened.board.accepts, "idea")).map((opened) => ({ machine: opened.machine, name: opened.name, accepts: opened.board.accepts, asleep: isAsleep({ lastSeen: opened.lastSeen }, now), repos: (opened.board.checkouts ?? []).map((checkout) => checkout.repo) }));
}

const ideaChoice = () => chooseMachine(ideaCandidates(), { lastUsed: state.lastMachine, chosen: state.idea.machine });

function backlogListHtml(entries, selected, composing = false, view = backlogView(entries, state.find, bodyTextOf)) {
  const action = listAction({ access: commandGate(), refusedUrl: state.refused?.url ?? null, ideaMachines: ideaCandidates(), composing });
  wantBodies(entries);
  return backlogList({ entries, selected, now: nowSeconds(), top, action, find: state.find, textOf: bodyTextOf, view });
}

const itemPanel = (entry) => `<span class="machine">${esc(chip(entry))}</span><h2>${esc(entry.row.title)}</h2>${itemCardHtml(entry)}`;
const missingItem = `<div class="center left">This item is no longer in the backlog.</div>`;

function backlogRoute() {
  const entries = backlogEntries();
  const view = backlogView(entries, state.find, bodyTextOf);
  if (twoColumns() && view.first) return shell("backlog", backlogListHtml(entries, view.first.id, false, view), itemPanel(view.first));
  return shell("backlog", backlogListHtml(entries, null, false, view));
}

function itemRoute(id) {
  const { entries, entry } = findEntry(id);
  if (twoColumns()) return shell("backlog", backlogListHtml(entries, id), entry ? itemPanel(entry) : `<h2>Backlog</h2>${missingItem}`);
  const back = backButton("#/backlog");
  if (!entry) return shell("backlog", back + top("Backlog") + missingItem);
  return shell("backlog", back + top(entry.row.title, `<span class="machine">${esc(chip(entry))}</span>`) + itemCardHtml(entry));
}

// iOS opens the keyboard only for a focus the tap itself makes, so the composer is rendered and focused inside the tap.
function openComposer() {
  location.hash = "#/idea";
  render();
  scrollListToTop();
  document.getElementById("idea")?.focus({ preventScroll: true });
}

function ideaRoute() {
  const form = ideaForm({ ...state.idea, candidates: ideaCandidates(), chosen: ideaChoice() });
  if (twoColumns()) return shell("backlog", backlogListHtml(backlogEntries(), null, true), `<h2>New idea</h2>${form}`);
  return shell("backlog", backButton("#/backlog") + top("New idea") + form);
}

const taskHref = (machine, address) => `#/task/${encodeURIComponent(machine)}/${encodeURIComponent(address)}`;

function reviewUiOf(machine, address, subtask) {
  const key = `${uiKey(machine, address)}|${subtask ?? ""}`;
  if (!state.reviewUi.has(key)) state.reviewUi.set(key, freshReviewUi());
  return state.reviewUi.get(key);
}

const sentOn = (found, kind, match = () => true) => state.kept.sent.filter((sent) => sent.kind === kind && sent.machine === found.machine && sent.address === found.item.address && match(sent)).sort((a, b) => b.at - a.at);

function reviewFetch(found, opener, access) {
  const opened = state.opened.find((machine) => machine.machine === found.machine);
  const fetch = { state: "fetching", sent: opener, standing: null, lastSeen: opened?.lastSeen ?? null };
  if (opener?.result?.outcome === "done") return { ...fetch, state: opener.result.blob ? "fetching" : "failed" };
  if (access !== "ready") return { ...fetch, state: access };
  if (!takes(found.accepts, "review-open")) return { ...fetch, state: "not-taken" };
  if (!opener) return { ...fetch, state: isAsleep({ lastSeen: fetch.lastSeen }, nowSeconds()) ? "asleep" : "fetching" };
  const shown = shownOf(opener);
  const named = { refused: "rejected", waiting: "asleep", sending: "fetching" };
  return { ...fetch, state: named[shown.state] ?? shown.state, standing: shown.standing };
}

function heldReview(found, opener, subtask) {
  const blob = opener?.result?.outcome === "done" ? opener.result.blob : null;
  if (!blob) return { review: null, failed: false };
  const held = blobs.peek(blob.id);
  if (!held) blobs.load(blob).then((loaded) => loaded && render());
  if (held?.review && (held.review.address !== found.item.address || (held.review.subtask?.id ?? null) !== subtask)) return { review: null, failed: true };
  return { review: held?.review ?? null, failed: Boolean(held?.failed) };
}

const boardSubtaskOf = (found, subtask) => (subtask === null ? null : (found.item.subtasks?.find((one) => one.id === subtask) ?? null));
const reviewHead = (found, subtask) => (subtask === null ? (found.item.head ?? null) : (boardSubtaskOf(found, subtask)?.commits.at(-1) ?? null));
const legacyMachine = (found) => !takes(found.accepts, "accept-subtask") && takes(found.accepts, "accept-step");
const naming = (found, subtask) => (subtask === null ? {} : legacyMachine(found) && /^[1-9][0-9]*$/.test(subtask) ? { step: Number(subtask) } : { subtask });

function reviewView(found, page) {
  const { subtask } = page;
  const boardSubtask = boardSubtaskOf(found, subtask);
  const head = reviewHead(found, subtask);
  const ui = reviewUiOf(found.machine, found.item.address, subtask);
  foldCommitsOn(ui, location.hash);
  const wanted = `${head ?? ""}|${subtask ?? ""}`;
  const access = commandGate();
  const opener = sentOn(found, "review-open", (sent) => (sent.subtask ?? null) === subtask && (sent.head ?? null) === head)[0] ?? null;
  const { review, failed } = heldReview(found, opener, subtask);
  settleReview(ui, review);
  const openable = subtask === null ? Boolean(found.item.review) : OPENABLE_STEPS.includes(boardSubtask?.state);
  const fetch = failed ? { state: "failed", sent: opener } : !openable && !opener ? { state: "not-ready", sent: null } : reviewFetch(found, opener, access);
  if (openable && !opener && !ui.busy && ui.requested !== wanted && access === "ready" && takes(found.accepts, "review-open")) {
    ui.requested = wanted;
    queueMicrotask(() => requestReview(found, subtask, head));
  }
  const sentComments = sentOn(found, "review-comments", (sent) => (sent.subtask ?? null) === subtask).flatMap((sent) => {
    const shown = shownOf(sent).state;
    return (sent.comments ?? []).map((comment) => ({ ...comment, at: sent.at, state: shown }));
  });
  const accepting = (sent) => sent.subtask === subtask && sent.head === head;
  const accepted = subtask === null || !head ? null : [...sentOn(found, "accept-subtask", accepting), ...sentOn(found, "accept-step", accepting)].sort((a, b) => b.at - a.at)[0];
  const readSent = opener ? sentOn(found, "review-read", (sent) => sent.at >= opener.at && !settled(shownOf(sent).state)) : [];
  const push = sentOn(found, "push", (sent) => sent.head === pushHead(found.item))[0] ?? null;
  const ready = access === "ready";
  return {
    machine: found.machine,
    name: found.name,
    address: found.item.address,
    task: found.task,
    title: taskName(found.item, found.task),
    item: found.item,
    accepts: found.accepts,
    subtask,
    page,
    boardSubtask,
    review,
    fetch,
    ui,
    sentComments,
    readSent,
    acceptSent: accepted ? shownOf(accepted) : null,
    push: push ? shownOf(push) : null,
    pushUi: uiOf(found.machine, found.item.address),
    access,
    canComment: ready && takes(found.accepts, "review-comments"),
    canRead: ready && takes(found.accepts, "review-read"),
    refusedUrl: state.refused?.url ?? null,
    now: nowSeconds(),
  };
}

function reviewRoute(params, diff = false) {
  const [machine, address] = params;
  const page = reviewPage(params, diff);
  const found = findTask(state.opened, machine, address);
  const up = page.kind === "subtask" || page.subtask === null ? taskHref(machine, address) : subtaskHref(machine, address, page.subtask);
  const back = backButton(up);
  if (!found) return shell("board", back + top("Review") + `<div class="center">This task is no longer on the board.</div>`);
  const v = reviewView(found, page);
  const here = page.kind === "subtask" ? subtaskPage : page.kind === "commit" ? commitPage : wholePage;
  if (twoColumns()) {
    const task = page.kind === "subtask" || page.subtask === null;
    const left = task ? top(taskName(found.item, found.task), esc(found.name)) + taskBody(found, page.subtask) : subtaskPage(v, { top, left: true, selected: page.commit });
    return shell("board", back + `<div class="narrow">${left}</div>`, here(v, { top, inPanel: true })) + sendControl(v);
  }
  return shell("board", back + `<div class="narrow">${here(v, { top })}</div>`) + sendControl(v);
}

const leadSeenKey = (machine) => `kebbie-lead-seen:${machine}`;

function leadSeen(machine) {
  try {
    return Number(localStorage.getItem(leadSeenKey(machine))) || 0;
  } catch {
    return 0;
  }
}

function markLeadSeen(machine, at) {
  if (!at || leadSeen(machine) >= at) return;
  try {
    localStorage.setItem(leadSeenKey(machine), String(at));
  } catch {}
}

function deviceName(id) {
  if (id === state.device?.id) return state.device.cert.name;
  return state.lead.names?.get(id) ?? "another device";
}

function loadDeviceNames() {
  if (state.lead.names) return;
  state.lead.names = new Map();
  account
    .devices()
    .then((listed) => {
      for (const device of listed.devices ?? []) state.lead.names.set(device.cert.device, device.cert.name);
      if (route().name === "lead") render();
    })
    .catch(() => undefined);
}

function leadView(machine, now) {
  const thread = threadOf(machine.board.lead, blobs.peek, state.lead.depth.get(machine.machine) ?? 0);
  if (thread.missing) blobs.load(thread.missing).then((loaded) => loaded && route().name === "lead" && render());
  const pending = pendingMessages(state.kept.sent, machine, new Set(thread.turns.map((turn) => turn.id)), now);
  return { thread, pending };
}

function leadRoute(wanted) {
  const now = nowSeconds();
  const machines = leadMachines(state.opened);
  const away = awayOn(state.kept.sent, state.opened, now);
  const machine = chosenLead(machines, wanted ?? state.lead.machine, now);
  if (!machine) return shell("lead", `<div class="narrow">${top("Lead", awaySwitch(away))}${away ? awayNote : ""}<div class="center">No machine's lead talks to the app yet. Update kebbie on a machine to write to its lead from here.</div></div>`);
  if (state.lead.machine !== machine.machine) state.lead.draft = "";
  state.lead.machine = machine.machine;
  loadDeviceNames();
  markLeadSeen(machine.machine, machine.board.lead?.replied_at);
  const { thread, pending } = leadView(machine, now);
  const body = threadHtml({ machine, thread, pending, now, nameOf: deviceName });
  const composer = composerHtml(machine, { draft: state.lead.draft, busy: state.lead.busy, error: state.lead.error, takes: (machine.board.accepts ?? []).includes("lead-message") });
  const when = (one) => {
    const view = one === machine ? { thread, pending } : leadView(one, now);
    const last = Math.max(view.thread.turns.at(-1)?.at ?? 0, view.pending.at(-1)?.sent.at ?? 0);
    return { when: shortWhen(last, now), preview: previewOf(view.thread.turns, view.pending) };
  };
  if (splitLike()) {
    const rows = machines.map((one) => pickRow(one, { now, selected: one === machine, unseen: one !== machine && (one.board.lead?.replied_at ?? 0) > leadSeen(one.machine), ...when(one) }));
    return shell("lead", `<div class="narrow">${top("Lead", awaySwitch(away))}${away ? awayNote : ""}<div class="mlist">${rows.join("")}</div></div>`, panelHead(machine, now) + body + composer, { said: `Thread with ${machine.name}'s lead` });
  }
  const pop = state.lead.picking ? popHtml(machines.map((one) => pickRow(one, { now, selected: one === machine, pop: true, unseen: one !== machine && (one.board.lead?.replied_at ?? 0) > leadSeen(one.machine), when: when(one).when }))) : "";
  return shell("lead", leadHead(machine, { now, away, picking: state.lead.picking, canPick: machines.length > 1 }) + body + composer + pop, "", { bare: true });
}

async function sendLead() {
  const machine = leadMachines(state.opened).find((one) => one.machine === state.lead.machine);
  const text = state.lead.draft.trim();
  if (!machine || !text || state.lead.busy) return;
  const sent = await sendTo({ machine: { machine: machine.machine, name: machine.name }, payload: { type: "lead-message", text }, meta: { text }, ui: state.lead });
  if (sent) {
    state.lead.draft = "";
    state.lead.error = null;
  }
  render();
  if (sent) scrollThreadToEnd();
}

async function switchAway(on) {
  const machines = state.opened.filter((one) => one.board && !one.problem && (one.board.accepts ?? []).includes("away"));
  const ui = { busy: false, error: null };
  for (const machine of machines) await sendTo({ machine: { machine: machine.machine, name: machine.name }, payload: { type: "away", on }, meta: { on }, ui });
  state.lead.error = ui.error;
  render();
}

function atThreadEnd() {
  const pane = root.querySelector(".app > .panel");
  if (pane && root.querySelector(".panel .thread")) return pane.scrollHeight - pane.scrollTop - pane.clientHeight < 80;
  return innerHeight + scrollY >= document.documentElement.scrollHeight - 80 || !root.querySelector(".thread");
}

function scrollThreadToEnd() {
  const pane = root.querySelector(".app > .panel");
  if (pane && root.querySelector(".panel .thread")) pane.scrollTop = pane.scrollHeight;
  else if (root.querySelector("main.screen .thread")) scrollTo(0, document.documentElement.scrollHeight);
}

function fitComposer(field = root.querySelector("#lead-say")) {
  if (field) {
    field.style.height = "auto";
    field.style.height = `${Math.min(field.scrollHeight + 2, 136)}px`;
  }
  const composer = root.querySelector(".composer");
  if (composer) document.documentElement.style.setProperty("--composer-h", `${Math.ceil(composer.getBoundingClientRect().height)}px`);
  else document.documentElement.style.removeProperty("--composer-h");
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
    const facts = opened.problem ? [problemText(opened.problem)] : awake && opened.board ? [`last heard ${when(opened.lastSeen)}`, `kebbie ${opened.board.kebbie}`] : opened.board || opened.expired ? [`last seen ${lastSeenText(opened.lastSeen ?? opened.storedAt, now)}`] : ["no board yet"];
    const policy = state.info?.commands && opened.board && !opened.problem ? [machinePolicy(opened.board.accepts)] : [];
    return machineRow({ machine: opened.machine, name: opened.name, awake, facts: [...policy, ...facts] });
  });
  const message = state.notice ? `<div class="center warn" role="alert">${esc(state.notice)}</div>` : "";
  return `<div class="narrow">${top("Machines", esc(`${state.opened.length} connected`))}${storeWarning()}${message}${rows.join("")}<a class="btn phone-only" href="/account">Account</a></div>`;
}

function lockedScreen() {
  const status = state.session?.state ?? "locked";
  if (needsFirstPasskey(state.me, state.session)) return addPasskeyScreen({ login: state.me.github, notice: state.notice, busy: state.busy });
  if (status === "locked") return unlockScreen({ notice: state.notice });
  if (status === "no-prf" || status === "not-wrapped" || status === "github") return cantUnlockScreen({ state: status, notice: state.notice });
  return authScreen({ intro: `<div class="center">${esc(STATE_TEXT[status] ?? STATE_TEXT.locked)}</div>`, actions: `<button type="button" class="btn primary" data-action="other-passkey">Try another passkey</button>`, notice: state.notice });
}

const emailSignIn = () => Boolean(state.info?.email_signin);
const liftKeyboard = keyboardLift();

function signedOutScreen() {
  const shown = { notice: state.notice, busy: state.busy };
  const step = signedOutStep({ info: state.info, remembered: rememberedCredential(), ...state.signin });
  if (step === "code") return codeScreen({ purpose: "sign-in", email: state.signin.email, code: state.draft.code, ...shown });
  if (step === "passkey") return passkeySignInScreen(shown);
  if (step === "email") return emailSignInScreen({ github: Boolean(state.info?.github), email: state.draft.email ?? state.signin.email, next: onward, ...shown });
  return signInScreen({ github: Boolean(state.info?.github), notice: state.notice, next: onward });
}

const signedInWaits = () => signedInStep({ info: state.info, me: state.me, session: state.session, address: state.address, emailStep: state.emailStep });

function signedInScreen(step) {
  const shown = { notice: state.notice, busy: state.busy };
  const who = signedInAs(state.me, state.address);
  if (step === "unlock") return keylessUnlockScreen({ who, ...shown });
  if (step === "create-passkey") return createPasskeyScreen({ who, ...shown });
  if (step === "confirm-email") return codeScreen({ purpose: "confirm", email: state.emailStep.email, code: state.draft.code, ...shown });
  return addEmailScreen({ email: state.draft.email ?? state.address?.email ?? "", ...shown });
}

function unsupportedScreen() {
  return authScreen({ intro: `<div class="center">This browser lacks the cryptography the app needs (Ed25519, X25519, HKDF and base64 in Uint8Array). Update to iOS 18.2 or later, or a current Chrome or Firefox.</div>`, actions: "" });
}

function render() {
  if (pageMode) return;
  unlockEnds.watch(state.session);
  const { name, params } = route();
  let html;
  if (!cryptoReady()) html = unsupportedScreen();
  else if (state.unlocking) html = unlockingScreen();
  else if (!state.me) html = signedOutScreen();
  else if (signedInWaits()) html = signedInScreen(signedInWaits());
  else if (state.session?.state !== "unlocked") html = lockedScreen();
  else if (name === "questions") html = shell("questions", questionsScreen());
  else if (name === "machines") html = shell("machines", machinesScreen());
  else if (name === "lead") html = leadRoute(params[0] ?? null);
  else if (name === "task") html = twoColumns() ? wideTaskScreen(params) : taskScreen(params);
  else if (name === "card") html = twoColumns() ? wideTaskScreen(params, cardPanel) : taskScreen(params, true);
  else if (name === "plan") html = twoColumns() ? wideTaskScreen(params, planPanel) : planScreen(params);
  else if (name === "section") html = sectionRoute(params[0]);
  else if (name === "backlog") html = backlogRoute();
  else if (name === "item") html = itemRoute(params[0]);
  else if (name === "idea") html = ideaRoute();
  else if (name === "review") html = reviewRoute(params);
  else if (name === "diff") html = reviewRoute(params, true);
  else html = boardRoute();
  const focused = document.activeElement?.id && root.contains(document.activeElement) ? { id: document.activeElement.id, start: document.activeElement.selectionStart, end: document.activeElement.selectionEnd } : null;
  const auth = isAuthScreen(html);
  liftKeyboard((auth && html.includes("<input")) || html.includes('id="lead-say"'));
  const reviewing = name === "review" || name === "diff";
  const listTop = listPane()?.scrollTop ?? (twoColumns() ? scrollY : 0);
  const anchor = twoColumns() && !listPane() ? rowTop(location.hash) : null;
  const panelTop = rendered === location.hash ? (root.querySelector(".panel")?.scrollTop ?? 0) : 0;
  const stick = name === "lead" && atThreadEnd();
  root.innerHTML = html.startsWith('<div class="app') ? html : `<main class="screen${auth ? " auth" : ""}">${html}</main>`;
  rendered = location.hash;
  document.documentElement.classList.toggle("panes", twoColumns() && Boolean(root.querySelector(".app.with-panel")));
  document.documentElement.classList.toggle("has-send", Boolean(root.querySelector(".rv-sendbar")));
  document.documentElement.classList.toggle("has-fab", Boolean(root.querySelector(".fab, .idea-add")));
  document.documentElement.classList.toggle("has-composer", Boolean(root.querySelector(".composer")));
  fitComposer();
  if (reviewing) hangLines(root);
  const refocus = focused ? document.getElementById(focused.id) : null;
  if (refocus) {
    refocus.focus({ preventScroll: true });
    if (focused.start !== null && focused.start !== undefined) refocus.setSelectionRange(focused.start, focused.end);
  }
  fillBlobs();
  for (const element of root.querySelectorAll("[data-markdown]")) element.replaceChildren(...markdown(element.textContent));
  document.title = ["task", "card", "plan", "review", "diff"].includes(name) && params[1] ? `${params[1].split("@")[0]} · kebbie` : "kebbie";
  if (listTop) listPane()?.scrollTo(0, listTop);
  const moved = anchor === null ? null : rowTop(location.hash);
  if (moved !== null) listPane()?.scrollBy(0, moved - anchor);
  if (panelTop) root.querySelector(".panel")?.scrollTo(0, panelTop);
  if (stick) scrollThreadToEnd();
}

const rowTop = (hash) => root.querySelector(`.app > main.screen a[href="${CSS.escape(hash)}"]`)?.getBoundingClientRect().top ?? null;
const listPane = () => (document.documentElement.classList.contains("panes") ? root.querySelector(".app > main.screen") : null);

function scrollListToTop() {
  scrollTo(0, 0);
  listPane()?.scrollTo(0, 0);
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
    return (await get(LISTING)) ?? null;
  } catch {
    state.storeFailed = true;
    return null;
  }
}

async function refresh() {
  const session = state.session;
  if (session?.state !== "unlocked") return;
  let listing;
  let fresh = false;
  try {
    listing = await account.boards();
    state.offline = false;
    state.fetchedAt = nowSeconds();
    fresh = true;
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
  if (fresh && result.list) await put(LISTING, { listing, at: state.fetchedAt }).catch(() => (state.storeFailed = true));
  state.opened = result.opened;
  state.marks = result.marks;
  state.keys = new Map((result.list?.machines ?? []).map((listed) => [listed.machine, listed.keys]));
  await loadCommands(session);
  closeSettledNotices();
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

function closeSettledNotices() {
  navigator.serviceWorker
    ?.getRegistration("/")
    .then((registration) => registration && closeSettled(registration, state.opened))
    .catch(() => {});
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

async function refusedDevice(error) {
  if (!(error instanceof HubError && error.code === "device-removed") || !state.session?.identity) return;
  state.device = null;
  await forgetDevice(deviceStore, state.session.identity.id);
  await stopNotifications(account, deviceStore);
}

function commandText(error) {
  if (error instanceof CommandProblem) return COMMAND_TEXT[error.reason] ?? error.reason;
  if (error instanceof HubError && COMMAND_TEXT[error.code]) return COMMAND_TEXT[error.code];
  return hubMessage(error);
}

async function loadCommands(session) {
  if (!state.info?.commands || session?.state !== "unlocked") return;
  const id = session.identity.id;
  if (state.keptFor !== id) {
    state.kept = await loadSent(deviceStore, id, nowSeconds());
    state.allowed = await allowsChanges(deviceStore, id);
    state.device = await loadDevice(deviceStore, id);
    state.lastMachine = (await get(`backlog-machine:${id}`).catch(() => null)) ?? null;
    state.keptFor = id;
  }
  collect();
}

const keysOf = (machine) => state.keys.get(machine) ?? null;
let collecting = null;

function collect() {
  const identity = state.session?.identity;
  if (!identity || !state.info?.commands || collecting || state.kept.sent.every((one) => one.result)) return collecting;
  collecting = (async () => {
    try {
      const kept = await collectResults({ calls: account, identity, kept: state.kept, keysOf, now: nowSeconds() });
      if (state.session?.identity !== identity) return;
      state.kept = kept;
      await keepSent(deviceStore, identity.id, kept);
      render();
    } catch {
    } finally {
      collecting = null;
    }
  })();
  return collecting;
}

async function send(target, payload, meta, ttl = undefined) {
  const { machine, address } = target.dataset;
  const ui = uiOf(machine, address);
  const keys = keysOf(machine);
  ui.busy = true;
  ui.error = null;
  render();
  try {
    if (!keys) throw new Error("Your account doesn't list this machine's keys.");
    const identity = state.session.identity;
    if (state.allowed !== true) throw new CommandProblem("view-only");
    state.device = await ensureDevice({ store: deviceStore, calls: account, identity, name: proposedName(navigator.userAgent), now: nowSeconds(), kept: state.device });
    const sent = await sendCommand({ calls: account, identity, device: state.device, allowed: state.allowed, machine: { id: machine, keys }, payload, at: nowSeconds(), meta, ttl });
    state.kept = { ...state.kept, sent: [...state.kept.sent, sent] };
    await keepSent(deviceStore, identity.id, state.kept);
    state.ui.set(uiKey(machine, address), freshUi());
  } catch (error) {
    ui.busy = false;
    ui.error = commandText(error);
    await refusedDevice(error);
    if (error instanceof HubError && error.status === 402) state.refused = { url: error.url };
  }
  render();
  collect();
}

function sendAnswer(target) {
  const { machine, address } = target.dataset;
  const found = findTask(state.opened, machine, address);
  const question = found ? shownQuestion(found.item) : null;
  const ui = uiOf(machine, address);
  const option = ui.free ? undefined : question?.options[ui.picked];
  if (!question || !answerReady(option, ui)) return;
  const choice = ui.free ? null : ui.picked;
  send(target, { type: "answer", address, question: question.id, choice, comment: ui.note.trim() }, { question: question.id, choice, label: option?.label ?? null, machineName: found.name }, question.kind ? KEPT_ANSWER_TTL : undefined);
}

function sendPromptAnswer(target) {
  const { machine, address } = target.dataset;
  const found = findTask(state.opened, machine, address);
  const prompt = found?.item.prompt;
  const ui = uiOf(machine, address);
  const option = prompt?.options[ui.picked];
  if (!prompt || ui.prompt !== prompt.id || !takesPromptAnswers(found) || !choosable(option)) return;
  send(target, { type: "prompt-answer", address, prompt: prompt.id, key: option.key }, { prompt: prompt.id, key: option.key, label: option.label, machineName: found.name });
}

function sendPush(target) {
  const { machine, address } = target.dataset;
  const found = findTask(state.opened, machine, address);
  if (!found || !pushReady(found.item)) return;
  const { item } = found;
  const head = pushHead(item);
  const last = item.pushable?.subtasks.at(-1);
  const through = head !== item.head && legacyMachine(found) && /^[1-9][0-9]*$/.test(last ?? "") ? { through_step: Number(last) } : {};
  send(target, { type: "push", address, head, card_sha: head === item.head ? (item.card?.sha ?? null) : null, ...through }, { head, short: short(head), count: pushCount(item), machineName: found.name });
}

function sendReview(target) {
  const { machine, address } = target.dataset;
  const found = findTask(state.opened, machine, address);
  if (!found || !takes(found.accepts, "review-send")) return;
  send(target, { type: "review-send", address }, { machineName: found.name, assessed: found.item.assessment?.at ?? null });
}

function sendNoticed(target, action, key) {
  const { machine, address } = target.dataset;
  const found = findTask(state.opened, machine, address);
  const kind = NOTICED_ACTIONS.find((one) => one.action === action).kind;
  if (!found || !key || !takes(found.accepts, kind)) return;
  const line = blobs.peek(found.item.card?.blob.id ?? "")?.card?.noticed.find((one) => one.found === key);
  const payload = kind === "found-add" ? { type: kind, item: key, task: address } : { type: kind, item: key };
  send(target, payload, { address: key, label: line?.text ?? null, ...(kind === "found-add" ? { task: address } : {}), machineName: found.name });
}

const lastMachineKey = () => `backlog-machine:${state.session?.identity?.id}`;

async function sendTo({ machine, payload, meta, ui }) {
  const keys = keysOf(machine.machine);
  ui.busy = true;
  ui.error = null;
  render();
  try {
    if (!keys) throw new Error("Your account doesn't list this machine's keys.");
    const identity = state.session.identity;
    if (state.allowed !== true) throw new CommandProblem("view-only");
    state.device = await ensureDevice({ store: deviceStore, calls: account, identity, name: proposedName(navigator.userAgent), now: nowSeconds(), kept: state.device });
    const sent = await sendCommand({ calls: account, identity, device: state.device, allowed: state.allowed, machine: { id: machine.machine, keys }, payload, at: nowSeconds(), meta: { machineName: machine.name, ...meta } });
    state.kept = { ...state.kept, sent: [...state.kept.sent, sent] };
    await keepSent(deviceStore, identity.id, state.kept);
    ui.busy = false;
    return sent;
  } catch (error) {
    ui.busy = false;
    ui.error = commandText(error);
    await refusedDevice(error);
    if (error instanceof HubError && error.status === 402) state.refused = { url: error.url };
    render();
    return null;
  } finally {
    collect();
  }
}

async function sendBacklog({ machine, payload, meta, ui }) {
  const sent = await sendTo({ machine, payload, meta, ui });
  if (!sent) return null;
  state.lastMachine = machine.machine;
  await put(lastMachineKey(), machine.machine).catch(() => undefined);
  return sent;
}

const machineOf = (found) => ({ machine: found.machine, name: found.name });

async function requestReview(found, subtask, head) {
  const ui = reviewUiOf(found.machine, found.item.address, subtask);
  const payload = { type: "review-open", address: found.item.address, ...naming(found, subtask) };
  await sendTo({ machine: machineOf(found), payload, meta: { head, subtask }, ui });
  render();
}

function reveal(selector, offset = null) {
  const element = root.querySelector(selector);
  if (!element) return;
  const panel = element.closest(".panel") ?? (listPane()?.contains(element) ? listPane() : null);
  const header = element.querySelector(".rv-fh") ?? element;
  const gap = offset ?? (parseFloat(getComputedStyle(header).top) || 0);
  if (panel) panel.scrollTop += element.getBoundingClientRect().top - panel.getBoundingClientRect().top - gap;
  else scrollTo(0, element.getBoundingClientRect().top + scrollY - gap);
}

function toDiff(v, sha, selector, offset, focus = null) {
  const commit = v.review?.subtask?.commits.findIndex((one) => one.sha === sha) ?? -1;
  const last = commit === (v.review?.subtask?.commits.length ?? 0) - 1;
  const href = v.subtask === null ? diffHref(v.machine, v.address) : diffHref(v.machine, v.address, v.subtask, commit < 0 || last ? "all" : commit + 1);
  const here = route().name === "diff" && shaOf(v) === sha;
  if (!here) location.hash = href;
  render();
  if (!here) scrollListToTop();
  reveal(selector, offset);
  if (focus) root.querySelector(focus)?.focus({ preventScroll: true });
}

let revealAfter = null;

async function sendComments(found, v) {
  const { ui, review } = v;
  if (!review || !ui.drafts.length || ui.busy) return;
  const comments = ui.drafts.map((draft) => ({ path: draft.path, line: draft.line, side: draft.side, sha: draft.sha ?? null, text: draft.text }));
  const payload = { address: v.address, ...naming(found, review.subtask?.id ?? null), comments };
  ui.error = commentsProblem(payload);
  if (ui.error) return render();
  const sent = await sendTo({ machine: machineOf(found), payload: { type: "review-comments", ...payload }, meta: { subtask: review.subtask?.id ?? null, comments, count: comments.length }, ui });
  if (sent) {
    const receipt = { count: comments.length, at: sent.at, device: sent.deviceName ?? "this device" };
    Object.assign(ui, { drafts: [], tray: false, composing: null, receipt });
    setTimeout(() => {
      if (ui.receipt !== receipt) return;
      ui.receipt = null;
      render();
    }, 4000);
  }
  render();
}

async function markRead(found, v, index) {
  const { ui } = v;
  const file = filesOf(v)[index];
  const read = !readOf(v, file);
  ui.read.set(readKey(file), read);
  ui.open.set(readKey(file), !read);
  if (ui.composing?.path === file.path) ui.composing = null;
  render();
  reveal(`#rv-f-${index}`);
  if (!v.canRead) return;
  const quiet = { busy: false, error: null };
  const sha = v.page.kind === "commit" ? shaOf(v) : null;
  const payload = { type: "review-read", address: v.address, path: file.path, read, content: file.content, ...(sha ? { sha } : {}) };
  const sent = await sendTo({ machine: machineOf(found), payload, meta: { path: file.path, read, content: file.content, sha }, ui: quiet });
  if (!sent) ui.error = quiet.error;
  render();
}

async function acceptSubtask(found, v) {
  const part = v.review?.subtask;
  if (!part || v.ui.busy) return;
  const head = part.commits.at(-1).sha;
  const named = naming(found, part.id);
  const payload = named.step ? { type: "accept-step", address: v.address, step: named.step, head } : { type: "accept-subtask", address: v.address, subtask: part.id, head };
  await sendTo({ machine: machineOf(found), payload, meta: { head, subtask: part.id }, ui: v.ui });
  render();
}

function routeView() {
  const { name, params } = route();
  const found = findTask(state.opened, params[0], params[1]);
  return found ? { found, v: reviewView(found, reviewPage(params, name === "diff")) } : null;
}

function reviewAction(action, target) {
  const viewed = routeView();
  if (!viewed) return;
  const { found, v } = viewed;
  const { ui } = v;
  const id = Number(target.dataset.id);
  const draft = ui.drafts.find((one) => one.id === id);
  const file = filesOf(v)[Number(target.dataset.file)];
  if (action === "rv-retry") {
    Object.assign(ui, { requested: null, error: null });
    return requestReview(found, v.subtask, reviewHead(found, v.subtask));
  }
  if (action === "rv-commit") {
    const commit = v.review?.subtask?.commits[Number(target.dataset.commit)];
    if (commit && !ui.commits.delete(commit.sha)) ui.commits.add(commit.sha);
    return render();
  }
  if (action === "rv-send") return sendComments(found, v);
  if (action === "rv-read" && file) return markRead(found, v, Number(target.dataset.file));
  if (action === "rv-accept") return acceptSubtask(found, v);
  if (action === "rv-line" && file) {
    if (ui.composing || !v.canComment) return;
    ui.composing = { path: file.path, side: target.dataset.side, line: Number(target.dataset.line), sha: shaOf(v), text: "" };
    ui.tray = false;
    render();
    return root.querySelector("#rv-c")?.focus({ preventScroll: true });
  }
  if (action === "rv-edit" && draft) {
    Object.assign(ui, { composing: { ...draft }, tray: false });
    ui.open.set(draft.path, true);
    return toDiff(v, draft.sha ?? null, "#rv-c", 160, "#rv-c");
  }
  if (action === "rv-show" && draft) {
    ui.tray = false;
    ui.open.set(draft.path, true);
    return toDiff(v, draft.sha ?? null, `#rv-c-${id}`, 200);
  }
  if (action === "rv-compose-cancel") ui.composing = null;
  else if (action === "rv-compose-add" && ui.composing) {
    const text = cleanComment(ui.composing.text);
    if (!text) return;
    ui.composing.problem = commentProblem(text);
    if (ui.composing.problem) return render();
    if (ui.composing.id) Object.assign(ui.drafts.find((one) => one.id === ui.composing.id) ?? {}, { text });
    else ui.drafts.push({ id: ui.nextId++, path: ui.composing.path, side: ui.composing.side, line: ui.composing.line, sha: ui.composing.sha ?? null, text });
    Object.assign(ui, { composing: null, receipt: null, error: null });
  } else if (action === "rv-remove") {
    ui.drafts = ui.drafts.filter((one) => one.id !== id);
    if (ui.composing?.id === id) ui.composing = null;
  } else if (action === "rv-tray") ui.tray = !ui.tray;
  else if (action === "rv-stale-dismiss") ui.stale = null;
  else if (action === "rv-fold" && file) ui.open.set(readKey(file), !openOf(v, file));
  render();
}

async function addIdea() {
  const chosen = ideaChoice();
  const text = state.idea.text.trim();
  if (!chosen || !text || state.idea.busy) return;
  const sent = await sendBacklog({ machine: chosen, payload: { type: "idea", text, repo: null }, meta: { address: null, text, label: text }, ui: state.idea });
  if (!sent) return;
  state.idea = { text: "", machine: null, choosing: false, busy: false, error: null };
  go(twoColumns() ? entryHref(`idea:${sent.id}`) : "#/backlog");
}

async function sendItemAnswer(entry) {
  const ui = backlogUi(entry.id);
  const question = blobs.peek(entry.row.body?.id ?? "")?.body?.questions[ui.picked];
  const chosen = chooseMachine(candidatesOf(entry, "item-answer"), { repo: entry.row.repo, lastUsed: state.lastMachine, chosen: ui.machine });
  const answer = ui.text.trim();
  if (!question || !chosen || !answer || ui.busy) return;
  const sent = await sendBacklog({ machine: chosen, payload: { type: "item-answer", item: entry.key, index: ui.picked, question: question.text, answer }, meta: { address: entry.key, index: ui.picked, text: answer, label: entry.row.title }, ui });
  if (sent) Object.assign(ui, { picked: null, text: "" });
  render();
}

async function startItem(entry) {
  const ui = backlogUi(entry.id);
  const chosen = chosenFor(entry, "start-item");
  if (!chosen || ui.busy) return;
  const anyway = startsAnyway(latestFor(entry, "start-item"), ui.visit);
  await sendBacklog({ machine: chosen, payload: { type: "start-item", item: entry.key, anyway }, meta: { address: entry.key, label: entry.row.title, anyway }, ui });
  render();
}

async function sendItemEdit(entry) {
  const ui = backlogUi(entry.id);
  const editing = ui.editing;
  const chosen = chosenFor(entry, "item-edit");
  if (!editing || !chosen || !editReady(editing, editing.required)) return;
  const base = await textHash(editing.original);
  const text = cleanEdit(editing);
  const payload = { type: "item-edit", item: entry.key, section: editing.section, text, base };
  editing.error = editProblem(payload);
  if (editing.error) return render();
  const sent = await sendBacklog({ machine: chosen, payload, meta: { address: entry.key, section: editing.section, heading: editing.heading, text, label: entry.row.title }, ui: editing });
  if (sent) ui.editing = null;
  render();
}

function openEditor(entry, section) {
  const ui = backlogUi(entry.id);
  const part = section === null ? null : bodyOf(entry).body?.sections.find((one) => one.key === section);
  if (section !== null && !part) return;
  const original = part ? part.text : entry.row.title;
  ui.editing = { section, heading: part?.heading ?? null, text: original, original, required: part ? part.required : true, busy: false, error: null };
  render();
  const field = document.getElementById(section === null ? "edit-title" : `edit-${section}`);
  if (!field) return;
  field.focus();
  field.setSelectionRange(field.value.length, field.value.length);
}

const BACKLOG_ACTIONS = new Set(["add-idea", "backlog-choose", "backlog-machine", "pick-question", "send-item-answer", "start-item", "item-edit", "item-edit-cancel", "item-edit-save", ...FOUND_KINDS]);

function backlogAction(action, target) {
  const id = target.closest("[data-item]")?.dataset.item ?? null;
  const entry = id ? findEntry(id).entry : null;
  const ui = entry ? backlogUi(entry.id) : state.idea;
  if (ui.busy || ui.editing?.busy) return;
  const section = target.dataset.section === "" ? null : (target.dataset.section ?? null);
  if (action === "item-edit" && entry) return openEditor(entry, section);
  if (action === "item-edit-save" && entry) return sendItemEdit(entry);
  if (action === "item-edit-cancel") ui.editing = null;
  if (action === "add-idea") return addIdea();
  if (action === "send-item-answer" && entry) return sendItemAnswer(entry);
  if (action === "start-item" && entry) return startItem(entry);
  if (FOUND_KINDS.includes(action) && entry) return foundAction(entry, action, target);
  if (action === "backlog-choose") ui.choosing = !ui.choosing;
  else if (action === "backlog-machine") Object.assign(ui, { machine: target.dataset.machine, choosing: false });
  else if (action === "pick-question") {
    const index = Number(target.dataset.index);
    if (ui.picked !== index) ui.text = "";
    Object.assign(ui, { picked: index, error: null });
  }
  render();
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
  else if (action === "choose") return sendPromptAnswer(target);
  else if (action === "push") return sendPush(target);
  else if (action === "review-send") return sendReview(target);
  else if (action === "card-seen") markSeen(machine, address, Number(element.dataset.version));
  else if (NOTICED_ACTIONS.some((one) => one.action === action)) return sendNoticed(target, action, element.dataset.item);
  render();
}

function stopCommands() {
  state.allowed = false;
  state.device = null;
  state.keptFor = null;
  state.kept = { cursor: 0, sent: [] };
  state.ui = new Map();
  state.keys = new Map();
  state.backlogUi = new Map();
  state.idea = { text: "", machine: null, choosing: false, busy: false, error: null };
  state.lastMachine = null;
  state.reviewUi = new Map();
}

function lockBoards() {
  blobs.forget();
  bodies.stop();
  stopCommands();
  following?.abort();
  following = null;
  state.opened = [];
  state.session = { state: "locked", me: state.me };
  render();
}

function signedOut(message) {
  blobs.forget();
  deviceStore.keptBlobs.clear().catch(() => undefined);
  bodies.stop();
  stopCommands();
  forgetUnlock();
  following?.abort();
  following = null;
  state.me = null;
  state.session = null;
  state.address = null;
  state.emailStep = null;
  state.signin = { step: "email", email: "", chosen: false };
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
          if (name === "result") collect();
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
  if (emailSignIn()) state.address = await account.email().catch(() => null);
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
  if (state.me || emailSignIn() || !(await autofillAvailable())) return;
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

async function emailStep(act) {
  state.busy = true;
  state.notice = null;
  render();
  try {
    await act();
  } catch (error) {
    state.notice = emailText(error);
  } finally {
    state.busy = false;
  }
  render();
}

const typedEmail = () => root.querySelector("#email")?.value.trim() ?? "";
const freshSignIn = () => ({ step: "email", email: "", chosen: true });

// The code field is shown and focused inside the tap, since iOS opens the keyboard only for a focus the tap itself makes.
function focusCode() {
  root.querySelector("#code")?.focus();
}

function sendCode() {
  const email = address(typedEmail());
  if (!email) {
    state.notice = typedEmail() ? "That doesn't look like an email address." : "Type your email first.";
    return render();
  }
  state.signin = { ...state.signin, step: "code", email };
  state.draft.code = "";
  state.notice = null;
  render();
  focusCode();
  return account.askCode(email).catch((error) => {
    state.signin = { ...state.signin, step: "email" };
    state.notice = emailText(error);
    render();
  });
}

function enterCode() {
  const code = typedCode(root.querySelector("#code")?.value);
  if (code.length !== 6) {
    state.notice = "Type the 6 digits from the mail.";
    return render();
  }
  return emailStep(async () => {
    const signed = await account.enterCode(state.signin.email, code);
    if (onward) return location.assign(onward);
    state.me = signed.account;
    state.session = { state: "locked", me: state.me };
    state.address = await account.email().catch(() => null);
    state.signin = freshSignIn();
    state.draft = { email: null, code: "" };
  });
}

function resendCode() {
  if (state.emailStep) return emailStep(async () => void (await account.setEmail(state.emailStep.email)));
  return emailStep(async () => void (await account.askCode(state.signin.email)));
}

function otherEmail() {
  state.notice = null;
  if (state.emailStep) state.emailStep = null;
  else state.signin = { ...state.signin, step: "email" };
  render();
}

function addEmail() {
  const email = address(typedEmail());
  if (!email) {
    state.notice = typedEmail() ? "That doesn't look like an email address." : "Type your email first.";
    return render();
  }
  if (state.address?.verified && state.address.email === email) return;
  state.emailStep = { email };
  state.draft.code = "";
  state.notice = null;
  render();
  focusCode();
  return account.setEmail(email).then(
    (asked) => {
      if (!asked.verified) return;
      state.address = asked;
      state.emailStep = null;
      render();
    },
    (error) => {
      state.emailStep = null;
      state.notice = emailText(error);
      render();
    },
  );
}

function confirmEmail() {
  const code = typedCode(root.querySelector("#code")?.value);
  if (code.length !== 6) {
    state.notice = "Type the 6 digits from the mail.";
    return render();
  }
  return emailStep(async () => {
    state.address = await account.confirmEmail(code);
    state.emailStep = null;
    state.draft = { email: null, code: "" };
  });
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

let accountPage = null;

// The account page opens inside the running app so the unlocked session outlives it: a fresh load can only resume the unlock the device store kept.
function openAccount() {
  history.pushState(null, "", "/account");
  pageMode = true;
  state.sheet = false;
  document.documentElement.classList.remove("panes");
  scrollTo(0, 0);
  accountPage = startPage(root, { showBoard, given: { info: state.info, me: state.me, session: state.session, address: state.address, device: state.device }, back: () => history.back() });
}

function leaveAccount() {
  const left = accountPage.leave();
  accountPage = null;
  pageMode = false;
  if (left.session) state.session = left.session;
  if (left.me) state.me = left.me;
  state.address = left.address;
  if (left.allowed !== null) state.allowed = left.allowed;
  if (left.device) state.device = left.device;
  if (left.kept) state.kept = left.kept;
  render();
}

addEventListener("popstate", () => {
  if (accountPage && location.pathname !== "/account") leaveAccount();
});

root.addEventListener("click", (event) => {
  const link = event.target.closest?.('a[href="/account"]');
  if (!link || pageMode || state.session?.state !== "unlocked" || event.metaKey || event.ctrlKey || event.shiftKey) return;
  event.preventDefault();
  openAccount();
});

root.addEventListener("click", (event) => {
  if (pageMode) return;
  const search = root.querySelector("#bl-q");
  if (search && event.target.closest('.tabbar a[href="/#/backlog"].on')) {
    event.preventDefault();
    scrollListToTop();
    return search.focus({ preventScroll: true });
  }
  const picked = event.target.closest(".sheet a.item");
  if (picked) {
    event.preventDefault();
    return closeSheet(() => go(picked.getAttribute("href")));
  }
  if (event.target.closest("a.fab")) {
    event.preventDefault();
    return openComposer();
  }
  const fileRow = event.target.closest("a[data-open-file]");
  if (fileRow) return openFile(fileRow);
  const target = event.target.closest("[data-action]");
  if (!target) return;
  const action = target.dataset.action;
  if (action.startsWith("rv-")) return reviewAction(action, target);
  if (action === "steps-more") {
    const ui = uiOf(route().params[0], route().params[1]);
    ui.moreSteps = !ui.moreSteps;
    return render();
  }
  if (BACKLOG_ACTIONS.has(action)) return backlogAction(action, target);
  if (action === "find-chip") return setFind(toggled(state.find, target.dataset.kind, target.dataset.value));
  if (action === "find-clear") return setFind(cleared(state.find));
  if (action === "find-sort") return setFind({ ...state.find, sort: target.dataset.sort });
  if (action === "find-q-clear") {
    const input = root.querySelector("#bl-q");
    if (input) input.value = "";
    setFind({ ...state.find, q: "" }, { patch: true });
    return input?.focus({ preventScroll: true });
  }
  if (action === "lead-pick") {
    state.lead.picking = !state.lead.picking;
    return render();
  }
  if (action === "lead-send") return sendLead();
  if (action === "lead-away") return switchAway(target.checked);
  if (action === "lead-older") {
    state.lead.depth.set(state.lead.machine, (state.lead.depth.get(state.lead.machine) ?? 0) + 1);
    return render();
  }
  const decided = target.closest(".decide");
  if (decided) return decideAction(action, decided, target);
  if (action === "reload") {
    if (!reloading) location.reload();
    reloading = true;
    return;
  }
  if (action === "dismiss-hint") {
    state.hint = false;
    try {
      localStorage.setItem("kebbie-hint-dismissed", "1");
    } catch {}
    render();
  } else if (action === "send-code") sendCode();
  else if (action === "enter-code") enterCode();
  else if (action === "resend-code") resendCode();
  else if (action === "other-email") otherEmail();
  else if (action === "email-instead") {
    state.signin = freshSignIn();
    state.notice = null;
    render();
  } else if (action === "use-passkey") signInWithPasskey(null);
  else if (action === "add-email") addEmail();
  else if (action === "confirm-email") confirmEmail();
  else if (action === "sign-in") signInWithPasskey(rememberedCredential());
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

function openFile(link) {
  const viewed = routeView();
  const index = Number(link.dataset.openFile);
  const file = viewed ? filesOf(viewed.v)[index] : null;
  if (!file) return;
  viewed.v.ui.open.set(readKey(file), true);
  revealAfter = { selector: `#rv-f-${index}`, offset: twoColumns() ? 8 : null };
}

root.addEventListener("input", (event) => {
  if (event.target.id === "bl-q") {
    if (!pageMode) setFind({ ...state.find, q: event.target.value }, { patch: true });
    return;
  }
  if (event.target.dataset?.draft === "review-comment") {
    const params = route().params;
    const ui = reviewUiOf(params[0], params[1], reviewPage(params, false).subtask);
    if (ui.composing) ui.composing.text = event.target.value;
    return;
  }
  if (event.target.id === "lead-say") {
    state.lead.draft = event.target.value;
    const button = root.querySelector('[data-action="lead-send"]');
    if (button) button.disabled = state.lead.busy || state.lead.draft.trim() === "";
    return fitComposer(event.target);
  }
  if (event.target.id === "email") state.draft.email = event.target.value;
  if (event.target.id === "code") state.draft.code = event.target.value;
  const draft = event.target.dataset?.draft;
  if (draft === "idea" || draft === "item-answer") {
    const id = event.target.closest("[data-item]")?.dataset.item;
    const ui = draft === "idea" ? state.idea : backlogUi(id);
    ui.text = event.target.value;
    const button = root.querySelector(draft === "idea" ? '[data-action="add-idea"]' : '[data-action="send-item-answer"]');
    if (button) button.disabled = ui.busy || ui.text.trim() === "";
    return;
  }
  if (draft === "item-edit") {
    const ui = backlogUi(event.target.closest("[data-item]")?.dataset.item);
    if (!ui.editing) return;
    ui.editing.text = event.target.value;
    const button = event.target.closest(".sec-edit")?.querySelector('[data-action="item-edit-save"]');
    if (button) button.disabled = !editReady(ui.editing, ui.editing.required);
    return;
  }
  const field = event.target.closest("textarea[data-draft]");
  const decided = field?.closest(".decide");
  if (!decided) return;
  const ui = uiOf(decided.dataset.machine, decided.dataset.address);
  ui.note = field.value;
  const found = findTask(state.opened, decided.dataset.machine, decided.dataset.address);
  const option = ui.free ? undefined : (found ? shownQuestion(found.item) : null)?.options[ui.picked];
  const button = decided.querySelector('[data-action="answer"]');
  if (button) button.disabled = ui.busy || !answerReady(option, ui);
});

const writingToLead = () => document.activeElement?.id === "lead-say";
root.addEventListener("focusin", (event) => event.target.id === "lead-say" && requestAnimationFrame(scrollThreadToEnd));
globalThis.visualViewport?.addEventListener("resize", () => root.querySelector(".composer") && requestAnimationFrame(() => {
  fitComposer();
  if (writingToLead()) scrollThreadToEnd();
}));

root.addEventListener("keydown", (event) => {
  if (pageMode) return;
  if (event.key === "Enter" && event.target.id === "bl-q") {
    event.preventDefault();
    return event.target.blur();
  }
  if (event.key === "Enter" && event.target.matches?.("#email, #code")) {
    event.preventDefault();
    return root.querySelector(SUBMIT)?.click();
  }
  const pick = event.target.closest?.('[data-action="pick"], [data-action="pick-question"], [data-action="backlog-machine"], [data-action="rv-line"]');
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
  if (name === "item") backlogUi(route().params[0]).visit = nowSeconds();
  state.sheet = false;
  state.lead.picking = false;
  if (rendered === location.hash && !revealAfter) return;
  render();
  if (name === "lead") scrollThreadToEnd();
  else if (!(twoColumns() && (name === "task" || name === "card" || name === "plan" || name === "item" || name === "idea"))) scrollListToTop();
  const pending = revealAfter;
  revealAfter = null;
  if (pending) reveal(pending.selector, pending.offset);
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
  else if (twoColumns() && (route().name === "task" || route().name === "card")) go(`#/${state.from}`);
  else if (twoColumns() && (route().name === "item" || route().name === "idea")) go("#/backlog");
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
  if ("serviceWorker" in navigator) {
    watchShell(navigator.serviceWorker);
    watchUpdate(navigator.serviceWorker, () => {
      state.updated = true;
      render();
    });
  }
  await forgetDeviceKeys();
  const params = new URLSearchParams(location.search);
  state.notice = githubErrorText(params.get("github_error"));
  if (params.has("github_error")) history.replaceState(null, "", location.pathname + location.hash);
  state.info = await account.hub().catch(() => null);
  state.me = await signedInAccount(account);
  if (state.me && onward) return location.assign(onward);
  if (state.me && emailSignIn()) state.address = await account.email().catch(() => null);
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
