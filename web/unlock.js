import { accountFingerprints, accountIdentity, encryptionPkcs8, fromB64, newAccountSecrets, signStatement, toB64, unwrapAccountSecrets, wrapAccountSecrets, wrapperStatement } from "../js/protocol.js";
import { HubError } from "./hub.js";
import * as deviceStore from "./store.js";
import { openEncryption, sealKey } from "./store.js";
import { browserPush } from "./notify.js";
import { forgetNotifyRecord, heldAccounts, keepAccount, listingKey, notifyRecord, othersNotified, unlockKey } from "./accounts.js";
import { assertionJSON, creationOptions, passkeyName, prfOutput, registrationJSON, requestOptions } from "./passkey.js";

const REMEMBERED = "kebbie-credential";
const PINS = "kebbie-credentials";
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

function pins(storage) {
  try {
    const kept = JSON.parse(storage?.getItem(PINS) ?? "null");
    return kept && typeof kept.accounts === "object" ? kept : { last: null, accounts: {} };
  } catch {
    return { last: null, accounts: {} };
  }
}

function legacy(storage) {
  try {
    const kept = JSON.parse(storage?.getItem(REMEMBERED) ?? "null");
    return kept && typeof kept.credential === "string" ? kept : null;
  } catch {
    return null;
  }
}

// One credential and key pin per account; the single entry kept before several accounts still pins the account it names.
export function remembered(storage = localStore(), account = undefined) {
  const held = pins(storage);
  const wanted = account === undefined ? held.last : account;
  const pinned = wanted ? held.accounts[wanted] : null;
  if (pinned && typeof pinned.credential === "string") return { credential: pinned.credential, account: wanted, signing: pinned.signing ?? null };
  const old = legacy(storage);
  if (!old) return null;
  return account === undefined || old.account === account ? old : null;
}

export const rememberedCredential = (storage = localStore(), account = undefined) => remembered(storage, account)?.credential ?? null;

export function rememberCredential(credential, account = null, signing = null, storage = localStore()) {
  try {
    const held = pins(storage);
    const before = account ? remembered(storage, account) : null;
    const fingerprint = signing ?? before?.signing ?? null;
    const accounts = account ? { ...held.accounts, [account]: { credential, signing: fingerprint } } : held.accounts;
    storage?.setItem(PINS, JSON.stringify({ last: account, accounts }));
  } catch {}
}

export function forgetCredential(account, storage = localStore()) {
  try {
    const held = pins(storage);
    const { [account]: _gone, ...accounts } = held.accounts;
    storage?.setItem(PINS, JSON.stringify({ last: held.last === account ? null : held.last, accounts }));
    if (legacy(storage)?.account === account) storage?.removeItem(REMEMBERED);
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
  if (!sealed) return forgetUnlock(store, session.me?.account);
  const { encryption, ...identity } = session.identity;
  try {
    await store.put(unlockKey(identity.id), { identity, encryption: sealed, until: session.until });
    await keepAccount(store, identity.id);
  } catch {}
}

async function keptIdentity(kept) {
  if (kept.identity?.encryption) return kept.identity;
  return { ...kept.identity, encryption: await openEncryption(kept.encryption, kept.identity.key.keys.encryption) };
}

export const forgetUnlock = (store = deviceStore, account = null) => (account ? store.del(unlockKey(account)).catch(() => undefined) : Promise.resolve());

export async function stopNotifications(account, store = deviceStore, subscription = browserPush, id = account.id) {
  const record = id ? await notifyRecord(store, id) : null;
  if (!(id && (await othersNotified(store, id)))) await (await subscription().catch(() => null))?.unsubscribe().catch(() => undefined);
  if (record?.device) await account.unsubscribePush(record.device).catch(() => undefined);
  if (id) await forgetNotifyRecord(store, id);
}

export async function signOut(account, store = deviceStore, subscription = browserPush, id = account.id) {
  try {
    await stopNotifications(account, store, subscription, id);
    await account.logout();
  } finally {
    await forgetUnlock(store, id);
    if (id) await store.del(listingKey(id)).catch(() => undefined);
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

const freshAt = (kept, now) => kept.until > now && kept.until - now <= KEEP_MS;

async function sweepUnlocks(store, now) {
  for (const { account } of await heldAccounts(store)) {
    const kept = await store.get(unlockKey(account)).catch(() => null);
    if (kept && !freshAt(kept, now)) await forgetUnlock(store, account);
  }
  return null;
}

export async function keptUnlock(me, store = deviceStore, now = Date.now(), storage = localStore()) {
  if (!me?.account) return sweepUnlocks(store, now);
  let kept;
  try {
    kept = await store.get(unlockKey(me.account));
  } catch {
    return null;
  }
  if (!kept) return null;
  const fresh = freshAt(kept, now);
  const credential = rememberedCredential(storage, me.account);
  const identity = fresh && credential ? await keptIdentity(kept).catch(() => null) : null;
  if (identity && nonExtractable(identity) && (await sameKey(identity, me))) return { state: "unlocked", me, identity, credential, until: kept.until };
  await forgetUnlock(store, me.account);
  return null;
}

export function lockWhenDue(relock, { store = deviceStore, page = globalThis.document, clock = () => Date.now() } = {}) {
  let watched = null;
  let timer = null;
  const until = () => watched?.until ?? 0;
  const expire = () => {
    clearTimeout(timer);
    const ended = watched?.me?.account ?? null;
    watched = null;
    relock();
    return forgetUnlock(store, ended);
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

// WebKit rejects a modal get() with NotAllowedError while the document lacks focus, so a prompt waits for focus.
export function unlockOnShow(prompt, locked, { page = globalThis.document, win = globalThis } = {}) {
  let armed = true;
  let waiting = false;
  const run = () => {
    waiting = false;
    if (!armed || page.visibilityState !== "visible" || !locked()) return;
    armed = false;
    prompt();
  };
  const shown = () => {
    if (!armed || waiting || page.visibilityState !== "visible") return;
    waiting = true;
    if (page.hasFocus?.() === false) win.addEventListener?.("focus", run, { once: true });
    else queueMicrotask(run);
  };
  page?.addEventListener?.("visibilitychange", () => {
    if (page.visibilityState !== "visible") armed = true;
    else setTimeout(() => locked() && shown(), 0);
  });
  return {
    shown,
    arm() {
      armed = true;
    },
    disarm() {
      armed = false;
    },
  };
}

export async function signedInAccount(account, store = deviceStore, id = account.id) {
  try {
    return await account.me();
  } catch (error) {
    if (error instanceof HubError && error.status === 401) for (const one of id ? [id] : (await heldAccounts(store)).map((held) => held.account)) await forgetUnlock(store, one);
    return null;
  }
}

export async function unlockAfter(hub, ceremony, storage = localStore(), store = deviceStore, clock = Date.now) {
  const { sealed, ...settled } = await settleUnlock(hub, ceremony, storage);
  if (settled.state !== "unlocked") {
    await forgetUnlock(store, settled.me?.account);
    return settled;
  }
  const session = { ...settled, until: clock() + KEEP_MS };
  await keepUnlock(session, sealed, store);
  return session;
}

const scopedTo = (hub, account) => (hub.account.as && account ? { ...hub, account: hub.account.as(account) } : hub);

async function settleUnlock(unscoped, ceremony, storage) {
  let { signedIn, prf, salt } = ceremony;
  const hub = scopedTo(unscoped, signedIn.account);
  if (!prf) ({ signedIn, prf, salt } = await prfCeremony(hub, signedIn.unlock.credential));
  const { unlock } = signedIn;
  const me = withoutUnlock(signedIn);
  const seen = remembered(storage, me.account)?.signing ?? null;
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

export async function addFirstPasskey(unscoped, me, name, storage = localStore(), store = deviceStore) {
  const hub = scopedTo(unscoped, me.account);
  const begun = await hub.account.passkeyBegin();
  const created = await hub.credentials.create(creationOptions(begun.options, begun.prf_salt, passkeyName(me)));
  const added = await hub.account.passkeyFinish(begun.ceremony, registrationJSON(created), name);
  const signedIn = { ...me, key_access: true, unlock: { credential: added.passkey, prf_salt: begun.prf_salt, wrapped: null } };
  return unlockAfter(hub, { signedIn, prf: prfOutput(created), salt: begun.prf_salt }, storage, store);
}

export async function addUnlockingPasskey(unscoped, session, name) {
  const hub = scopedTo(unscoped, session?.me?.account);
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
