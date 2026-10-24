import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import { fromB64 } from "../js/protocol.js";
import { connectCommitment, finishConnect, parseConnectFragment, planLine, safeNext, withReauth } from "./connect.js";
import { HubError } from "./hub.js";
import { PairingFailed } from "./pairing.js";

const load = (name) => JSON.parse(readFileSync(new URL(`../vectors/${name}`, import.meta.url), "utf8"));
const { parties } = load("keys.json");
const vector = load("pairing.json").connect;
const MACHINE = parties.machine.id;
const fragment = parseConnectFragment(new URL(vector.link).hash);
const phone = parties[vector.device].public;

function fakeHub({ machineKeys = parties.machine.public, settles = "admitted", settledMachine = MACHINE } = {}) {
  const log = [];
  return {
    log,
    async watch(pairing, seen) {
      log.push(["watch", seen]);
      if (seen === "committed") return { pairing, state: "replied", machine: MACHINE, machine_name: "mac", machine_keys: machineKeys, machine_nonce: vector.machine_nonce };
      return { pairing, state: settles, reason: settles === "refused" ? "mismatch" : null, machine: settledMachine };
    },
    async reveal(pairing, keys, nonce) {
      log.push(["reveal", pairing, keys, nonce]);
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

test("the browser commits to the secret in the link's fragment, as Rust does", async () => {
  const sealed = await connectCommitment(phone, fragment, fromB64(vector.device_nonce));
  assert.equal(sealed.commitment, vector.commitment);
});

test("the browser reveals only to the machine the link names, then is admitted without digits", async () => {
  const hub = fakeHub();
  const nonce = fromB64(vector.device_nonce);
  const machine = await finishConnect({ hub, pairing: "pr_1", publicKeys: phone, fragment, nonce });
  assert.deepEqual(machine, { id: MACHINE, keys: parties.machine.public, hubName: "mac" });
  assert.deepEqual(hub.log.map(([step]) => step), ["watch", "reveal", "watch"]);
  assert.equal(hub.log[1][3], vector.device_nonce);
});

test("keys of another machine end the connect before the browser reveals anything", async () => {
  const hub = fakeHub({ machineKeys: parties.intruder.public });
  assert.equal(await reason(finishConnect({ hub, pairing: "pr_1", publicKeys: phone, fragment, nonce: fromB64(vector.device_nonce) })), "wrong-machine");
  assert.ok(!hub.log.some(([step]) => step === "reveal"));
});

test("a machine that refuses, or a hub that swaps machines at the end, is not trusted", async () => {
  const nonce = fromB64(vector.device_nonce);
  assert.equal(await reason(finishConnect({ hub: fakeHub({ settles: "refused" }), pairing: "pr_1", publicKeys: phone, fragment, nonce })), "refused-mismatch");
  assert.equal(await reason(finishConnect({ hub: fakeHub({ settledMachine: parties["other-machine"].id }), pairing: "pr_1", publicKeys: phone, fragment, nonce })), "machine-changed");
});

test("a link without a whole secret and fingerprint offers no board", () => {
  assert.equal(parseConnectFragment(""), null);
  assert.equal(parseConnectFragment(`#k=${vector.code}`), null);
  assert.deepEqual(parseConnectFragment(`k=${vector.code}&m=${vector.fingerprint}`), { code: vector.code, fingerprint: vector.fingerprint });
});

test("only a path on this hub is followed after signing up", () => {
  assert.equal(safeNext("/connect?code=K7QF-M2XD"), "/connect?code=K7QF-M2XD");
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
