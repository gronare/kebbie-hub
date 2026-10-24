import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import { checkPayload, checkShape, comparisonCode, decryptEnvelope, fromB64, headerBytes, importIdentity, importSender, machineComparisonCode, machineFingerprint, openEnvelope, pairingCommitment, pairingNonce, parseConnectFragment, ProtocolError, sealEnvelope, toB64 } from "./protocol.js";

const load = (name) => JSON.parse(readFileSync(new URL(`../vectors/${name}`, import.meta.url), "utf8"));
const { parties } = load("keys.json");
const envelopes = load("envelopes.json");

const identity = async (name) => importIdentity(parties[name]);
const sender = async (name) => importSender(parties[name].id, parties[name].public);
const headerOf = ({ keys, ct, sig, ...header }) => header;

const failure = async (promise) => {
  try {
    await promise;
  } catch (error) {
    if (error instanceof ProtocolError) return error.code;
    throw error;
  }
  return null;
};

test("private keys stay non-extractable", async () => {
  const phone = await identity("phone");
  assert.equal(phone.signing.extractable, false);
  assert.equal(phone.encryption.privateKey.extractable, false);
});

test("header bytes match the Rust encoding", () => {
  for (const vector of envelopes.cases) {
    assert.equal(toB64(headerBytes(vector.envelope)), vector.header_bytes, vector.name);
  }
});

test("envelopes open or fail as the vectors say", async () => {
  for (const vector of envelopes.cases) {
    const recipient = await identity(vector.recipient);
    const from = await sender(vector.sender);
    if (vector.expect.payload) {
      assert.deepEqual(await openEnvelope(vector.envelope, recipient, from, envelopes.now), vector.expect.payload, vector.name);
    } else {
      assert.equal(await failure(openEnvelope(vector.envelope, recipient, from, envelopes.now)), vector.expect.error, vector.name);
    }
  }
});

test("a changed header fails decryption even with the signature unchecked", async () => {
  const tampered = envelopes.cases.filter((vector) => vector.expect.decrypt_error);
  assert.ok(tampered.length >= 5);
  for (const vector of tampered) {
    const recipient = await identity(vector.recipient);
    assert.equal(await failure(decryptEnvelope(vector.envelope, recipient)), vector.expect.decrypt_error, vector.name);
  }
});

test("the web seals byte for byte what Rust sealed", async () => {
  const resealed = envelopes.cases.filter((vector) => vector.seal);
  assert.ok(resealed.length >= 4);
  for (const vector of resealed) {
    const from = await identity(vector.sender);
    const recipients = Object.keys(vector.envelope.keys).map((id) => ({ id, encryption: Object.values(parties).find((party) => party.id === id).public.encryption }));
    const ephemeral = Object.fromEntries(Object.entries(vector.seal.ephemeral).map(([id, ikm]) => [id, fromB64(ikm)]));
    const randomness = { contentKey: fromB64(vector.seal.content_key), nonce: fromB64(vector.seal.nonce), ephemeral };
    const sealed = await sealEnvelope(headerOf(vector.envelope), fromB64(vector.seal.plaintext), from, recipients, randomness);
    assert.deepEqual(sealed, vector.envelope, vector.name);
  }
});

test("a command sealed with fresh randomness opens on the other side", async () => {
  const vector = envelopes.cases.find((candidate) => candidate.name === "answer-opened-by-the-machine");
  const phone = await identity("phone");
  const recipients = ["phone", "machine"].map((name) => ({ id: parties[name].id, encryption: parties[name].public.encryption }));
  const plaintext = new TextEncoder().encode(JSON.stringify(vector.expect.payload));
  const sealed = await sealEnvelope(headerOf(vector.envelope), plaintext, phone, recipients);
  assert.notEqual(sealed.ct, vector.envelope.ct);
  assert.deepEqual(await openEnvelope(sealed, await identity("machine"), await sender("phone"), envelopes.now), vector.expect.payload);
});

test("signatures verify and reproduce", async () => {
  for (const vector of load("signatures.json").cases) {
    const signer = await identity(vector.signer);
    const verifier = await sender(vector.signer);
    const message = fromB64(vector.message);
    assert.ok(await crypto.subtle.verify({ name: "Ed25519" }, verifier.signing, fromB64(vector.signature), message), vector.signer);
    assert.equal(toB64(await crypto.subtle.sign({ name: "Ed25519" }, signer.signing, message)), vector.signature, vector.signer);
  }
});

test("pairing commits before the comparison code and refuses a different reveal", async () => {
  for (const vector of load("pairing.json").cases) {
    const machine = parties[vector.machine].public;
    const device = parties[vector.device].public;
    const deviceNonce = fromB64(vector.device_nonce);
    const machineNonce = fromB64(vector.machine_nonce);
    const commitment = await pairingCommitment(device, deviceNonce, vector.code);
    assert.equal(toB64(commitment), vector.commitment, vector.name);
    const revealed = machineComparisonCode(commitment, machine, machineNonce, parties[vector.revealed.device].public, fromB64(vector.revealed.nonce), vector.machine_code);
    if (vector.expect.sas) {
      assert.equal(await revealed, vector.expect.sas, vector.name);
      assert.equal(await comparisonCode(machine, device, machineNonce, deviceNonce, vector.code), vector.expect.sas, vector.name);
    } else {
      assert.equal(await failure(revealed), vector.expect.error, vector.name);
    }
  }
});

test("a connect link names its machine and the browser commits to its secret", async () => {
  const vector = load("pairing.json").connect;
  const machine = parties[vector.machine].public;
  assert.equal(await machineFingerprint(machine), vector.fingerprint);
  assert.equal(await machineFingerprint(parties.intruder.public), vector.other_fingerprint);
  const parsed = parseConnectFragment(new URL(vector.link).hash);
  assert.deepEqual(parsed, { code: vector.code, fingerprint: vector.fingerprint });
  const commitment = await pairingCommitment(parties[vector.device].public, fromB64(vector.device_nonce), parsed.code);
  assert.equal(toB64(commitment), vector.commitment);
  for (const broken of ["", "#k=AAAA&m=BBBB", `#k=${vector.code}`, `#m=${vector.fingerprint}`, "#k=!!&m=??"]) assert.equal(parseConnectFragment(broken), null, broken);
});

test("a pairing nonce is 32 fresh bytes", () => {
  const [first, second] = [pairingNonce(), pairingNonce()];
  assert.equal(first.length, 32);
  assert.notDeepEqual(first, second);
});

test("sealing refuses a recipient listed twice", async () => {
  const vector = envelopes.cases.find((candidate) => candidate.name === "answer-opened-by-the-machine");
  const machine = { id: parties.machine.id, encryption: parties.machine.public.encryption };
  const phone = await identity("phone");
  assert.equal(await failure(sealEnvelope(headerOf(vector.envelope), new Uint8Array([123, 125]), phone, [machine, machine])), "duplicate-recipient");
});

test("decrypted payloads with unknown fields or wrong types are refused", () => {
  const board = envelopes.cases.find((vector) => vector.name === "board-opened-by-a-device").expect.payload;
  checkPayload(board);
  const leaked = structuredClone(board);
  leaked.items[0].path = "/Users/someone";
  const wrongType = structuredClone(board);
  wrongType.rev = "1842";
  const missing = structuredClone(board);
  delete missing.items[0].follows_main;
  for (const payload of [leaked, wrongType, missing, { ...board, type: "toString" }]) {
    assert.throws(() => checkPayload(payload), (error) => error.code === "malformed");
  }
});

test("a key row under a string that is not an id is refused", () => {
  const good = envelopes.cases.find((vector) => vector.name === "answer-opened-by-the-machine").envelope;
  assert.throws(() => checkShape({ ...good, keys: { ...good.keys, __proto__x: "AAAA" } }), (error) => error.code === "malformed");
});

test("base64url decoding is strict", () => {
  assert.deepEqual([...fromB64("AQID")], [1, 2, 3]);
  for (const bad of ["AQI=", "AQ+/", "AR", "A", "AQI D", 3]) {
    assert.throws(() => fromB64(bad), ProtocolError, String(bad));
  }
});

test("a header of the wrong types is refused before any crypto", () => {
  const good = envelopes.cases.find((vector) => vector.name === "answer-opened-by-the-machine").envelope;
  checkShape(good);
  const variants = [
    [{ v: "1" }, "unsupported-version"],
    [{ schema: 2 }, "unsupported-schema"],
    [{ at: String(good.at) }, "malformed"],
    [{ exp: good.at + 86_401 }, "malformed"],
    [{ kind: "toString" }, "malformed"],
    [{ kc: 7 }, "malformed"],
    [{ keys: [] }, "malformed"],
    [{ machine: good.from }, "malformed"],
  ];
  for (const [change, code] of variants) {
    assert.throws(() => checkShape({ ...good, ...change }), (error) => error.code === code, JSON.stringify(change));
  }
});
