import { DEFAULT_NOTICE_CATEGORIES, fromB64, NOTICE_CATEGORIES, openNotice, verifyMachineList } from "../js/protocol.js";

export const NOTIFY_RECORD = "notify";
export const LISTING = "listing";
export const categoriesKey = (device) => `notify-categories:${device}`;
export const GENERIC = { title: "kebbie", body: "Open kebbie to see what changed." };

export const CATEGORY_GROUPS = [
  {
    title: "Notify me when it needs you",
    rows: [
      { category: "questions", id: "n-ask", name: "Questions and prompts", meta: "A task asks you something, or an agent's prompt waits for a choice" },
      { category: "new-device", id: "n-device", name: "A new device to approve", meta: "A device wants to make changes on one of your machines" },
      { category: "idea-questions", id: "n-lead", name: "The lead's questions on an idea", meta: "The lead wrote out an idea and needs answers before it starts" },
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
  if (data.category === "questions") return !item || (!item.question && !item.prompt);
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

async function verifiedListing(record, get) {
  const kept = await get(LISTING);
  const signed = kept?.listing?.list;
  const key = record?.account;
  if (!signed || !key?.keys?.signing) return null;
  const verifying = await crypto.subtle.importKey("raw", fromB64(key.keys.signing), { name: "Ed25519" }, false, ["verify"]);
  return (await verifyMachineList({ id: key.account, verifying }, signed)) ? kept.listing : null;
}

export async function pushedNotice(envelope, { get, now }) {
  const tag = typeof envelope?.collapse === "string" ? envelope.collapse : "kebbie";
  const generic = { title: GENERIC.title, body: GENERIC.body, tag, data: { url: "/#/board" }, retract: [] };
  try {
    const record = await get(NOTIFY_RECORD);
    const device = await get(`device-key:${record?.account?.account}`);
    if (!device?.encryption || device.id !== record.device) return generic;
    const listing = await verifiedListing(record, get);
    const listed = listing?.list.list.machines.find((one) => one.machine === envelope.machine);
    if (!listed) return generic;
    const notice = await openNotice(envelope, { id: device.id, encryption: device.encryption }, { id: listed.machine, keys: listed.keys }, now);
    const name = listing.boards?.find((entry) => entry.machine === listed.machine)?.name || null;
    return { title: notice.title, body: notice.body, tag, data: { machine: listed.machine, name, category: notice.category, open: notice.open, url: routeOf(listed.machine, notice.open) }, retract: notice.retract ?? [] };
  } catch {
    return generic;
  }
}

export async function showPushed(registration, envelope, { get, now }) {
  const shown = await pushedNotice(envelope, { get, now });
  if (shown.retract.length) {
    for (const notification of await registration.getNotifications()) if (shown.retract.includes(notification.tag)) notification.close();
  }
  await registration.showNotification(shown.title, { body: shown.body, tag: shown.tag, data: shown.data, icon: "/icon-192.png" });
  return shown;
}
