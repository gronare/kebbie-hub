import { comparisonCode, pairingCommitment, pairingNonce, toB64 } from "../js/protocol.js";

const ALPHABET = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const CONFUSABLE = { O: "0", I: "1", L: "1" };
const WAIT_SECONDS = 25;

export class PairingFailed extends Error {
  constructor(reason, machine = null) {
    super(reason);
    this.reason = reason;
    this.machine = machine;
  }
}

export function normalizeCode(typed) {
  const symbols = [...typed.replace(/[- ]/g, "").toUpperCase()].map((char) => CONFUSABLE[char] ?? char);
  if (symbols.length !== 8 || !symbols.every((char) => ALPHABET.includes(char))) return null;
  return `${symbols.slice(0, 4).join("")}-${symbols.slice(4).join("")}`;
}

export function ended(view) {
  if (view.state === "refused") return new PairingFailed(`refused-${view.reason ?? "declined"}`, view.machine);
  if (view.state === "failed") return new PairingFailed(view.reason ?? "failed", view.machine);
  return new PairingFailed(view.state, view.machine);
}

export async function next(hub, pairing, seen, deadline, clock) {
  while (clock() < deadline) {
    const view = await hub.watch(pairing, seen, Math.max(1, Math.min(WAIT_SECONDS, Math.ceil((deadline - clock()) / 1000))));
    if (view.state !== seen) return view;
  }
  throw new PairingFailed("timeout");
}

export async function pair({ hub, publicKeys, typed, confirm, timeoutMs = 10 * 60_000, clock = Date.now, nonce = pairingNonce() }) {
  const code = normalizeCode(typed);
  if (!code) throw new PairingFailed("bad-code");
  const deadline = clock() + timeoutMs;
  const commitment = await pairingCommitment(publicKeys, nonce, code);
  const committed = await hub.commit(code, toB64(commitment)).catch((error) => {
    if (error.status === 401) throw new PairingFailed("unknown-code");
    if (error.status === 409) throw new PairingFailed("second-commitment");
    throw error;
  });
  const replied = await next(hub, committed.pairing, "committed", deadline, clock);
  if (replied.state !== "replied") throw ended(replied);
  if (!replied.machine_keys || !replied.machine_nonce) throw new PairingFailed("incomplete", replied.machine);
  const machine = { id: replied.machine, keys: structuredClone(replied.machine_keys), hubName: replied.machine_name ?? null };
  await hub.reveal(committed.pairing, publicKeys, toB64(nonce));
  const digits = await comparisonCode(machine.keys, publicKeys, Uint8Array.fromBase64(replied.machine_nonce, { alphabet: "base64url" }), nonce, code);
  const settled = next(hub, committed.pairing, "revealed", deadline, clock).then((view) => {
    if (view.state !== "admitted") throw ended(view);
    if (view.machine !== machine.id) throw new PairingFailed("machine-changed", view.machine);
  });
  settled.catch(() => {});
  const answer = await Promise.race([confirm(digits, machine), settled.then(() => new Promise(() => {}))]);
  if (!answer?.matches) throw new PairingFailed("phone-mismatch", machine.id);
  await settled;
  const own = answer.name?.trim();
  return { id: machine.id, keys: machine.keys, name: own && own !== machine.hubName ? own : null, hubName: machine.hubName };
}

export const FAILURE_TEXT = {
  "bad-code": "The code is eight characters, like the one kelpie hub pair shows.",
  "unknown-code": "That code does not exist or has expired. Run kelpie hub pair again.",
  "refused-mismatch": "The machine stopped: the keys did not match. Something may be between you and the machine.",
  "refused-declined": "You answered n in the terminal. This device was not paired.",
  "refused-timeout": "The machine stopped waiting. Run kelpie hub pair again.",
  "second-commitment": "Another device used the code at the same time. Run kelpie hub pair again.",
  expired: "The code expired. Run kelpie hub pair again.",
  timeout: "Nobody answered in time. Run kelpie hub pair again.",
  "machine-changed": "The hub switched machines halfway. This device does not trust it.",
  incomplete: "The hub left out the machine's keys.",
  "phone-mismatch": "You saw different digits than the terminal. This device does not trust the machine; answer n in the terminal and try again.",
};

export const failureText = (reason) => FAILURE_TEXT[reason] ?? `Pairing failed (${reason}).`;
