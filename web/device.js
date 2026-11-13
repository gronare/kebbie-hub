import { deviceFingerprint, deviceId, importSender, newDeviceKey, openCommand, openEnvelope, sealCommand, signDeviceCert, verifyDeviceCert } from "../js/protocol.js";
import { browserPush, categoriesKey, keptCategories, NOTIFY_RECORD } from "./notify.js";
import { openEncryption, sealKey } from "./store.js";

const sentKey = (account) => `hub-sent:${account}`;
const allowKey = (account) => `allow-changes:${account}`;
const deviceKey = (account) => `device-key:${account}`;
const KEEP_SENT = 7 * 86_400;

export class CommandProblem extends Error {
  constructor(reason) {
    super(reason);
    this.reason = reason;
  }
}

export async function allowsChanges(store, account) {
  try {
    return (await store.get(allowKey(account))) === true;
  } catch {
    return false;
  }
}

export const keepAllowsChanges = (store, account, allowed) => store.put(allowKey(account), allowed === true);

export async function loadSent(store, account, now) {
  try {
    const kept = (await store.get(sentKey(account))) ?? { cursor: 0, sent: [] };
    return { cursor: kept.cursor ?? 0, sent: (kept.sent ?? []).filter((one) => one.at + KEEP_SENT > now) };
  } catch {
    return { cursor: 0, sent: [] };
  }
}

const WORDED = {
  "review-comments": (sent) => ({ ...sent, comments: (sent.comments ?? []).map(({ text, ...where }) => where) }),
  "item-edit": ({ text, ...sent }) => sent,
};
export const storedSent = (sent, now) => (WORDED[sent.kind] && (sent.result || sent.exp <= now) ? WORDED[sent.kind](sent) : sent);
export const keepSent = (store, account, kept, now = Math.floor(Date.now() / 1000)) => store.put(sentKey(account), { ...kept, sent: kept.sent.map((sent) => storedSent(sent, now)) }).catch(() => undefined);

export async function heldDevice(store, account) {
  try {
    const kept = await store.get(deviceKey(account));
    if (kept === undefined) return { device: null, missing: true };
    if (kept?.cert?.account !== account || !kept.signing || !kept.encryption) return { device: null, missing: false };
    return { device: { ...kept, encryption: await openEncryption(kept.encryption, kept.cert.encryption) }, missing: false };
  } catch {
    return { device: null, missing: false };
  }
}

export const loadDevice = async (store, account) => (await heldDevice(store, account)).device;

export const forgetDevice = (store, account) => store.del(deviceKey(account)).catch(() => undefined);

export async function verifiedDevices(key, rows) {
  const devices = [];
  for (const row of rows) {
    const { cert } = row;
    if (row.device !== cert?.device || !(await verifyDeviceCert(key, cert))) continue;
    devices.push({ id: cert.device, name: cert.name, kind: cert.kind, key: await deviceFingerprint(cert.signing), added: row.created_at, seen: row.last_seen_at, removed: row.removed_at ?? null, push: row.push ?? null });
  }
  return devices;
}

// The browser holds one push subscription, so when notifications were on for another of its devices the hub moves it here and the worker's record names the key it now holds.
export async function pushFollows({ store, calls, identity, device, subscription = browserPush }) {
  const record = await store.get(NOTIFY_RECORD).catch(() => null);
  if (!record || record.account?.account !== identity.id || record.device === device.id) return false;
  const held = await subscription().catch(() => null);
  if (!held) return false;
  const categories = keptCategories(await store.get(categoriesKey(record.device)).catch(() => null));
  const { endpoint, keys } = held.toJSON();
  await calls.subscribePush(device.id, { endpoint, keys: { p256dh: keys.p256dh, auth: keys.auth }, categories });
  await store.put(categoriesKey(device.id), categories).catch(() => undefined);
  await store.put(NOTIFY_RECORD, { account: identity.key, device: device.id }).catch(() => undefined);
  return true;
}

export async function ensureDevice({ store, calls, identity, name, now, kept = undefined, subscription = browserPush }) {
  const held = kept === undefined ? await loadDevice(store, identity.id) : kept;
  if (held && (await verifyDeviceCert(identity.key, held.cert))) return held;
  const key = await newDeviceKey();
  const sealed = await sealKey(key.encryptionPkcs8).finally(() => key.encryptionPkcs8.fill(0));
  const id = deviceId(now * 1000);
  const cert = await signDeviceCert(identity, { device: id, name, signing: key.public, encryption: key.encryptionPublic, created_at: now });
  await calls.addDevice(cert);
  const device = { id, cert, signing: key.signing, encryption: key.encryption, fingerprint: await deviceFingerprint(key.public) };
  await store.put(deviceKey(identity.id), { ...device, encryption: sealed }).catch(() => undefined);
  await pushFollows({ store, calls, identity, device, subscription }).catch(() => undefined);
  return device;
}

export async function sendCommand({ calls, identity, device, allowed, machine, payload, at, meta = {} }) {
  if (allowed !== true) throw new CommandProblem("view-only");
  if (!device) throw new CommandProblem("no-device");
  const envelope = await sealCommand({ device, account: identity.key, machine, payload, at });
  await calls.command(envelope);
  return { id: envelope.id, kind: payload.type, machine: machine.id, address: payload.address, at, exp: envelope.exp, device: device.id, deviceName: device.cert.name, ...meta, result: null };
}

export async function openResult(entry, identity, machineKeys, now) {
  if (entry.envelope?.machine !== entry.machine || entry.envelope?.id !== entry.command) throw new CommandProblem("misfiled-result");
  const payload = await openEnvelope(entry.envelope, identity, await importSender(entry.machine, machineKeys), now);
  if (payload.type !== "result" || payload.command !== entry.command) throw new CommandProblem("misfiled-result");
  return { outcome: payload.outcome, reason: payload.reason ?? null, ...(payload.blocking ? { blocking: payload.blocking } : {}), ...(payload.blob ? { blob: payload.blob } : {}), text: payload.text, device: payload.device, at: entry.stored_at };
}

export async function openHistory(commands, identity, keysOf) {
  const opened = [];
  for (const entry of commands) {
    try {
      const { device, payload } = await openCommand(entry.envelope, identity, identity.key, entry.envelope.at);
      if (payload.type !== entry.kind || entry.envelope.machine !== entry.machine) continue;
      const keys = keysOf(entry.machine);
      const result = entry.result && keys ? await openResult({ machine: entry.machine, command: entry.command, envelope: entry.result, stored_at: entry.result_at }, identity, keys, entry.result.at).catch(() => null) : null;
      opened.push({ id: entry.command, machine: entry.machine, kind: entry.kind, device: device.device, payload, at: entry.queued_at, exp: entry.envelope.exp, delivered: entry.delivered_at, result });
    } catch {}
  }
  return opened;
}

export async function collectResults({ calls, identity, kept, keysOf, now, wait = 0 }) {
  const listing = await calls.results(kept.cursor, wait);
  const sent = [...kept.sent];
  for (const entry of listing.results) {
    const index = sent.findIndex((one) => one.id === entry.command && one.machine === entry.machine);
    const keys = keysOf(entry.machine);
    if (index < 0 || !keys) continue;
    try {
      sent[index] = { ...sent[index], result: await openResult(entry, identity, keys, now) };
    } catch {}
  }
  return { cursor: listing.cursor, sent };
}
