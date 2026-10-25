import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import { accountCommitment, accountFingerprints, accountIdentity, accountWrapKey, checkPayload, checkShape, machineListStatement, machineListTrusts, machineListWith, machineListWithout, emptyMachineList, newAccountSecrets, signMachineList, signStatement, unwrapAccountSecrets, verifyMachineList, wrapAccountSecrets, wrapAssociatedData, wrapperStatement, comparisonCode, decryptEnvelope, fromB64, headerBytes, importIdentity, importSender, machineComparisonCode, machineFingerprint, openEnvelope, pairingCommitment, pairingNonce, parseConnectFragment, ProtocolError, sealEnvelope, toB64 } from "./protocol.js";

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

test("account keys wrap, sign and hand over as the Rust vectors say", async () => {
  const vectors = load("accounts.json");
  const party = parties[vectors.account];
  const { key, wrap } = vectors;
  assert.deepEqual([key.account, key.keys], [party.id, party.public]);
  const credential = fromB64(wrap.credential);
  const unlock = { account: key.account, credential, version: key.version, prfOutput: fromB64(wrap.prf_output), salt: fromB64(wrap.salt) };
  assert.equal(toB64(await accountWrapKey(unlock.prfOutput, unlock.salt)), wrap.wrap_key);
  assert.equal(toB64(wrapAssociatedData(key.account, credential, key.version)), wrap.associated_data);
  const secrets = { signing: fromB64(party.signing_secret), encryption: fromB64(party.encryption_secret) };
  assert.equal(toB64(await wrapAccountSecrets(unlock, secrets, fromB64(wrap.nonce))), wrap.wrapped);
  const opened = await unwrapAccountSecrets(unlock, fromB64(wrap.wrapped));
  assert.deepEqual([toB64(opened.signing), toB64(opened.encryption)], [party.signing_secret, party.encryption_secret]);
  for (const refusal of wrap.refusals) {
    const changed = { account: refusal.account, credential: fromB64(refusal.credential), version: refusal.key_version, prfOutput: fromB64(refusal.prf_output), salt: fromB64(refusal.salt) };
    assert.equal(await failure(unwrapAccountSecrets(changed, fromB64(wrap.wrapped))), refusal.expect, refusal.name);
  }
  const account = await accountIdentity(key, opened, key.account);
  assert.equal(account.signing.extractable, false);
  assert.equal(account.encryption.privateKey.extractable, false);
  assert.equal(await failure(accountIdentity({ ...key, keys: parties.intruder.public }, opened, key.account)), "key-mismatch");
  assert.equal(await failure(accountIdentity({ ...key, keys: { ...key.keys, encryption: parties.intruder.public.encryption } }, opened, key.account)), "key-mismatch");
  assert.deepEqual(await accountFingerprints(key), vectors.fingerprints);
  const statement = await wrapperStatement(key, credential, fromB64(wrap.wrapped));
  assert.equal(toB64(statement), vectors.wrapper.statement);
  assert.equal(toB64(await signStatement(account, statement)), vectors.wrapper.signature);
  const lists = vectors.machines;
  assert.equal(toB64(machineListStatement(key.account, lists.list)), lists.statement);
  assert.equal(await signMachineList(account, lists.list), lists.signature);
  assert.equal(await verifyMachineList(account, { list: lists.list, signature: lists.signature }), true);
  assert.equal(await verifyMachineList(account, { list: lists.removed, signature: lists.removed_signature }), true);
  assert.equal(await verifyMachineList(account, { list: lists.removed, signature: lists.signature }), false);
  assert.equal(await verifyMachineList(account, { list: { ...lists.list, counter: lists.removed.counter }, signature: lists.signature }), false);
  assert.equal(await verifyMachineList(account, { list: { ...lists.list, machines: [...lists.list.machines].reverse() }, signature: lists.signature }), false);
  const trusted = parties[lists.trusted];
  const gone = parties[lists.removed_machine];
  assert.ok(machineListTrusts(lists.list, gone.id, gone.public) && !machineListTrusts(lists.removed, gone.id, gone.public));
  assert.ok(machineListTrusts(lists.removed, trusted.id, trusted.public) && !machineListTrusts(lists.removed, trusted.id, parties.intruder.public));
  const rebuilt = machineListWith(machineListWith(emptyMachineList(), gone.id, gone.public), trusted.id, trusted.public);
  assert.deepEqual(rebuilt, lists.list);
  assert.deepEqual(machineListWithout(rebuilt, gone.id), lists.removed);
  const { handoff } = vectors;
  assert.equal(toB64(await accountCommitment(key, fromB64(handoff.nonce), handoff.code)), handoff.commitment);
  for (const reveal of handoff.reveals) {
    const matches = toB64(await accountCommitment(reveal.revealed, fromB64(handoff.nonce), reveal.machine_code)) === handoff.commitment;
    assert.equal(matches, Boolean(reveal.expect.pinned), reveal.name);
    if (reveal.expect.pinned) assert.deepEqual(await accountFingerprints(reveal.revealed), reveal.expect.pinned);
  }
});

test("a board sealed to the account opens with the unwrapped account key and no device row", async () => {
  const vectors = load("accounts.json");
  const party = parties[vectors.account];
  const account = await accountIdentity(vectors.key, { signing: fromB64(party.signing_secret), encryption: fromB64(party.encryption_secret) }, vectors.key.account);
  const vector = envelopes.cases.find((candidate) => candidate.name === "board-opened-by-the-account");
  assert.deepEqual(await openEnvelope(vector.envelope, account, await sender("machine"), envelopes.now), vector.expect.payload);
  assert.equal(await failure(openEnvelope(vector.envelope, await identity("phone"), await sender("machine"), envelopes.now)), "not-a-recipient");
});

test("a fresh account key is random, and its public half is what the browser reports", async () => {
  const [first, second] = [await newAccountSecrets(parties.account.id), await newAccountSecrets(parties.account.id)];
  assert.notDeepEqual(first.key.keys, second.key.keys);
  const account = await accountIdentity(first.key, first.secrets, parties.account.id);
  assert.equal(account.id, parties.account.id);
  first.wipe();
  assert.deepEqual([...first.secrets.signing], new Array(32).fill(0));
});

test("an account key is used only for the account the hub signed in", async () => {
  const vectors = load("accounts.json");
  const party = parties[vectors.account];
  const secrets = { signing: fromB64(party.signing_secret), encryption: fromB64(party.encryption_secret) };
  assert.equal(await failure(accountIdentity(vectors.key, secrets, "acc_AAAAAAAAAAAAAAAAAAAAAA")), "wrong-account");
  assert.equal(await failure(accountIdentity(vectors.key, secrets)), "wrong-account");
});

test("making and checking account keys never exports a private key as JWK", async () => {
  const subtle = globalThis.crypto.subtle;
  const exportKey = subtle.exportKey.bind(subtle);
  subtle.exportKey = async (format, key) => {
    if (format === "jwk") throw new Error("a private key was exported as JWK");
    return exportKey(format, key);
  };
  try {
    const fresh = await newAccountSecrets(parties.account.id);
    const account = await accountIdentity(fresh.key, fresh.secrets, parties.account.id);
    assert.equal(account.key.keys.signing, fresh.key.keys.signing);
  } finally {
    subtle.exportKey = exportKey;
  }
});

test("a board lives at most a day", () => {
  const board = envelopes.cases.find((candidate) => candidate.name === "board-opened-by-the-account").envelope;
  checkShape({ ...board, exp: board.at + 86_400 });
  assert.throws(() => checkShape({ ...board, exp: board.at + 86_401 }), (error) => error.code === "malformed");
});
