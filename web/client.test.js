import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import { importIdentity, toB64 } from "../js/protocol.js";
import { parseEvents } from "./hub.js";
import { FALLBACK, openNotice } from "./notice.js";
import { assertionJSON, creationOptions, registrationJSON, requestOptions } from "./passkey.js";

const load = (name) => JSON.parse(readFileSync(new URL(`../vectors/${name}`, import.meta.url), "utf8"));
const { parties } = load("keys.json");
const envelopes = load("envelopes.json");

test("passkey options from the hub become the buffers WebAuthn takes", () => {
  const created = creationOptions({ publicKey: { challenge: "AQID", user: { id: "BAU", name: "carl", displayName: "carl" }, rp: { id: "localhost", name: "kebbie hub" }, excludeCredentials: [{ type: "public-key", id: "Bg" }] } });
  assert.deepEqual([...created.publicKey.challenge], [1, 2, 3]);
  assert.deepEqual([...created.publicKey.user.id], [4, 5]);
  assert.deepEqual([...created.publicKey.excludeCredentials[0].id], [6]);
  assert.equal(created.publicKey.rp.id, "localhost");
  const requested = requestOptions({ publicKey: { challenge: "BwgJ", allowCredentials: [{ type: "public-key", id: "Cg" }], userVerification: "required" } });
  assert.deepEqual([...requested.publicKey.challenge], [7, 8, 9]);
  assert.deepEqual([...requested.publicKey.allowCredentials[0].id], [10]);
});

test("credentials go back to the hub as base64url, in the shape webauthn-rs reads", () => {
  const bytes = (...values) => new Uint8Array(values).buffer;
  const registered = registrationJSON({ id: "AQ", rawId: bytes(1), type: "public-key", response: { attestationObject: bytes(2), clientDataJSON: bytes(3), getTransports: () => ["internal", "hybrid"] } });
  assert.deepEqual(registered, { id: "AQ", rawId: "AQ", type: "public-key", response: { attestationObject: "Ag", clientDataJSON: "Aw", transports: ["internal", "hybrid"] }, extensions: {} });
  const asserted = assertionJSON({ id: "AQ", rawId: bytes(1), type: "public-key", response: { authenticatorData: bytes(4), clientDataJSON: bytes(5), signature: bytes(6), userHandle: null } });
  assert.deepEqual(asserted.response, { authenticatorData: "BA", clientDataJSON: "BQ", signature: "Bg", userHandle: null });
});

test("server-sent events are parsed across chunk boundaries and keep-alives", () => {
  const first = parseEvents(': keep-alive\n\nevent: board\ndata: {"machine":"mch_a","rev":3}\n\nevent: res');
  assert.deepEqual(first.events, [{ name: "board", data: '{"machine":"mch_a","rev":3}', id: null }]);
  const second = parseEvents(`${first.rest}ult\nid: 7\ndata: {"machine":"mch_a","command":"cmd_x"}\n\n`);
  assert.deepEqual(second.events, [{ name: "result", data: '{"machine":"mch_a","command":"cmd_x"}', id: "7" }]);
  assert.equal(second.rest, "");
});

test("a pushed notice opens on the phone into the notification it shows", async () => {
  const vector = envelopes.cases.find((candidate) => candidate.name === "notice-with-retract");
  const machines = [{ id: parties.machine.id, keys: parties.machine.public }];
  const shown = await openNotice(vector.envelope, await importIdentity(parties.phone), machines, envelopes.now);
  assert.equal(shown.title, vector.expect.payload.title);
  assert.equal(shown.body, vector.expect.payload.question);
  assert.equal(shown.tag, vector.envelope.collapse);
  assert.equal(shown.retract, vector.expect.payload.retract);
  assert.equal(shown.url, `/#/task/${encodeURIComponent(parties.machine.id)}/${encodeURIComponent("refunds@greenhouse")}`);
});

test("a notice that does not open shows a plain notification without its content", async () => {
  const vector = envelopes.cases.find((candidate) => candidate.name === "notice-with-retract");
  const phone = await importIdentity(parties.phone);
  const machines = [{ id: parties.machine.id, keys: parties.machine.public }];
  assert.equal(await openNotice(vector.envelope, phone, [], envelopes.now), FALLBACK);
  assert.equal(await openNotice({ ...vector.envelope, sig: toB64(new Uint8Array(64)) }, phone, machines, envelopes.now), FALLBACK);
  assert.equal(await openNotice(vector.envelope, phone, machines, vector.envelope.exp), FALLBACK);
  assert.equal(await openNotice(null, phone, machines, envelopes.now), FALLBACK);
});
