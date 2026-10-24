import { machineFingerprint, pairingCommitment, pairingNonce, parseConnectFragment, toB64 } from "../js/protocol.js";
import { HubError } from "./hub.js";
import { ended, next, PairingFailed } from "./pairing.js";

export { parseConnectFragment };

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

export async function connectCommitment(publicKeys, fragment, nonce = pairingNonce()) {
  return { nonce, commitment: toB64(await pairingCommitment(publicKeys, nonce, fragment.code)) };
}

export async function finishConnect({ hub, pairing, publicKeys, fragment, nonce, timeoutMs = 10 * 60_000, clock = Date.now }) {
  const deadline = clock() + timeoutMs;
  const replied = await next(hub, pairing, "committed", deadline, clock);
  if (replied.state !== "replied") throw ended(replied);
  if (!replied.machine_keys || !replied.machine_nonce) throw new PairingFailed("incomplete", replied.machine);
  if ((await machineFingerprint(replied.machine_keys)) !== fragment.fingerprint) throw new PairingFailed("wrong-machine", replied.machine);
  await hub.reveal(pairing, publicKeys, toB64(nonce));
  const settled = await next(hub, pairing, "revealed", deadline, clock);
  if (settled.state !== "admitted") throw ended(settled);
  if (settled.machine !== replied.machine) throw new PairingFailed("machine-changed", settled.machine);
  return { id: replied.machine, keys: structuredClone(replied.machine_keys), hubName: replied.machine_name ?? null };
}

export const CONNECT_TEXT = {
  "unknown-code": "This code has expired or was already used. Run kelpie hub connect again.",
  "wrong-machine": "The hub offered keys of another machine than the one that opened this link. Nothing was trusted. Run kelpie hub connect again.",
  "machine-changed": "The hub switched machines halfway. Nothing was trusted.",
  "refused-mismatch": "The machine refused this browser. Run kelpie hub connect again.",
  "not-your-machine": "This machine belongs to another account.",
  "rate-limited": "Too many tries. Wait a minute.",
  expired: "The connection timed out. Run kelpie hub connect again.",
  timeout: "The machine did not answer in time. Run kelpie hub connect again.",
};

export const connectText = (reason) => CONNECT_TEXT[reason] ?? `That did not work (${reason}).`;

export function planLine(plan, billingUrl) {
  if (plan?.entitled ?? true) return null;
  return billingUrl ? { text: "Your plan is not active. Start a subscription to connect machines.", url: billingUrl } : { text: "Your plan is not active.", url: null };
}
