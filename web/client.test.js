import assert from "node:assert/strict";
import { test } from "node:test";

import { parseEvents } from "./hub.js";
import { assertionJSON, creationOptions, prfOutput, registrationJSON, requestOptions } from "./passkey.js";

test("passkey options from the hub become the buffers WebAuthn takes", () => {
  const created = creationOptions({ publicKey: { challenge: "AQID", user: { id: "BAU", name: "carl", displayName: "carl" }, rp: { id: "localhost", name: "kebbie hub" }, excludeCredentials: [{ type: "public-key", id: "Bg" }] } });
  assert.deepEqual([...created.publicKey.challenge], [1, 2, 3]);
  assert.deepEqual([...created.publicKey.user.id], [4, 5]);
  assert.deepEqual([...created.publicKey.excludeCredentials[0].id], [6]);
  assert.equal(created.publicKey.rp.id, "localhost");
  assert.equal(created.publicKey.extensions, undefined);
  const requested = requestOptions({ publicKey: { challenge: "BwgJ", allowCredentials: [{ type: "public-key", id: "Cg" }], userVerification: "required" } });
  assert.deepEqual([...requested.publicKey.challenge], [7, 8, 9]);
  assert.deepEqual([...requested.publicKey.allowCredentials[0].id], [10]);
  assert.equal(requested.publicKey.extensions, undefined);
});

test("the credential's PRF salt goes into create() and each allowed credential's into get()", () => {
  const created = creationOptions({ publicKey: { challenge: "AQID", user: { id: "BAU", name: "carl", displayName: "carl" } } }, "CwwN");
  assert.deepEqual([...created.publicKey.extensions.prf.eval.first], [11, 12, 13]);
  const requested = requestOptions({ publicKey: { challenge: "BwgJ", allowCredentials: [{ type: "public-key", id: "Cg" }] } }, { Cg: "Dg8Q" });
  assert.deepEqual(Object.keys(requested.publicKey.extensions.prf.evalByCredential), ["Cg"]);
  assert.deepEqual([...requested.publicKey.extensions.prf.evalByCredential.Cg.first], [14, 15, 16]);
  assert.equal(requestOptions({ publicKey: { challenge: "BwgJ" } }, {}).publicKey.extensions, undefined);
});

test("a PRF output is read only from the authenticator's actual results", () => {
  const output = new Uint8Array(32).fill(7);
  assert.deepEqual(prfOutput({ getClientExtensionResults: () => ({ prf: { results: { first: output.buffer } } }) }), output);
  assert.equal(prfOutput({ getClientExtensionResults: () => ({ prf: { enabled: true } }) }), null);
  assert.equal(prfOutput({ getClientExtensionResults: () => ({}) }), null);
  assert.equal(prfOutput({}), null);
  assert.equal(prfOutput({ getClientExtensionResults: () => ({ prf: { results: { first: new Uint8Array(16).buffer } } }) }), null);
});

test("credentials go back to the hub as base64url, in the shape webauthn-rs reads, without the PRF results", () => {
  const bytes = (...values) => new Uint8Array(values).buffer;
  const prf = () => ({ prf: { enabled: true, results: { first: new Uint8Array(32).fill(9).buffer } } });
  const registered = registrationJSON({ id: "AQ", rawId: bytes(1), type: "public-key", getClientExtensionResults: prf, response: { attestationObject: bytes(2), clientDataJSON: bytes(3), getTransports: () => ["internal", "hybrid"] } });
  assert.deepEqual(registered, { id: "AQ", rawId: "AQ", type: "public-key", response: { attestationObject: "Ag", clientDataJSON: "Aw", transports: ["internal", "hybrid"] }, extensions: {} });
  const asserted = assertionJSON({ id: "AQ", rawId: bytes(1), type: "public-key", getClientExtensionResults: prf, response: { authenticatorData: bytes(4), clientDataJSON: bytes(5), signature: bytes(6), userHandle: null } });
  assert.deepEqual(asserted.response, { authenticatorData: "BA", clientDataJSON: "BQ", signature: "Bg", userHandle: null });
  assert.deepEqual(asserted.extensions, {});
  assert.doesNotMatch(JSON.stringify([registered, asserted]), /CQkJ|prf/);
});

test("server-sent events are parsed across chunk boundaries and keep-alives", () => {
  const first = parseEvents(': keep-alive\n\nevent: board\ndata: {"machine":"mch_a","rev":3}\n\nevent: mach');
  assert.deepEqual(first.events, [{ name: "board", data: '{"machine":"mch_a","rev":3}', id: null }]);
  const second = parseEvents(`${first.rest}ine-removed\nid: 7\ndata: {"machine":"mch_a"}\n\n`);
  assert.deepEqual(second.events, [{ name: "machine-removed", data: '{"machine":"mch_a"}', id: "7" }]);
  assert.equal(second.rest, "");
});
