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
  "bad-code": "Koden ska vara åtta tecken, som den kelpie hub pair visar.",
  "unknown-code": "Koden finns inte eller har gått ut. Kör kelpie hub pair igen.",
  "refused-mismatch": "Maskinen avbröt: nycklarna stämde inte. Något kan stå mellan dig och maskinen.",
  "refused-declined": "Du svarade n i terminalen. Telefonen kopplades inte.",
  "refused-timeout": "Maskinen slutade vänta. Kör kelpie hub pair igen.",
  "second-commitment": "Koden användes av en annan enhet samtidigt. Kör kelpie hub pair igen.",
  expired: "Koden gick ut. Kör kelpie hub pair igen.",
  timeout: "Ingen svarade i tid. Kör kelpie hub pair igen.",
  "machine-changed": "Hubben bytte maskin mitt i kopplingen. Telefonen litar inte på den.",
  incomplete: "Hubben skickade inte maskinens nycklar.",
  "phone-mismatch": "Du såg andra siffror än terminalen. Telefonen litar inte på maskinen; svara n i terminalen och försök igen.",
};

export const failureText = (reason) => FAILURE_TEXT[reason] ?? `Kopplingen misslyckades (${reason}).`;
