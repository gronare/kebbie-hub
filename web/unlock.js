import { accountFingerprints, accountIdentity, encryptionPkcs8, fromB64, newAccountSecrets, signStatement, toB64, unwrapAccountSecrets, wrapAccountSecrets, wrapperStatement } from "../js/protocol.js";
import { HubError } from "./hub.js";
import * as deviceStore from "./store.js";
import { openEncryption, sealKey } from "./store.js";
import { browserPush, LISTING, NOTIFY_RECORD } from "./notify.js";
import { assertionJSON, creationOptions, passkeyName, prfOutput, registrationJSON, requestOptions } from "./passkey.js";

const REMEMBERED = "kebbie-credential";
const KEPT = "unlock";
export { LISTING };
export const KEEP_MS = 60 * 60 * 1000;

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
  const got = await credentials.get(requestOptions(begun.options, { byCredential: begun.prf_salts }));
  return { signedIn: await account.loginFinish(begun.ceremony, assertionJSON(got)), prf: prfOutput(got), salt: begun.prf_salts?.[credential] ?? null };
}

const withoutUnlock = ({ unlock, ...me }) => me;

const unlockOf = (me, credential, prf, salt) => ({ account: me.account, credential: fromB64(credential), version: me.key?.version ?? 1, prfOutput: prf, salt: fromB64(salt) });

const saltChange = (salt, unlock) => (salt === unlock.prf_salt ? {} : { prf_salt: salt });

async function sealedEncryption(secrets) {
  const pkcs8 = encryptionPkcs8(secrets.encryption);
  try {
    return await sealKey(pkcs8);
  } catch {
    return null;
  } finally {
    pkcs8.fill(0);
  }
}

async function createKey(hub, me, unlock, prf, salt) {
  const fresh = await newAccountSecrets(me.account);
  try {
    const params = unlockOf({ ...me, key: fresh.key }, unlock.credential, prf, salt);
    const wrapped = await wrapAccountSecrets(params, fresh.secrets);
    const identity = await accountIdentity(fresh.key, fresh.secrets, me.account);
    const signature = await signStatement(identity, await wrapperStatement(fresh.key, params.credential, wrapped));
    await hub.account.setKey({ key: fresh.key, credential: unlock.credential, wrapped: toB64(wrapped), signature: toB64(signature), ...saltChange(salt, unlock) });
    return { state: "unlocked", me: { ...me, key: fresh.key }, identity, credential: unlock.credential, sealed: await sealedEncryption(fresh.secrets) };
  } finally {
    fresh.wipe();
  }
}

async function unlockOntoSalt(hub, me, unlock, prf, salt) {
  const before = await prfCeremony(hub, unlock.credential);
  if (!before.prf) return { state: "no-prf", me, credential: unlock.credential };
  try {
    if (!before.signedIn.unlock.wrapped) return { state: "not-wrapped", me, credential: unlock.credential };
    const opened = await unwrapAccountSecrets(unlockOf(me, unlock.credential, before.prf, before.salt), fromB64(before.signedIn.unlock.wrapped));
    try {
      const identity = await accountIdentity(me.key, opened, me.account);
      const params = unlockOf(me, unlock.credential, prf, salt);
      const wrapped = await wrapAccountSecrets(params, opened);
      const signature = await signStatement(identity, await wrapperStatement(me.key, params.credential, wrapped));
      await hub.account.putWrapper(unlock.credential, { key_version: me.key.version, wrapped: toB64(wrapped), signature: toB64(signature), prf_salt: salt });
      return { state: "unlocked", me, identity, credential: unlock.credential, sealed: await sealedEncryption(opened) };
    } finally {
      opened.wipe();
    }
  } finally {
    before.prf.fill(0);
  }
}

export async function keepUnlock(session, sealed, store = deviceStore) {
  if (!sealed) return forgetUnlock(store);
  const { encryption, ...identity } = session.identity;
  try {
    await store.put(KEPT, { identity, encryption: sealed, until: session.until });
  } catch {}
}

async function keptIdentity(kept) {
  if (kept.identity?.encryption) return kept.identity;
  return { ...kept.identity, encryption: await openEncryption(kept.encryption, kept.identity.key.keys.encryption) };
}

export const forgetUnlock = (store = deviceStore) => store.del(KEPT).catch(() => undefined);

export async function stopNotifications(account, store = deviceStore, subscription = browserPush) {
  const record = await store.get(NOTIFY_RECORD).catch(() => null);
  await (await subscription().catch(() => null))?.unsubscribe().catch(() => undefined);
  if (record?.device) await account.unsubscribePush(record.device).catch(() => undefined);
  await store.del(NOTIFY_RECORD).catch(() => undefined);
}

export async function signOut(account, store = deviceStore, subscription = browserPush) {
  try {
    await stopNotifications(account, store, subscription);
    await account.logout();
  } finally {
    await forgetUnlock(store);
    await store.del(LISTING).catch(() => undefined);
  }
}

const nonExtractable = (identity) => identity?.signing?.extractable === false && identity.encryption?.privateKey?.extractable === false;

async function sameKey(identity, me) {
  const key = identity?.key;
  if (!me?.key || identity.id !== me.account || key?.account !== me.account || key.version !== me.key.version) return false;
  try {
    return (await signingPrint(key)) === (await signingPrint(me.key));
  } catch {
    return false;
  }
}

export async function keptUnlock(me, store = deviceStore, now = Date.now(), storage = localStore()) {
  let kept;
  try {
    kept = await store.get(KEPT);
  } catch {
    return null;
  }
  if (!kept) return null;
  const fresh = kept.until > now && kept.until - now <= KEEP_MS;
  if (fresh && !me) return null;
  const held = remembered(storage);
  const credential = held?.account === me?.account ? held.credential : null;
  const identity = fresh && credential ? await keptIdentity(kept).catch(() => null) : null;
  if (identity && nonExtractable(identity) && (await sameKey(identity, me))) return { state: "unlocked", me, identity, credential, until: kept.until };
  await forgetUnlock(store);
  return null;
}

export function lockWhenDue(relock, { store = deviceStore, page = globalThis.document, clock = () => Date.now() } = {}) {
  let watched = null;
  let timer = null;
  const until = () => watched?.until ?? 0;
  const expire = () => {
    clearTimeout(timer);
    watched = null;
    relock();
    return forgetUnlock(store);
  };
  const check = () => (watched && clock() >= until() ? expire() : undefined);
  const arm = () => {
    timer = setTimeout(() => check() ?? (watched && arm()), Math.max(0, until() - clock()));
  };
  page?.addEventListener?.("visibilitychange", () => page.visibilityState === "visible" && check());
  return {
    check,
    watch(session) {
      const next = session?.state === "unlocked" ? session : null;
      if (next === watched) return;
      clearTimeout(timer);
      watched = next;
      if (watched) arm();
    },
  };
}

export async function signedInAccount(account, store = deviceStore) {
  try {
    return await account.me();
  } catch (error) {
    if (error instanceof HubError && error.status === 401) await forgetUnlock(store);
    return null;
  }
}

export async function unlockAfter(hub, ceremony, storage = localStore(), store = deviceStore, clock = Date.now) {
  const { sealed, ...settled } = await settleUnlock(hub, ceremony, storage);
  if (settled.state !== "unlocked") {
    await forgetUnlock(store);
    return settled;
  }
  const session = { ...settled, until: clock() + KEEP_MS };
  await keepUnlock(session, sealed, store);
  return session;
}

async function settleUnlock(hub, ceremony, storage) {
  let { signedIn, prf, salt } = ceremony;
  if (!prf) ({ signedIn, prf, salt } = await prfCeremony(hub, signedIn.unlock.credential));
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
        const created = await createKey(hub, me, unlock, prf, salt);
        rememberCredential(unlock.credential, me.account, await signingPrint(created.me.key), storage);
        return created;
      } catch (error) {
        if (!(error instanceof HubError && error.code === "key-exists")) throw error;
        return { state: "not-wrapped", me: await hub.account.me(), credential: unlock.credential };
      }
    }
    if (!unlock.wrapped) return { state: "not-wrapped", me, credential: unlock.credential };
    if (salt !== unlock.prf_salt) return await unlockOntoSalt(hub, me, unlock, prf, salt);
    const opened = await unwrapAccountSecrets(unlockOf(me, unlock.credential, prf, salt), fromB64(unlock.wrapped));
    try {
      return { state: "unlocked", me, identity: await accountIdentity(me.key, opened, me.account), credential: unlock.credential, sealed: await sealedEncryption(opened) };
    } finally {
      opened.wipe();
    }
  } finally {
    prf.fill(0);
  }
}

export async function addFirstPasskey(hub, me, name, storage = localStore(), store = deviceStore) {
  const begun = await hub.account.passkeyBegin();
  const created = await hub.credentials.create(creationOptions(begun.options, begun.prf_salt, passkeyName(me)));
  const added = await hub.account.passkeyFinish(begun.ceremony, registrationJSON(created), name);
  const signedIn = { ...me, key_access: true, unlock: { credential: added.passkey, prf_salt: begun.prf_salt, wrapped: null } };
  return unlockAfter(hub, { signedIn, prf: prfOutput(created), salt: begun.prf_salt }, storage, store);
}

export async function addUnlockingPasskey(hub, session, name) {
  const begun = await hub.account.passkeyBegin();
  const created = await hub.credentials.create(creationOptions(begun.options, begun.prf_salt, passkeyName(session?.me)));
  const added = await hub.account.passkeyFinish(begun.ceremony, registrationJSON(created), name);
  if (session?.state !== "unlocked") return { added, unlocks: false };
  const fromCreate = prfOutput(created);
  const { prf, salt } = fromCreate ? { prf: fromCreate, salt: begun.prf_salt } : await prfCeremony(hub, added.passkey);
  if (!prf) return { added, unlocks: false };
  const current = await prfCeremony(hub, session.credential);
  if (!current.prf || !current.signedIn.unlock.wrapped) {
    prf.fill(0);
    return { added, unlocks: false };
  }
  const { key } = session.me;
  const opened = await unwrapAccountSecrets(unlockOf(session.me, current.signedIn.unlock.credential, current.prf, current.salt), fromB64(current.signedIn.unlock.wrapped));
  try {
    await accountIdentity(key, opened, session.me.account);
    const credential = fromB64(added.passkey);
    const wrapped = await wrapAccountSecrets({ account: key.account, credential, version: key.version, prfOutput: prf, salt: fromB64(salt) }, opened);
    const signature = await signStatement(session.identity, await wrapperStatement(key, credential, wrapped));
    await hub.account.putWrapper(added.passkey, { key_version: key.version, wrapped: toB64(wrapped), signature: toB64(signature) });
    return { added, unlocks: true };
  } finally {
    opened.wipe();
    prf.fill(0);
    current.prf.fill(0);
  }
}
