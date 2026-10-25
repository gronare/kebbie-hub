import { accountCommitment, machineFingerprint, machineListWith, machineListWithout, pairingNonce, parseConnectFragment, signMachineList, toB64 } from "../js/protocol.js";
import { emptyMarks, trustedList } from "./board.js";
import { HubError } from "./hub.js";

export { parseConnectFragment };

const WAIT_SECONDS = 25;

export class PairingFailed extends Error {
  constructor(reason, machine = null) {
    super(reason);
    this.reason = reason;
    this.machine = machine;
  }
}

function ended(view) {
  if (view.state === "refused") return new PairingFailed(`refused-${view.reason ?? "declined"}`, view.machine);
  if (view.state === "failed") return new PairingFailed(view.reason ?? "failed", view.machine);
  return new PairingFailed(view.state, view.machine);
}

async function next(hub, pairing, seen, deadline, clock) {
  while (clock() < deadline) {
    const view = await hub.watch(pairing, seen, Math.max(1, Math.min(WAIT_SECONDS, Math.ceil((deadline - clock()) / 1000))));
    if (view.state !== seen) return view;
  }
  throw new PairingFailed("timeout");
}

export function safeNext(value) {
  return typeof value === "string" && /^\/(?![/\\])/.test(value) ? value : null;
}

export async function withReauth(act, reauth) {
  try {
    return await act();
  } catch (error) {
    if (!(error instanceof HubError) || error.code !== "reauth-required") throw error;
    await reauth();
    return act();
  }
}

export async function connectCommitment(key, fragment, nonce = pairingNonce()) {
  return { nonce, commitment: toB64(await accountCommitment(key, nonce, fragment.code)) };
}

export async function publishList({ hub, identity, marks = emptyMarks(), change }) {
  for (let attempt = 0; ; attempt += 1) {
    const trusted = await trustedList(await hub.boards(), identity, marks);
    if (trusted.problem) throw new PairingFailed(trusted.problem);
    const list = change(trusted.list);
    try {
      await hub.putMachines({ list, signature: await signMachineList(identity, list) });
      return list;
    } catch (error) {
      if (!(error instanceof HubError && error.code === "stale-list") || attempt > 0) throw error;
    }
  }
}

export const listWithout = (machine) => (list) => machineListWithout(list, machine);

export function connectBlock({ fragment, session }) {
  if (!fragment) return null;
  if (session?.state === "unlocked") return null;
  return session?.state ?? "locked";
}

const FRAGMENT = (code) => `kebbie-connect:${code}`;

export function keptFragment(storage, code, hash) {
  try {
    if (hash) storage?.setItem(FRAGMENT(code), hash);
    return parseConnectFragment(hash || (storage?.getItem(FRAGMENT(code)) ?? ""));
  } catch {
    return parseConnectFragment(hash);
  }
}

export function forgetFragment(storage, code) {
  try {
    storage?.removeItem(FRAGMENT(code));
  } catch {}
}

export async function finishConnect({ hub, pairing, identity, fragment, nonce, marks = emptyMarks(), timeoutMs = 10 * 60_000, clock = Date.now }) {
  const deadline = clock() + timeoutMs;
  const replied = await next(hub, pairing, "committed", deadline, clock);
  if (replied.state !== "replied") throw ended(replied);
  if (!replied.machine_keys) throw new PairingFailed("incomplete", replied.machine);
  if ((await machineFingerprint(replied.machine_keys)) !== fragment.fingerprint) throw new PairingFailed("wrong-machine", replied.machine);
  await hub.reveal(pairing, identity.key, toB64(nonce));
  const settled = await next(hub, pairing, "revealed", deadline, clock);
  if (settled.state !== "admitted") throw ended(settled);
  if (settled.machine !== replied.machine) throw new PairingFailed("machine-changed", settled.machine);
  const list = await publishList({ hub, identity, marks, change: (current) => machineListWith(current, replied.machine, replied.machine_keys) });
  return { id: replied.machine, keys: structuredClone(replied.machine_keys), hubName: replied.machine_name ?? null, counter: list.counter };
}

export const CONNECT_TEXT = {
  "unknown-code": "This code has expired or was already used. Run kebbie hub connect again.",
  "wrong-machine": "The hub offered keys of another machine than the one that opened this link. Nothing was trusted. Run kebbie hub connect again.",
  "machine-changed": "The hub switched machines halfway. Nothing was trusted.",
  "refused-mismatch": "The machine refused your account's key. Run kebbie hub connect again.",
  "not-your-machine": "This machine belongs to another account.",
  "no-key": "Your account has no key yet. Sign in with a passkey that can unlock it first.",
  "keys-differ": "The hub holds another key for your account than this device unlocked. Nothing was handed over.",
  "rate-limited": "Too many tries. Wait a minute.",
  incomplete: "The hub left out the machine's keys.",
  "rolled-back-list": "The hub served an older list of your machines than this device has seen. Nothing was changed.",
  "unknown-machine": "The hub's list of your machines is not signed by your account. Nothing was changed.",
  "stale-list": "Another device changed your list of machines at the same time. Try again.",
  expired: "The connection timed out. Run kebbie hub connect again.",
  timeout: "The machine did not answer in time. Run kebbie hub connect again.",
};

export const connectText = (reason) => CONNECT_TEXT[reason] ?? `That did not work (${reason}).`;

export function planLine(plan, billingUrl) {
  if (plan?.entitled ?? true) return null;
  return billingUrl ? { text: "Your plan is not active. Start a subscription to connect machines.", url: billingUrl } : { text: "Your plan is not active.", url: null };
}
