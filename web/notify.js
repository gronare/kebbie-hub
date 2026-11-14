import { DEFAULT_NOTICE_CATEGORIES, fromB64, NOTICE_CATEGORIES, noticeCategoryOf, openNotice, ProtocolError, verifyMachineList } from "../js/protocol.js";
import { openEncryption } from "./store.js";

export const NOTIFY_RECORD = "notify";
export const LISTING = "listing";
export const categoriesKey = (device) => `notify-categories:${device}`;
export const GENERIC = { title: "kebbie", body: "Open kebbie to see what changed." };
export const NOTICE_FAILURE = "notice-failure";

export const browserPush = async () => (await globalThis.navigator?.serviceWorker?.getRegistration("/"))?.pushManager.getSubscription() ?? null;

export const CATEGORY_GROUPS = [
  {
    title: "Notify me when it needs you",
    rows: [
      { category: "questions", id: "n-ask", name: "Questions and prompts", meta: "A task asks you something, or an agent's prompt waits for a choice" },
      { category: "new-device", id: "n-device", name: "A new device to approve", meta: "A device wants to make changes on one of your machines" },
      { category: "idea-questions", id: "n-lead", name: "The lead's questions on an idea", meta: "The lead wrote out an idea and needs answers before it starts" },
      { category: "lead-answered", id: "n-reply", name: "The lead answered", meta: "A machine's lead replied to you in its thread" },
    ],
  },
  {
    title: "Notify me when work moves",
    rows: [
      { category: "ready", id: "n-push", name: "Ready to push", meta: "A task is reviewed and waits for your push" },
      { category: "ci-failed", id: "n-ci", name: "CI failed", meta: "A push came back red" },
      { category: "started", id: "n-start", name: "A start went through", meta: "A task you started from here began on its machine" },
    ],
  },
];

export const ordered = (categories) => NOTICE_CATEGORIES.filter((category) => categories.includes(category));

export function withCategory(categories, category, on) {
  const rest = categories.filter((one) => one !== category);
  return ordered(on ? [...rest, category] : rest);
}

export const keptCategories = (kept) => (Array.isArray(kept) && kept.every((one) => NOTICE_CATEGORIES.includes(one)) ? ordered(kept) : [...DEFAULT_NOTICE_CATEGORIES]);

export function deviceWord(userAgent = "") {
  if (/iPhone|iPod/.test(userAgent)) return "iPhone";
  if (/iPad/.test(userAgent)) return "iPad";
  if (/Android/.test(userAgent)) return "phone";
  return "browser";
}

export function notifyPhase({ ios = false, standalone = false, supported = true, permission = "default", subscribed = false, asking = false }) {
  if (ios && !standalone) return "install";
  if (!supported) return "unsupported";
  if (permission === "denied") return "denied";
  if (asking) return "asking";
  if (subscribed && permission === "granted") return "on";
  return "off";
}

export function namesSendHere(names) {
  if (names.length === 0) return "";
  const listed = names.length === 1 ? names[0] : `${names.slice(0, -1).join(", ")} and ${names.at(-1)}`;
  return ` ${listed} ${names.length === 1 ? "sends" : "send"} here.`;
}

export function notifyText(phase, { ios = false, word = "browser", machines = [] } = {}) {
  if (phase === "asking") return ios ? "Asking iOS for permission…" : "Asking for permission…";
  if (phase === "on") return `On for this ${word}.${namesSendHere(machines)}`;
  if (phase === "denied") return ios ? "Turned off for kebbie in iOS. Turn them on in Settings › Notifications › kebbie." : "Turned off for kebbie in this browser. Turn them on in its settings for this site.";
  if (phase === "install") return "Add kebbie to your Home Screen to get notifications. Share, then Add to Home Screen.";
  if (phase === "unsupported") return "This browser can't show notifications.";
  return "Off";
}

export const switchable = (phase) => phase === "off" || phase === "on" || phase === "asking";

const part = (text) => encodeURIComponent(text);

export function routeOf(machine, open) {
  if (open?.to === "task") return `/#/task/${part(machine)}/${part(open.address)}`;
  if (open?.to === "item") return `/#/item/${part(open.item.startsWith("vault:") ? open.item : `${machine}|${open.item}`)}`;
  if (open?.to === "device") return `/account#device/${part(open.device)}`;
  if (open?.to === "lead") return `/#/lead/${part(machine)}`;
  return "/#/board";
}

export function isSettled(data, opened) {
  const board = opened.find((machine) => machine.machine === data?.machine)?.board;
  const open = data?.open;
  if (!board || !open) return false;
  if (open.to === "device") return board.devices?.[open.device]?.state !== "waiting";
  if (open.to === "item") {
    const row = board.backlog?.find((one) => one.item === open.item);
    return data.category === "idea-questions" ? !row || row.questions === 0 : false;
  }
  const item = board.items?.find((one) => one.address === open.address);
  if (data.category === "questions") return !item || (!item.question && !item.prompt && !item.kebbie_questions?.length);
  if (data.category === "ready") return !item || item.group !== "READY TO PUSH";
  if (data.category === "ci-failed") return !item || item.ci?.state !== "failed";
  return false;
}

export async function closeSettled(registration, opened) {
  const shown = await registration.getNotifications();
  let closed = 0;
  for (const notification of shown) {
    if (!isSettled(notification.data, opened)) continue;
    notification.close();
    closed += 1;
  }
  return closed;
}

const CATEGORY_TEXT = {
  questions: "A question or a prompt waits for you.",
  "new-device": "A new device wants to make changes.",
  "idea-questions": "The lead has questions on an idea.",
  ready: "A task is ready to push.",
  "ci-failed": "CI failed on a push.",
  started: "A start you asked for has news.",
  "lead-answered": "A lead answered you.",
};

const machineIn = (listing, machine) => listing.list.list.machines.find((one) => one.machine === machine) ?? null;
const nameIn = (listing, machine) => listing?.boards?.find((entry) => entry.machine === machine)?.name || null;

async function trusted(listing, record, get) {
  const signed = listing?.list;
  const key = record.account;
  if (!signed || !key?.keys?.signing) return { reason: "no-list" };
  const seen = (await get(`marks:${key.account}`).catch(() => null))?.list ?? 0;
  if (!(signed.list?.counter >= seen)) return { reason: "list-older" };
  const verifying = await crypto.subtle.importKey("raw", fromB64(key.keys.signing), { name: "Ed25519" }, false, ["verify"]);
  return (await verifyMachineList({ id: key.account, verifying }, signed)) ? { listing } : { reason: "list-unsigned" };
}

// A machine missing from the kept list may have joined since the app last ran, so the worker reads the hub's list once and keeps it when the account vouches for it.
async function vouched(machine, record, { get, put, boards, now }) {
  const kept = (await get(LISTING))?.listing ?? null;
  const held = await trusted(kept, record, get);
  if (held.listing && machineIn(held.listing, machine)) return held;
  const missing = held.reason ?? "unknown-machine";
  if (!boards) return { reason: missing, named: kept };
  let fetched;
  try {
    fetched = await boards();
  } catch (error) {
    return { reason: error?.status === 401 ? "signed-out" : missing, named: kept };
  }
  const fresh = await trusted(fetched, record, get);
  if (!fresh.listing) return { reason: fresh.reason, named: kept };
  if (put && fresh.listing.list.list.counter >= (kept?.list?.list?.counter ?? 0)) await put(LISTING, { listing: fresh.listing, at: now }).catch(() => undefined);
  return machineIn(fresh.listing, machine) ? fresh : { reason: "unknown-machine", named: fresh.listing };
}

function unopened(envelope, tag, failure, named) {
  const category = noticeCategoryOf(tag);
  const name = nameIn(named, envelope?.machine);
  return { title: name ?? GENERIC.title, body: CATEGORY_TEXT[category] ?? GENERIC.body, tag, data: { url: "/#/board" }, retract: [], failure: { ...failure, name } };
}

export async function pushedNotice(envelope, { get, now, put = null, boards = null }) {
  const tag = typeof envelope?.collapse === "string" ? envelope.collapse : "kebbie";
  const failure = { at: now, reason: null, detail: null, machine: typeof envelope?.machine === "string" ? envelope.machine : null, recipient: Object.keys(envelope?.keys ?? {})[0] ?? null, device: null };
  let named = null;
  const failed = (reason, detail = null) => unopened(envelope, tag, { ...failure, reason, detail }, named);
  try {
    named = (await get(LISTING).catch(() => null))?.listing ?? null;
    if (!envelope) return failed("no-envelope");
    const record = await get(NOTIFY_RECORD);
    if (!record?.account?.account) return failed("no-record");
    const device = await get(`device-key:${record.account.account}`);
    failure.device = device?.id ?? null;
    if (!device?.encryption) return failed("no-key");
    if (!Object.hasOwn(envelope.keys ?? {}, device.id)) return failed(record.device !== device.id && Object.hasOwn(envelope.keys ?? {}, record.device) ? "device-changed" : "not-a-recipient", record.device);
    const found = await vouched(envelope.machine, record, { get, put, boards, now });
    if (!found.listing) {
      named = found.named ?? named;
      return failed(found.reason);
    }
    named = found.listing;
    const listed = machineIn(found.listing, envelope.machine);
    const notice = await openNotice(envelope, { id: device.id, encryption: await openEncryption(device.encryption, device.cert?.encryption) }, { id: listed.machine, keys: listed.keys }, now);
    const name = nameIn(found.listing, listed.machine);
    return { title: notice.title, body: notice.body, tag, data: { machine: listed.machine, name, category: notice.category, open: notice.open, url: routeOf(listed.machine, notice.open) }, retract: notice.retract ?? [], failure: null };
  } catch (error) {
    return error instanceof ProtocolError ? failed(error.code) : failed("error", `${error?.name ?? "Error"}: ${error?.message ?? error}`);
  }
}

const REASON_TEXT = {
  "no-envelope": "the push carried no notice.",
  "no-record": "this browser no longer knows it turned notifications on.",
  "no-key": "this device holds no key to open it.",
  "device-changed": "it was sealed to an earlier key of this device.",
  "not-a-recipient": "it was sealed to another device.",
  "no-list": "this browser holds no list of your machines. Open your boards once.",
  "list-older": "the list of your machines here is older than one this browser has seen.",
  "list-unsigned": "the list of your machines isn't signed by your account.",
  "signed-out": "the hub asked this browser to sign in again.",
  "unknown-machine": "the machine that sent it isn't on your account's list.",
  expired: "it had expired when it arrived.",
  "not-yet-valid": "the sending machine's clock is ahead of this device's.",
  "bad-signature": "the sending machine's signature didn't check out.",
  undecryptable: "this device's key didn't open it.",
  "key-mismatch": "this device's key didn't open it.",
  "category-mismatch": "its tag names another kind of notice than the one inside.",
};

const clock = (seconds, now) => {
  const at = new Date(seconds * 1000);
  const time = at.toLocaleTimeString("en-GB", { hour: "2-digit", minute: "2-digit" });
  return at.toDateString() === new Date(now * 1000).toDateString() ? time : `${at.toLocaleDateString("en-GB", { day: "numeric", month: "short" })} ${time}`;
};

export function failureLine(failure, now) {
  if (!failure?.reason || !Number.isSafeInteger(failure.at)) return null;
  const why = REASON_TEXT[failure.reason] ?? (failure.reason === "error" ? `this browser failed while opening it (${failure.detail ?? "no detail"}).` : `this app can't read it (${failure.reason}).`);
  return `The last notification, at ${clock(failure.at, now)}${failure.name ? ` from ${failure.name}` : ""}, couldn't be opened: ${why}`;
}

export const PUSHED = "pushed";

export async function watching(clients) {
  if (!clients) return [];
  const windows = await clients.matchAll({ type: "window", includeUncontrolled: true }).catch(() => []);
  return windows.filter((client) => client.visibilityState === "visible" && client.focused);
}

// WebKit counts a push as shown once showNotification is called inside its event, so a quiet notice can be closed at once without the subscription being revoked.
export async function showPushed(registration, envelope, { get, now, put = null, del = null, boards = null, clients = null }) {
  const shown = await pushedNotice(envelope, { get, now, put, boards });
  const open = await watching(clients);
  if (shown.failure && put) await put(NOTICE_FAILURE, shown.failure).catch(() => undefined);
  if (!shown.failure && del) await del(NOTICE_FAILURE).catch(() => undefined);
  if (shown.retract.length) {
    for (const notification of await registration.getNotifications()) if (shown.retract.includes(notification.tag)) notification.close();
  }
  for (const client of open) client.postMessage({ type: PUSHED, tag: shown.tag, url: shown.data.url });
  await registration.showNotification(shown.title, { body: shown.body, tag: shown.tag, data: shown.data, icon: "/icon-192.png", silent: open.length > 0 });
  if (open.length) for (const notification of await registration.getNotifications({ tag: shown.tag })) if (notification.tag === shown.tag) notification.close();
  return { ...shown, quiet: open.length > 0 };
}
