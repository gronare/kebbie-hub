import { approveFromLink, CONNECT_TEXT, connectText, finishConnect, forgetFragment, keptFragment, PairingFailed, safeNext, signedInDestination, unlockFailedText, withReauth } from "./connect.js";
import { emptyMarks, mergeMarks, openBoards } from "./board.js";
import { proposedName } from "./device-name.js";
import { signInText, signInWith, signUpText, signUpWith } from "./signin.js";
import { account, connectCalls, HubError } from "./hub.js";
import { put, get, del } from "./store.js";
import { allowsChanges, collectResults, CommandProblem, ensureDevice, forgetDevice, heldDevice, keepAllowsChanges, keepSent, loadDevice, loadSent, openHistory, pushFollows, sendCommand, verifiedDevices } from "./device.js";
import { fromB64, toB64 } from "../js/protocol.js";
import { COMMAND_TEXT, decisionBlock, standingOn } from "./decide.js";
import { categoriesKey, deviceWord, failureLine, keptCategories, namesSendHere, NOTICE_FAILURE, NOTIFY_RECORD, notifyPhase, notifyText, withCategory } from "./notify.js";
import { addFirstPasskey, keptUnlock, lockWhenDue, rememberedCredential, signedInAccount, signOut, stopNotifications, unlockAfter } from "./unlock.js";
import { supported as passkeysSupported } from "./passkey.js";
import { accountScreen, authScreen, connectScreen, deviceScreen, esc, githubErrorText, heading, nav, notificationsScreen, setupScreen, signupScreen } from "./screens.js";
import { currentPlan, machineNeedsPro, placeSheet, proCard } from "./pro.js";
import { address, codeScreen, connectEmailScreen, createPasskeyScreen, emailText, isAuthScreen, keyboardLift, signedInAs, typedCode } from "./email.js";

export const PAGES = ["/setup", "/signup", "/connect", "/account"];

export const SUBMIT = '.auth-actions .btn.primary, [data-action="save-email"]';

const top = (title, aside = "") => `<header class="top"><b>${esc(title)}</b><span>${esc(aside)}</span></header>`;
const deviceName = () => proposedName(navigator.userAgent);
const nowSeconds = () => Math.floor(Date.now() / 1000);

const EMAIL_ERRORS = {
  "invalid-email": "That doesn't look like an email address.",
};

const ERRORS = {
  unauthorized: "That link is used or expired. Ask for a new one.",
  "endpoint-not-allowed": "The hub doesn't send to this browser's push service.",
  "endpoint-not-https": "The hub doesn't send to this browser's push service.",
  "bad-endpoint": "This browser gave a subscription the hub can't use.",
  "bad-push-keys": "This browser gave a subscription the hub can't use.",
  "endpoint-taken": "Another device holds this browser's subscription. Turn notifications off and on there.",
  "unknown-device": "The hub doesn't know this device. Reload the page and try again.",
  ...CONNECT_TEXT,
};

const PROBLEMS = {
  "view-only": "Allow changes from this device first.",
  "no-device": "This device isn't registered yet. Allow changes from it first.",
};

export function notifyEnvironment(scope = globalThis) {
  const agent = scope.navigator?.userAgent ?? "";
  const ios = /iPhone|iPad|iPod/.test(agent) || (scope.navigator?.platform === "MacIntel" && scope.navigator?.maxTouchPoints > 1);
  const standalone = scope.navigator?.standalone === true || Boolean(scope.matchMedia?.("(display-mode: standalone)")?.matches);
  const supported = Boolean(scope.navigator?.serviceWorker) && "PushManager" in scope && "Notification" in scope;
  const word = deviceWord(agent);
  return { ios, standalone, supported, permission: supported ? scope.Notification.permission : "default", word: ios && word === "browser" ? "iPad" : word };
}

function message(error) {
  if (error instanceof CommandProblem) return PROBLEMS[error.reason] ?? error.reason;
  if (error instanceof PairingFailed) return connectText(error.reason);
  if (error instanceof HubError) return ERRORS[error.code] ?? `The hub answered ${error.status} (${error.code}).`;
  if (error?.name === "NotAllowedError") return "The passkey was cancelled.";
  return error?.message ?? String(error);
}

export function startPage(root, { showBoard = () => location.assign("/#/board"), given = null, back = null } = {}) {
  const listening = new AbortController();
  const params = new URLSearchParams(location.search);
  const sessionStore = (() => {
    try {
      return globalThis.sessionStorage ?? null;
    } catch {
      return null;
    }
  })();
  const fragment = location.pathname === "/connect" ? keptFragment(sessionStore, params.get("code") ?? "", location.hash) : null;
  if (location.hash && location.pathname === "/connect") history.replaceState(null, "", location.pathname + location.search);
  const githubError = githubErrorText(params.get("github_error"));
  if (params.has("github_error")) {
    params.delete("github_error");
    const rest = params.toString();
    history.replaceState(null, "", location.pathname + (rest ? `?${rest}` : "") + location.hash);
  }
  const state = { info: null, me: null, address: null, session: null, notice: githubError, busy: false, phase: "start", view: null, machines: [], left: false, codeFor: null, typed: "", emailStep: null, changingEmail: false, allowed: null, device: null, devices: null, opened: [], keys: new Map(), viewing: null, history: null, older: null, removing: false, notify: null, notifying: false, kept: null, deciding: false, pro: null };
  const hub = { account, credentials: navigator.credentials };
  const path = location.pathname;
  const code = params.get("code") ?? "";
  const unlockEnds = lockWhenDue(() => {
    state.session = { state: "locked", me: state.me };
    draw();
  });

  const liftKeyboard = keyboardLift();
  const render = (html) => {
    const inApp = path === "/account" && state.me;
    const auth = isAuthScreen(html);
    liftKeyboard(auth && html.includes("<input"));
    const focused = document.activeElement?.id && root.contains(document.activeElement) ? document.activeElement.id : null;
    const main = `<main class="screen page${inApp ? " in-app" : ""}${auth ? " auth" : ""}">${html}</main>`;
    const sheet = state.pro ? proCard({ plan: currentPlan(state.me?.plan, nowSeconds()), kind: "machine", machine: state.view?.name ?? null, busy: state.pro.busy, error: state.pro.error }) : "";
    root.innerHTML = (inApp ? `<div class="app">${nav({ active: "account", deskOnly: true })}${main}</div>` : main) + sheet;
    placeSheet(root);
    if (focused) document.getElementById(focused)?.focus({ preventScroll: true });
  };

  const fail = (error) => {
    state.busy = false;
    state.notice = message(error);
    draw();
  };

  const signingIn = signInWith(hub);

  const settle = (ceremony) => unlockAfter(hub, ceremony);

  async function signIn() {
    state.session = await settle(await signingIn.withPasskey(rememberedCredential()));
    state.me = state.session.me;
    state.notice = null;
  }

  async function signUp(ticket) {
    state.session = await unlockAfter(hub, await signUpWith(hub, ticket, deviceName()));
    state.me = state.session.me;
    state.notice = null;
  }

  const reauth = signIn;

  const marksKey = () => `marks:${state.me?.account}`;

  async function storedMarks() {
    try {
      return (await get(marksKey())) ?? emptyMarks();
    } catch {
      return emptyMarks();
    }
  }

  async function keepCounter(counter) {
    try {
      await put(marksKey(), mergeMarks(await storedMarks(), { list: counter }));
    } catch {}
  }

  const accountSignIn = () =>
    authScreen({
      intro: heading("See your account", emailSignIn() ? "Sign in to see your account." : "Sign in with your passkey to see your account."),
      actions: emailSignIn() ? `<a class="btn primary" href="/?next=%2Faccount">Sign in</a>` : `<button type="button" class="btn primary" data-action="sign-in">Sign in with passkey</button>`,
      notice: state.notice,
    });

  const keepsEmail = () => Boolean(state.info?.email);
  const emailSignIn = () => Boolean(state.info?.email_signin);
  const typedEmail = () => root.querySelector("#email")?.value.trim() ?? "";

  function setupPage() {
    return setupScreen({ phase: state.phase, needsSetup: state.info?.needs_setup, token: params.get("token"), github: Boolean(state.info?.github), notice: state.notice });
  }

  function signupPage() {
    return signupScreen({ signedIn: Boolean(state.me), next: safeNext(params.get("next")), invite: params.get("invite"), github: Boolean(state.info?.github), email: keepsEmail(), notice: state.notice });
  }

  function connectPage() {
    const name = state.view?.name ?? "this machine";
    const shown = { notice: state.notice, busy: state.busy };
    if (emailSignIn() && state.view && state.phase === "start" && passkeysSupported()) {
      if (!state.me && state.codeFor) return codeScreen({ purpose: "connect", email: state.codeFor, name, ...shown });
      if (!state.me) return connectEmailScreen({ name, code: state.view.code || code, github: Boolean(state.info?.github), email: state.typed, ...shown });
      if (state.me.key_access === false && !state.me.key) return createPasskeyScreen({ who: signedInAs(state.me, state.address), ...shown });
    }
    return connectScreen({ supported: passkeysSupported(), phase: state.phase, view: state.view, me: state.me, session: state.session, fragment, signupOpen: state.info?.signup === "open", github: Boolean(state.info?.github), email: keepsEmail(), machines: state.machines, notice: state.notice, busy: state.busy, code });
  }

  function accountPage() {
    if (!state.me) return accountSignIn();
    if (state.emailStep) return codeScreen({ purpose: "confirm", email: state.emailStep.email, notice: state.notice, busy: state.busy });
    const viewing = state.viewing && state.devices?.find((device) => device.id === state.viewing);
    if (viewing) return deviceScreen({ device: viewing, history: state.history ?? [], loading: state.history === null, older: state.older !== null, removing: state.removing, busy: state.busy, notice: state.notice, decisions: decisionsFor(viewing), now: nowSeconds() });
    const notify = state.info?.commands && state.notify ? { phase: state.notify.phase, categories: state.notify.categories, failure: failureLine(state.notify.failure, nowSeconds()), text: notifyText(state.notify.phase, { ios: state.notify.env.ios, word: state.notify.env.word }), senders: namesSendHere(state.opened.map((machine) => machine.name)).trim() } : null;
    if (notify && state.notifying) return notificationsScreen({ notify, notice: state.notice });
    return accountScreen({ notify, me: state.me, github: Boolean(state.info?.github), email: keepsEmail(), codes: emailSignIn(), changing: state.changingEmail, address: state.address, notice: state.notice, busy: state.busy, allowed: state.info?.commands ? state.allowed === true : null, devices: state.info?.commands ? state.devices : null, now: nowSeconds() });
  }

  const unlockedIdentity = () => (state.session?.state === "unlocked" ? state.session.identity : null);

  async function loadDevices() {
    const identity = unlockedIdentity();
    if (!identity || !state.info?.commands) return;
    const [listed, boards] = await Promise.all([account.devices(), account.boards().catch(() => null)]);
    const opened = boards ? await openBoards(boards, identity, nowSeconds(), await storedMarks()).catch(() => null) : null;
    state.opened = opened?.opened ?? [];
    state.keys = new Map((opened?.list?.machines ?? []).map((listed) => [listed.machine, listed.keys]));
    const devices = (await verifiedDevices(identity.key, listed.devices)).map((device) => {
      const standing = device.removed
        ? []
        : state.opened
            .map((machine) => ({ machine: machine.name, machineId: machine.machine, accepts: machine.board?.accepts ?? null, mine: standingOn(machine.board, state.device)?.state ?? null, ...(machine.board?.devices?.[device.id] ?? {}) }))
            .filter((one) => one.state && one.fingerprint === device.key);
      return { ...device, self: device.id === state.device?.id, standing };
    });
    state.devices = devices.sort((a, b) => Number(Boolean(a.removed)) - Number(Boolean(b.removed)) || Number(b.self) - Number(a.self));
  }

  async function historyPage(id, identity, before) {
    const listing = await account.deviceCommands(id, before);
    const opened = await openHistory(listing.commands, identity, (machine) => state.keys.get(machine) ?? null);
    const kept = await loadSent({ get }, state.me.account, nowSeconds());
    const local = new Map(kept.sent.map((one) => [one.id, one]));
    const names = new Map(state.opened.map((machine) => [machine.machine, machine.name]));
    const deviceNames = new Map((state.devices ?? []).map((device) => [device.id, device.name]));
    const rows = opened.map((entry) => ({ ...entry, machineName: names.get(entry.machine) ?? entry.machine.slice(-6), label: local.get(entry.id)?.label ?? null, short: entry.payload.head?.slice(0, 7) ?? null, targetName: entry.payload.type === "device-decision" ? (deviceNames.get(entry.payload.device) ?? null) : null }));
    return { rows, older: listing.cursor ?? null };
  }

  async function openDevice(id) {
    const identity = unlockedIdentity();
    Object.assign(state, { viewing: id, history: null, older: null, removing: false, notice: null });
    scrollTo(0, 0);
    draw();
    if (!identity) return;
    try {
      const page = await historyPage(id, identity, null);
      if (state.viewing !== id) return;
      Object.assign(state, { history: page.rows, older: page.older });
    } catch (error) {
      state.history = [];
      state.notice = message(error);
    }
    draw();
  }

  async function showOlder() {
    const identity = unlockedIdentity();
    const id = state.viewing;
    if (!identity || state.older === null || state.busy) return;
    state.busy = true;
    draw();
    try {
      const page = await historyPage(id, identity, state.older);
      if (state.viewing === id) Object.assign(state, { history: [...(state.history ?? []), ...page.rows], older: page.older });
    } catch (error) {
      state.notice = message(error);
    }
    state.busy = false;
    draw();
  }

  async function removeDevice() {
    const identity = unlockedIdentity();
    const removed = state.devices?.find((device) => device.id === state.viewing);
    if (!identity || !removed || state.busy) return;
    state.busy = true;
    state.notice = null;
    draw();
    try {
      await account.removeDevice(removed.id);
      if (removed.self) {
        await stopNotifications(account, { get, del });
        await forgetDevice({ del }, identity.id);
        state.device = null;
        if (state.notify) state.notify.phase = "off";
      }
      await loadDevices();
      Object.assign(state, { viewing: null, history: null, older: null, removing: false });
      scrollTo(0, 0);
    } catch (error) {
      state.notice = message(error);
    }
    state.busy = false;
    draw();
  }

  function latestDecision(machine, device) {
    const mine = (state.kept?.sent ?? []).filter((one) => one.kind === "device-decision" && one.machine === machine && one.target === device);
    return mine.sort((a, b) => b.at - a.at)[0] ?? null;
  }

  function decisionsFor(device) {
    return new Map(device.standing.map((one) => [one.machineId, decisionBlock({ standing: one, device, sent: latestDecision(one.machineId, device.id), self: device.self, allowed: state.allowed === true, busy: state.deciding, now: nowSeconds() })]));
  }

  async function decideDevice(machine, approve) {
    const identity = unlockedIdentity();
    const viewing = state.devices?.find((device) => device.id === state.viewing);
    const standing = viewing?.standing.find((one) => one.machineId === machine);
    const keys = state.keys.get(machine);
    if (!identity || !standing || !keys || state.deciding) return;
    state.deciding = true;
    state.notice = null;
    draw();
    try {
      const kept = state.kept ?? (await loadSent({ get }, identity.id, nowSeconds()));
      const payload = { type: "device-decision", device: viewing.id, fingerprint: standing.fingerprint, approve };
      const sent = await sendCommand({ calls: account, identity, device: state.device, allowed: state.allowed, machine: { id: machine, keys }, payload, at: nowSeconds(), meta: { machineName: standing.machine, target: viewing.id, targetName: viewing.name, approve } });
      state.kept = { ...kept, sent: [...kept.sent, sent] };
      await keepSent({ put }, identity.id, state.kept);
    } catch (error) {
      state.notice = message(error);
    }
    state.deciding = false;
    draw();
    follow();
  }

  let following = false;

  async function follow() {
    const identity = unlockedIdentity();
    if (following || !identity) return;
    following = true;
    const waiting = () => state.kept?.sent.some((one) => one.kind === "device-decision" && !one.result && nowSeconds() < one.exp);
    try {
      while (!state.left && waiting()) {
        const answered = state.kept.sent.filter((one) => one.result).length;
        const kept = await collectResults({ calls: account, identity, kept: state.kept, keysOf: (machine) => state.keys.get(machine) ?? null, now: nowSeconds(), wait: 25 });
        if (state.left) return;
        state.kept = kept;
        await keepSent({ put }, identity.id, kept);
        if (kept.sent.filter((one) => one.result).length > answered) await loadDevices().catch(() => undefined);
        draw();
      }
    } catch {
    } finally {
      following = false;
    }
  }

  async function workerRegistration() {
    const workers = navigator.serviceWorker;
    if (!(await workers.getRegistration("/"))) await workers.register("/sw.js", { type: "module", scope: "/", updateViaCache: "none" });
    return workers.ready;
  }

  async function currentSubscription() {
    const registration = await navigator.serviceWorker.getRegistration("/");
    return (await registration?.pushManager.getSubscription()) ?? null;
  }

  async function keepNotifyRecord(identity) {
    if (!identity || !state.device) return;
    await put(NOTIFY_RECORD, { account: identity.key, device: state.device.id }).catch(() => undefined);
  }

  async function loadNotify() {
    if (!state.info?.commands || !state.info?.vapid) {
      state.notify = null;
      return;
    }
    const env = notifyEnvironment();
    const reachable = env.supported && !(env.ios && !env.standalone);
    const subscription = reachable ? await currentSubscription().catch(() => null) : null;
    if (subscription && env.permission === "granted") await healPush(subscription);
    const listed = state.devices ? (state.devices.find((device) => device.self)?.push ?? null) : undefined;
    const kept = state.device ? await get(categoriesKey(state.device.id)).catch(() => null) : null;
    const subscribed = Boolean(subscription) && listed !== null;
    const failure = await get(NOTICE_FAILURE).catch(() => null);
    state.notify = { env, phase: notifyPhase({ ...env, subscribed }), categories: keptCategories(listed?.categories ?? kept), busy: false, failure };
    if (state.notify.phase === "on") await keepNotifyRecord(unlockedIdentity());
  }

  async function healPush(subscription) {
    const identity = unlockedIdentity();
    const record = await get(NOTIFY_RECORD).catch(() => null);
    if (!identity || record?.account?.account !== identity.id || record.device === state.device?.id) return;
    if (!state.device && !(await heldDevice({ get }, identity.id)).missing) return;
    const store = { get, put };
    try {
      if (state.device) await pushFollows({ store, calls: account, identity, device: state.device, subscription: async () => subscription });
      else state.device = await ensureDevice({ store, calls: account, identity, name: deviceName(), now: nowSeconds(), subscription: async () => subscription });
      await loadDevices();
    } catch {}
  }

  function toggleNotify(input) {
    const notify = state.notify;
    if (!notify || notify.busy) return;
    if (!input.checked) return turnOff();
    const identity = unlockedIdentity();
    if (!identity) {
      state.notice = "Unlock your boards to turn on notifications.";
      return draw();
    }
    const asked = Notification.permission === "granted" ? Promise.resolve("granted") : Notification.requestPermission();
    notify.busy = true;
    notify.phase = "asking";
    draw();
    return turnOn(asked, identity);
  }

  async function turnOn(asked, identity) {
    const notify = state.notify;
    try {
      const permission = await asked;
      notify.env.permission = permission;
      if (permission !== "granted") {
        notify.phase = permission === "denied" ? "denied" : "off";
        return;
      }
      state.device = await ensureDevice({ store: { get, put }, calls: account, identity, name: deviceName(), now: nowSeconds(), kept: state.device });
      const registration = await workerRegistration();
      let subscription = await registration.pushManager.getSubscription();
      const held = subscription?.options?.applicationServerKey;
      if (subscription && held && toB64(new Uint8Array(held)) !== state.info.vapid) {
        await subscription.unsubscribe();
        subscription = null;
      }
      subscription ??= await registration.pushManager.subscribe({ userVisibleOnly: true, applicationServerKey: fromB64(state.info.vapid) });
      const { endpoint, keys } = subscription.toJSON();
      await account.subscribePush(state.device.id, { endpoint, keys: { p256dh: keys.p256dh, auth: keys.auth }, categories: notify.categories });
      await keepNotifyRecord(identity);
      await put(categoriesKey(state.device.id), notify.categories).catch(() => undefined);
      notify.phase = "on";
      await loadDevices().catch(() => undefined);
    } catch (error) {
      notify.phase = "off";
      state.notice = message(error);
    } finally {
      notify.busy = false;
      draw();
    }
  }

  async function turnOff() {
    const notify = state.notify;
    notify.busy = true;
    draw();
    try {
      await (await currentSubscription())?.unsubscribe();
      if (state.device) await account.unsubscribePush(state.device.id);
      await del(NOTIFY_RECORD).catch(() => undefined);
      await loadDevices().catch(() => undefined);
    } catch (error) {
      state.notice = message(error);
    } finally {
      notify.phase = "off";
      notify.busy = false;
      draw();
    }
  }

  async function toggleCategory(input) {
    const notify = state.notify;
    if (!notify || !state.device) return;
    const before = notify.categories;
    notify.categories = withCategory(before, input.value, input.checked);
    await put(categoriesKey(state.device.id), notify.categories).catch(() => undefined);
    try {
      await account.setCategories(state.device.id, notify.categories);
    } catch (error) {
      if (error instanceof HubError && error.code === "not-subscribed") notify.phase = "off";
      else {
        notify.categories = before;
        state.notice = message(error);
      }
    }
    draw();
    root.querySelector(`input[value="${input.value}"]`)?.focus({ preventScroll: true });
  }

  function draw() {
    if (state.left) return;
    unlockEnds.watch(state.session);
    const html = { "/setup": setupPage, "/signup": signupPage, "/connect": connectPage, "/account": accountPage }[path]();
    render(html);
    document.title = `${{ "/setup": "Set up", "/signup": "Sign up", "/connect": "Connect", "/account": "Account" }[path]} · kebbie`;
  }

  async function loadView() {
    try {
      if (path === "/connect") await loadConnect();
      if (state.me && keepsEmail()) state.address = await account.email().catch(() => null);
      if (state.me && path === "/account") {
        state.allowed = await allowsChanges({ get }, state.me.account);
        state.device = (await loadDevice({ get }, state.me.account)) ?? state.device;
        await loadDevices().catch((error) => (state.notice = message(error)));
        await loadNotify().catch(() => undefined);
        if (unlockedIdentity()) {
          state.kept = await loadSent({ get }, state.me.account, nowSeconds());
          follow();
        }
      }
    } catch (error) {
      state.view = null;
      state.notice = message(error);
    }
  }

  async function loadConnect() {
    state.view = state.me ? await connectCalls.view(code) : await connectCalls.peek(code);
    if (!state.me) return;
    const listing = await account.boards().catch(() => null);
    state.machines = (listing?.boards ?? []).map((entry) => entry.name || entry.machine.slice(-6));
  }

  function askMachine() {
    return new Promise((resolve) => {
      state.pro = { resolve, busy: false, error: null };
      draw();
    });
  }

  async function startTrial() {
    if (!state.pro || state.pro.busy) return;
    Object.assign(state.pro, { busy: true, error: null });
    draw();
    try {
      const plan = await account.startTrial();
      state.me = { ...state.me, plan };
      const { resolve } = state.pro;
      state.pro = null;
      resolve(true);
    } catch (error) {
      Object.assign(state.pro, { busy: false, error: COMMAND_TEXT[error?.code] ?? message(error) });
      if (error?.code === "trial-used") state.me = await account.me().catch(() => state.me);
      draw();
    }
  }

  async function stopForPro() {
    const pro = state.pro;
    state.pro = null;
    await connectCalls.deny(code, true).catch(() => undefined);
    state.phase = "cancelled";
    pro?.resolve(false);
    draw();
  }

  function closeCard() {
    const pro = state.pro;
    state.pro = null;
    state.busy = false;
    pro?.resolve(false);
    draw();
  }

  async function connect() {
    if (state.me && machineNeedsPro(currentPlan(state.me.plan, nowSeconds()), state.machines.length) && !(await askMachine())) return;
    state.busy = true;
    state.notice = null;
    draw();
    const unlock = async () => settle(await signingIn.withPasskey(rememberedCredential(), { connect: code }));
    let unlocking = true;
    try {
      const result = await approveFromLink({
        fragment,
        unlock: async () => {
          const unlocked = await unlock();
          unlocking = false;
          state.session = unlocked;
          state.me = unlocked.me;
          return unlocked;
        },
        approve: (commitment) => {
          unlocking = false;
          return withReauth(() => connectCalls.approve({ code, commitment }), reauth);
        },
      });
      if (result.blocked) {
        state.busy = false;
        return draw();
      }
      const { approved, sealed } = result;
      state.view = { ...state.view, name: approved.name };
      forgetFragment(sessionStore, code);
      if (!approved.pairing || !sealed) {
        state.phase = "connected";
        return draw();
      }
      state.phase = "pairing";
      draw();
      const machine = await finishConnect({ hub: connectCalls, pairing: approved.pairing, identity: result.session.identity, fragment, nonce: sealed.nonce, marks: await storedMarks() });
      await keepCounter(machine.counter);
      state.left = true;
      unlockEnds.watch(null);
      showBoard(state.session, approved.name);
    } catch (error) {
      const said = unlocking ? unlockFailedText(error, state.view?.name ?? "this machine") : null;
      if (said) {
        state.busy = false;
        state.notice = said;
        return draw();
      }
      if (state.phase === "pairing") state.phase = "start";
      if (error instanceof HubError && error.code === "pro-required") {
        state.busy = false;
        state.me = await account.me().catch(() => state.me);
        if (await askMachine()) return connect();
        return;
      }
      fail(error);
    }
  }

  function ticket() {
    if (path === "/setup") return { setup: params.get("token") };
    if (path === "/connect") return { connect: code };
    return { invite: params.get("invite") };
  }

  async function createPasskey() {
    const email = keepsEmail() ? typedEmail() : "";
    state.busy = true;
    state.notice = null;
    draw();
    try {
      await signUp(ticket());
      if (email) await account.setEmail(email).catch(() => null);
      state.busy = false;
      if (path === "/setup") {
        state.phase = "done";
        return draw();
      }
      const next = path === "/signup" ? safeNext(params.get("next")) : null;
      if (next) return location.assign(next);
      await loadView();
      draw();
    } catch (error) {
      state.busy = false;
      state.notice = signUpText(error) ?? message(error);
      draw();
    }
  }

  async function emailStep(act) {
    state.busy = true;
    state.notice = null;
    draw();
    try {
      await act();
    } catch (error) {
      state.notice = emailText(error);
    } finally {
      state.busy = false;
    }
    draw();
  }

  function sendCode() {
    const typed = typedEmail();
    const email = address(typed);
    if (!email) {
      state.notice = typed ? "That doesn't look like an email address." : "Type your email first.";
      return draw();
    }
    state.typed = typed;
    state.codeFor = email;
    state.notice = null;
    draw();
    root.querySelector("#code")?.focus();
    return account.askCode(email, code).catch((error) => {
      state.codeFor = null;
      state.notice = emailText(error);
      draw();
    });
  }

  function enterCode() {
    const typed = typedCode(root.querySelector("#code")?.value);
    if (typed.length !== 6) {
      state.notice = "Type the 6 digits from the mail.";
      return draw();
    }
    return emailStep(async () => {
      const signed = await account.enterCode(state.codeFor, typed, code);
      state.me = signed.account;
      state.session = { state: "locked", me: state.me };
      state.address = await account.email().catch(() => null);
      state.codeFor = null;
      await loadView();
    });
  }

  function resendCode() {
    if (state.emailStep) return emailStep(async () => void (await account.setEmail(state.emailStep.email)));
    return emailStep(async () => void (await account.askCode(state.codeFor, code)));
  }

  function confirmEmail() {
    const typed = typedCode(root.querySelector("#code")?.value);
    if (typed.length !== 6) {
      state.notice = "Type the 6 digits from the mail.";
      return draw();
    }
    return emailStep(async () => {
      state.address = await account.confirmEmail(typed);
      state.emailStep = null;
    });
  }

  async function saveEmail(email) {
    if (emailSignIn()) {
      if (!email) return;
      return emailStep(async () => {
        const asked = await account.setEmail(email);
        if (asked.verified) state.address = asked;
        else state.emailStep = { email: asked.email };
        state.changingEmail = false;
      });
    }
    state.busy = true;
    state.notice = null;
    draw();
    try {
      state.address = email ? await account.setEmail(email) : (await account.removeEmail(), { email: null, verified: false, sent: false });
      state.changingEmail = false;
      state.busy = false;
      draw();
    } catch (error) {
      state.busy = false;
      state.notice = EMAIL_ERRORS[error?.code] ?? message(error);
      draw();
    }
  }

  async function addPasskey() {
    state.busy = true;
    state.notice = null;
    draw();
    try {
      state.session = await addFirstPasskey(hub, state.me, deviceName());
      state.me = state.session.me;
      state.busy = false;
      await loadView();
      draw();
    } catch (error) {
      state.busy = false;
      state.notice = signUpText(error) ?? message(error);
      draw();
    }
  }

  async function signOutClicked() {
    state.busy = true;
    state.notice = null;
    draw();
    try {
      await signOut(account);
    } finally {
      state.session = null;
      state.me = null;
      state.address = null;
      unlockEnds.watch(null);
    }
    state.left = true;
    location.replace("/");
  }

  async function signInClicked() {
    state.notice = null;
    try {
      await signIn();
      const next = path === "/signup" ? safeNext(params.get("next")) : null;
      if (next) return location.assign(next);
      await loadView();
      draw();
    } catch (error) {
      if (error?.name === "AbortError") return;
      state.notice = signInText(error);
      draw();
    }
  }

  root.addEventListener("click", (event) => {
    const home = back && event.target.closest?.('a[href="/"]');
    if (!home || state.left) return;
    event.preventDefault();
    back();
  }, { signal: listening.signal });

  root.addEventListener("click", async (event) => {
    if (state.left) return;
    const target = event.target.closest("[data-action]");
    if (!target) return;
    const action = target.dataset.action;
    if (action === "notifications" || action === "notifications-back") {
      event.preventDefault();
      Object.assign(state, { notifying: action === "notifications", notice: null });
      history.replaceState(null, "", action === "notifications" ? "#notifications" : location.pathname + location.search);
      scrollTo(0, 0);
      return draw();
    }
    if (action === "device" || action === "devices-back") {
      event.preventDefault();
      if (action === "device") return openDevice(target.dataset.id);
      Object.assign(state, { viewing: null, history: null, older: null, removing: false, notice: null });
      scrollTo(0, 0);
      return draw();
    }
    try {
      if (action === "pro-trial") return await startTrial();
      if (action === "pro-close") return await stopForPro();
      if (action === "pro-continue") return setTimeout(closeCard);
      if (action === "decide-device") return await decideDevice(target.dataset.machine, target.dataset.approve === "true");
      if (action === "device-older") return await showOlder();
      if (action === "device-remove") return await removeDevice();
      if (action === "device-remove-ask" || action === "device-remove-cancel") {
        state.removing = action === "device-remove-ask";
        state.notice = null;
        return draw();
      }
      if (action === "sign-in" || action === "other-passkey") return await signInClicked();
      if (action === "add-passkey") return await addPasskey();
      if (action === "sign-out") return await signOutClicked();
      if (action === "not-now") {
        state.session = { state: "github", me: state.me };
        return draw();
      }
      if (action === "remove-github") {
        await account.removeGitHub();
        state.me = await account.me();
      }
      if (action === "create-passkey") return await createPasskey();
      if (action === "change-email" || action === "keep-email") {
        state.changingEmail = action === "change-email";
        state.notice = null;
        draw();
        if (state.changingEmail) root.querySelector("#email")?.focus();
        return;
      }
      if (action === "save-email" || action === "remove-email") return await saveEmail(action === "save-email" ? typedEmail() : "");
      if (action === "connect") return await connect();
      if (action === "send-code") return sendCode();
      if (action === "enter-code") return await enterCode();
      if (action === "resend-code") return await resendCode();
      if (action === "confirm-email") return await confirmEmail();
      if (action === "other-email") {
        state.codeFor = null;
        state.emailStep = null;
        state.notice = null;
        return draw();
      }
      if (action === "cancel") {
        await connectCalls.deny(code);
        state.phase = "cancelled";
        return draw();
      }
      state.notice = null;
      draw();
    } catch (error) {
      fail(error);
    }
  }, { signal: listening.signal });

  root.addEventListener("change", async (event) => {
    if (state.left || !state.me) return;
    if (event.target.name === "notify") return toggleNotify(event.target);
    if (event.target.name === "notice-category") return toggleCategory(event.target);
    if (event.target.name !== "allow-changes") return;
    state.allowed = event.target.checked;
    await keepAllowsChanges({ put }, state.me.account, state.allowed).catch(() => (state.notice = "This browser can't keep the setting; it lasts until you close the page."));
    const identity = unlockedIdentity();
    if (state.allowed && identity) {
      try {
        state.device = await ensureDevice({ store: { get, put }, calls: account, identity, name: deviceName(), now: nowSeconds(), kept: state.device });
        await loadDevices();
      } catch (error) {
        state.notice = message(error);
      }
    }
    draw();
    root.querySelector('input[name="allow-changes"]')?.focus({ preventScroll: true });
  }, { signal: listening.signal });

  root.addEventListener("keydown", (event) => {
    if (event.key !== "Enter" || !event.target.matches?.("#email, #code")) return;
    event.preventDefault();
    root.querySelector(SUBMIT)?.click();
  }, { signal: listening.signal });

  const started = (async () => {
    if (given) {
      Object.assign(state, { info: given.info, me: given.me, session: given.session, address: given.address ?? null, device: given.device ?? null });
    } else {
      render(top("kebbie hub"));
      state.info = await account.hub().catch(() => null);
      state.me = await signedInAccount(account);
      const onward = state.me ? signedInDestination(path, params) : null;
      if (onward) return location.assign(onward);
      const kept = await keptUnlock(state.me);
      state.session = kept ?? (state.me ? { state: "locked", me: state.me } : null);
    }
    await loadView();
    draw();
    if (path === "/account" && location.hash === "#notifications" && state.notify) {
      state.notifying = true;
      draw();
    }
    const linked = path === "/account" && location.hash.startsWith("#device/") ? decodeURIComponent(location.hash.slice("#device/".length)) : null;
    if (linked && state.devices?.some((device) => device.id === linked)) await openDevice(linked);
  })();

  return {
    started,
    leave() {
      state.left = true;
      unlockEnds.watch(null);
      listening.abort();
      return { session: state.session, me: state.me, address: state.address, allowed: state.allowed, device: state.device, kept: state.kept };
    },
  };
}
