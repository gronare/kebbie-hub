import { CONNECT_TEXT, connectCommitment, connectText, finishConnect, parseConnectFragment, planLine, safeNext, withReauth } from "./connect.js";
import { keepDeviceName, proposedName } from "./device-name.js";
import { addPasskey, autofillAvailable, NoPasskeyOffered, OFFER_TEXT, signInText, signInWith, signUpText } from "./signin.js";
import { account, admin, connectCalls, deviceClient, HubError } from "./hub.js";
import { get, loadDevice, loadMachines, newDeviceKeys, put, saveDevice, saveMachines } from "./keystore.js";
import { failureText, pair, PairingFailed } from "./pairing.js";
import { creationOptions, registrationJSON, supported as passkeysSupported } from "./passkey.js";

export const PAGES = ["/setup", "/signup", "/connect", "/add", "/admin"];

const esc = (value) => String(value ?? "").replace(/[&<>"']/g, (char) => `&#${char.charCodeAt(0)};`);
const top = (title, aside = "") => `<header class="top"><b>${esc(title)}</b><span>${esc(aside)}</span></header>`;
const say = (text, tone = "") => `<div class="center${tone ? ` ${tone}` : ""}"${tone === "warn" ? ' role="alert"' : ""}>${text}</div>`;
const when = (seconds) => (seconds ? new Date(seconds * 1000).toLocaleString("en-GB", { day: "numeric", month: "short", hour: "2-digit", minute: "2-digit" }) : "never");
const deviceName = () => proposedName(navigator.userAgent);

const ERRORS = {
  unauthorized: "That link is used or expired. Ask for a new one.",
  "email-taken": "That email already has an account. Sign in instead.",
  "bad-email": "That does not look like an email address.",
  "signup-closed": "This hub signs people up by invite only.",
  "mail-failed": "The hub could not send the mail. Try again later.",
  "not-owner": "Only the hub's owner sees this page.",
  ...CONNECT_TEXT,
};

function message(error) {
  if (error instanceof PairingFailed) return CONNECT_TEXT[error.reason] ? connectText(error.reason) : failureText(error.reason);
  if (error instanceof HubError) return ERRORS[error.code] ?? `The hub answered ${error.status} (${error.code}).`;
  if (error?.name === "NotAllowedError") return "The passkey was cancelled.";
  return error?.message ?? String(error);
}

export function startPage(root) {
  const params = new URLSearchParams(location.search);
  const fragment = parseConnectFragment(location.hash);
  if (location.hash && location.pathname === "/connect") history.replaceState(null, "", location.pathname + location.search);
  const state = { info: null, me: null, notice: null, busy: false, phase: "start", view: null, tab: "overview", byName: false, offerPasskey: false };
  const path = location.pathname;
  const here = () => location.pathname + location.search;

  const render = (html) => {
    root.innerHTML = `<main class="screen page">${html}</main>`;
  };

  const fail = (error) => {
    state.busy = false;
    state.notice = message(error);
    draw();
  };

  const signingIn = signInWith({ account, credentials: navigator.credentials });

  async function signedIn(me, byName) {
    signingIn.stop();
    state.me = me;
    state.byName = false;
    state.offerPasskey = byName;
    state.notice = null;
    const next = path === "/signup" ? safeNext(params.get("next")) : null;
    if (next) return location.assign(next);
    await loadView();
    draw();
  }

  const signInFailed = (error) => {
    if (error instanceof NoPasskeyOffered) state.byName = true;
    state.busy = false;
    state.notice = signInText(error);
    draw();
    if (state.byName) startAutofill();
  };

  async function startAutofill() {
    if (state.me || !(await autofillAvailable())) return;
    try {
      await signedIn(await signingIn.autofill(), false);
    } catch (error) {
      if (error?.name !== "AbortError") signInFailed(error);
    }
  }

  async function signUp(ticket, handle) {
    const begun = await account.registerBegin({ ...ticket, handle: handle.trim().toLowerCase() });
    const credential = await navigator.credentials.create(creationOptions(begun.options));
    state.me = await account.registerFinish(begun.ceremony, registrationJSON(credential), deviceName());
  }

  const reauth = async () => {
    state.me = await signingIn.byName(state.me.handle);
  };

  async function thisDevice() {
    const known = await loadDevice();
    if (known) return known;
    const keys = await newDeviceKeys();
    const name = deviceName();
    const registered = await account.registerDevice(name);
    const device = { id: registered.device, token: registered.token, name, keys, public: keys.public };
    await saveDevice(device);
    return device;
  }

  async function keepMachine(machine) {
    const machines = (await loadMachines()).filter((known) => known.id !== machine.id);
    await saveMachines([...machines, { id: machine.id, keys: machine.keys, name: machine.name ?? null, pairedAt: Math.floor(Date.now() / 1000) }]);
    await put("revs", { ...((await get("revs")) ?? {}), [machine.id]: 0 });
  }

  const signInForm = (purpose) => {
    const next = encodeURIComponent(here());
    const join = state.info?.signup === "open" ? "No account yet? Sign up" : "Have an invite? Sign up";
    const fallback = state.byName
      ? `<form data-form="sign-in" class="stack"><label class="task field"><span class="meta">Account name</span><input name="handle" autocomplete="username webauthn" autocapitalize="none" spellcheck="false" required></label><button class="btn">Sign in with this account</button></form>`
      : `<button type="button" class="center link" data-action="by-name">Use an account name instead</button>`;
    return `${say(esc(purpose))}<button type="button" class="btn primary" data-action="sign-in">Sign in with passkey</button>${notice()}${fallback}<a class="center link" href="/signup?next=${next}">${join}</a>`;
  };

  const handleForm = (button) => `<form data-form="handle" class="stack"><label class="task field"><span class="meta">Account name</span><input name="handle" autocomplete="username" autocapitalize="none" spellcheck="false" pattern="[a-zA-Z][a-zA-Z0-9\\-]*" maxlength="32" required></label><button class="btn primary">${esc(button)}</button></form>`;

  const offer = () => (state.offerPasskey && state.me ? `<section class="task"><div class="meta">${esc(OFFER_TEXT)}</div><div class="pair-answer"><button type="button" class="btn" data-action="passkey-later">Not now</button><button type="button" class="btn primary" data-action="passkey-add">Add passkey</button></div></section>` : "");
  const notice = () => offer() + (state.notice ? say(esc(state.notice), "warn") : "");

  function setupPage() {
    if (state.phase === "done") return top("Set up this hub") + say(`You own this hub, ${esc(state.me.handle)}.`, "ok") + `<a class="btn primary" href="/admin">Open admin</a><a class="btn" href="/">Open kelpie</a>`;
    if (!state.info?.needs_setup) return top("Set up this hub") + say("This hub has an owner already.") + `<a class="btn" href="/">Sign in</a>`;
    if (!params.get("token")) return top("Set up this hub") + say("Open the setup link the hub printed in its log when it started.");
    return top("Set up this hub") + say("Pick an account name for the owner. A passkey signs you in; there is no password.") + handleForm("Create owner account") + notice();
  }

  function signupPage() {
    const parts = [top("Sign up")];
    if (state.me) return parts.join("") + say(`You're signed in as ${esc(state.me.handle)}.`, "ok") + `<a class="btn primary" href="${esc(safeNext(params.get("next")) ?? "/")}">Continue</a>`;
    if (params.get("verify")) {
      if (!state.view) return parts.join("") + say("This link has expired or was already used. Sign up again.", "warn");
      return parts.join("") + say(`Your email ${esc(state.view.email)} is verified. Pick an account name.`) + handleForm("Create account with passkey") + notice();
    }
    if (params.get("invite")) return parts.join("") + say("You're invited. Pick an account name; a passkey signs you in.") + handleForm("Create account with passkey") + notice();
    if (state.info?.signup === "open") {
      if (state.phase === "mailed") return parts.join("") + say(`Check your inbox at <b>${esc(state.view)}</b>. The link works once, for 24 hours.`, "ok");
      return parts.join("") + say("Enter your email. You get a link to finish signing up.") + `<form data-form="email" class="stack"><label class="task field"><span class="meta">Email</span><input name="email" type="email" autocomplete="email" required></label><button class="btn primary">Send the link</button></form>` + notice() + `<a class="center link" href="/">Have an account? Sign in</a>`;
    }
    return parts.join("") + say("Enter your invite and pick an account name.") + `<form data-form="invite" class="stack"><label class="task field"><span class="meta">Invite</span><input name="invite" class="code-input" autocomplete="off" autocapitalize="characters" spellcheck="false" placeholder="XXXX-XXXX" required></label><label class="task field"><span class="meta">Account name</span><input name="handle" autocomplete="username" autocapitalize="none" spellcheck="false" required></label><button class="btn primary">Create account with passkey</button></form>` + notice();
  }

  function planBlock() {
    const line = planLine(state.me?.plan, state.me?.billing_url);
    if (!line) return "";
    return say(`${esc(line.text)}${line.url ? ` <a class="link" href="${esc(line.url)}">Start a subscription</a>` : ""}`, "warn");
  }

  function connectPage() {
    const code = params.get("code") ?? "";
    const parts = [top("Connect a machine")];
    if (!passkeysSupported()) return parts.join("") + say("This browser cannot sign in with a passkey.");
    if (!state.me) return parts.join("") + signInForm("Sign in to connect this machine.");
    parts.push(say(`You're signed in as <b>${esc(state.me.handle)}</b>.`));
    if (state.phase === "connected") return parts.join("") + say(`Connected. kelpie on ${esc(state.view.name)} reaches this hub now. You can close this tab.`, "ok");
    if (state.phase === "pairing") return parts.join("") + say(`Connected. Opening ${esc(state.view.name)}'s board in this browser …`, "ok");
    if (state.phase === "paired") return parts.join("") + say("Done. Opening the board …", "ok");
    if (state.phase === "cancelled") return parts.join("") + say("Cancelled. The terminal shows it too.");
    if (!state.view) return parts.join("") + notice() + planBlock();
    const board = fragment ? `<label class="task check"><input type="checkbox" id="show-board" checked> <span>Also show its board in this browser</span></label>` : "";
    return (
      parts.join("") +
      `<section class="task"><div class="name">Connect ${esc(state.view.name)}?</div><div class="meta">Check that your terminal shows this code:</div><div class="code" aria-live="polite">${esc(state.view.code || code)}</div></section>` +
      board +
      planBlock() +
      `<div class="pair-answer"><button type="button" class="btn" data-action="cancel" ${state.busy ? "disabled" : ""}>Cancel</button><button type="button" class="btn primary" data-action="connect" ${state.busy ? "disabled" : ""}>Connect</button></div>` +
      notice()
    );
  }

  function addPage() {
    const parts = [top("Add this phone")];
    if (!passkeysSupported()) return parts.join("") + say("This browser cannot sign in with a passkey.");
    if (!state.me) return parts.join("") + signInForm("Sign in to add this phone to your machine.");
    if (state.phase === "done") return parts.join("") + say(`Added to ${esc(state.view.name)}. Opening the board …`, "ok");
    if (state.phase === "compare") {
      return (
        parts.join("") +
        `<section class="task"><div class="name">Check that your terminal shows:</div><div class="code" aria-live="polite">${esc(state.digits)}</div><div class="meta">Same digits on both? Answer y in the terminal and tap Same here. Different digits mean someone is in between: answer n there and tap Different.</div></section>` +
        `<label class="task field"><span class="meta">Name of this device, as the machine lists it</span><input name="device-name" id="device-name" autocomplete="off" maxlength="64" value="${esc(state.deviceName ?? "")}"></label>` +
        `<div class="pair-answer"><button type="button" class="btn" data-action="different">Different</button><button type="button" class="btn primary" data-action="same">Same digits</button></div>`
      );
    }
    if (state.phase === "waiting") return parts.join("") + say("Waiting for the terminal …");
    if (!state.view) return parts.join("") + notice();
    return parts.join("") + `<section class="task"><div class="name">Add this phone to ${esc(state.view.name)}?</div><div class="meta">You're signed in as ${esc(state.me.handle)}. The phone gets its own keys and reads the board itself.</div></section><button type="button" class="btn primary" data-action="add" ${state.busy ? "disabled" : ""}>Add phone</button>` + notice();
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
        const facts = [row.email, `${row.machines} machines`, `${row.devices} devices`, `seen ${when(row.last_seen)}`, planText(row.plan)].filter(Boolean).join(" · ");
        const action = row.owner ? `<span class="machine">owner</span>` : row.deactivated_at ? `<button type="button" class="btn small" data-action="activate" data-id="${esc(row.account)}">Activate</button>` : `<button type="button" class="btn small" data-action="deactivate" data-id="${esc(row.account)}">Deactivate</button>`;
        return `<div class="task${row.deactivated_at ? " asleep" : ""}"><div class="row"><div class="name">${esc(row.handle)}</div>${action}</div><div class="meta">${esc(facts)}</div></div>`;
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
    return view.signals.map((row) => `<div class="task"><div class="row"><div class="name">${esc(`${row.component} ${row.kind}`)}</div><span class="machine">${esc(row.count)}</span></div><div class="meta">${esc([new Date(row.day * 86_400_000).toISOString().slice(0, 10), `kelpie ${row.version}${row.commit ? ` (${row.commit})` : ""}`, `${row.os} ${row.arch}`, row.vetted ? null : "unvetted"].filter(Boolean).join(" · "))}</div></div>`).join("");
  }

  function adminPage() {
    if (!state.me) return top("Admin") + signInForm("Sign in as the hub's owner.");
    if (!state.me.owner) return top("Admin") + say("Only the hub's owner sees this page.");
    const tabs = `<nav class="tabs">${TABS.map(([key, label]) => `<a href="#${key}" class="${state.tab === key ? "on" : ""}">${esc(label)}</a>`).join("")}</nav>`;
    return top("Admin", state.me.handle) + tabs + notice() + adminBody();
  }

  function draw() {
    const html = { "/setup": setupPage, "/signup": signupPage, "/connect": connectPage, "/add": addPage, "/admin": adminPage }[path]();
    render(html);
    document.title = `${{ "/setup": "Set up", "/signup": "Sign up", "/connect": "Connect", "/add": "Add phone", "/admin": "Admin" }[path]} · kelpie`;
  }

  async function loadView() {
    state.notice = null;
    try {
      if (path === "/connect" && state.me) state.view = await connectCalls.view(params.get("code") ?? "");
      if (path === "/add" && state.me) state.view = await connectCalls.addView(params.get("code") ?? "");
      if (path === "/signup" && params.get("verify")) state.view = await account.verification(params.get("verify"));
      if (path === "/admin" && state.me?.owner) await loadAdmin();
    } catch (error) {
      state.view = null;
      if (!(path === "/signup" && error instanceof HubError)) state.notice = message(error);
    }
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
    const showBoard = Boolean(fragment) && document.getElementById("show-board")?.checked;
    try {
      let device = null;
      let sealed = null;
      if (showBoard) {
        device = await thisDevice();
        sealed = await connectCommitment(device.public, fragment);
      }
      const approved = await withReauth(() => connectCalls.approve({ code: params.get("code"), device: device?.id ?? null, commitment: sealed?.commitment ?? null }), reauth);
      state.view = { ...state.view, name: approved.name };
      if (!approved.pairing) {
        state.phase = "connected";
        return draw();
      }
      state.phase = "pairing";
      draw();
      const machine = await finishConnect({ hub: deviceClient(device.token), pairing: approved.pairing, publicKeys: device.public, fragment, nonce: sealed.nonce });
      await keepMachine({ ...machine, name: null });
      state.phase = "paired";
      draw();
      setTimeout(() => location.assign("/#/board"), 800);
    } catch (error) {
      if (error instanceof HubError && error.status === 402) state.me = { ...state.me, plan: { entitled: false }, billing_url: error.url };
      if (state.phase === "pairing") state.phase = "start";
      fail(error);
    }
  }

  let answer = null;

  async function addPhone() {
    state.busy = true;
    state.notice = null;
    draw();
    try {
      const device = await thisDevice();
      state.deviceName = device.name ?? deviceName();
      const machine = await pair({
        hub: deviceClient(device.token),
        publicKeys: device.public,
        typed: params.get("code") ?? "",
        confirm: (digits) =>
          new Promise((resolve) => {
            answer = (matches) => {
              answer = null;
              state.phase = "waiting";
              draw();
              resolve({ matches });
            };
            state.phase = "compare";
            state.digits = digits;
            draw();
          }),
      });
      await keepMachine(machine);
      await keepDeviceName(deviceClient(device.token), device, state.deviceName, saveDevice).catch(() => device);
      state.phase = "done";
      draw();
      setTimeout(() => location.assign("/#/board"), 800);
    } catch (error) {
      answer = null;
      state.phase = "start";
      fail(error);
    }
  }

  root.addEventListener("input", (event) => {
    if (event.target.id === "device-name") state.deviceName = event.target.value;
  });

  root.addEventListener("submit", async (event) => {
    const form = event.target.closest("form[data-form]");
    if (!form) return;
    event.preventDefault();
    const button = form.querySelector("button");
    if (button) button.disabled = true;
    try {
      const kind = form.dataset.form;
      if (kind === "sign-in") return await signingIn.byName(form.handle.value).then((me) => signedIn(me, true), signInFailed);
      if (kind === "email") {
        await account.emailSignup(form.email.value.trim());
        state.view = form.email.value.trim().toLowerCase();
        state.phase = "mailed";
        return draw();
      }
      if (kind === "invite") await signUp({ invite: form.invite.value.trim() }, form.handle.value);
      if (kind === "handle") {
        const ticket = path === "/setup" ? { setup: params.get("token") } : params.get("verify") ? { verification: params.get("verify") } : { invite: params.get("invite") };
        await signUp(ticket, form.handle.value);
        if (path === "/setup") {
          state.phase = "done";
          return draw();
        }
      }
      const next = path === "/signup" ? safeNext(params.get("next")) : null;
      if (next) return location.assign(next);
      await loadView();
      draw();
    } catch (error) {
      const said = signUpText(error);
      if (said) {
        state.notice = said;
        return draw();
      }
      fail(error);
    }
  });

  root.addEventListener("click", async (event) => {
    const target = event.target.closest("[data-action]");
    if (!target) return;
    const action = target.dataset.action;
    try {
      if (action === "sign-in") return await signingIn.withPasskey().then((me) => signedIn(me, false), signInFailed);
      if (action === "by-name") {
        state.byName = true;
        state.notice = null;
        draw();
        return startAutofill();
      }
      if (action === "passkey-add" || action === "passkey-later") {
        state.offerPasskey = false;
        if (action === "passkey-add") await addPasskey({ account, credentials: navigator.credentials, name: deviceName() }).catch((error) => (state.notice = signUpText(error) ?? message(error)));
        return draw();
      }
      if (action === "connect") return await connect();
      if (action === "cancel") {
        await connectCalls.deny(params.get("code"));
        state.phase = "cancelled";
        return draw();
      }
      if (action === "add") return await addPhone();
      if (action === "same") return answer?.(true);
      if (action === "different") return answer?.(false);
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
    draw();
    await loadView();
    draw();
  });

  (async () => {
    render(top("kelpie"));
    state.info = await account.hub().catch(() => null);
    state.me = await account.me().catch(() => null);
    await loadView();
    draw();
    startAutofill();
  })();
}
