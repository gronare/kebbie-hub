import { deviceFingerprint, deviceId, importSender, newDeviceKey, openCommand, openEnvelope, sealCommand, signDeviceCert, verifyDeviceCert } from "../js/protocol.js";

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

export const keepSent = (store, account, kept) => store.put(sentKey(account), kept).catch(() => undefined);

export async function loadDevice(store, account) {
  try {
    const kept = await store.get(deviceKey(account));
    return kept?.cert?.account === account && kept.signing ? kept : null;
  } catch {
    return null;
  }
}

export async function ensureDevice({ store, calls, identity, name, now, kept = undefined }) {
  const held = kept === undefined ? await loadDevice(store, identity.id) : kept;
  if (held && (await verifyDeviceCert(identity.key, held.cert))) return held;
  const key = await newDeviceKey();
  const id = deviceId(now * 1000);
  const cert = await signDeviceCert(identity, { device: id, name, signing: key.public, created_at: now });
  await calls.addDevice(cert);
  const device = { id, cert, signing: key.signing, fingerprint: await deviceFingerprint(key.public) };
  await store.put(deviceKey(identity.id), device).catch(() => undefined);
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
  return { outcome: payload.outcome, reason: payload.reason ?? null, ...(payload.blocking ? { blocking: payload.blocking } : {}), text: payload.text, device: payload.device, at: entry.stored_at };
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
