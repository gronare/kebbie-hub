import { arrange, findTask, followHubNames, highestRevs, isAsleep, openBoards } from "./board.js";
import { account, deviceClient, follow, HubError } from "./hub.js";
import { get, identity, loadAttempts, loadDevice, loadMachines, newDeviceKeys, put, recordAttempt, remove, saveDevice, saveMachines } from "./keystore.js";
import { failureText, pair, PairingFailed } from "./pairing.js";
import { assertionJSON, creationOptions, registrationJSON, requestOptions, supported as passkeysSupported } from "./passkey.js";

const root = document.getElementById("app");
const nowSeconds = () => Math.floor(Date.now() / 1000);
const state = { device: null, machines: [], opened: [], attempts: [], offline: false, fetchedAt: null, pairing: { phase: "enter" }, notice: null, push: null, selected: {}, hint: false };

const esc = (value) => String(value ?? "").replace(/[&<>"']/g, (char) => `&#${char.charCodeAt(0)};`);
const plural = (count, one, many) => `${count} ${count === 1 ? one : many}`;
const short = (sha) => (sha ?? "").slice(0, 7);
const kb = (bytes) => `${Math.max(1, Math.round(bytes / 1024))} kB`;

function when(seconds) {
  if (!seconds) return "aldrig";
  const date = new Date(seconds * 1000);
  const time = date.toLocaleTimeString("sv-SE", { hour: "2-digit", minute: "2-digit" });
  return date.toDateString() === new Date().toDateString() ? time : `${date.toLocaleDateString("sv-SE", { day: "numeric", month: "short" })} ${time}`;
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
  return `<nav class="tabbar">${tab("#/board", "Board", "board")}${tab("#/questions", `Frågor${questions ? ` ${questions}` : ""}`, "questions")}${tab("#/machines", "Maskiner", "machines")}</nav>`;
}

function top(title, aside = "") {
  return `<header class="top"><b>${esc(title)}</b><span>${aside}</span></header>`;
}

function taskRow(row) {
  const href = `#/task/${encodeURIComponent(row.machine)}/${encodeURIComponent(row.item.address)}`;
  const meta = [row.repo, row.item.summary].filter(Boolean).join(" · ");
  const you = row.item.next_for_you ? `<div class="you"><b>du:</b> ${esc(row.item.next_for_you)}</div>` : "";
  return `<a class="task" href="${href}"><div class="name">${esc(row.task)} <span class="machine">${esc(row.name)}</span></div><div class="meta">${esc(meta)}</div>${you}</a>`;
}

const PROBLEMS = {
  "unknown-machine": "okänd maskin, koppla den igen",
  "not-sealed-to-this-device": "boardet är inte förseglat till den här telefonen än",
  "revision-differs": "hubbens revision stämmer inte med maskinens",
  "bad-signature": "signaturen stämmer inte, visas inte",
  expired: "boardet är för gammalt",
  "not-yet-valid": "telefonens klocka går fel",
  "rolled-back": "äldre version än den senast lästa",
  "not-a-recipient": "boardet är inte förseglat till den här telefonen",
  undecryptable: "går inte att avkryptera",
};
const problemText = (code) => PROBLEMS[code] ?? `går inte att öppna (${code})`;

function installHint() {
  if (!state.hint) return "";
  return `<div class="hint"><div>Lägg kelpie på hemskärmen: tryck på <b>Dela</b> och välj <b>Lägg till på hemskärmen</b>. Då öppnas appen som en egen app och kan visa notiser.</div><button type="button" class="close" data-action="dismiss-hint" aria-label="Stäng">×</button></div>`;
}

function freshness() {
  const machines = plural(state.opened.length, "maskin", "maskiner");
  return state.offline ? "kan inte nå hubben just nu" : machines;
}

function boardScreen() {
  const view = arrange(state.opened, nowSeconds());
  const parts = [top("kelpie", esc(freshness())), installHint()];
  if (state.machines.length === 0) {
    parts.push(`<div class="center">Ingen maskin är kopplad än.</div><a class="btn primary" href="#/pair">Koppla maskin</a>`);
  } else if (state.opened.length === 0) {
    parts.push(`<div class="center">Ingen kopplad maskin har skickat något board än.</div>`);
  }
  for (const group of view.groups) {
    parts.push(`<h2 class="group g-${group.tone}">${esc(group.title)} · ${group.rows.length}</h2>`, ...group.rows.map(taskRow));
  }
  if (view.sleeping.length > 0) {
    parts.push(`<h2 class="group g-muted">Sover</h2>`);
    for (const machine of view.sleeping) {
      const working = machine.working ? `${plural(machine.working, "task", "tasks")} arbetade då` : "inget arbetade då";
      parts.push(`<div class="task asleep"><div class="name">${esc(machine.name)}</div><div class="meta">senast sedd ${esc(when(machine.lastSeen))} · ${working}</div></div>`);
    }
  }
  if (view.problems.length > 0) {
    parts.push(`<h2 class="group g-muted">Går inte att läsa</h2>`);
    for (const machine of view.problems) parts.push(`<div class="task asleep"><div class="name">${esc(machine.name)}</div><div class="meta">${esc(problemText(machine.problem))}</div></div>`);
  }
  return parts.join("") + tabbar("board");
}

function questionsScreen() {
  const { questions } = arrange(state.opened, nowSeconds());
  const list = questions.length ? questions.map(taskRow).join("") : `<div class="center">Inga frågor väntar på dig.</div>`;
  return top("Frågor", questions.length ? plural(questions.length, "väntar", "väntar") : "") + list + tabbar("questions");
}

function chips(key, options, recommend) {
  const selected = state.selected[key] ?? (recommend ? recommend - 1 : 0);
  const row = options.map((option, index) => `<button type="button" class="chip${index === selected ? " on" : ""}" data-action="choose" data-key="${esc(key)}" data-index="${index}" aria-pressed="${index === selected}">${esc(option.label)}</button>`).join("");
  const chosen = options[selected];
  const meaning = chosen ? `${esc(chosen.meaning ?? "")}${recommend && selected === recommend - 1 ? " · rekommenderat" : ""}` : "";
  return `<div class="chips pad">${row}</div><div class="center">${meaning}</div>`;
}

function questionBlock(found) {
  const { item } = found;
  if (item.question) {
    const question = item.question;
    const plan = question.plan ? `<h3 class="sec">Plan</h3><div class="plan"><div>Planen (${kb(question.plan.bytes)}) visas här i nästa steg. Den ligger krypterad hos maskinen.</div></div>` : "";
    const context = [found.repo, item.summary].filter(Boolean).join(" · ");
    return `<section class="task"><div class="name">${esc(question.text)}</div><div class="meta">${esc(context)} · frågade ${esc(when(question.at))}</div>${plan}</section>
      <h3 class="sec pad">Svar</h3>${chips(`${found.machine}|${item.address}|${question.id}`, question.options, question.recommend)}
      <button type="button" class="btn primary off" disabled>Svara · kommer i nästa steg</button>`;
  }
  const prompt = item.prompt;
  return `<section class="task"><div class="name">${esc(prompt.question)}</div></section>
    <h3 class="sec pad">Svar</h3>${chips(`${found.machine}|${item.address}|${prompt.id}`, prompt.options.map((option) => ({ label: option.label, meaning: option.takes_text ? "med text" : "" })), null)}
    <button type="button" class="btn primary off" disabled>Svara · kommer i nästa steg</button>`;
}

function cardBlock(found) {
  const { item } = found;
  const lines = [];
  if (item.card) {
    lines.push(item.card.current ? `<div class="meta ok">✓ aktuellt på ${esc(short(item.card.sha))}</div>` : `<div class="meta warn">kortet gäller ${esc(short(item.card.sha))}, HEAD har gått vidare</div>`);
    lines.push(`<h3 class="sec">Card</h3><div class="plan"><div>Kortet (${kb(item.card.blob.bytes)}) visas här i nästa steg. Det ligger krypterat hos maskinen.</div></div>`);
  }
  if (item.next_for_you) lines.push(`<div class="you"><b>du:</b> ${esc(item.next_for_you)}</div>`);
  const progress = [item.summary, item.report?.summary].filter(Boolean);
  if (progress.length) lines.push(`<h3 class="sec">Läge</h3><div class="plan">${progress.map((line) => `<div>${esc(line)}</div>`).join("")}</div>`);
  if (item.git) {
    const git = item.git;
    const facts = [`${git.ahead} före ${git.upstream}`, git.behind ? `${git.behind} efter` : null, git.uncommitted ? `${git.uncommitted} ändringar ej committade` : null, git.approved ? "godkänd för push" : null].filter(Boolean);
    lines.push(`<h3 class="sec">Commits · ${git.ahead}</h3><div class="plan"><div>${esc(facts.join(" · "))}</div>${item.head ? `<div class="mono">HEAD ${esc(short(item.head))}</div>` : ""}</div>`);
  }
  if (item.verdict) {
    const verdict = item.verdict;
    const text = verdict.running ? "granskning pågår" : verdict.verdict === "ok" ? `granskad, ok${verdict.sha ? ` på ${short(verdict.sha)}` : ""}` : verdict.verdict === "changes" ? `granskad, ${plural(verdict.findings ?? 0, "fynd", "fynd")}` : "inte granskad";
    lines.push(`<h3 class="sec">Granskning</h3><div class="plan"><div>${esc(text)}${verdict.stale ? " · inaktuell" : ""}</div></div>`);
  }
  if (item.ci) lines.push(`<h3 class="sec">CI</h3><div class="plan"><div>${item.ci.state === "running" ? "CI kör" : `CI föll${item.ci.failure ? `: ${esc(item.ci.failure)}` : ""}`}</div></div>`);
  if (item.push_waits) lines.push(`<div class="meta">${esc(item.push_waits)}</div>`);
  const card = lines.length ? `<section class="task">${lines.join("")}</section>` : "";
  const pushable = item.group === "READY TO PUSH" || item.card;
  const push = pushable && item.head ? `<button type="button" class="btn primary off" disabled>Pusha ${esc(short(item.head))} · kommer i nästa steg</button>` : "";
  return card + push;
}

function taskScreen([machine, address]) {
  const found = findTask(state.opened, machine, address);
  if (!found) return top("Task") + `<div class="center">Tasken finns inte längre på boardet.</div>` + tabbar("board");
  const asks = found.item.question || found.item.prompt;
  return top(found.task, esc(found.name)) + (asks ? questionBlock(found) : cardBlock(found)) + tabbar(asks ? "questions" : "board");
}

function pushSection() {
  const push = state.push;
  if (!push) return "";
  const text = {
    "needs-home-screen": "Notiser fungerar när kelpie ligger på hemskärmen.",
    unsupported: "Den här webbläsaren kan inte ta emot notiser.",
    denied: "Notiser är avstängda för kelpie i inställningarna.",
    on: "Notiser är på. Telefonen avkrypterar dem själv.",
    off: "Få en notis när en task behöver dig.",
  }[push.status];
  const button = push.status === "off" ? `<button type="button" class="btn primary" data-action="push-on">Slå på notiser</button>` : push.status === "on" ? `<button type="button" class="btn" data-action="push-off">Stäng av notiser</button>` : "";
  const error = push.error ? `<div class="center warn">${esc(push.error)}</div>` : "";
  return `<h3 class="sec">Notiser</h3><div class="center left">${text}</div>${button}${error}`;
}

function machinesScreen() {
  const now = nowSeconds();
  const rows = state.machines.map((machine) => {
    const opened = state.opened.find((candidate) => candidate.machine === machine.id);
    const name = machine.name || opened?.name || machine.id.slice(-6);
    const lastSeen = opened?.lastSeen ?? null;
    const awake = !isAsleep({ lastSeen }, now);
    const facts = opened?.board ? [`senast hörd ${when(lastSeen)}`, `kelpie ${opened.board.kelpie}`, `board rev ${opened.rev}`] : opened?.problem ? [problemText(opened.problem)] : ["inget board än"];
    return `<div class="task${awake ? "" : " asleep"}"><div class="name">${esc(name)} <span class="machine">${awake ? "vaken" : "sover"}</span></div><div class="meta">${esc(facts.join(" · "))}</div></div>`;
  });
  const attempts = state.attempts.length
    ? `<h3 class="sec">Kopplingsförsök</h3><div class="plan">${state.attempts.map((attempt) => `<div class="meta">${esc(when(attempt.at))} · ${esc(attempt.outcome === "admitted" ? `kopplad till ${state.machines.find((known) => known.id === attempt.machine)?.name ?? attempt.name ?? attempt.machine}` : failureText(attempt.outcome))}</div>`).join("")}</div>`
    : "";
  const device = state.device ? `<h3 class="sec">Den här telefonen</h3><div class="meta pad">${esc(state.device.name ?? "")} · ${esc(state.device.id)}</div>` : "";
  return top("Maskiner", esc(plural(state.machines.length, "kopplad", "kopplade"))) + rows.join("") + `<a class="btn primary" href="#/pair">Koppla maskin</a>` + pushSection() + attempts + device + tabbar("machines");
}

function pairScreen() {
  const pairing = state.pairing;
  const busy = ["working", "compare", "admitting"].includes(pairing.phase);
  const parts = [top("Koppla maskin"), `<div class="center">Kör <b>kelpie hub pair</b> på datorn och skriv koden den visar.</div>`];
  parts.push(`<form data-form="pair" class="stack"><label class="task field"><span class="meta">Kod</span><input name="code" class="code-input" autocomplete="off" autocapitalize="characters" spellcheck="false" inputmode="text" maxlength="12" placeholder="XXXX-XXXX" value="${esc(pairing.typed ?? "")}" ${busy ? "disabled" : ""} required></label><button class="btn primary" ${busy ? "disabled" : ""}>Koppla</button></form>`);
  if (pairing.phase === "working") parts.push(`<div class="center">Väntar på maskinen …</div>`);
  if (["compare", "admitting", "done"].includes(pairing.phase)) {
    parts.push(`<h3 class="sec">Jämför med terminalen</h3><div class="code" aria-live="polite">${esc(pairing.digits)}</div>`);
  }
  if (pairing.phase === "compare") {
    parts.push(`<div class="center"><b>Stämmer siffrorna med terminalen?</b> Svara likadant här och i terminalen. Skiljer de sig: svara n där och Stämmer inte här, något står mellan dig och maskinen.</div>`);
    parts.push(`<label class="task field"><span class="meta">Namn på maskinen</span><input name="machine-name" id="machine-name" autocomplete="off" maxlength="64" value="${esc(pairing.hubName)}"></label>`);
    parts.push(`<div class="pair-answer"><button type="button" class="btn" data-action="pair-no">Stämmer inte</button><button type="button" class="btn primary" data-action="pair-yes">Siffrorna stämmer</button></div>`);
  }
  if (pairing.phase === "admitting") parts.push(`<div class="center">Väntar på att du svarar y i terminalen …</div>`);
  if (pairing.phase === "done") parts.push(`<div class="center ok">Kopplad till ${esc(pairing.name)}. Boardet laddas.</div>`);
  if (pairing.phase === "failed") parts.push(`<div class="center warn" role="alert">${esc(failureText(pairing.reason))}</div>`);
  return parts.join("") + (state.machines.length ? tabbar("machines") : "");
}

function loginScreen(signUp) {
  const message = state.notice ? `<div class="center warn" role="alert">${esc(state.notice)}</div>` : "";
  if (!passkeysSupported()) return top("kelpie") + `<div class="center">Den här webbläsaren kan inte logga in med passkey.</div>`;
  if (signUp) {
    return (
      top("Skapa konto") +
      `<div class="center">Skriv inbjudningskoden du fått och välj ett användarnamn. Sedan lägger telefonen till en passkey.</div>
      <form data-form="signup" class="stack"><label class="task field"><span class="meta">Inbjudan</span><input name="invite" class="code-input" autocomplete="off" autocapitalize="characters" spellcheck="false" placeholder="XXXX-XXXX" required></label>
      <label class="task field"><span class="meta">Användarnamn</span><input name="handle" autocomplete="username webauthn" autocapitalize="none" spellcheck="false" pattern="[a-zA-Z][a-zA-Z0-9\\-]*" maxlength="32" required></label>
      <button class="btn primary">Skapa konto med passkey</button></form>${message}<a class="center link" href="#/login">Har du redan ett konto? Logga in</a>`
    );
  }
  return (
    top("kelpie") +
    `<div class="center">Logga in med din passkey. Telefonen får sina egna nycklar och läser boardet själv; hubben ser aldrig vad som står där.</div>
    <form data-form="login" class="stack"><label class="task field"><span class="meta">Användarnamn</span><input name="handle" autocomplete="username webauthn" autocapitalize="none" spellcheck="false" required></label>
    <button class="btn primary">Logga in med passkey</button></form>${message}<a class="center link" href="#/signup">Har du en inbjudan? Skapa konto</a>`
  );
}

function unsupportedScreen() {
  return top("kelpie") + `<div class="center">Den här webbläsaren saknar krypteringen appen behöver (Ed25519, X25519 och base64 i Uint8Array). Uppdatera till iOS 18.2 eller senare, eller en aktuell Chrome eller Firefox.</div>`;
}

function render() {
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
  document.title = name === "task" && params[1] ? `${params[1].split("@")[0]} · kelpie` : "kelpie";
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
    if (error instanceof HubError && error.code === "unknown-device") return forgetDevice("Hubben känner inte längre igen telefonen. Logga in och koppla igen.");
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
        if (error instanceof HubError && error.code === "unknown-device") return forgetDevice("Telefonen togs bort från hubben. Logga in och koppla igen.");
      }
      await new Promise((resolve) => setTimeout(resolve, delay));
      delay = Math.min(delay * 2, 60_000);
    }
  };
  loop();
}

const deviceName = () => (/iPhone/.test(navigator.userAgent) ? "iPhone" : /iPad/.test(navigator.userAgent) ? "iPad" : /Macintosh/.test(navigator.userAgent) ? "Mac" : "Webbläsare");

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
  if (error instanceof HubError) {
    return (
      {
        unauthorized: "Inbjudan eller användarnamnet stämmer inte.",
        "handle-taken": "Användarnamnet är upptaget.",
        "bad-handle": "Användarnamnet får bara ha små bokstäver, siffror och bindestreck och börja med en bokstav.",
        "rate-limited": "För många försök. Vänta en minut.",
        "not-found": "Det finns inget konto med det användarnamnet.",
      }[error.code] ?? `Hubben svarade ${error.status} (${error.code}).`
    );
  }
  if (error?.name === "NotAllowedError") return "Passkeyn avbröts.";
  return error?.message ?? String(error);
}

async function signUp(form) {
  const begun = await account.registerBegin(form.invite.value.trim(), form.handle.value.trim().toLowerCase());
  const credential = await navigator.credentials.create(creationOptions(begun.options));
  await account.registerFinish(begun.ceremony, registrationJSON(credential), deviceName());
  await becomeDevice();
}

async function logIn(form) {
  const begun = await account.loginBegin(form.handle.value.trim().toLowerCase());
  const credential = await navigator.credentials.get(requestOptions(begun.options));
  await account.loginFinish(begun.ceremony, assertionJSON(credential));
  await becomeDevice();
}

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
          state.pairing = { phase: "compare", typed, digits, hubName: offered.hubName ?? "" };
          render();
        }),
    });
    const name = machine.name ?? machine.hubName ?? machine.id.slice(-6);
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
    state.push = { ...(await pushStatus()), error: error?.name === "NotAllowedError" ? "Notiser tilläts inte." : hubMessage(error) };
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
    if (form.dataset.form === "login") await logIn(form);
    if (form.dataset.form === "signup") await signUp(form);
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
      localStorage.setItem("kelpie-hint-dismissed", "1");
    } catch {}
    render();
  } else if (action === "pair-yes") answerPairing?.({ matches: true, name: document.getElementById("machine-name")?.value ?? "" });
  else if (action === "pair-no") answerPairing?.({ matches: false });
  else if (action === "push-on") pushOn();
  else if (action === "push-off") pushOff();
});

root.addEventListener("input", (event) => {
  if (event.target.id === "machine-name") state.pairing.hubName = event.target.value;
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
  if (!cryptoReady()) return render();
  try {
    state.hint = isIos() && !isStandalone() && localStorage.getItem("kelpie-hint-dismissed") !== "1";
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
  if (!state.device) return;
  await refresh();
  startFeed();
  state.push = await pushStatus().catch(() => ({ status: "unsupported" }));
  render();
  setInterval(() => document.visibilityState === "visible" && refresh(), 60_000);
}

start();
