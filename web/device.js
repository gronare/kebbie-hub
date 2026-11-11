import { importSender, openEnvelope, sealCommand } from "../js/protocol.js";

const sentKey = (account) => `hub-sent:${account}`;
const allowKey = (account) => `allow-changes:${account}`;
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

export async function sendCommand({ calls, identity, allowed, machine, payload, at, meta = {} }) {
  if (allowed !== true) throw new CommandProblem("view-only");
  const envelope = await sealCommand({ identity, machine, payload, at });
  await calls.command(envelope);
  return { id: envelope.id, kind: payload.type, machine: machine.id, address: payload.address, at, exp: envelope.exp, ...meta, result: null };
}

export async function openResult(entry, identity, machineKeys, now) {
  if (entry.envelope?.machine !== entry.machine || entry.envelope?.id !== entry.command) throw new CommandProblem("misfiled-result");
  const payload = await openEnvelope(entry.envelope, identity, await importSender(entry.machine, machineKeys), now);
  if (payload.type !== "result" || payload.command !== entry.command) throw new CommandProblem("misfiled-result");
  return { outcome: payload.outcome, reason: payload.reason ?? null, text: payload.text, at: entry.stored_at };
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
