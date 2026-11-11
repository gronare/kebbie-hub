import { approveFromLink, CONNECT_TEXT, connectText, finishConnect, forgetFragment, keptFragment, PairingFailed, safeNext, signedInDestination, unlockFailedText, withReauth } from "./connect.js";
import { emptyMarks, mergeMarks, openBoards } from "./board.js";
import { proposedName } from "./device-name.js";
import { signInText, signInWith, signUpText, signUpWith } from "./signin.js";
import { account, connectCalls, HubError } from "./hub.js";
import { put, get } from "./store.js";
import { allowsChanges, ensureDevice, keepAllowsChanges, loadDevice, loadSent, openHistory } from "./device.js";
import { verifyDeviceCert } from "../js/protocol.js";
import { addFirstPasskey, keptUnlock, lockWhenDue, rememberedCredential, signedInAccount, signOut, unlockAfter } from "./unlock.js";
import { supported as passkeysSupported } from "./passkey.js";
import { accountScreen, authScreen, connectScreen, deviceScreen, esc, githubErrorText, heading, nav, setupScreen, signupScreen } from "./screens.js";
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
  ...CONNECT_TEXT,
};

function message(error) {
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
  const state = { info: null, me: null, address: null, session: null, notice: githubError, busy: false, phase: "start", view: null, machines: [], left: false, codeFor: null, typed: "", emailStep: null, changingEmail: false, allowed: null, device: null, devices: null, opened: [], keys: new Map(), viewing: null, history: null };
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
    root.innerHTML = inApp ? `<div class="app">${nav({ active: "account", deskOnly: true })}${main}</div>` : main;
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
    if (viewing) return deviceScreen({ device: viewing, history: state.history ?? [], loading: state.history === null, notice: state.notice, now: nowSeconds() });
    return accountScreen({ me: state.me, github: Boolean(state.info?.github), email: keepsEmail(), codes: emailSignIn(), changing: state.changingEmail, address: state.address, notice: state.notice, busy: state.busy, allowed: state.info?.commands ? state.allowed === true : null, devices: state.info?.commands ? state.devices : null, now: nowSeconds() });
  }

  const unlockedIdentity = () => (state.session?.state === "unlocked" ? state.session.identity : null);

  async function loadDevices() {
    const identity = unlockedIdentity();
    if (!identity || !state.info?.commands) return;
    const [listed, boards] = await Promise.all([account.devices(), account.boards().catch(() => null)]);
    const opened = boards ? await openBoards(boards, identity, nowSeconds(), await storedMarks()).catch(() => null) : null;
    state.opened = opened?.opened ?? [];
    state.keys = new Map((opened?.list?.machines ?? []).map((listed) => [listed.machine, listed.keys]));
    const devices = [];
    for (const row of listed.devices) {
      if (!(await verifyDeviceCert(identity.key, row.cert))) continue;
      const standing = state.opened
        .map((machine) => ({ machine: machine.name, ...(machine.board?.devices?.[row.device] ?? {}) }))
        .filter((one) => one.state && one.fingerprint === row.fingerprint);
      devices.push({ id: row.device, name: row.name, kind: row.kind, self: row.device === state.device?.id, added: row.created_at, seen: row.last_seen_at, key: row.fingerprint, standing });
    }
    state.devices = devices.sort((a, b) => Number(b.self) - Number(a.self));
  }

  async function openDevice(id) {
    const identity = unlockedIdentity();
    state.viewing = id;
    state.history = null;
    state.notice = null;
    scrollTo(0, 0);
    draw();
    if (!identity) return;
    try {
      const listing = await account.deviceCommands(id);
      const opened = await openHistory(listing.commands, identity, (machine) => state.keys.get(machine) ?? null);
      const kept = await loadSent({ get }, state.me.account, nowSeconds());
      const local = new Map(kept.sent.map((one) => [one.id, one]));
      const names = new Map(state.opened.map((machine) => [machine.machine, machine.name]));
      if (state.viewing !== id) return;
      state.history = opened.map((entry) => ({ ...entry, machineName: names.get(entry.machine) ?? entry.machine.slice(-6), label: local.get(entry.id)?.label ?? null, short: entry.payload.head?.slice(0, 7) ?? null }));
    } catch (error) {
      state.history = [];
      state.notice = message(error);
    }
    draw();
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
        state.device = await loadDevice({ get }, state.me.account);
        await loadDevices().catch((error) => (state.notice = message(error)));
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

  async function connect() {
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
    if (action === "device" || action === "devices-back") {
      event.preventDefault();
      if (action === "device") return openDevice(target.dataset.id);
      state.viewing = null;
      state.history = null;
      state.notice = null;
      scrollTo(0, 0);
      return draw();
    }
    try {
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
    if (state.left || event.target.name !== "allow-changes" || !state.me) return;
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
      Object.assign(state, { info: given.info, me: given.me, session: given.session, address: given.address ?? null });
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
  })();

  return {
    started,
    leave() {
      state.left = true;
      unlockEnds.watch(null);
      listening.abort();
      return { session: state.session, me: state.me, address: state.address, allowed: state.allowed, device: state.device };
    },
  };
}
