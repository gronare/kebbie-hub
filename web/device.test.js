import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import { accountIdentity, fromB64, importIdentity, importSender, openEnvelope, sealEnvelope } from "../js/protocol.js";
import { allowsChanges, collectResults, CommandProblem, keepAllowsChanges, sendCommand } from "./device.js";

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

test("an answer is sealed by the account to the machine and the account, and its result opens with the account key", async () => {
  const identity = await account();
  const machine = await importIdentity(parties.machine);
  const posted = [];
  const calls = { command: async (envelope) => (posted.push(envelope), { command: envelope.id, seq: 1 }) };
  const sent = await sendCommand({ calls, identity, allowed: true, machine: { id: machine.id, keys: parties.machine.public }, payload: answer, at: AT, meta: { label: "Keep it" } });
  const [envelope] = posted;
  assert.equal(envelope.from, identity.id);
  assert.deepEqual(Object.keys(envelope.keys).sort(), [identity.id, machine.id].sort());
  assert.deepEqual([sent.id, sent.kind, sent.exp, sent.label, sent.result], [envelope.id, "answer", AT + 3600, "Keep it", null]);
  assert.deepEqual(await openEnvelope(envelope, machine, await importSender(identity.id, accounts.key.keys), AT), answer);

  const header = { v: 1, id: envelope.id, kind: "result", from: machine.id, machine: machine.id, at: AT, exp: AT + 86_400, schema: 1 };
  const result = { type: "result", command: envelope.id, outcome: "refused", reason: "not-allowed", text: "Not pushed: devbox takes answers, not pushes." };
  const sealed = await sealEnvelope(header, new TextEncoder().encode(JSON.stringify(result)), machine, [{ id: machine.id, encryption: parties.machine.public.encryption }, { id: identity.id, encryption: accounts.key.keys.encryption }]);
  const listing = { cursor: 7, results: [{ seq: 7, machine: machine.id, command: envelope.id, stored_at: AT + 5, envelope: sealed }] };
  const asked = [];
  const kept = await collectResults({ calls: { results: async (after, wait) => (asked.push([after, wait]), listing) }, identity, kept: { cursor: 3, sent: [sent] }, keysOf: () => parties.machine.public, now: AT + 10 });
  assert.deepEqual(asked, [[3, 0]]);
  assert.equal(kept.cursor, 7);
  assert.deepEqual(kept.sent[0].result, { outcome: "refused", reason: "not-allowed", text: "Not pushed: devbox takes answers, not pushes.", at: AT + 5 });

  const misfiled = { cursor: 8, results: [{ ...listing.results[0], command: "cmd_019a07ed-8d00-7f3f-8567-000000000000" }] };
  const untouched = await collectResults({ calls: { results: async () => misfiled }, identity, kept: { cursor: 7, sent: [{ ...sent, id: misfiled.results[0].command }] }, keysOf: () => parties.machine.public, now: AT + 10 });
  assert.equal(untouched.sent[0].result, null, "a result filed under another command is not believed");
});
