import { approveFromLink, CONNECT_TEXT, connectText, finishConnect, forgetFragment, keptFragment, PairingFailed, planLine, safeNext, signedInDestination, unlockFailedText, withReauth } from "./connect.js";
import { emptyMarks, mergeMarks } from "./board.js";
import { proposedName } from "./device-name.js";
import { signInText, signInWith, signUpText, signUpWith } from "./signin.js";
import { account, admin, connectCalls, HubError } from "./hub.js";
import { put, get } from "./store.js";
import { addFirstPasskey, rememberedCredential, unlockAfter } from "./unlock.js";
import { supported as passkeysSupported } from "./passkey.js";
import { accountScreen, connectScreen, esc, githubErrorText, nav, setupScreen, signupScreen } from "./screens.js";

export const PAGES = ["/setup", "/signup", "/connect", "/admin", "/account"];

const top = (title, aside = "") => `<header class="top"><b>${esc(title)}</b><span>${esc(aside)}</span></header>`;
const say = (text, tone = "") => `<div class="center${tone ? ` ${tone}` : ""}"${tone === "warn" ? ' role="alert"' : ""}>${text}</div>`;
const when = (seconds) => (seconds ? new Date(seconds * 1000).toLocaleString("en-GB", { day: "numeric", month: "short", hour: "2-digit", minute: "2-digit" }) : "never");
const deviceName = () => proposedName(navigator.userAgent);

const ERRORS = {
  unauthorized: "That link is used or expired. Ask for a new one.",
  "not-owner": "Only the hub's owner sees this page.",
  ...CONNECT_TEXT,
};

function message(error) {
  if (error instanceof PairingFailed) return connectText(error.reason);
  if (error instanceof HubError) return ERRORS[error.code] ?? `The hub answered ${error.status} (${error.code}).`;
  if (error?.name === "NotAllowedError") return "The passkey was cancelled.";
  return error?.message ?? String(error);
}

export function startPage(root, { showBoard = () => location.assign("/#/board") } = {}) {
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
  const state = { info: null, me: null, session: null, notice: githubError, busy: false, phase: "start", view: null, machines: [], tab: "overview", left: false };
  const hub = { account, credentials: navigator.credentials };
  const path = location.pathname;
  const code = params.get("code") ?? "";

  const render = (html) => {
    const inApp = path === "/account" && state.me;
    const main = `<main class="screen page${inApp ? " in-app" : ""}">${html}</main>`;
    root.innerHTML = inApp ? `<div class="app">${nav({ active: "account", deskOnly: true })}${main}</div>` : main;
  };

  const fail = (error) => {
    state.busy = false;
    state.notice = message(error);
    draw();
  };

  const signingIn = signInWith(hub);

  async function settle(ceremony) {
    if (path === "/admin") {
      const { unlock, ...me } = ceremony.signedIn;
      return { state: "locked", me };
    }
    return unlockAfter(hub, ceremony);
  }

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

  const signInBlock = (purpose) => `${say(esc(purpose))}<button type="button" class="btn primary" data-action="sign-in">Sign in with passkey</button>${state.notice ? say(esc(state.notice), "warn") : ""}`;

  function setupPage() {
    return setupScreen({ phase: state.phase, needsSetup: state.info?.needs_setup, token: params.get("token"), github: Boolean(state.info?.github), notice: state.notice });
  }

  function signupPage() {
    return signupScreen({ signedIn: Boolean(state.me), next: safeNext(params.get("next")), invite: params.get("invite"), github: Boolean(state.info?.github), notice: state.notice });
  }

  function planBlock() {
    const line = planLine(state.me?.plan, state.me?.billing_url);
    if (!line) return "";
    return say(`${esc(line.text)}${line.url ? ` <a class="link" href="${esc(line.url)}">Start a subscription</a>` : ""}`, "warn");
  }

  function connectPage() {
    return connectScreen({ supported: passkeysSupported(), phase: state.phase, view: state.view, me: state.me, session: state.session, fragment, signupOpen: state.info?.signup === "open", github: Boolean(state.info?.github), machines: state.machines, plan: planBlock(), notice: state.notice, busy: state.busy, code });
  }

  function accountPage() {
    if (!state.me) return top("Account") + signInBlock("Sign in to see your account.");
    return accountScreen({ me: state.me, github: Boolean(state.info?.github), notice: state.notice });
  }

  const TABS = [
    ["overview", "Overview"],
    ["accounts", "Accounts"],
    ["machines", "Machines"],
    ["reports", "Bug reports"],
    ["signals", "Error signals"],
  ];

  function planText(plan) {
    if (!state.info?.plans) return "";
    const until = plan.until ? ` until ${when(plan.until)}` : "";
    return plan.entitled ? `plan active${until}` : `plan ${plan.status}${until}`;
  }

  function adminBody() {
    const view = state.view;
    if (!view) return say("Loading …");
    if (state.tab === "overview") {
      const tile = (count, label) => `<div class="task tile"><div class="count">${esc(count)}</div><div class="meta">${esc(label)}</div></div>`;
      return `<div class="tiles">${tile(view.accounts, "accounts")}${tile(view.machines, "machines")}${tile(view.devices, "devices")}${tile(view.reports, "reports to read")}${tile(view.quarantined, "reports in quarantine")}${tile(view.signals_this_week, "error signals this week")}</div>`;
    }
    if (state.tab === "accounts") {
      const invite = state.invite ? `<section class="task"><div class="meta">Send this link. It signs up one person, within 7 days.</div><div class="mono">${esc(state.invite.url)}</div></section>` : "";
      const rows = view.accounts.map((row) => {
        const facts = [row.email, `since ${when(row.created_at)}`, `${row.machines} machines`, `${row.devices} devices`, `seen ${when(row.last_seen)}`, planText(row.plan)].filter(Boolean).join(" · ");
        const action = row.owner ? `<span class="machine">owner</span>` : row.deactivated_at ? `<button type="button" class="btn small" data-action="activate" data-id="${esc(row.account)}">Activate</button>` : `<button type="button" class="btn small" data-action="deactivate" data-id="${esc(row.account)}">Deactivate</button>`;
        return `<div class="task${row.deactivated_at ? " asleep" : ""}"><div class="row"><div class="name mono">${esc(row.account)}</div>${action}</div><div class="meta">${esc(facts)}</div></div>`;
      });
      return `<button type="button" class="btn primary" data-action="invite">Invite by link</button>${invite}${rows.join("")}`;
    }
    if (state.tab === "machines") {
      if (!view.machines.length) return say("No machine has connected yet.");
      return view.machines.map((row) => `<div class="task"><div class="row"><div class="name">${esc(row.name)}</div><button type="button" class="btn small" data-action="remove-machine" data-id="${esc(row.machine)}" data-name="${esc(row.name)}">Remove</button></div><div class="meta">${esc([row.owner ?? "no account", `${row.devices} devices`, `joined ${when(row.joined_at)}`, `seen ${when(row.last_seen)}`].join(" · "))}</div></div>`).join("");
    }
    if (state.tab === "reports") {
      const toggle = `<div class="chips pad"><button type="button" class="chip${state.quarantine ? "" : " on"}" data-action="reports-allowed">To read</button><button type="button" class="chip${state.quarantine ? " on" : ""}" data-action="reports-quarantine">Quarantine</button></div>`;
      if (!view.reports.length) return toggle + say(state.quarantine ? "Nothing waits in quarantine." : "No report waits for you.");
      return toggle + view.reports.map((row) => {
        const actions = state.quarantine ? `<button type="button" class="btn small" data-action="block" data-key="${esc(row.key)}">Block</button><button type="button" class="btn small primary" data-action="allow" data-key="${esc(row.key)}">Allow</button>` : `<button type="button" class="btn small" data-action="ack" data-id="${esc(row.id)}">Handled</button>`;
        return `<div class="task"><div class="name">${esc(row.report.title)}</div><div class="meta">${esc(`${row.report.version} · ${row.report.os} · ${when(row.received_at)} · key ${row.key.slice(0, 8)}`)}</div><pre class="report">${esc(row.report.text)}</pre><div class="row-actions">${actions}</div></div>`;
      }).join("");
    }
    if (!view.signals.length) return say("No error signals in the last 7 days.");
    return view.signals.map((row) => `<div class="task"><div class="row"><div class="name">${esc(`${row.component} ${row.kind}`)}</div><span class="machine">${esc(row.count)}</span></div><div class="meta">${esc([new Date(row.day * 86_400_000).toISOString().slice(0, 10), `kebbie ${row.version}${row.commit ? ` (${row.commit})` : ""}`, `${row.os} ${row.arch}`, row.vetted ? null : "unvetted"].filter(Boolean).join(" · "))}</div></div>`).join("");
  }

  function adminPage() {
    if (!state.me) return top("Admin") + signInBlock("Sign in as the hub's owner.");
    if (!state.me.owner) return top("Admin") + say("Only the hub's owner sees this page.");
    const tabs = `<nav class="tabs">${TABS.map(([key, label]) => `<a href="#${key}" class="${state.tab === key ? "on" : ""}">${esc(label)}</a>`).join("")}</nav>`;
    return top("Admin") + tabs + (state.notice ? say(esc(state.notice), "warn") : "") + adminBody();
  }

  function draw() {
    if (state.left) return;
    const html = { "/setup": setupPage, "/signup": signupPage, "/connect": connectPage, "/admin": adminPage, "/account": accountPage }[path]();
    render(html);
    document.title = `${{ "/setup": "Set up", "/signup": "Sign up", "/connect": "Connect", "/admin": "Admin", "/account": "Account" }[path]} · kebbie`;
  }

  async function loadView() {
    try {
      if (path === "/connect") await loadConnect();
      if (path === "/admin" && state.me?.owner) await loadAdmin();
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

  async function loadAdmin() {
    state.tab = TABS.some(([key]) => key === location.hash.slice(1)) ? location.hash.slice(1) : "overview";
    const load = {
      overview: () => admin.overview(),
      accounts: () => admin.accounts(),
      machines: () => admin.machines(),
      reports: () => admin.reports(state.quarantine),
      signals: () => admin.signals(),
    }[state.tab];
    state.view = await load();
  }

  async function connect() {
    state.busy = true;
    state.notice = null;
    draw();
    const unlock = async () => settle(await signingIn.withPasskey(rememberedCredential()));
    let unlocking = true;
    try {
      const result = await approveFromLink({
        fragment,
        session: state.session,
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
      const machine = await finishConnect({ hub: connectCalls, pairing: approved.pairing, identity: state.session.identity, fragment, nonce: sealed.nonce, marks: await storedMarks() });
      await keepCounter(machine.counter);
      state.left = true;
      showBoard(state.session, approved.name);
    } catch (error) {
      const said = unlocking ? unlockFailedText(error, state.view?.name ?? "this machine") : null;
      if (said) {
        state.busy = false;
        state.notice = said;
        return draw();
      }
      if (error instanceof HubError && error.status === 402) state.me = { ...state.me, plan: { entitled: false }, billing_url: error.url };
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
    state.busy = true;
    state.notice = null;
    draw();
    try {
      await signUp(ticket());
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

  root.addEventListener("click", async (event) => {
    if (state.left) return;
    const target = event.target.closest("[data-action]");
    if (!target) return;
    const action = target.dataset.action;
    try {
      if (action === "sign-in" || action === "other-passkey") return await signInClicked();
      if (action === "add-passkey") return await addPasskey();
      if (action === "not-now") {
        state.session = { state: "github", me: state.me };
        return draw();
      }
      if (action === "remove-github") {
        await account.removeGitHub();
        state.me = await account.me();
      }
      if (action === "create-passkey") return await createPasskey();
      if (action === "connect") return await connect();
      if (action === "cancel") {
        await connectCalls.deny(code);
        state.phase = "cancelled";
        return draw();
      }
      if (action === "invite") state.invite = await admin.invite();
      if (action === "activate") await admin.setActive(target.dataset.id, true);
      if (action === "deactivate") await withReauth(() => admin.setActive(target.dataset.id, false), reauth);
      if (action === "remove-machine") {
        if (!confirm(`Remove ${target.dataset.name}? Its token stops working and its devices stop seeing its board.`)) return;
        await withReauth(() => admin.removeMachine(target.dataset.id), reauth);
      }
      if (action === "allow") await admin.decideKey(target.dataset.key, "allow");
      if (action === "block") await withReauth(() => admin.decideKey(target.dataset.key, "block"), reauth);
      if (action === "ack") await admin.ack(target.dataset.id);
      if (action === "reports-allowed" || action === "reports-quarantine") state.quarantine = action === "reports-quarantine";
      if (path === "/admin") await loadAdmin();
      state.notice = null;
      draw();
    } catch (error) {
      fail(error);
    }
  });

  addEventListener("hashchange", async () => {
    if (path !== "/admin") return;
    state.view = null;
    state.notice = null;
    draw();
    await loadView();
    draw();
  });

  (async () => {
    render(top("kebbie hub"));
    state.info = await account.hub().catch(() => null);
    state.me = await account.me().catch(() => null);
    const onward = state.me ? signedInDestination(path, params) : null;
    if (onward) return location.assign(onward);
    state.session = state.me ? { state: "locked", me: state.me } : null;
    await loadView();
    draw();
  })();
}
