import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import { fromB64, machineComparisonCode } from "../js/protocol.js";
import { normalizeCode, pair, PairingFailed } from "./pairing.js";

const load = (name) => JSON.parse(readFileSync(new URL(`../vectors/${name}`, import.meta.url), "utf8"));
const { parties } = load("keys.json");
const vector = load("pairing.json").cases.find((candidate) => candidate.name === "phone-pairs");
const MACHINE = parties.machine.id;

function fakeHub({ machineKeys = parties.machine.public, code = vector.machine_code, decide = (sas) => (sas ? "admitted" : "refused"), commitError = null, admittedMachine = MACHINE, silent = false } = {}) {
  const log = [];
  let pairing = null;
  return {
    log,
    machineSas: null,
    async commit(sentCode, commitment) {
      log.push(["commit", sentCode]);
      if (commitError) throw commitError;
      pairing = { state: "replied", commitment: fromB64(commitment) };
      return { pairing: "pr_1", device: "dev_x", token: null, expires_at: 0 };
    },
    async watch(id, seen) {
      log.push(["watch", seen]);
      if (silent) return { state: seen, machine: MACHINE };
      if (seen === "committed") return { pairing: id, state: pairing.state, machine: MACHINE, machine_name: "devbox", machine_keys: machineKeys, machine_nonce: vector.machine_nonce };
      return { pairing: id, state: pairing.state, reason: pairing.reason ?? null, machine: pairing.state === "admitted" ? admittedMachine : MACHINE, machine_keys: parties.intruder.public, machine_name: "evil" };
    },
    async reveal(id, keys, nonce) {
      log.push(["reveal"]);
      try {
        this.machineSas = await machineComparisonCode(pairing.commitment, parties.machine.public, fromB64(vector.machine_nonce), keys, fromB64(nonce), code);
        pairing.state = decide(this.machineSas);
      } catch (error) {
        pairing.state = "refused";
        pairing.reason = error.code === "commitment-mismatch" ? "mismatch" : "declined";
      }
      if (pairing.state === "refused") pairing.reason ??= "declined";
    },
  };
}

const run = (hub, extra = {}, answer = { matches: true, name: "  Kontoret " }) => {
  const shown = [];
  const confirm = async (digits) => {
    shown.push(digits);
    return answer;
  };
  const promise = pair({ hub, publicKeys: parties.phone.public, typed: vector.code.toLowerCase().replace("-", " "), nonce: fromB64(vector.device_nonce), confirm, ...extra });
  return { promise, shown };
};

const reason = async (promise) => {
  try {
    await promise;
  } catch (error) {
    if (error instanceof PairingFailed) return error.reason;
    throw error;
  }
  return null;
};

test("codes are read the way people type them, as the hub reads them", () => {
  assert.equal(normalizeCode("o123 zyxw"), "0123-ZYXW");
  assert.equal(normalizeCode("0l23-ZYXW"), "0123-ZYXW");
  assert.equal(normalizeCode("0123-ZYXU"), null);
  assert.equal(normalizeCode("0123-ZYX"), null);
});

test("the phone commits, then shows the same six digits as the machine and ends admitted", async () => {
  const hub = fakeHub();
  const { promise, shown } = run(hub);
  const machine = await promise;
  assert.deepEqual(machine, { id: MACHINE, keys: parties.machine.public, name: "Kontoret", hubName: "devbox" });
  assert.deepEqual(shown, [vector.expect.sas]);
  assert.equal(hub.machineSas, vector.expect.sas);
  assert.deepEqual(hub.log.map(([step]) => step), ["commit", "watch", "reveal", "watch"]);
  assert.equal(hub.log[0][1], vector.code);
});

test("the commitment is sent before the phone learns anything from the machine", async () => {
  const hub = fakeHub();
  await run(hub).promise;
  assert.deepEqual(hub.log.slice(0, 2), [["commit", vector.code], ["watch", "committed"]]);
});

test("a machine holding another code refuses and the phone reports it", async () => {
  const hub = fakeHub({ code: "Q2ZR-8HWN" });
  assert.equal(await reason(run(hub).promise), "refused-mismatch");
});

test("a user who answers n ends the pairing as declined", async () => {
  const hub = fakeHub({ decide: () => "refused" });
  const { promise, shown } = run(hub);
  assert.equal(await reason(promise), "refused-declined");
  assert.equal(shown.length, 1);
});

test("swapped machine keys give different digits on the two sides", async () => {
  const hub = fakeHub({ machineKeys: parties.intruder.public, decide: () => "refused" });
  const { promise, shown } = run(hub);
  await reason(promise);
  assert.notEqual(shown[0], hub.machineSas);
});

test("an unknown code, a taken code, a bad code and a silent machine each fail visibly", async () => {
  assert.equal(await reason(run(fakeHub({ commitError: Object.assign(new Error("401"), { status: 401 }) })).promise), "unknown-code");
  assert.equal(await reason(run(fakeHub({ commitError: Object.assign(new Error("409"), { status: 409 }) })).promise), "second-commitment");
  assert.equal(await reason(run(fakeHub(), { typed: "nope" }).promise), "bad-code");
  let clock = 0;
  const silent = fakeHub({ silent: true });
  silent.watch = async (id, seen) => {
    clock += 30_000;
    return { state: seen, machine: MACHINE };
  };
  assert.equal(await reason(run(silent, { clock: () => clock, timeoutMs: 60_000 }).promise), "timeout");
});

test("a pairing admitted for another machine than the one that replied is refused", async () => {
  assert.equal(await reason(run(fakeHub({ admittedMachine: parties["other-machine"].id })).promise), "machine-changed");
});

test("digits the user says do not match leave the phone trusting nothing, even when the hub reports admitted", async () => {
  const { promise, shown } = run(fakeHub(), {}, { matches: false });
  assert.equal(await reason(promise), "phone-mismatch");
  assert.deepEqual(shown, [vector.expect.sas]);
});

test("the keys kept are the ones the digits were computed over, never ones the hub sends later", async () => {
  const machine = await run(fakeHub()).promise;
  assert.deepEqual(machine.keys, parties.machine.public);
  assert.notDeepEqual(machine.keys, parties.intruder.public);
});

test("a machine left unnamed or with the hub's name follows the hub's name, so a rename shows", async () => {
  for (const name of [" ", "devbox"]) {
    const machine = await run(fakeHub(), {}, { matches: true, name }).promise;
    assert.equal(machine.name, null);
    assert.equal(machine.hubName, "devbox");
  }
});

test("a machine that refuses while the phone is still asking ends the pairing", async () => {
  const hub = fakeHub({ decide: () => "refused" });
  const promise = pair({ hub, publicKeys: parties.phone.public, typed: vector.code, nonce: fromB64(vector.device_nonce), confirm: () => new Promise(() => {}) });
  assert.equal(await reason(promise), "refused-declined");
});
