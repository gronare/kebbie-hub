import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import { accountIdentity, fromB64, importIdentity, openEnvelope, sealEnvelope, signDeviceList, verifyDeviceList } from "../js/protocol.js";
import { collectResults, DeviceListProblem, enableDevice, loadDevice, removeListedDevice, sendCommand } from "./device.js";
import { HubError } from "./hub.js";

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

function fakeHub(identity, held = null) {
  const calls = [];
  const hub = {
    calls,
    held,
    registerDevice: async (request) => (calls.push(["register", request]), { device: "dev_01K83YV380AAAAAAAAAAAAAAAA", token: "khd_secret" }),
    deviceList: async () => (calls.push(["get"]), { list: hub.held }),
    putDeviceList: async (signed) => {
      calls.push(["put", signed]);
      if (!(await verifyDeviceList(identity, signed))) throw new HubError(400, "bad-signature");
      hub.held = signed;
    },
    removeDevice: async (device) => void calls.push(["delete", device]),
  };
  return hub;
}

test("letting this device answer registers its public keys, asks for the passkey, then signs it onto the account's list", async () => {
  const identity = await account();
  const signed = { list: accounts.devices.list, signature: accounts.devices.signature };
  const hub = fakeHub(identity, signed);
  const store = memoryStore();
  const order = [];
  const reauth = async () => void order.push(hub.calls.length);
  const { device, list } = await enableDevice({ hub, store, identity, name: "iPhone", reauth, seen: 2 });
  assert.deepEqual(hub.calls[0][0], "register");
  assert.deepEqual(Object.keys(hub.calls[0][1].keys).sort(), ["encryption", "signing"]);
  assert.deepEqual(order, [1], "the passkey is asked for after registering and before the list is read and signed");
  assert.equal(list.counter, 3);
  assert.deepEqual(list.devices.map((listed) => listed.device), [...accounts.devices.list.devices.map((listed) => listed.device), device.id].sort());
  const mine = list.devices.find((listed) => listed.device === device.id);
  assert.deepEqual([mine.name, mine.role, mine.keys], ["iPhone", "control", device.public]);
  assert.equal(device.signing.extractable, false);
  assert.equal(device.encryption.privateKey.extractable, false);
  assert.equal((await loadDevice(store, identity.id)).listed, 3);
});

test("a device list the account did not sign, or one older than seen, is never extended", async () => {
  const identity = await account();
  const forged = { list: { ...accounts.devices.list, counter: 9 }, signature: accounts.devices.signature };
  const store = memoryStore();
  const problem = async (hub, seen) => {
    try {
      await enableDevice({ hub, store, identity, name: "iPhone", reauth: async () => {}, seen });
      return null;
    } catch (error) {
      assert.ok(error instanceof DeviceListProblem);
      return error.reason;
    }
  };
  const forgedHub = fakeHub(identity, forged);
  assert.equal(await problem(forgedHub, 0), "unsigned-device-list");
  assert.ok(!forgedHub.calls.some(([name]) => name === "put"));
  const old = fakeHub(identity, { list: accounts.devices.list, signature: accounts.devices.signature });
  assert.equal(await problem(old, 5), "rolled-back-device-list");
  assert.ok(!old.calls.some(([name]) => name === "put"));
});

test("removing a device signs a list without it before the hub drops its token", async () => {
  const identity = await account();
  const hub = fakeHub(identity, { list: accounts.devices.list, signature: await signDeviceList(identity, accounts.devices.list) });
  const gone = parties[accounts.devices.removed_device].id;
  const list = await removeListedDevice({ hub, store: memoryStore(), identity, device: gone, reauth: async () => {} });
  assert.deepEqual(list, accounts.devices.removed);
  assert.deepEqual(hub.calls.map(([name]) => name), ["get", "put", "delete"]);
});

test("an answer is sealed to the machine and this device, sent with the device token, and its result opens with the device key", async () => {
  const identity = await account();
  const hub = fakeHub(identity, null);
  const { device } = await enableDevice({ hub, store: memoryStore(), identity, name: "iPhone", reauth: async () => {} });
  const machine = await importIdentity(parties.machine);
  const posted = [];
  const at = 1_761_069_600;
  const calls = { command: async (token, envelope) => (posted.push([token, envelope]), { command: envelope.id, seq: 1 }) };
  const payload = { type: "answer", address: "refunds@shop", question: "q1", choice: 0, comment: "" };
  const sent = await sendCommand({ calls, device, machine: { id: machine.id, keys: parties.machine.public }, payload, at, meta: { label: "Keep it" } });
  const [[token, envelope]] = posted;
  assert.equal(token, "khd_secret");
  assert.deepEqual([sent.id, sent.kind, sent.exp, sent.label, sent.result], [envelope.id, "answer", at + 3600, "Keep it", null]);
  assert.deepEqual(await openEnvelope(envelope, machine, { id: device.id, signing: device.verifying }, at), payload);

  const header = { v: 1, id: envelope.id, kind: "result", from: machine.id, machine: machine.id, at, exp: at + 86_400, schema: 1 };
  const result = { type: "result", command: envelope.id, outcome: "done", text: "Answered." };
  const sealed = await sealEnvelope(header, new TextEncoder().encode(JSON.stringify(result)), machine, [{ id: machine.id, encryption: parties.machine.public.encryption }, { id: device.id, encryption: device.public.encryption }]);
  const listing = { cursor: 7, results: [{ seq: 7, machine: machine.id, command: envelope.id, stored_at: at + 5, envelope: sealed }] };
  const kept = await collectResults({ calls: { results: async () => listing }, device, kept: { cursor: 0, sent: [sent] }, keysOf: () => parties.machine.public, now: at + 10 });
  assert.equal(kept.cursor, 7);
  assert.deepEqual(kept.sent[0].result, { outcome: "done", reason: null, text: "Answered.", at: at + 5 });

  const misfiled = { cursor: 8, results: [{ ...listing.results[0], command: "cmd_019a07ed-8d00-7f3f-8567-000000000000" }] };
  const untouched = await collectResults({ calls: { results: async () => misfiled }, device, kept: { cursor: 7, sent: [{ ...sent, id: misfiled.results[0].command }] }, keysOf: () => parties.machine.public, now: at + 10 });
  assert.equal(untouched.sent[0].result, null, "a result filed under another command is not believed");
});
