import { deviceListWith, deviceListWithout, emptyDeviceList, importSender, newDeviceKeys, openEnvelope, sealCommand, signDeviceList, verifyDeviceList } from "../js/protocol.js";
import { HubError } from "./hub.js";

const deviceKey = (account) => `hub-device:${account}`;
const sentKey = (account) => `hub-sent:${account}`;
const KEEP_SENT = 7 * 86_400;

export class DeviceListProblem extends Error {
  constructor(reason) {
    super(reason);
    this.reason = reason;
  }
}

export async function loadDevice(store, account) {
  try {
    return (await store.get(deviceKey(account))) ?? null;
  } catch {
    return null;
  }
}

export const forgetDevice = (store, account) => store.del(deviceKey(account)).catch(() => undefined);

export async function trustedDeviceList(view, identity, seen = 0) {
  if (!view?.list) return { list: emptyDeviceList() };
  if (!(await verifyDeviceList(identity, view.list))) return { problem: "unsigned-device-list" };
  if (view.list.list.counter < seen) return { problem: "rolled-back-device-list" };
  return { list: view.list.list };
}

export async function publishDeviceList({ hub, identity, seen = 0, change }) {
  for (let attempt = 0; ; attempt += 1) {
    const trusted = await trustedDeviceList(await hub.deviceList(), identity, seen);
    if (trusted.problem) throw new DeviceListProblem(trusted.problem);
    const list = change(trusted.list);
    try {
      await hub.putDeviceList({ list, signature: await signDeviceList(identity, list) });
      return list;
    } catch (error) {
      if (!(error instanceof HubError && error.code === "stale-list") || attempt > 0) throw error;
    }
  }
}

export async function enableDevice({ hub, store, identity, name, reauth, seen = 0 }) {
  let device = await loadDevice(store, identity.id);
  if (!device) {
    const keys = await newDeviceKeys();
    const registered = await hub.registerDevice({ name, keys: keys.public });
    device = { account: identity.id, id: registered.device, token: registered.token, name, ...keys, listed: 0 };
    await store.put(deviceKey(identity.id), device);
  }
  const fresh = await reauth();
  const list = await publishDeviceList({ hub, identity: fresh ?? identity, seen, change: (current) => deviceListWith(current, { device: device.id, name: device.name, keys: device.public, role: "control" }) });
  device = { ...device, listed: list.counter };
  await store.put(deviceKey(identity.id), device);
  return { device, list };
}

export async function removeListedDevice({ hub, store, identity, device, reauth, seen = 0 }) {
  const fresh = await reauth();
  const list = await publishDeviceList({ hub, identity: fresh ?? identity, seen, change: (current) => deviceListWithout(current, device) });
  await hub.removeDevice(device).catch((error) => {
    if (!(error instanceof HubError && error.status === 404)) throw error;
  });
  const mine = await loadDevice(store, identity.id);
  if (mine?.id === device) await forgetDevice(store, identity.id);
  return list;
}

export async function loadSent(store, account, now) {
  try {
    const kept = (await store.get(sentKey(account))) ?? { cursor: 0, sent: [] };
    return { cursor: kept.cursor ?? 0, sent: (kept.sent ?? []).filter((one) => one.at + KEEP_SENT > now) };
  } catch {
    return { cursor: 0, sent: [] };
  }
}

export const keepSent = (store, account, kept) => store.put(sentKey(account), kept).catch(() => undefined);

export async function sendCommand({ calls, device, machine, payload, at, meta = {} }) {
  const envelope = await sealCommand({ device, machine, payload, at });
  await calls.command(device.token, envelope);
  return { id: envelope.id, kind: payload.type, machine: machine.id, address: payload.address, at, exp: envelope.exp, ...meta, result: null };
}

export async function openResult(entry, device, machineKeys, now) {
  if (entry.envelope?.machine !== entry.machine || entry.envelope?.id !== entry.command) throw new DeviceListProblem("misfiled-result");
  const payload = await openEnvelope(entry.envelope, device, await importSender(entry.machine, machineKeys), now);
  if (payload.type !== "result" || payload.command !== entry.command) throw new DeviceListProblem("misfiled-result");
  return { outcome: payload.outcome, reason: payload.reason ?? null, text: payload.text, at: entry.stored_at };
}

export async function collectResults({ calls, device, kept, keysOf, now, wait = 0 }) {
  const listing = await calls.results(device.token, kept.cursor, wait);
  const sent = [...kept.sent];
  for (const entry of listing.results) {
    const index = sent.findIndex((one) => one.id === entry.command && one.machine === entry.machine);
    const keys = keysOf(entry.machine);
    if (index < 0 || !keys) continue;
    try {
      sent[index] = { ...sent[index], result: await openResult(entry, device, keys, now) };
    } catch {}
  }
  return { cursor: listing.cursor, sent };
}
