import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import { accountIdentity, deviceFingerprint, fromB64, importIdentity, newDeviceKey, openCommand, sealEnvelope, verifyDeviceCert } from "../js/protocol.js";
import { allowsChanges, collectResults, CommandProblem, ensureDevice, forgetDevice, heldDevice, keepAllowsChanges, keepSent, loadDevice, loadSent, openHistory, pushFollows, sendCommand, verifiedDevices } from "./device.js";

const load = (name) => JSON.parse(readFileSync(new URL(`../vectors/${name}`, import.meta.url), "utf8"));
const { parties } = load("keys.json");
const accounts = load("accounts.json");

async function account() {
  const party = parties[accounts.account];
  return accountIdentity(accounts.key, { signing: fromB64(party.signing_secret), encryption: fromB64(party.encryption_secret) }, accounts.key.account);
}

const memoryStore = () => {
  const kept = new Map();
  return { kept, get: async (key) => kept.get(key), put: async (key, value) => void kept.set(key, value), del: async (key) => void kept.delete(key) };
};

const AT = 1_761_069_600;
const answer = { type: "answer", address: "refunds@shop", question: "q1", choice: 0, comment: "" };

test("this device views only until it is allowed, per account, and an unreadable store reads as view only", async () => {
  const store = memoryStore();
  assert.equal(await allowsChanges(store, "acc_a"), false);
  await keepAllowsChanges(store, "acc_a", true);
  assert.equal(await allowsChanges(store, "acc_a"), true);
  assert.equal(await allowsChanges(store, "acc_b"), false, "another account on this browser starts view only");
  await keepAllowsChanges(store, "acc_a", false);
  assert.equal(await allowsChanges(store, "acc_a"), false);
  assert.equal(await allowsChanges({ get: async () => { throw new Error("blocked"); } }, "acc_a"), false);
});

test("with the switch off nothing is sealed or sent", async () => {
  const identity = await account();
  const posted = [];
  const calls = { command: async (envelope) => void posted.push(envelope) };
  for (const allowed of [false, undefined, "yes"]) {
    await assert.rejects(sendCommand({ calls, identity, allowed, machine: { id: parties.machine.id, keys: parties.machine.public }, payload: answer, at: AT }), (error) => error instanceof CommandProblem && error.reason === "view-only");
  }
  assert.deepEqual(posted, []);
});

async function certified(identity, calls, store = memoryStore()) {
  return ensureDevice({ store, calls, identity, name: "iPhone", now: AT });
}

test("turning changes on makes a key that never leaves this browser, certifies it with the account and keeps it per account", async () => {
  const identity = await account();
  const added = [];
  const calls = { addDevice: async (cert) => void added.push(cert) };
  const store = memoryStore();
  const device = await certified(identity, calls, store);
  assert.equal(device.signing.extractable, false);
  assert.match(device.id, /^dev_/);
  assert.deepEqual([device.cert.account, device.cert.device, device.cert.name, device.cert.created_at], [identity.id, device.id, "iPhone", AT]);
  assert.equal(await verifyDeviceCert(accounts.key, device.cert), true);
  assert.equal(device.fingerprint, await deviceFingerprint(device.cert.signing));
  assert.deepEqual(added, [device.cert]);
  assert.equal((await loadDevice(store, identity.id)).id, device.id);
  assert.equal(await loadDevice(store, "acc_other"), null, "another account on this browser has no key yet");
  assert.equal((await ensureDevice({ store, calls, identity, name: "iPhone", now: AT + 60 })).id, device.id, "the kept key is used again");
  assert.equal(added.length, 1);
  const forged = { ...device, cert: { ...device.cert, name: "Renamed by someone" } };
  assert.notEqual((await ensureDevice({ store, calls, identity, name: "iPhone", now: AT, kept: forged })).id, device.id, "a cert the account didn't sign is replaced");
  await assert.rejects(ensureDevice({ store: memoryStore(), calls: { addDevice: async () => { throw new Error("hub down"); } }, identity, name: "iPhone", now: AT }), /hub down/);
});

const x25519Keys = (value) => (value instanceof CryptoKey ? (value.algorithm.name === "X25519" ? 1 : 0) : value && typeof value === "object" ? Object.values(value).reduce((sum, inner) => sum + x25519Keys(inner), 0) : 0);

test("the kept device holds no X25519 CryptoKey, which WebKit reads back as null, and still loads with a non-extractable key that opens what was sealed to it", async () => {
  const identity = await account();
  const store = memoryStore();
  const device = await certified(identity, { addDevice: async () => undefined }, store);
  assert.equal(x25519Keys(store.kept.get(`device-key:${identity.id}`)), 0);
  const loaded = await loadDevice(store, identity.id);
  assert.equal(loaded.id, device.id);
  assert.equal(loaded.encryption.privateKey.extractable, false);
  assert.equal(loaded.encryption.privateKey.algorithm.name, "X25519");
  const peer = await crypto.subtle.generateKey({ name: "X25519" }, false, ["deriveBits"]);
  const shared = async (privateKey, publicKey) => new Uint8Array(await crypto.subtle.deriveBits({ name: "X25519", public: publicKey }, privateKey, 256)).toHex();
  assert.equal(await shared(loaded.encryption.privateKey, peer.publicKey), await shared(peer.privateKey, loaded.encryption.publicKey));
  assert.equal(await shared(device.encryption.privateKey, peer.publicKey), await shared(peer.privateKey, loaded.encryption.publicKey));
});

test("a device kept by an earlier build with its keys as CryptoKeys still loads, so a browser that can read it keeps its device", async () => {
  const identity = await account();
  const store = memoryStore();
  const device = await certified(identity, { addDevice: async () => undefined }, store);
  const key = await newDeviceKey();
  await store.put(`device-key:${identity.id}`, { ...device, encryption: key.encryption });
  assert.equal((await loadDevice(store, identity.id)).encryption.privateKey, key.encryption.privateKey);
});

test("a kept device that reads back unreadable is told apart from no device at all", async () => {
  const identity = await account();
  const store = memoryStore();
  assert.deepEqual(await heldDevice(store, identity.id), { device: null, missing: true });
  await store.put(`device-key:${identity.id}`, null);
  assert.deepEqual(await heldDevice(store, identity.id), { device: null, missing: false });
  assert.deepEqual(await heldDevice({ get: async () => Promise.reject(new Error("blocked")) }, identity.id), { device: null, missing: false });
});

test("a device that registers anew while notifications are on takes the browser's subscription along, so the worker's record and key name the same device", async () => {
  const identity = await account();
  const subscribed = [];
  const calls = { addDevice: async () => undefined, subscribePush: async (device, body) => void subscribed.push({ device, body }) };
  const store = memoryStore();
  const before = await ensureDevice({ store, calls, identity, name: "iPhone", now: AT, subscription: async () => null });
  await store.put("notify", { account: identity.key, device: before.id });
  await store.put(`notify-categories:${before.id}`, ["questions", "ready"]);
  const held = { toJSON: () => ({ endpoint: "https://web.push.apple.com/QGuQyavXutnMH", keys: { p256dh: "p256", auth: "auth" } }) };
  await store.del(`device-key:${identity.id}`);
  const after = await ensureDevice({ store, calls, identity, name: "iPhone", now: AT + 60, subscription: async () => held });
  assert.notEqual(after.id, before.id);
  assert.deepEqual(subscribed, [{ device: after.id, body: { endpoint: "https://web.push.apple.com/QGuQyavXutnMH", keys: { p256dh: "p256", auth: "auth" }, categories: ["questions", "ready"] } }]);
  assert.equal((await store.get("notify")).device, after.id);
  assert.equal((await loadDevice(store, identity.id)).id, (await store.get("notify")).device);
  assert.deepEqual(await store.get(`notify-categories:${after.id}`), ["questions", "ready"]);
  assert.equal(await pushFollows({ store, calls, identity, device: after, subscription: async () => held }), false, "a record that names this device moves nothing");
  await store.del("notify");
  await store.del(`device-key:${identity.id}`);
  await ensureDevice({ store, calls, identity, name: "iPhone", now: AT + 120, subscription: async () => held });
  assert.equal(subscribed.length, 1, "with notifications off nothing is subscribed");
});

test("an answer is signed by this device, sealed to the machine and the account, and its result names the device", async () => {
  const identity = await account();
  const machine = await importIdentity(parties.machine);
  const posted = [];
  const calls = { addDevice: async () => undefined, command: async (envelope) => (posted.push(envelope), { command: envelope.id, seq: 1 }) };
  const device = await certified(identity, calls);
  await assert.rejects(sendCommand({ calls, identity, device: null, allowed: true, machine: { id: machine.id, keys: parties.machine.public }, payload: answer, at: AT }), (error) => error.reason === "no-device");
  const sent = await sendCommand({ calls, identity, device, allowed: true, machine: { id: machine.id, keys: parties.machine.public }, payload: answer, at: AT, meta: { label: "Keep it" } });
  const [envelope] = posted;
  assert.equal(envelope.from, device.id);
  assert.deepEqual(Object.keys(envelope.keys).sort(), [identity.id, machine.id].sort());
  assert.deepEqual([sent.id, sent.kind, sent.exp, sent.label, sent.device, sent.deviceName, sent.result], [envelope.id, "answer", AT + 3600, "Keep it", device.id, "iPhone", null]);
  assert.deepEqual((await openCommand(envelope, machine, accounts.key, AT)).payload, answer);

  const header = { v: 1, id: envelope.id, kind: "result", from: machine.id, machine: machine.id, at: AT, exp: AT + 86_400, schema: 1 };
  const result = { type: "result", command: envelope.id, device: device.id, outcome: "refused", reason: "not-approved", text: "Not delivered: devbox didn't approve this device in time." };
  const sealed = await sealEnvelope(header, new TextEncoder().encode(JSON.stringify(result)), machine, [{ id: machine.id, encryption: parties.machine.public.encryption }, { id: identity.id, encryption: accounts.key.keys.encryption }]);
  const listing = { cursor: 7, results: [{ seq: 7, machine: machine.id, command: envelope.id, stored_at: AT + 5, envelope: sealed }] };
  const asked = [];
  const kept = await collectResults({ calls: { results: async (after, wait) => (asked.push([after, wait]), listing) }, identity, kept: { cursor: 3, sent: [sent] }, keysOf: () => parties.machine.public, now: AT + 10 });
  assert.deepEqual(asked, [[3, 0]]);
  assert.equal(kept.cursor, 7);
  assert.deepEqual(kept.sent[0].result, { outcome: "refused", reason: "not-approved", text: result.text, device: device.id, at: AT + 5 });

  const misfiled = { cursor: 8, results: [{ ...listing.results[0], command: "cmd_019a07ed-8d00-7f3f-8567-000000000000" }] };
  const untouched = await collectResults({ calls: { results: async () => misfiled }, identity, kept: { cursor: 7, sent: [{ ...sent, id: misfiled.results[0].command }] }, keysOf: () => parties.machine.public, now: AT + 10 });
  assert.equal(untouched.sent[0].result, null, "a result filed under another command is not believed");

  const history = await openHistory([{ command: envelope.id, machine: machine.id, kind: "answer", queued_at: AT + 1, delivered_at: AT + 2, envelope: { ...envelope, keys: { [identity.id]: envelope.keys[identity.id] } }, result: sealed, result_at: AT + 5 }], identity, () => parties.machine.public);
  assert.deepEqual(history.map((entry) => [entry.id, entry.device, entry.payload, entry.result.reason, entry.at]), [[envelope.id, device.id, answer, "not-approved", AT + 1]]);
  const swapped = await openHistory([{ command: envelope.id, machine: machine.id, kind: "push", queued_at: AT, envelope, result: null }], identity, () => null);
  assert.deepEqual(swapped, [], "an entry whose kind the hub misreports is left out");
});

test("a review comment's and a section edit's words stay in this browser's store only until the machine answers or the command expires", async () => {
  const store = memoryStore();
  const comments = [{ path: "src/orders.rs", line: 41, side: "old", sha: null, text: "Why the cast?" }];
  const entry = (id, fields) => ({ id, kind: "review-comments", machine: "mch_a", address: "payments@api", at: AT, exp: AT + 3600, comments, count: 1, result: null, ...fields });
  const sending = entry("cmd_1", {});
  const answered = entry("cmd_2", { result: { outcome: "done" } });
  const expired = entry("cmd_3", { exp: AT - 1 });
  await keepSent(store, "acc_a", { cursor: 4, sent: [sending, answered, expired] }, AT);
  const kept = await loadSent(store, "acc_a", AT);
  assert.deepEqual(kept.sent.map((one) => one.comments[0].text), ["Why the cast?", undefined, undefined]);
  assert.deepEqual(kept.sent[1].comments[0], { path: "src/orders.rs", line: 41, side: "old", sha: null }, "where the comment sat is kept");
  assert.equal(sending.comments[0].text, "Why the cast?", "the page in memory keeps the words");
  const edit = { id: "cmd_4", kind: "item-edit", machine: "mch_a", address: "vault:Projects/session.md", section: "goal", text: "You stay signed in.", at: AT, exp: AT + 3600, result: { outcome: "done" } };
  await keepSent(store, "acc_a", { cursor: 5, sent: [{ ...edit, result: null }, { ...edit, id: "cmd_5" }] }, AT);
  assert.deepEqual((await loadSent(store, "acc_a", AT)).sent.map((one) => one.text), ["You stay signed in.", undefined], "a section's words go once the machine answered");
});

test("Account shows a device as its certificate says, never the hub's copies beside it, and skips a row whose certificate names another device", async () => {
  const identity = await account();
  const store = memoryStore();
  const device = await ensureDevice({ store, calls: { addDevice: async () => undefined }, identity, name: "iPhone 16", now: AT });
  const row = { device: device.id, name: "Evil phone", kind: "cli", fingerprint: "0000-0000", cert: device.cert, created_at: AT, last_seen_at: AT + 5, removed_at: null };
  const [shown] = await verifiedDevices(identity.key, [row]);
  assert.deepEqual([shown.id, shown.name, shown.kind, shown.key, shown.removed], [device.id, "iPhone 16", "web", device.fingerprint, null]);
  assert.deepEqual(await verifiedDevices(identity.key, [{ ...row, device: "dev_01JB8Q6Z3V9K2M4N5P6R7S8T9W" }]), []);
  assert.deepEqual(await verifiedDevices(identity.key, [{ ...row, cert: { ...device.cert, name: "Renamed by the hub" } }]), []);
  assert.equal((await verifiedDevices(identity.key, [{ ...row, removed_at: AT + 9 }]))[0].removed, AT + 9);
  await forgetDevice(store, identity.id);
  assert.equal(await loadDevice(store, identity.id), null, "a device removed from this browser mints a new key on its next send");
});
