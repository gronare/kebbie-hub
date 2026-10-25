import { accountFingerprints, accountIdentity, fromB64, newAccountSecrets, signStatement, toB64, unwrapAccountSecrets, wrapAccountSecrets, wrapperStatement } from "../js/protocol.js";
import { HubError } from "./hub.js";
import { assertionJSON, creationOptions, prfOutput, registrationJSON, requestOptions } from "./passkey.js";

const REMEMBERED = "kebbie-credential";

export const STATE_TEXT = {
  "no-prf": "This device can't unlock with its passkey, so it can't read your boards.",
  "not-wrapped": "This passkey can't unlock your boards yet. Sign in once with a passkey that can, and add this one from there.",
  locked: "Your boards are locked on this device. Unlock them with your passkey.",
  "key-missing": "The hub says your account has no key, but this device has unlocked one before. Nothing was created. Ask the hub's owner what happened.",
  "key-changed": "The hub offers another key for your account than the one this device unlocked before. Nothing was read.",
};

const localStore = () => {
  try {
    return globalThis.localStorage ?? null;
  } catch {
    return null;
  }
};

export function remembered(storage = localStore()) {
  try {
    const kept = JSON.parse(storage?.getItem(REMEMBERED) ?? "null");
    return kept && typeof kept.credential === "string" ? kept : null;
  } catch {
    return null;
  }
}

export const rememberedCredential = (storage = localStore()) => remembered(storage)?.credential ?? null;

export function rememberCredential(credential, account = null, signing = null, storage = localStore()) {
  try {
    const held = remembered(storage);
    const fingerprint = signing ?? (held?.account === account ? held.signing : null);
    storage?.setItem(REMEMBERED, JSON.stringify({ credential, account, signing: fingerprint ?? null }));
  } catch {}
}

const signingPrint = async (key) => (await accountFingerprints(key)).signing;

async function prfCeremony({ account, credentials }, credential) {
  const begun = await account.loginBegin({ credential });
  const got = await credentials.get(requestOptions(begun.options, begun.prf_salts));
  return { signedIn: await account.loginFinish(begun.ceremony, assertionJSON(got)), prf: prfOutput(got) };
}

const withoutUnlock = ({ unlock, ...me }) => me;

const unlockOf = (me, unlock, prf) => ({ account: me.account, credential: fromB64(unlock.credential), version: me.key?.version ?? 1, prfOutput: prf, salt: fromB64(unlock.prf_salt) });

async function createKey(hub, me, unlock, prf) {
  const fresh = await newAccountSecrets(me.account);
  try {
    const params = unlockOf({ ...me, key: fresh.key }, unlock, prf);
    const wrapped = await wrapAccountSecrets(params, fresh.secrets);
    const identity = await accountIdentity(fresh.key, fresh.secrets, me.account);
    const signature = await signStatement(identity, await wrapperStatement(fresh.key, params.credential, wrapped));
    await hub.account.setKey({ key: fresh.key, credential: unlock.credential, wrapped: toB64(wrapped), signature: toB64(signature) });
    return { state: "unlocked", me: { ...me, key: fresh.key }, identity, credential: unlock.credential };
  } finally {
    fresh.wipe();
  }
}

export async function unlockAfter(hub, ceremony, storage = localStore()) {
  let { signedIn, prf } = ceremony;
  if (!prf) ({ signedIn, prf } = await prfCeremony(hub, signedIn.unlock.credential));
  const { unlock } = signedIn;
  const me = withoutUnlock(signedIn);
  const held = remembered(storage);
  const seen = held?.account === me.account ? held.signing : null;
  if (me.key && me.key.account !== me.account) return { state: "key-changed", me, credential: unlock.credential };
  if (me.key && seen && (await signingPrint(me.key)) !== seen) return { state: "key-changed", me, credential: unlock.credential };
  if (!me.key && seen) return { state: "key-missing", me, credential: unlock.credential };
  rememberCredential(unlock.credential, me.account, me.key ? await signingPrint(me.key) : null, storage);
  if (!prf) return { state: "no-prf", me, credential: unlock.credential };
  try {
    if (!me.key) {
      try {
        const created = await createKey(hub, me, unlock, prf);
        rememberCredential(unlock.credential, me.account, await signingPrint(created.me.key), storage);
        return created;
      } catch (error) {
        if (!(error instanceof HubError && error.code === "key-exists")) throw error;
        return { state: "not-wrapped", me: await hub.account.me(), credential: unlock.credential };
      }
    }
    if (!unlock.wrapped) return { state: "not-wrapped", me, credential: unlock.credential };
    const opened = await unwrapAccountSecrets(unlockOf(me, unlock, prf), fromB64(unlock.wrapped));
    try {
      return { state: "unlocked", me, identity: await accountIdentity(me.key, opened, me.account), credential: unlock.credential };
    } finally {
      opened.wipe();
    }
  } finally {
    prf.fill(0);
  }
}

export async function addUnlockingPasskey(hub, session, name) {
  const begun = await hub.account.passkeyBegin();
  const created = await hub.credentials.create(creationOptions(begun.options, begun.prf_salt));
  const added = await hub.account.passkeyFinish(begun.ceremony, registrationJSON(created), name);
  if (session?.state !== "unlocked") return { added, unlocks: false };
  const prf = prfOutput(created) ?? (await prfCeremony(hub, added.passkey)).prf;
  if (!prf) return { added, unlocks: false };
  const current = await prfCeremony(hub, session.credential);
  if (!current.prf || !current.signedIn.unlock.wrapped) {
    prf.fill(0);
    return { added, unlocks: false };
  }
  const { key } = session.me;
  const opened = await unwrapAccountSecrets(unlockOf(session.me, current.signedIn.unlock, current.prf), fromB64(current.signedIn.unlock.wrapped));
  try {
    await accountIdentity(key, opened, session.me.account);
    const credential = fromB64(added.passkey);
    const wrapped = await wrapAccountSecrets({ account: key.account, credential, version: key.version, prfOutput: prf, salt: fromB64(begun.prf_salt) }, opened);
    const signature = await signStatement(session.identity, await wrapperStatement(key, credential, wrapped));
    await hub.account.putWrapper(added.passkey, { key_version: key.version, wrapped: toB64(wrapped), signature: toB64(signature) });
    return { added, unlocks: true };
  } finally {
    opened.wipe();
    prf.fill(0);
    current.prf.fill(0);
  }
}
