import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import { fromB64, importSender, openEnvelope, toB64, unwrapAccountSecrets, wrapperStatement } from "../js/protocol.js";
import { HubError } from "./hub.js";
import { signUpWith, signInWith } from "./signin.js";
import { addFirstPasskey, addUnlockingPasskey, rememberedCredential, unlockAfter } from "./unlock.js";

const load = (name) => JSON.parse(readFileSync(new URL(`../vectors/${name}`, import.meta.url), "utf8"));
const { parties } = load("keys.json");
const envelopes = load("envelopes.json");
const accounts = load("accounts.json");
const ACCOUNT = parties.account.id;
const bytes = (...values) => new Uint8Array(values).buffer;

function authenticator({ prfAtCreate = true, prf = true, outputs = {} } = {}) {
  const made = [];
  const asked = [];
  const output = async (id, salt) => {
    if (outputs[id]) return fromB64(outputs[id]).buffer;
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
      const salt = options.publicKey.extensions?.prf?.evalByCredential?.[id]?.first;
      const value = prf && salt ? await output(id, salt) : null;
      return { id, rawId: fromB64(id).buffer, type: "public-key", getClientExtensionResults: results(value), response: { authenticatorData: bytes(3), clientDataJSON: bytes(4), signature: bytes(5), userHandle: bytes(6) } };
    },
  };
}

function hub({ key = null, passkeys = {} } = {}) {
  const posted = [];
  const state = { key, passkeys: structuredClone(passkeys), fresh: 0 };
  const view = () => ({ account: ACCOUNT, handle: "carl", created_at: 1, owner: true, email: null, plan: { status: "active", until: null, entitled: true }, billing_url: null, key: state.key });
  const salt = () => toB64(crypto.getRandomValues(new Uint8Array(32)));
  const signedIn = (credential) => ({ ...view(), unlock: { credential, prf_salt: state.passkeys[credential].salt, wrapped: state.passkeys[credential].wrapped ?? null } });
  const account = {
    posted,
    state,
    me: async () => view(),
    registerBegin: async () => ((state.pending = salt()), { ceremony: "r1", options: { publicKey: { challenge: "AQID", user: { id: "BAU", name: "carl", displayName: "carl" } } }, prf_salt: state.pending }),
    registerFinish: async (ceremony, credential) => {
      posted.push(credential);
      state.passkeys[credential.rawId] = { salt: state.pending };
      return signedIn(credential.rawId);
    },
    loginBegin: async (request) => {
      if (request.credential && !state.passkeys[request.credential]) throw new HubError(401, "unauthorized");
      const allowed = request.credential ? [request.credential] : [];
      return { ceremony: "l1", options: { publicKey: { challenge: "AQID", allowCredentials: allowed.map((id) => ({ type: "public-key", id })) } }, prf_salts: Object.fromEntries(allowed.map((id) => [id, state.passkeys[id].salt])) };
    },
    loginFinish: async (ceremony, credential) => {
      posted.push(credential);
      return signedIn(credential.rawId);
    },
    passkeyBegin: async () => ((state.pending = salt()), { ceremony: "p1", options: { publicKey: { challenge: "AQID", user: { id: "BAU", name: "carl", displayName: "carl" } } }, prf_salt: state.pending }),
    passkeyFinish: async (ceremony, credential, name) => {
      posted.push(credential);
      state.passkeys[credential.rawId] = { salt: state.pending };
      return { passkey: credential.rawId, name, created_at: 1, last_used_at: null, unlocks: false };
    },
    setKey: async (setup) => {
      if (state.key) throw new HubError(409, "key-exists");
      state.key = setup.key;
      state.passkeys[setup.credential].wrapped = setup.wrapped;
      state.setup = setup;
    },
    putWrapper: async (passkey, wrapper) => {
      state.passkeys[passkey].wrapped = wrapper.wrapped;
      state.wrapper = { passkey, ...wrapper };
    },
  };
  return account;
}

function storage() {
  const values = new Map();
  return { getItem: (key) => values.get(key) ?? null, setItem: (key, value) => values.set(key, value), removeItem: (key) => values.delete(key) };
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

test("a later sign-in unwraps the account key, which opens the account's board", async () => {
  const account = hub({ key: accounts.key, passkeys: vectorPasskeys() });
  const credentials = authenticator({ outputs: { [accounts.wrap.credential]: accounts.wrap.prf_output } });
  const kept = storage();
  const signingIn = signInWith({ account, credentials });
  const first = await unlockAfter({ account, credentials }, await signingIn.withPasskey(), kept);
  assert.equal(first.state, "unlocked");
  assert.equal(credentials.asked.length, 2, "a discoverable sign-in has no salt yet, so the PRF comes from a get on that passkey");
  const vector = envelopes.cases.find((candidate) => candidate.name === "board-opened-by-the-account");
  assert.deepEqual(await openEnvelope(vector.envelope, first.identity, await importSender(parties.machine.id, parties.machine.public), envelopes.now), vector.expect.payload);
  const again = await unlockAfter({ account, credentials }, await signingIn.withPasskey(rememberedCredential(kept)), kept);
  assert.equal(again.state, "unlocked");
  assert.equal(credentials.asked.length, 3, "the remembered passkey unlocks in one prompt");
  assert.ok(extensionsStripped(account));
});

test("a hub that reports other public keys for the account is refused", async () => {
  const account = hub({ key: { ...accounts.key, keys: parties.intruder.public }, passkeys: vectorPasskeys() });
  const credentials = authenticator({ outputs: { [accounts.wrap.credential]: accounts.wrap.prf_output } });
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
  const credentials = authenticator({ outputs: { [accounts.wrap.credential]: accounts.wrap.prf_output } });
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
  const credentials = authenticator({ outputs: { [accounts.wrap.credential]: accounts.wrap.prf_output } });
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
  const credentials = authenticator({ outputs: { [accounts.wrap.credential]: accounts.wrap.prf_output } });
  const kept = storage();
  const signingIn = signInWith({ account, credentials });
  await unlockAfter({ account, credentials }, await signingIn.withPasskey(accounts.wrap.credential), kept);
  account.state.key = { ...accounts.key, keys: parties.intruder.public };
  assert.equal((await unlockAfter({ account, credentials }, await signingIn.withPasskey(accounts.wrap.credential), kept)).state, "key-changed");
  account.state.key = { ...accounts.key, account: "acc_AAAAAAAAAAAAAAAAAAAAAA" };
  assert.equal((await unlockAfter({ account, credentials }, await signingIn.withPasskey(accounts.wrap.credential), storage())).state, "key-changed");
});

const githubMe = () => ({ account: ACCOUNT, handle: "carl", created_at: 1, owner: false, email: null, github: "octocat", key: null });

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
