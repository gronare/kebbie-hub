import { arrange, findTask, followHubNames, highestRevs, isAsleep, openBoards } from "./board.js";
import { keepDeviceName, proposedName } from "./device-name.js";
import { addPasskey, autofillAvailable, NoPasskeyOffered, OFFER_TEXT, signInText, signInWith, signUpText } from "./signin.js";
import { account, deviceClient, follow, HubError } from "./hub.js";
import { get, identity, loadAttempts, loadDevice, loadMachines, newDeviceKeys, put, recordAttempt, remove, saveDevice, saveMachines } from "./keystore.js";
import { failureText, pair, PairingFailed } from "./pairing.js";
import { PAGES, startPage } from "./pages.js";
import { creationOptions, registrationJSON, supported as passkeysSupported } from "./passkey.js";

const root = document.getElementById("app");
const onPage = PAGES.includes(location.pathname);
const nowSeconds = () => Math.floor(Date.now() / 1000);
const state = { device: null, machines: [], opened: [], attempts: [], offline: false, fetchedAt: null, pairing: { phase: "enter" }, notice: null, byName: false, offerPasskey: false, push: null, selected: {}, hint: false };

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
  "unknown-machine": "unknown machine; connect it again",
  "not-sealed-to-this-device": "the board is not sealed to this device yet",
  "revision-differs": "the hub's revision differs from the machine's",
  "bad-signature": "the signature does not match; not shown",
  expired: "the board is too old",
  "not-yet-valid": "this device's clock is wrong",
  "rolled-back": "older than the board you last read",
  "not-a-recipient": "the board is not sealed to this device",
  undecryptable: "does not decrypt",
};
const problemText = (code) => PROBLEMS[code] ?? `does not open (${code})`;

function installHint() {
  if (!state.hint) return "";
  return `<div class="hint"><div>Add kebbie to your home screen: tap <b>Share</b>, then <b>Add to Home Screen</b>. It then opens as its own app and can show notifications.</div><button type="button" class="close" data-action="dismiss-hint" aria-label="Close">×</button></div>`;
}

function freshness() {
  const machines = plural(state.opened.length, "machine", "machines");
  return state.offline ? "can't reach the hub right now" : machines;
}

function boardScreen() {
  const view = arrange(state.opened, nowSeconds());
  const parts = [top("kebbie", esc(freshness())), installHint()];
  if (state.machines.length === 0) {
    parts.push(`<div class="center">No machine is connected yet. Run <b>kebbie hub connect</b> on your computer.</div><a class="btn primary" href="#/pair">Pair with a code</a>`);
  } else if (state.opened.length === 0) {
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

function pushSection() {
  const push = state.push;
  if (!push) return "";
  const text = {
    "needs-home-screen": "Notifications work once kebbie is on your home screen.",
    unsupported: "This browser can't receive notifications.",
    denied: "Notifications are off for kebbie in your settings.",
    on: "Notifications are on. This device decrypts them itself.",
    off: "Get a notification when a task needs you.",
  }[push.status];
  const button = push.status === "off" ? `<button type="button" class="btn primary" data-action="push-on">Turn on notifications</button>` : push.status === "on" ? `<button type="button" class="btn" data-action="push-off">Turn off notifications</button>` : "";
  const error = push.error ? `<div class="center warn">${esc(push.error)}</div>` : "";
  return `<h3 class="sec">Notifications</h3><div class="center left">${text}</div>${button}${error}`;
}

function machinesScreen() {
  const now = nowSeconds();
  const rows = state.machines.map((machine) => {
    const opened = state.opened.find((candidate) => candidate.machine === machine.id);
    const name = machine.name || opened?.name || machine.id.slice(-6);
    const lastSeen = opened?.lastSeen ?? null;
    const awake = !isAsleep({ lastSeen }, now);
    const facts = opened?.board ? [`last heard ${when(lastSeen)}`, `kebbie ${opened.board.kelpie}`, `board rev ${opened.rev}`] : opened?.problem ? [problemText(opened.problem)] : ["no board yet"];
    return `<div class="task${awake ? "" : " asleep"}"><div class="name">${esc(name)} <span class="machine">${awake ? "awake" : "asleep"}</span></div><div class="meta">${esc(facts.join(" · "))}</div></div>`;
  });
  const attempts = state.attempts.length
    ? `<h3 class="sec">Pairing attempts</h3><div class="plan">${state.attempts.map((attempt) => `<div class="meta">${esc(when(attempt.at))} · ${esc(attempt.outcome === "admitted" ? `paired with ${state.machines.find((known) => known.id === attempt.machine)?.name ?? attempt.name ?? attempt.machine}` : failureText(attempt.outcome))}</div>`).join("")}</div>`
    : "";
  const device = state.device ? `<h3 class="sec">This device</h3><div class="meta pad">${esc(state.device.name ?? "")} · ${esc(state.device.id)}</div>` : "";
  return top("Machines", esc(`${state.machines.length} connected`)) + rows.join("") + `<a class="btn primary" href="#/pair">Pair with a code</a>` + pushSection() + attempts + device + tabbar("machines");
}

function pairScreen() {
  const pairing = state.pairing;
  const busy = ["working", "compare", "admitting"].includes(pairing.phase);
  const parts = [top("Pair a machine"), passkeyOffer(), `<div class="center">Run <b>kebbie hub pair</b> on your computer and type the code it shows.</div>`];
  parts.push(`<form data-form="pair" class="stack"><label class="task field"><span class="meta">Code</span><input name="code" class="code-input" autocomplete="off" autocapitalize="characters" spellcheck="false" inputmode="text" maxlength="12" placeholder="XXXX-XXXX" value="${esc(pairing.typed ?? "")}" ${busy ? "disabled" : ""} required></label><button class="btn primary" ${busy ? "disabled" : ""}>Pair</button></form>`);
  if (pairing.phase === "working") parts.push(`<div class="center">Waiting for the machine …</div>`);
  if (["compare", "admitting", "done"].includes(pairing.phase)) {
    parts.push(`<h3 class="sec">Compare with your terminal</h3><div class="code" aria-live="polite">${esc(pairing.digits)}</div>`);
  }
  if (pairing.phase === "compare") {
    parts.push(`<div class="center"><b>Does your terminal show the same digits?</b> Answer the same here and there. If they differ, answer n there and Different here: something is between you and the machine.</div>`);
    parts.push(`<label class="task field"><span class="meta">Name of the machine</span><input name="machine-name" id="machine-name" autocomplete="off" maxlength="64" value="${esc(pairing.hubName)}"></label>`);
    parts.push(`<label class="task field"><span class="meta">Name of this device, as the machine lists it</span><input name="device-name" id="device-name" autocomplete="off" maxlength="64" value="${esc(pairing.deviceName)}"></label>`);
    parts.push(`<div class="pair-answer"><button type="button" class="btn" data-action="pair-no">Different</button><button type="button" class="btn primary" data-action="pair-yes">Same digits</button></div>`);
  }
  if (pairing.phase === "admitting") parts.push(`<div class="center">Waiting for you to answer y in the terminal …</div>`);
  if (pairing.phase === "done") parts.push(`<div class="center ok">Paired with ${esc(pairing.name)}. Loading the board.</div>`);
  if (pairing.phase === "failed") parts.push(`<div class="center warn" role="alert">${esc(failureText(pairing.reason))}</div>`);
  return parts.join("") + (state.machines.length ? tabbar("machines") : "");
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
    `<div class="center">Sign in with your passkey. This device gets its own keys and reads the board itself; the hub never sees what it says.</div>
    <button type="button" class="btn primary" data-action="sign-in">Sign in with passkey</button>${message}${fallback}<a class="center link" href="/signup">No account yet? Sign up</a>`
  );
}

function unsupportedScreen() {
  return top("kebbie") + `<div class="center">This browser lacks the cryptography the app needs (Ed25519, X25519 and base64 in Uint8Array). Update to iOS 18.2 or later, or a current Chrome or Firefox.</div>`;
}

function render() {
  if (onPage) return;
  const { name, params } = route();
  let html;
  if (!cryptoReady()) html = unsupportedScreen();
  else if (!state.device) html = loginScreen(name === "signup");
  else if (name === "pair") html = pairScreen();
  else if (state.machines.length === 0) html = pairScreen();
  else if (name === "questions") html = questionsScreen();
  else if (name === "machines") html = machinesScreen();
  else if (name === "task") html = taskScreen(params);
  else html = boardScreen();
  root.innerHTML = `<main class="screen">${html}</main>`;
  document.title = name === "task" && params[1] ? `${params[1].split("@")[0]} · kebbie` : "kebbie";
}

let retrying = null;

async function followNames(entries) {
  const followed = followHubNames(state.machines, entries ?? []);
  if (!followed) return;
  state.machines = followed;
  await saveMachines(followed);
}

async function refresh() {
  if (!state.device) return;
  let boards;
  try {
    boards = await deviceClient(state.device.token).boards();
    state.offline = false;
    state.fetchedAt = nowSeconds();
    await put("boards", { boards: boards.boards, at: state.fetchedAt });
  } catch (error) {
    if (error instanceof HubError && error.code === "unknown-device") return forgetDevice("The hub no longer knows this device. Sign in and pair again.");
    clearTimeout(retrying);
    retrying = setTimeout(refresh, 15_000);
    state.offline = true;
    const cached = await get("boards");
    if (!cached) return render();
    boards = cached;
    state.fetchedAt = cached.at;
  }
  await followNames(boards.boards);
  const highest = (await get("revs")) ?? {};
  state.opened = await openBoards(boards.boards, identity(state.device), state.machines, nowSeconds(), highest);
  await put("revs", highestRevs(state.opened, highest));
  render();
}

async function forgetDevice(message) {
  await remove("device");
  await saveMachines([]);
  state.device = null;
  state.machines = [];
  state.opened = [];
  state.notice = message;
  go("#/login");
}

let following = null;
function startFeed() {
  if (!state.device || following) return;
  const controller = new AbortController();
  following = controller;
  let delay = 2000;
  const loop = async () => {
    while (!controller.signal.aborted) {
      try {
        await follow(state.device.token, (name) => {
          delay = 2000;
          if (name === "board" || name === "machine-removed") refresh();
        }, controller.signal);
      } catch (error) {
        if (error instanceof HubError && error.code === "unknown-device") return forgetDevice("This device was removed from the hub. Sign in and pair again.");
      }
      await new Promise((resolve) => setTimeout(resolve, delay));
      delay = Math.min(delay * 2, 60_000);
    }
  };
  loop();
}

const deviceName = () => proposedName(navigator.userAgent);

async function becomeDevice() {
  const keys = await newDeviceKeys();
  const name = deviceName();
  const registered = await account.registerDevice(name);
  state.device = { id: registered.device, token: registered.token, name, keys, public: keys.public };
  await saveDevice(state.device);
  state.notice = null;
  go("#/pair");
}

function hubMessage(error) {
  if (error instanceof HubError) return `The hub answered ${error.status} (${error.code}).`;
  if (error?.name === "NotAllowedError") return "The passkey was cancelled.";
  return error?.message ?? String(error);
}

const signingIn = signInWith({ account, credentials: navigator.credentials });

async function signedIn(byName) {
  signingIn.stop();
  state.byName = false;
  state.offerPasskey = byName;
  await becomeDevice();
}

async function startAutofill() {
  if (state.device || !(await autofillAvailable())) return;
  try {
    await signingIn.autofill();
    await signedIn(false);
  } catch (error) {
    if (error?.name === "AbortError") return;
    state.notice = signInText(error);
    render();
  }
}

async function signUp(form) {
  const begun = await account.registerBegin({ invite: form.invite.value.trim(), handle: form.handle.value.trim().toLowerCase() });
  const credential = await navigator.credentials.create(creationOptions(begun.options));
  await account.registerFinish(begun.ceremony, registrationJSON(credential), deviceName());
  await becomeDevice();
}

async function signInWithPasskey() {
  try {
    await signingIn.withPasskey();
    await signedIn(false);
  } catch (error) {
    if (error instanceof NoPasskeyOffered) state.byName = true;
    state.notice = signInText(error);
    render();
    if (state.byName) startAutofill();
  }
}

async function logIn(form) {
  await signingIn.byName(form.handle.value);
  await signedIn(true);
}

async function offerPasskey(yes) {
  state.offerPasskey = false;
  if (yes) {
    try {
      await addPasskey({ account, credentials: navigator.credentials, name: deviceName() });
    } catch (error) {
      state.notice = signUpText(error) ?? hubMessage(error);
    }
  }
  render();
}

const passkeyOffer = () =>
  state.offerPasskey ? `<section class="task"><div class="meta">${esc(OFFER_TEXT)}</div><div class="pair-answer"><button type="button" class="btn" data-action="passkey-later">Not now</button><button type="button" class="btn primary" data-action="passkey-add">Add passkey</button></div></section>` : "";

let answerPairing = null;

async function startPairing(form) {
  const typed = form.code.value;
  state.pairing = { phase: "working", typed };
  render();
  const client = deviceClient(state.device.token);
  try {
    const machine = await pair({
      hub: client,
      publicKeys: state.device.public,
      typed,
      confirm: (digits, offered) =>
        new Promise((resolve) => {
          answerPairing = (answer) => {
            answerPairing = null;
            state.pairing = { ...state.pairing, phase: answer.matches ? "admitting" : "working" };
            render();
            resolve(answer);
          };
          state.pairing = { phase: "compare", typed, digits, hubName: offered.hubName ?? "", deviceName: state.device.name ?? deviceName() };
          render();
        }),
    });
    const name = machine.name ?? machine.hubName ?? machine.id.slice(-6);
    state.device = await keepDeviceName(client, state.device, state.pairing.deviceName, saveDevice).catch(() => state.device);
    state.machines = [...state.machines.filter((known) => known.id !== machine.id), { id: machine.id, keys: machine.keys, name: machine.name, pairedAt: nowSeconds() }];
    await saveMachines(state.machines);
    await put("revs", { ...((await get("revs")) ?? {}), [machine.id]: 0 });
    state.attempts = await recordAttempt({ at: nowSeconds(), machine: machine.id, name, outcome: "admitted" });
    state.pairing = { ...state.pairing, phase: "done", name };
    render();
    startFeed();
    await refresh();
    setTimeout(() => route().name === "pair" && go("#/board"), 1500);
  } catch (error) {
    answerPairing = null;
    const reason = error instanceof PairingFailed ? error.reason : error instanceof HubError ? error.code : "failed";
    state.attempts = await recordAttempt({ at: nowSeconds(), machine: error.machine ?? null, outcome: reason });
    state.pairing = { phase: "failed", typed, reason };
    render();
  }
}

async function pushStatus() {
  if (!("serviceWorker" in navigator) || !("PushManager" in window) || !("Notification" in window)) {
    return { status: isIos() && !isStandalone() ? "needs-home-screen" : "unsupported" };
  }
  if (Notification.permission === "denied") return { status: "denied" };
  const registration = await navigator.serviceWorker.getRegistration();
  if (!registration) return { status: "unsupported" };
  return { status: (await registration.pushManager.getSubscription()) ? "on" : "off" };
}

async function pushOn() {
  const client = deviceClient(state.device.token);
  try {
    const registration = await navigator.serviceWorker.ready;
    const { key } = await client.vapid();
    const subscription = await registration.pushManager.subscribe({ userVisibleOnly: true, applicationServerKey: Uint8Array.fromBase64(key, { alphabet: "base64url" }) });
    await client.subscribePush(subscription.toJSON());
    state.push = { status: "on" };
  } catch (error) {
    state.push = { ...(await pushStatus()), error: error?.name === "NotAllowedError" ? "Notifications were not allowed." : hubMessage(error) };
  }
  render();
}

async function pushOff() {
  try {
    const registration = await navigator.serviceWorker.ready;
    await (await registration.pushManager.getSubscription())?.unsubscribe();
    await deviceClient(state.device.token).unsubscribePush();
    state.push = { status: "off" };
  } catch (error) {
    state.push = { ...(await pushStatus()), error: hubMessage(error) };
  }
  render();
}

root.addEventListener("submit", async (event) => {
  const form = event.target.closest("form[data-form]");
  if (!form) return;
  event.preventDefault();
  const button = form.querySelector("button");
  if (button) button.disabled = true;
  try {
    if (form.dataset.form === "pair") return await startPairing(form);
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
  } else if (action === "pair-yes") answerPairing?.({ matches: true, name: document.getElementById("machine-name")?.value ?? "" });
  else if (action === "pair-no") answerPairing?.({ matches: false });
  else if (action === "sign-in") signInWithPasskey();
  else if (action === "by-name") {
    state.byName = true;
    state.notice = null;
    render();
    startAutofill();
  } else if (action === "passkey-add") offerPasskey(true);
  else if (action === "passkey-later") offerPasskey(false);
  else if (action === "push-on") pushOn();
  else if (action === "push-off") pushOff();
});

root.addEventListener("input", (event) => {
  if (event.target.id === "machine-name") state.pairing.hubName = event.target.value;
  if (event.target.id === "device-name") state.pairing.deviceName = event.target.value;
});

addEventListener("hashchange", () => {
  if (route().name === "pair" && !["working", "compare", "admitting"].includes(state.pairing.phase)) state.pairing = { phase: "enter" };
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
  state.device = (await loadDevice()) ?? null;
  state.machines = await loadMachines();
  // The boards cached by the last read hold the names the phone saw then, before any rename since.
  await followNames((await get("boards"))?.boards);
  state.attempts = await loadAttempts();
  render();
  if (!state.device) return startAutofill();
  await refresh();
  startFeed();
  state.push = await pushStatus().catch(() => ({ status: "unsupported" }));
  render();
  setInterval(() => document.visibilityState === "visible" && refresh(), 60_000);
}

start();
