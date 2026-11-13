import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import { fromB64, importSender, openEnvelope, toB64, unwrapAccountSecrets, wrapperStatement } from "../js/protocol.js";
import { HubError } from "./hub.js";
import { signUpWith, signInWith } from "./signin.js";
import { passkeySignInScreen, signedOutStep } from "./email.js";
import { addFirstPasskey, addUnlockingPasskey, forgetUnlock, KEEP_MS, keptUnlock, LISTING, lockWhenDue, rememberedCredential, signedInAccount, signOut, unlockAfter } from "./unlock.js";

const load = (name) => JSON.parse(readFileSync(new URL(`../vectors/${name}`, import.meta.url), "utf8"));
const { parties } = load("keys.json");
const envelopes = load("envelopes.json");
const accounts = load("accounts.json");
const ACCOUNT = parties.account.id;
const HUB_SALT = accounts.hub_prf_salt.salt;
const bytes = (...values) => new Uint8Array(values).buffer;

function authenticator({ prfAtCreate = true, prf = true, outputs = {} } = {}) {
  const made = [];
  const asked = [];
  const output = async (id, salt) => {
    if (outputs[id]?.salt === toB64(new Uint8Array(salt))) return fromB64(outputs[id].output).buffer;
    const key = await crypto.subtle.importKey("raw", new TextEncoder().encode(`secret of ${id}`), { name: "HMAC", hash: "SHA-256" }, false, ["sign"]);
    return crypto.subtle.sign("HMAC", key, salt);
  };
  const results = (value) => () => (value ? { prf: { enabled: true, results: { first: value } } } : prf ? { prf: { enabled: true } } : {});
  return {
    made,
    asked,
    async create(options) {
      const id = toB64(crypto.getRandomValues(new Uint8Array(16)));
      made.push(id);
      const salt = options.publicKey.extensions?.prf?.eval?.first;
      const value = prf && prfAtCreate && salt ? await output(id, salt) : null;
      return { id, rawId: fromB64(id).buffer, type: "public-key", getClientExtensionResults: results(value), response: { attestationObject: bytes(1), clientDataJSON: bytes(2), getTransports: () => ["internal"] } };
    },
    async get(options) {
      asked.push(options);
      const allowed = options.publicKey.allowCredentials.map((credential) => toB64(credential.id));
      const id = allowed[0] ?? made[0] ?? Object.keys(outputs)[0];
      const asks = options.publicKey.extensions?.prf;
      const salt = asks?.evalByCredential?.[id]?.first ?? asks?.eval?.first;
      const value = prf && salt ? await output(id, salt) : null;
      return { id, rawId: fromB64(id).buffer, type: "public-key", getClientExtensionResults: results(value), response: { authenticatorData: bytes(3), clientDataJSON: bytes(4), signature: bytes(5), userHandle: bytes(6) } };
    },
  };
}

function hub({ key = null, passkeys = {} } = {}) {
  const posted = [];
  const state = { key, passkeys: structuredClone(passkeys), fresh: 0 };
  const view = () => ({ account: ACCOUNT, handle: "alice", created_at: 1, owner: true, key: state.key });
  const signedIn = (credential) => ({ ...view(), unlock: { credential, prf_salt: state.passkeys[credential].salt, wrapped: state.passkeys[credential].wrapped ?? null } });
  const account = {
    posted,
    state,
    me: async () => view(),
    registerBegin: async () => ((state.pending = HUB_SALT), { ceremony: "r1", options: { publicKey: { challenge: "AQID", user: { id: "BAU", name: "alice", displayName: "alice" } } }, prf_salt: state.pending }),
    registerFinish: async (ceremony, credential) => {
      posted.push(credential);
      state.passkeys[credential.rawId] = { salt: state.pending };
      return signedIn(credential.rawId);
    },
    loginBegin: async (request) => {
      if (request.credential && !state.passkeys[request.credential]) throw new HubError(401, "unauthorized");
      const allowed = request.credential ? [request.credential] : [];
      return { ceremony: "l1", options: { publicKey: { challenge: "AQID", allowCredentials: allowed.map((id) => ({ type: "public-key", id })) } }, prf_salts: Object.fromEntries(allowed.map((id) => [id, state.passkeys[id].salt])), prf_salt: HUB_SALT };
    },
    loginFinish: async (ceremony, credential) => {
      posted.push(credential);
      return signedIn(credential.rawId);
    },
    passkeyBegin: async () => ((state.pending = HUB_SALT), { ceremony: "p1", options: { publicKey: { challenge: "AQID", user: { id: "BAU", name: "alice", displayName: "alice" } } }, prf_salt: state.pending }),
    passkeyFinish: async (ceremony, credential, name) => {
      posted.push(credential);
      state.passkeys[credential.rawId] = { salt: state.pending };
      return { passkey: credential.rawId, name, created_at: 1, last_used_at: null, unlocks: false };
    },
    setKey: async (setup) => {
      if (state.key) throw new HubError(409, "key-exists");
      state.key = setup.key;
      state.passkeys[setup.credential].wrapped = setup.wrapped;
      if (setup.prf_salt) state.passkeys[setup.credential].salt = setup.prf_salt;
      state.setup = setup;
    },
    putWrapper: async (passkey, wrapper) => {
      state.passkeys[passkey].wrapped = wrapper.wrapped;
      if (wrapper.prf_salt) state.passkeys[passkey].salt = wrapper.prf_salt;
      state.wrapper = { passkey, ...wrapper };
    },
  };
  return account;
}

function storage() {
  const values = new Map();
  return { getItem: (key) => values.get(key) ?? null, setItem: (key, value) => values.set(key, value), removeItem: (key) => values.delete(key) };
}

const vectorOutputs = () => ({ [accounts.wrap.credential]: { salt: accounts.wrap.salt, output: accounts.wrap.prf_output } });

async function hmacOutput(credential, salt) {
  const key = await crypto.subtle.importKey("raw", new TextEncoder().encode(`secret of ${credential}`), { name: "HMAC", hash: "SHA-256" }, false, ["sign"]);
  return new Uint8Array(await crypto.subtle.sign("HMAC", key, fromB64(salt)));
}

const vectorPasskeys = () => ({ [accounts.wrap.credential]: { salt: accounts.wrap.salt, wrapped: accounts.wrap.wrapped } });

async function verifies(key, credential, wrapped, signature) {
  const signer = await crypto.subtle.importKey("raw", fromB64(key.keys.signing), { name: "Ed25519" }, false, ["verify"]);
  return crypto.subtle.verify({ name: "Ed25519" }, signer, fromB64(signature), await wrapperStatement(key, fromB64(credential), fromB64(wrapped)));
}

const extensionsStripped = (account) => account.posted.every((credential) => JSON.stringify(credential.extensions) === "{}" && !JSON.stringify(credential).includes("prf"));

test("signing up with a passkey whose create() gives a PRF makes a random account key and wraps it, in one prompt", async () => {
  const account = hub();
  const credentials = authenticator();
  const kept = storage();
  const session = await unlockAfter({ account, credentials }, await signUpWith({ account, credentials }, { invite: "K7QF-M2XD" }, "iPhone"), kept);
  assert.equal(session.state, "unlocked");
  assert.equal(credentials.asked.length, 0, "no second prompt");
  const { setup } = account.state;
  assert.equal(setup.key.account, ACCOUNT);
  assert.equal(setup.key.version, 1);
  assert.deepEqual(session.me.key, setup.key);
  assert.equal(setup.credential, credentials.made[0]);
  assert.ok(await verifies(setup.key, setup.credential, setup.wrapped, setup.signature));
  assert.equal(session.identity.signing.extractable, false);
  assert.equal(session.identity.encryption.privateKey.extractable, false);
  assert.equal(rememberedCredential(kept), credentials.made[0]);
  assert.ok(extensionsStripped(account));
});

test("a create() without a PRF result is followed by one get() before PRF counts as missing", async () => {
  const account = hub();
  const credentials = authenticator({ prfAtCreate: false });
  const session = await unlockAfter({ account, credentials }, await signUpWith({ account, credentials }, { invite: "K7QF-M2XD" }, "Mac"), storage());
  assert.equal(session.state, "unlocked");
  assert.equal(credentials.asked.length, 1);
  assert.deepEqual(credentials.asked[0].publicKey.allowCredentials.map((allowed) => toB64(allowed.id)), [credentials.made[0]]);
  assert.ok(account.state.key);
  assert.ok(extensionsStripped(account));
});

test("an authenticator without PRF leaves a no-prf state and no key on the hub", async () => {
  const account = hub();
  const credentials = authenticator({ prf: false });
  const session = await unlockAfter({ account, credentials }, await signUpWith({ account, credentials }, { invite: "K7QF-M2XD" }, "Mac"), storage());
  assert.equal(session.state, "no-prf");
  assert.equal(session.identity, undefined);
  assert.equal(account.state.key, null);
  assert.equal(credentials.asked.length, 1, "the immediate get was tried");
});

test("a passkey whose wrapper has another salt than the hub's asks twice once, moves onto the hub's salt, then unlocks in one prompt", async () => {
  const account = hub({ key: accounts.key, passkeys: vectorPasskeys() });
  const credentials = authenticator({ outputs: vectorOutputs() });
  const kept = storage();
  const signingIn = signInWith({ account, credentials });
  const first = await unlockAfter({ account, credentials }, await signingIn.withPasskey(), kept);
  assert.equal(first.state, "unlocked");
  assert.equal(credentials.asked.length, 2, "the old salt's PRF takes one more prompt");
  assert.equal(toB64(credentials.asked[0].publicKey.extensions.prf.eval.first), HUB_SALT);
  assert.equal(toB64(credentials.asked[1].publicKey.extensions.prf.evalByCredential[accounts.wrap.credential].first), accounts.wrap.salt);
  const vector = envelopes.cases.find((candidate) => candidate.name === "board-opened-by-the-account");
  assert.deepEqual(await openEnvelope(vector.envelope, first.identity, await importSender(parties.machine.id, parties.machine.public), envelopes.now), vector.expect.payload);
  const { wrapper } = account.state;
  assert.deepEqual([wrapper.passkey, wrapper.prf_salt, account.state.passkeys[wrapper.passkey].salt], [accounts.wrap.credential, HUB_SALT, HUB_SALT]);
  assert.ok(await verifies(accounts.key, wrapper.passkey, wrapper.wrapped, wrapper.signature));
  const unlock = { account: ACCOUNT, credential: fromB64(wrapper.passkey), version: 1, prfOutput: await hmacOutput(wrapper.passkey, HUB_SALT), salt: fromB64(HUB_SALT) };
  assert.equal(toB64((await unwrapAccountSecrets(unlock, fromB64(wrapper.wrapped))).signing), parties.account.signing_secret);
  const again = await unlockAfter({ account, credentials }, await signingIn.withPasskey(), kept);
  assert.equal(again.state, "unlocked");
  assert.equal(credentials.asked.length, 3, "a discoverable sign-in unlocks in one prompt");
  const remembered = await unlockAfter({ account, credentials }, await signingIn.withPasskey(rememberedCredential(kept)), kept);
  assert.equal(remembered.state, "unlocked");
  assert.equal(credentials.asked.length, 4, "the remembered passkey unlocks in one prompt");
  assert.ok(extensionsStripped(account));
});

test("a hub that reports other public keys for the account is refused", async () => {
  const account = hub({ key: { ...accounts.key, keys: parties.intruder.public }, passkeys: vectorPasskeys() });
  const credentials = authenticator({ outputs: vectorOutputs() });
  await assert.rejects(unlockAfter({ account, credentials }, await signInWith({ account, credentials }).withPasskey(accounts.wrap.credential), storage()), (error) => error.code === "key-mismatch");
});

test("a passkey with the wrong PRF, or none wrapped yet, does not unlock", async () => {
  const wrong = hub({ key: accounts.key, passkeys: vectorPasskeys() });
  const credentials = authenticator();
  await assert.rejects(unlockAfter({ account: wrong, credentials }, await signInWith({ account: wrong, credentials }).withPasskey(accounts.wrap.credential), storage()), (error) => error.code === "wrong-account-key");
  const unwrapped = hub({ key: accounts.key, passkeys: { [accounts.wrap.credential]: { salt: accounts.wrap.salt } } });
  const session = await unlockAfter({ account: unwrapped, credentials }, await signInWith({ account: unwrapped, credentials }).withPasskey(accounts.wrap.credential), storage());
  assert.equal(session.state, "not-wrapped");
});

test("a passkey added from an unlocked session gets its own wrapper of the same account key", async () => {
  const account = hub({ key: accounts.key, passkeys: vectorPasskeys() });
  const credentials = authenticator({ outputs: vectorOutputs() });
  const hubSide = { account, credentials };
  const session = await unlockAfter(hubSide, await signInWith(hubSide).withPasskey(accounts.wrap.credential), storage());
  const added = await addUnlockingPasskey(hubSide, session, "iPad");
  assert.equal(added.unlocks, true);
  const { wrapper } = account.state;
  assert.equal(wrapper.passkey, credentials.made[0]);
  assert.equal(wrapper.key_version, accounts.key.version);
  assert.ok(await verifies(accounts.key, wrapper.passkey, wrapper.wrapped, wrapper.signature));
  const salt = fromB64(account.state.passkeys[wrapper.passkey].salt);
  const key = await crypto.subtle.importKey("raw", new TextEncoder().encode(`secret of ${wrapper.passkey}`), { name: "HMAC", hash: "SHA-256" }, false, ["sign"]);
  const prfOutput = new Uint8Array(await crypto.subtle.sign("HMAC", key, salt));
  const opened = await unwrapAccountSecrets({ account: ACCOUNT, credential: fromB64(wrapper.passkey), version: 1, prfOutput, salt }, fromB64(wrapper.wrapped));
  assert.equal(toB64(opened.signing), parties.account.signing_secret);
  assert.ok(extensionsStripped(account));
});

test("a hub that drops the key of an account this device unlocked before gets no new key minted", async () => {
  const account = hub({ key: accounts.key, passkeys: vectorPasskeys() });
  const credentials = authenticator({ outputs: vectorOutputs() });
  const kept = storage();
  const signingIn = signInWith({ account, credentials });
  assert.equal((await unlockAfter({ account, credentials }, await signingIn.withPasskey(accounts.wrap.credential), kept)).state, "unlocked");
  account.state.key = null;
  delete account.state.passkeys[accounts.wrap.credential].wrapped;
  const forked = await unlockAfter({ account, credentials }, await signingIn.withPasskey(accounts.wrap.credential), kept);
  assert.equal(forked.state, "key-missing");
  assert.equal(account.state.setup, undefined, "nothing was posted to /v1/a/key");
  assert.equal(forked.identity, undefined);
});

test("a hub that offers another account key, or one of another account, than this device unlocked is refused", async () => {
  const account = hub({ key: accounts.key, passkeys: vectorPasskeys() });
  const credentials = authenticator({ outputs: vectorOutputs() });
  const kept = storage();
  const signingIn = signInWith({ account, credentials });
  await unlockAfter({ account, credentials }, await signingIn.withPasskey(accounts.wrap.credential), kept);
  account.state.key = { ...accounts.key, keys: parties.intruder.public };
  assert.equal((await unlockAfter({ account, credentials }, await signingIn.withPasskey(accounts.wrap.credential), kept)).state, "key-changed");
  account.state.key = { ...accounts.key, account: "acc_AAAAAAAAAAAAAAAAAAAAAA" };
  assert.equal((await unlockAfter({ account, credentials }, await signingIn.withPasskey(accounts.wrap.credential), storage())).state, "key-changed");
});

const githubMe = () => ({ account: ACCOUNT, handle: "alice", created_at: 1, owner: false, email: null, github: "octocat", key: null });

test("a passkey added after GitHub sign-in makes the account key and wraps it, in one prompt when create() gives a PRF", async () => {
  const account = hub();
  const credentials = authenticator();
  const session = await addFirstPasskey({ account, credentials }, githubMe(), "iPhone", storage());
  assert.equal(session.state, "unlocked");
  assert.equal(credentials.asked.length, 0, "no second prompt");
  const { setup } = account.state;
  assert.equal(setup.credential, credentials.made[0]);
  assert.deepEqual(session.me.key, setup.key);
  assert.ok(await verifies(setup.key, setup.credential, setup.wrapped, setup.signature));
  assert.ok(extensionsStripped(account));
});

test("a passkey added after GitHub sign-in without a PRF from create() asks once more and then makes the key", async () => {
  const account = hub();
  const credentials = authenticator({ prfAtCreate: false });
  const session = await addFirstPasskey({ account, credentials }, githubMe(), "Mac", storage());
  assert.equal(session.state, "unlocked");
  assert.equal(credentials.asked.length, 1);
  assert.equal(account.state.setup.credential, credentials.made[0]);
});

test("a passkey added after GitHub sign-in on a device without PRF leaves the account without a key", async () => {
  const account = hub();
  const session = await addFirstPasskey({ account, credentials: authenticator({ prf: false }) }, githubMe(), "Mac", storage());
  assert.equal(session.state, "no-prf");
  assert.equal(account.state.key, null);
});

function deviceStore() {
  const values = new Map();
  return { values, get: async (key) => structuredClone(values.get(key)), put: async (key, value) => void values.set(key, structuredClone(value)), del: async (key) => void values.delete(key) };
}

async function unlockedOnce() {
  const account = hub({ key: accounts.key, passkeys: vectorPasskeys() });
  const credentials = authenticator({ outputs: vectorOutputs() });
  const store = deviceStore();
  const local = storage();
  const before = Date.now();
  const session = await unlockAfter({ account, credentials }, await signInWith({ account, credentials }).withPasskey(), local, store);
  assert.equal(session.state, "unlocked");
  return { account, credentials, store, local, session, before, me: await account.me() };
}

const boardOpens = async (identity) => {
  const vector = envelopes.cases.find((candidate) => candidate.name === "board-opened-by-the-account");
  assert.deepEqual(await openEnvelope(vector.envelope, identity, await importSender(parties.machine.id, parties.machine.public), envelopes.now), vector.expect.payload);
};

test("an unlock kept less than an hour ago opens the boards after a reload without a passkey prompt", async () => {
  const { credentials, store, local, me, before } = await unlockedOnce();
  const asked = credentials.asked.length;
  const resumed = await keptUnlock(me, store, before + KEEP_MS - 60_000, local);
  assert.equal(resumed?.state, "unlocked");
  assert.equal(credentials.asked.length, asked, "no passkey prompt");
  await boardOpens(resumed.identity);
});

test("an unlock kept more than an hour ago is deleted and leaves the boards locked", async () => {
  const { store, local, me } = await unlockedOnce();
  assert.equal(await keptUnlock(me, store, Date.now() + KEEP_MS + 1000, local), null);
  assert.equal(store.values.size, 0);
});

test("an unlock that ends further out than an hour from now, as after the clock is set back, is deleted", async () => {
  const { store, local, me, before } = await unlockedOnce();
  assert.equal(await keptUnlock(me, store, before - 60_000, local), null);
  assert.equal(store.values.size, 0);
});

test("an expired unlock is deleted at start even when nobody is signed in, an unexpired one is kept for later", async () => {
  const { store, local, before } = await unlockedOnce();
  assert.equal(await keptUnlock(null, store, before + 60_000, local), null);
  assert.equal(store.values.size, 1);
  assert.equal(await keptUnlock(null, store, before + KEEP_MS + 1000, local), null);
  assert.equal(store.values.size, 0);
});

test("a kept unlock holding an extractable signing or encryption key is not used and is deleted", async () => {
  const signing = (await crypto.subtle.generateKey({ name: "Ed25519" }, true, ["sign", "verify"])).privateKey;
  const privateKey = (await crypto.subtle.generateKey({ name: "X25519" }, true, ["deriveBits"])).privateKey;
  for (const swap of [(identity) => ({ ...identity, signing }), (identity) => ({ ...identity, encryption: { ...identity.encryption, privateKey } })]) {
    const { store, local, me, before } = await unlockedOnce();
    const kept = await store.get("unlock");
    await store.put("unlock", { ...kept, identity: swap(kept.identity) });
    assert.equal(await keptUnlock(me, store, before + 60_000, local), null);
    assert.equal(store.values.size, 0);
  }
});

test("a kept unlock without this account's remembered passkey on the device is deleted", async () => {
  const { store, me, before } = await unlockedOnce();
  assert.equal(await keptUnlock(me, store, before + 60_000, storage()), null);
  assert.equal(store.values.size, 0);
});

test("an unlock that does not end unlocked deletes the one kept before", async () => {
  const { store, local } = await unlockedOnce();
  const unwrapped = hub({ key: accounts.key, passkeys: { [accounts.wrap.credential]: { salt: accounts.wrap.salt } } });
  const credentials = authenticator();
  const session = await unlockAfter({ account: unwrapped, credentials }, await signInWith({ account: unwrapped, credentials }).withPasskey(accounts.wrap.credential), local, store);
  assert.equal(session.state, "not-wrapped");
  assert.equal(store.values.size, 0);
});

function page() {
  const target = new EventTarget();
  target.visibilityState = "visible";
  return target;
}

test("an open page locks its boards and deletes the kept unlock when the hour ends", async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout", "Date"], now: Date.now() });
  const store = deviceStore();
  const session = { state: "unlocked", until: Date.now() + KEEP_MS };
  await store.put("unlock", { until: session.until });
  let locked = 0;
  const ends = lockWhenDue(() => locked++, { store, page: page() });
  ends.watch(session);
  t.mock.timers.tick(KEEP_MS - 1);
  assert.equal(locked, 0);
  t.mock.timers.tick(1);
  assert.equal(locked, 1);
  await Promise.resolve();
  assert.equal(store.values.size, 0);
});

test("a page shown again after the hour ended while its timer slept locks at once", async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout", "Date"], now: Date.now() });
  const shown = page();
  let locked = 0;
  const ends = lockWhenDue(() => locked++, { store: deviceStore(), page: shown });
  ends.watch({ state: "unlocked", until: Date.now() + KEEP_MS });
  shown.dispatchEvent(new Event("visibilitychange"));
  assert.equal(locked, 0);
  t.mock.timers.setTime(Date.now() + KEEP_MS);
  shown.dispatchEvent(new Event("visibilitychange"));
  assert.equal(locked, 1);
});

test("a page that locked or unlocked anew does not lock on the old hour", async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout", "Date"], now: Date.now() });
  let locked = 0;
  const ends = lockWhenDue(() => locked++, { store: deviceStore(), page: page() });
  ends.watch({ state: "unlocked", until: Date.now() + KEEP_MS });
  t.mock.timers.tick(KEEP_MS / 2);
  ends.watch({ state: "unlocked", until: Date.now() + KEEP_MS });
  t.mock.timers.tick(KEEP_MS / 2);
  assert.equal(locked, 0);
  ends.watch({ state: "locked" });
  t.mock.timers.tick(KEEP_MS);
  assert.equal(locked, 0);
});

test("a kept unlock is never readable as key bytes", async () => {
  const { store } = await unlockedOnce();
  const { identity, encryption } = await store.get("unlock");
  await assert.rejects(crypto.subtle.exportKey("pkcs8", identity.signing));
  await assert.rejects(crypto.subtle.exportKey("jwk", identity.signing));
  await assert.rejects(crypto.subtle.exportKey("raw", encryption.key));
});

const x25519Keys = (value) => (value instanceof CryptoKey ? (value.algorithm.name === "X25519" ? 1 : 0) : value && typeof value === "object" ? Object.values(value).reduce((sum, inner) => sum + x25519Keys(inner), 0) : 0);

test("a kept unlock holds no X25519 CryptoKey, which WebKit reads back as null, and still opens the boards after a reload", async () => {
  const { store, local, me, before } = await unlockedOnce();
  assert.equal(x25519Keys(store.values.get("unlock")), 0);
  const resumed = await keptUnlock(me, store, before + 60_000, local);
  assert.equal(resumed.identity.encryption.privateKey.extractable, false);
  await boardOpens(resumed.identity);
});

test("an unlock kept by an earlier build with its keys as CryptoKeys still opens the boards", async () => {
  const { store, local, me, before, session } = await unlockedOnce();
  await store.put("unlock", { identity: session.identity, until: session.until });
  const resumed = await keptUnlock(me, store, before + 60_000, local);
  assert.equal(resumed?.state, "unlocked");
  await boardOpens(resumed.identity);
});

test("a kept unlock for another account, key version or signing key is ignored and deleted", async () => {
  for (const me of [(m) => ({ ...m, account: "acc_other", key: { ...m.key, account: "acc_other" } }), (m) => ({ ...m, key: { ...m.key, version: m.key.version + 1 } }), (m) => ({ ...m, key: { ...m.key, keys: { ...m.key.keys, signing: parties.machine.public.signing } } })]) {
    const { store, local, me: signedIn } = await unlockedOnce();
    assert.equal(await keptUnlock(me(signedIn), store, Date.now(), local), null);
    assert.equal(store.values.size, 0);
  }
});

test("a 401 from the hub when reading the account deletes the kept unlock, being offline keeps it", async () => {
  const { store } = await unlockedOnce();
  assert.equal(await signedInAccount({ me: async () => Promise.reject(new TypeError("offline")) }, store), null);
  assert.equal(store.values.size, 1);
  assert.equal(await signedInAccount({ me: async () => Promise.reject(new HubError(401, "unauthorized")) }, store), null);
  assert.equal(store.values.size, 0);
});

test("signing out deletes the kept unlock", async () => {
  const { store } = await unlockedOnce();
  await forgetUnlock(store);
  assert.equal(store.values.size, 0);
});

test("signing out ends the hub session, deletes the kept unlock and the cached boards, and keeps the passkey so sign-in offers it first", async () => {
  const { account, store, local, session } = await unlockedOnce();
  await store.put(LISTING, { listing: { boards: ["sealed"] }, at: 1 });
  const ended = [];
  await store.put("notify", { account: session.identity.key, device: "dev_phone" });
  const subscription = { unsubscribe: async () => void ended.push("browser unsubscribed") };
  await signOut({ unsubscribePush: async (device) => void ended.push(`hub forgot ${device}`), logout: async () => void ended.push("logout") }, store, async () => subscription);
  assert.deepEqual(ended, ["browser unsubscribed", "hub forgot dev_phone", "logout"], "the next account on this browser can take the push endpoint");
  assert.equal(store.values.size, 0);
  assert.equal(await keptUnlock(await account.me(), store, Date.now(), local), null);
  assert.equal(rememberedCredential(local), session.credential);
  const step = signedOutStep({ info: { email_signin: true }, remembered: rememberedCredential(local) });
  assert.equal(step, "passkey");
  assert.match(passkeySignInScreen({}), /data-action="sign-in"[^>]*>Sign in with your passkey</);
});

test("signing out while the hub can't be reached still deletes what the device kept", async () => {
  const { store, local, session } = await unlockedOnce();
  await store.put(LISTING, { listing: { boards: ["sealed"] }, at: 1 });
  await assert.rejects(signOut({ unsubscribePush: async () => Promise.reject(new TypeError("offline")), logout: async () => Promise.reject(new TypeError("offline")) }, store, async () => null), TypeError);
  assert.equal(store.values.size, 0);
  assert.equal(rememberedCredential(local), session.credential);
});

test("a device that can't store the unlock still unlocks and stays locked after a reload", async () => {
  const account = hub({ key: accounts.key, passkeys: vectorPasskeys() });
  const credentials = authenticator({ outputs: vectorOutputs() });
  const broken = { get: async () => Promise.reject(new Error("no indexeddb")), put: async () => Promise.reject(new Error("no indexeddb")), del: async () => Promise.reject(new Error("no indexeddb")) };
  const session = await unlockAfter({ account, credentials }, await signInWith({ account, credentials }).withPasskey(), storage(), broken);
  assert.equal(session.state, "unlocked");
  assert.equal(await keptUnlock(await account.me(), broken), null);
});
