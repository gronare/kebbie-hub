import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import { fromB64, toB64 } from "../js/protocol.js";
import { HubError } from "./hub.js";
import { heldAccounts, unlockKey } from "./accounts.js";
import { rememberCredential } from "./unlock.js";
import { lockedAccounts } from "./screens.js";
import { addedScreen } from "./email.js";
import { addedStep, heldSession, keepAdded, lockedMachines, NoPasskeyHere, noPasskeyText, OtherAccount, unlockHeld } from "./locked.js";

const load = (name) => JSON.parse(readFileSync(new URL(`../vectors/${name}`, import.meta.url), "utf8"));
const { parties } = load("keys.json");
const envelopes = load("envelopes.json");
const accounts = load("accounts.json");
const ACCOUNT = parties.account.id;
const CREDENTIAL = accounts.wrap.credential;
const HUB = accounts.wrap.salt;
const WORK = { account: ACCOUNT, name: "Work", colour: "peach" };
const bytes = (...values) => new Uint8Array(values).buffer;
const text = (html) => html.replace(/<[^>]+>/g, " ").replace(/\s+/g, " ").trim();

function memoryStore({ failPut = false } = {}) {
  const values = new Map();
  return {
    values,
    get: async (key) => structuredClone(values.get(key)),
    put: async (key, value) => {
      if (failPut) throw new Error("quota");
      values.set(key, structuredClone(value));
    },
    del: async (key) => void values.delete(key),
  };
}

function storage() {
  const values = new Map();
  return { getItem: (key) => values.get(key) ?? null, setItem: (key, value) => values.set(key, value), removeItem: (key) => values.delete(key) };
}

function authenticator({ refuse = null } = {}) {
  const asked = [];
  const made = [];
  return {
    asked,
    made,
    async create(options) {
      made.push(options);
      throw new Error("no passkey is made while adding or unlocking");
    },
    async get(options) {
      asked.push(options);
      if (refuse) throw Object.assign(new Error(refuse), { name: refuse });
      const allowed = options.publicKey.allowCredentials.map((credential) => toB64(credential.id));
      const id = allowed[0] ?? CREDENTIAL;
      const asks = options.publicKey.extensions?.prf;
      const salt = asks?.evalByCredential?.[id]?.first ?? asks?.eval?.first;
      const output = salt && toB64(new Uint8Array(salt)) === accounts.wrap.salt ? fromB64(accounts.wrap.prf_output).buffer : bytes(...new Uint8Array(32));
      return { id, rawId: fromB64(id).buffer, type: "public-key", getClientExtensionResults: () => ({ prf: { enabled: true, results: { first: output } } }), response: { authenticatorData: bytes(3), clientDataJSON: bytes(4), signature: bytes(5), userHandle: bytes(6) } };
    },
  };
}

function workHub({ passkeys = [CREDENTIAL] } = {}) {
  const calls = [];
  const view = (keyAccess) => ({ account: ACCOUNT, handle: "carl", created_at: 1, owner: true, key: accounts.key, key_access: keyAccess, email: "carl@work.example" });
  return {
    calls,
    me: async () => view(false),
    enterCode: async (email, code) => (calls.push(["enterCode", email, code]), code === "123456" ? { account: view(false) } : Promise.reject(new HubError(401, "wrong-code"))),
    email: async () => ({ email: "carl@work.example", verified: true }),
    logout: async () => void calls.push(["logout"]),
    passkeys: async () => ({ passkeys: passkeys.map((passkey) => ({ passkey, name: "iPhone", created_at: 1, last_used_at: null, unlocks: true })) }),
    loginBegin: async (request) => {
      calls.push(["loginBegin", request]);
      const allowed = request.credential ? [request.credential] : [];
      return { ceremony: "l1", options: { publicKey: { challenge: "AQID", rpId: "hub.example", allowCredentials: allowed.map((id) => ({ type: "public-key", id })) } }, prf_salts: Object.fromEntries(allowed.map((id) => [id, accounts.wrap.salt])), prf_salt: HUB };
    },
    loginFinish: async (ceremony, credential) => ({ ...view(true), unlock: { credential: credential.rawId, prf_salt: accounts.wrap.salt, wrapped: accounts.wrap.wrapped } }),
  };
}

async function addByEmail(account, store, code) {
  const signed = await account.enterCode("carl@work.example", code);
  return keepAdded(store, account, signed.account);
}

test("adding an account by email and code keeps it on the phone locked, without asking for a passkey", async () => {
  const account = workHub();
  const credentials = authenticator();
  const store = memoryStore();
  await store.put(unlockKey(ACCOUNT), { identity: { id: ACCOUNT }, encryption: null, until: Date.now() + 1000 });
  await addByEmail(account, store, "123456");
  const [held] = await heldAccounts(store);
  assert.equal(held.account, ACCOUNT);
  assert.equal(held.email, "carl@work.example");
  assert.equal(held.signedOut, false);
  const session = await heldSession(await account.me(), store, Date.now(), storage());
  assert.equal(session.state, "locked", "a session signed in by code holds no key, whatever unlock the phone kept");
  assert.deepEqual([credentials.asked.length, credentials.made.length], [0, 0]);
  assert.ok(!account.calls.some(([name]) => name === "loginBegin"));
});

test("a locked account unlocks with the passkey this phone remembers for it, in one prompt", async () => {
  const account = workHub();
  const credentials = authenticator();
  const kept = storage();
  rememberCredential(CREDENTIAL, ACCOUNT, null, kept);
  const session = await unlockHeld({ account, credentials }, WORK, { remembered: CREDENTIAL, storage: kept, store: memoryStore() });
  assert.equal(session.state, "unlocked");
  assert.equal(session.me.account, ACCOUNT);
  assert.equal(credentials.asked.length, 1);
  assert.deepEqual(credentials.asked[0].publicKey.allowCredentials.map((allowed) => toB64(allowed.id)), [CREDENTIAL]);
  assert.deepEqual(account.calls.find(([name]) => name === "loginBegin")[1], { credential: CREDENTIAL });
});

test("without a remembered passkey the prompt offers only the locked account's own passkeys", async () => {
  const account = workHub({ passkeys: [CREDENTIAL, "c2Vjb25k"] });
  const credentials = authenticator();
  const session = await unlockHeld({ account, credentials }, WORK, { storage: storage(), store: memoryStore() });
  assert.equal(session.state, "unlocked");
  assert.deepEqual(credentials.asked[0].publicKey.allowCredentials.map((allowed) => toB64(allowed.id)), [CREDENTIAL, "c2Vjb25k"]);
});

test("a phone without a passkey for the account says so and where to add one", async () => {
  const expected = "This phone has no passkey for Work. Unlock Work here with a passkey that belongs to it, such as one saved in your password manager.";
  assert.equal(noPasskeyText("Work"), expected);
  const refused = authenticator({ refuse: "NotAllowedError" });
  await assert.rejects(unlockHeld({ account: workHub(), credentials: refused }, WORK, { storage: storage(), store: memoryStore() }), (error) => error instanceof NoPasskeyHere && error.message === expected);
  const none = authenticator();
  await assert.rejects(unlockHeld({ account: workHub({ passkeys: [] }), credentials: none }, WORK, { storage: storage(), store: memoryStore() }), (error) => error.message === expected);
  assert.equal(none.asked.length, 0, "an account with no passkey at all is not prompted for");
  assert.match(text(lockedAccounts([{ ...WORK, notice: expected }])), /Work Locked This phone has no passkey for Work\. .* Unlock with passkey$/);
});

test("a locked account's machines are named from its signed list while its boards stay sealed and offer no command", async () => {
  const vector = envelopes.cases.find((candidate) => candidate.name === "board-opened-by-the-account");
  const unreadable = { ...vector.envelope, ct: "AAAA" };
  const machines = accounts.machines.list.machines.map(({ machine }) => ({ machine, name: `box-${machine.slice(-4)}`, machine_keys: parties.machine.public, rev: 4, envelope: unreadable }));
  const listing = { boards: machines, list: { list: accounts.machines.list, signature: accounts.machines.signature } };
  const named = await lockedMachines(accounts.key, listing);
  assert.deepEqual(named, machines.map(({ machine, name }) => ({ machine, name })));
  assert.deepEqual(await lockedMachines(accounts.key, { ...listing, list: { ...listing.list, signature: accounts.machines.removed_signature } }), [], "a list the account did not sign names nothing");
  assert.deepEqual(await lockedMachines(null, listing), []);
  const html = lockedAccounts([{ ...WORK, machines: named }]);
  assert.match(text(html), new RegExp(`^Work Locked Unlock with passkey ${named.map(({ name }) => `${name} Locked · unlock to read`).join(" ")}$`));
  assert.deepEqual([...html.matchAll(/data-action="([^"]+)"/g)].map((found) => found[1]), ["unlock-account"], "nothing but Unlock can be tapped");
});

test("an add cancelled at the code, or one the phone cannot keep, leaves no account on the phone", async () => {
  const account = workHub();
  const store = memoryStore();
  await assert.rejects(addByEmail(account, store, "000000"), HubError);
  assert.deepEqual(await heldAccounts(store), []);
  const full = memoryStore({ failPut: true });
  await assert.rejects(addByEmail(account, full, "123456"), /quota/);
  assert.deepEqual(await heldAccounts(full), []);
  assert.deepEqual(account.calls.at(-1), ["logout"], "a session the phone could not keep is ended");
});

async function added(account, store) {
  await addByEmail(account, store, "123456");
  const [held] = await heldAccounts(store);
  return { held, step: await addedStep(account, held) };
}

const actions = (html) => [...html.matchAll(/data-action="([^"]+)"/g)].map((found) => found[1]);

async function stillLocked(account, store) {
  const [held] = await heldAccounts(store);
  assert.deepEqual([held.account, held.signedOut], [ACCOUNT, false], "the account stays saved on the phone");
  assert.equal(await store.get(unlockKey(ACCOUNT)), undefined, "no unlock is kept");
  assert.equal((await heldSession(await account.me(), store, Date.now(), storage())).state, "locked");
}

test("after the code the add flow offers to unlock the saved account with its passkey, or later", async () => {
  const account = workHub();
  const { held, step } = await added(account, memoryStore());
  assert.deepEqual(step, { account: ACCOUNT, name: held.name || held.email, offered: true });
  const html = addedScreen(step);
  assert.equal(text(html), `kebbie hub ${step.name} is added. Unlock it now with its passkey? Unlock with passkey Later`);
  assert.deepEqual(actions(html), ["unlock-added", "added-later"]);
});

test("Later leaves the added account saved and locked", async () => {
  const account = workHub();
  const store = memoryStore();
  await added(account, store);
  await stillLocked(account, store);
  assert.ok(!account.calls.some(([name]) => name === "loginBegin"), "no passkey prompt was started");
});

test("a cancelled passkey prompt after adding leaves the account saved and locked", async () => {
  const account = workHub();
  const store = memoryStore();
  const { held } = await added(account, store);
  const refused = authenticator({ refuse: "NotAllowedError" });
  await assert.rejects(unlockHeld({ account, credentials: refused }, held, { storage: storage(), store }), NoPasskeyHere);
  assert.equal(refused.asked.length, 1);
  await stillLocked(account, store);
});

test("unlocking with the passkey after adding opens the account at once", async () => {
  const account = workHub();
  const store = memoryStore();
  const { held } = await added(account, store);
  const credentials = authenticator();
  const session = await unlockHeld({ account, credentials }, held, { storage: storage(), store });
  assert.equal(session.state, "unlocked");
  assert.equal(session.me.account, ACCOUNT);
  assert.ok(await store.get(unlockKey(ACCOUNT)), "the unlock is kept for the boards to open with");
  assert.deepEqual(credentials.asked[0].publicKey.allowCredentials.map((allowed) => toB64(allowed.id)), [CREDENTIAL]);
});

test("an added account with no passkey skips the step and says so", async () => {
  const account = workHub({ passkeys: [] });
  const store = memoryStore();
  const { step } = await added(account, store);
  assert.equal(step.offered, false);
  const html = addedScreen(step);
  assert.equal(text(html), `kebbie hub ${step.name} is added. It has no passkey yet. Back to your boards`);
  assert.deepEqual(actions(html), [], "no prompt is offered that cannot succeed");
  await stillLocked(account, store);
});

test("a passkey of another account is refused before anything is kept, and that account's session ends unless the phone holds it", async () => {
  const STRAY = "acc_0123456789abcdefghijzz";
  for (const [held, ended] of [[[WORK], [STRAY]], [[WORK, { account: STRAY, colour: "teal" }], []]]) {
    const ends = [];
    const account = { ...workHub(), loginFinish: async () => ({ account: STRAY, handle: "other", key_access: true, key: null }), as: (id) => ({ logout: async () => void ends.push(id) }) };
    const store = memoryStore();
    await store.put("accounts", held);
    await assert.rejects(unlockHeld({ account, credentials: authenticator() }, WORK, { storage: storage(), store }), (error) => error instanceof OtherAccount && error.message === "That passkey belongs to another account than Work. Nothing changed.");
    assert.deepEqual(ends, ended);
    assert.equal(await store.get(unlockKey(STRAY)), undefined);
    assert.deepEqual((await heldAccounts(store)).map((one) => one.account), held.map((one) => one.account), "the other account is not adopted");
  }
});
