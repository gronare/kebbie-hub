import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import { accountIdentity, fromB64, machineListTrusts, verifyMachineList } from "../js/protocol.js";
import { approveFromLink, connectBlock, connectCommitment, finishConnect, forgetFragment, keptFragment, PairingFailed, parseConnectFragment, planLine, safeNext, unlockFailedText, withReauth } from "./connect.js";
import { HubError } from "./hub.js";

const load = (name) => JSON.parse(readFileSync(new URL(`../vectors/${name}`, import.meta.url), "utf8"));
const { parties } = load("keys.json");
const vector = load("pairing.json").connect;
const accounts = load("accounts.json");
const MACHINE = parties.machine.id;
const fragment = parseConnectFragment(new URL(vector.link).hash);
const account = () => accountIdentity(accounts.key, { signing: fromB64(parties.account.signing_secret), encryption: fromB64(parties.account.encryption_secret) }, accounts.key.account);

function fakeHub({ machineKeys = parties.machine.public, settles = "admitted", settledMachine = MACHINE, list = null, staleOnce = false } = {}) {
  const log = [];
  let stale = staleOnce;
  return {
    log,
    async boards() {
      log.push(["boards"]);
      return { boards: [], list };
    },
    async putMachines(signed) {
      log.push(["put", signed]);
      if (stale) {
        stale = false;
        throw new HubError(409, "stale-list");
      }
    },
    async watch(pairing, seen) {
      log.push(["watch", seen]);
      if (seen === "committed") return { pairing, state: "replied", machine: MACHINE, machine_name: "mac", machine_keys: machineKeys, machine_nonce: vector.machine_nonce };
      return { pairing, state: settles, reason: settles === "refused" ? "mismatch" : null, machine: settledMachine };
    },
    async reveal(pairing, key, nonce) {
      log.push(["reveal", pairing, key, nonce]);
    },
  };
}

const reason = async (promise) => {
  try {
    await promise;
  } catch (error) {
    if (error instanceof PairingFailed) return error.reason;
    throw error;
  }
  return "no-failure";
};

const nonce = fromB64(accounts.handoff.nonce);

test("the browser commits the account key to the secret in the link's fragment, as Rust does", async () => {
  assert.equal(fragment.code, accounts.handoff.code);
  const sealed = await connectCommitment(accounts.key, fragment, nonce);
  assert.equal(sealed.commitment, accounts.handoff.commitment);
});

test("the browser reveals the account key only to the machine the link names, then adds it to the signed list", async () => {
  const hub = fakeHub();
  const identity = await account();
  const machine = await finishConnect({ hub, pairing: "pr_1", identity, fragment, nonce });
  assert.deepEqual(machine, { id: MACHINE, keys: parties.machine.public, hubName: "mac", counter: 1 });
  assert.deepEqual(hub.log.map(([step]) => step), ["watch", "reveal", "watch", "boards", "put"]);
  assert.deepEqual(hub.log[1][2], accounts.key);
  assert.equal(hub.log[1][3], accounts.handoff.nonce);
  const signed = hub.log[4][1];
  assert.equal(await verifyMachineList(identity, signed), true);
  assert.ok(machineListTrusts(signed.list, MACHINE, parties.machine.public));
});

test("a new list follows the newest one the hub holds, and an older one than this device saw is refused", async () => {
  const identity = await account();
  const current = { list: accounts.machines.removed, signature: accounts.machines.removed_signature };
  const hub = fakeHub({ list: current, staleOnce: true });
  const machine = await finishConnect({ hub, pairing: "pr_1", identity, fragment, nonce });
  assert.equal(machine.counter, current.list.counter + 1);
  assert.equal(hub.log.filter(([step]) => step === "put").length, 2, "one retry after a stale list");
  const older = fakeHub({ list: { list: accounts.machines.list, signature: accounts.machines.signature } });
  assert.equal(await reason(finishConnect({ hub: older, pairing: "pr_1", identity, fragment, nonce, marks: { list: current.list.counter, machines: {} } })), "rolled-back-list");
  assert.ok(!older.log.some(([step]) => step === "put"));
});

test("with a link's fragment, Connect waits for an unlocked account instead of connecting without the handoff", () => {
  assert.equal(connectBlock({ fragment, session: { state: "unlocked" } }), null);
  for (const state of ["no-prf", "not-wrapped", "locked", "key-missing"]) assert.equal(connectBlock({ fragment, session: { state } }), state);
  assert.equal(connectBlock({ fragment, session: null }), "locked");
  assert.equal(connectBlock({ fragment: null, session: null }), null);
});

test("the link's fragment survives a reload of the page until the connect is done", () => {
  const values = new Map();
  const storage = { getItem: (key) => values.get(key) ?? null, setItem: (key, value) => values.set(key, value), removeItem: (key) => values.delete(key) };
  const hash = new URL(vector.link).hash;
  assert.deepEqual(keptFragment(storage, "K7QF-M2XD", hash), fragment);
  assert.deepEqual(keptFragment(storage, "K7QF-M2XD", ""), fragment, "after the reload");
  assert.equal(keptFragment(storage, "ZZZZ-ZZZZ", ""), null, "only for its own code");
  forgetFragment(storage, "K7QF-M2XD");
  assert.equal(keptFragment(storage, "K7QF-M2XD", ""), null);
});

test("keys of another machine end the connect before the browser reveals or signs anything", async () => {
  const hub = fakeHub({ machineKeys: parties.intruder.public });
  assert.equal(await reason(finishConnect({ hub, pairing: "pr_1", identity: await account(), fragment, nonce })), "wrong-machine");
  assert.ok(!hub.log.some(([step]) => step === "reveal" || step === "put"));
});

test("a machine that refuses, or a hub that swaps machines at the end, is never signed", async () => {
  const identity = await account();
  const refusing = fakeHub({ settles: "refused" });
  assert.equal(await reason(finishConnect({ hub: refusing, pairing: "pr_1", identity, fragment, nonce })), "refused-mismatch");
  const swapping = fakeHub({ settledMachine: parties["other-machine"].id });
  assert.equal(await reason(finishConnect({ hub: swapping, pairing: "pr_1", identity, fragment, nonce })), "machine-changed");
  assert.ok(![...refusing.log, ...swapping.log].some(([step]) => step === "put"));
});

test("a link without a whole secret and fingerprint offers no board", () => {
  assert.equal(parseConnectFragment(""), null);
  assert.equal(parseConnectFragment(`#k=${vector.code}`), null);
  assert.deepEqual(parseConnectFragment(`k=${vector.code}&m=${vector.fingerprint}`), { code: vector.code, fingerprint: vector.fingerprint });
});

test("only a path on this hub is followed after signing up", () => {
  assert.equal(safeNext("/connect?code=K7QF-M2XD"), "/connect?code=K7QF-M2XD");
  assert.equal(safeNext("/v1/a/billing/portal"), "/v1/a/billing/portal");
  for (const refused of ["//evil.example/x", "https://evil.example", "/\\evil.example", "javascript:alert(1)", null, ""]) assert.equal(safeNext(refused), null, String(refused));
});

test("a stale session signs in again once, then retries", async () => {
  let calls = 0;
  let signedIn = 0;
  const approved = await withReauth(
    async () => {
      calls += 1;
      if (calls === 1) throw new HubError(401, "reauth-required");
      return "approved";
    },
    async () => {
      signedIn += 1;
    },
  );
  assert.deepEqual([approved, calls, signedIn], ["approved", 2, 1]);
  await assert.rejects(withReauth(async () => Promise.reject(new HubError(402, "plan-inactive", "https://billing.example")), async () => assert.fail("no sign-in for a plan")), (error) => error.url === "https://billing.example");
});

test("an inactive plan points at the billing page when the hub names one", () => {
  assert.equal(planLine({ entitled: true }, "https://billing.example"), null);
  assert.deepEqual(planLine({ entitled: false }, "https://billing.example"), { text: "Your plan is not active. Start a subscription to connect machines.", url: "https://billing.example" });
  assert.equal(planLine({ entitled: false }, null).url, null);
});

test("Connect on a cookie-only session unlocks first, and a cancelled unlock approves nothing", async () => {
  const approvals = [];
  const approve = async (commitment) => (approvals.push(commitment), { name: "mac", pairing: "p1" });
  const cancelled = Object.assign(new Error("cancelled"), { name: "NotAllowedError" });
  await assert.rejects(approveFromLink({ fragment, session: { state: "locked" }, unlock: async () => Promise.reject(cancelled), approve }), { name: "NotAllowedError" });
  assert.deepEqual(approvals, [], "the code stays unspent");
  assert.equal(unlockFailedText(cancelled, "mac"), "The passkey was cancelled, so mac isn't connected yet.");

  const noPrf = await approveFromLink({ fragment, session: { state: "locked" }, unlock: async () => ({ state: "no-prf" }), approve });
  assert.equal(noPrf.blocked, "no-prf");
  assert.deepEqual(approvals, [], "a passkey that can't unlock spends no code either");

  const identity = await account();
  const done = await approveFromLink({ fragment, session: { state: "locked" }, unlock: async () => ({ state: "unlocked", identity }), approve, commit: async () => ({ commitment: "c1", nonce: nonce }) });
  assert.equal(done.blocked, null);
  assert.deepEqual(approvals, ["c1"]);
});
