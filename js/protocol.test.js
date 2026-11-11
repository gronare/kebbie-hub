import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import { commandId, COMMAND_TTL, deviceListStatement, deviceListTrusts, deviceListWith, deviceListWithout, emptyDeviceList, newDeviceKeys, sealCommand, signDeviceList, verifyDeviceList, accountCommitment, accountFingerprints, accountIdentity, accountWrapKey, checkPayload, checkShape, machineListStatement, machineListTrusts, machineListWith, machineListWithout, emptyMachineList, newAccountSecrets, signMachineList, signStatement, unwrapAccountSecrets, verifyMachineList, wrapAccountSecrets, wrapAssociatedData, wrapperStatement, comparisonCode, decryptEnvelope, fromB64, headerBytes, importIdentity, importSender, machineComparisonCode, machineFingerprint, openBlob, openEnvelope, pairingCommitment, pairingNonce, parseConnectFragment, ProtocolError, sealEnvelope, toB64 } from "./protocol.js";

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

test("a board item's steps carry only their own fields and known states, and a board without them still passes", () => {
  const stepped = envelopes.cases.find((vector) => vector.name === "board-with-steps-opened-by-the-account").expect.payload;
  checkPayload(stepped);
  const plain = envelopes.cases.find((vector) => vector.name === "board-opened-by-a-device").expect.payload;
  assert.equal(Object.hasOwn(plain.items[0], "steps"), false);
  checkPayload(plain);
  const subject = structuredClone(stepped);
  subject.items[0].steps.items[0].subject = "Charge through a PaymentGateway";
  const state = structuredClone(stepped);
  state.items[0].steps.items[0].state = "pushed";
  const pushed = structuredClone(stepped);
  pushed.items[0].steps.pushed = -1;
  for (const payload of [subject, state, pushed]) {
    assert.throws(() => checkPayload(payload), (error) => error.code === "malformed");
  }
});

test("a question's plan and a task's card are blob refs, the plan's time sits on the question, and neither carries its text", () => {
  const plain = envelopes.cases.find((vector) => vector.name === "board-opened-by-a-device").expect.payload;
  const blob = load("blobs.json").cases[0].blob;
  const referenced = structuredClone(plain);
  referenced.items[0].question.plan = blob;
  referenced.items[0].card = { sha: "9f3c2a1", current: true, blob };
  checkPayload(referenced);
  const written = structuredClone(referenced);
  written.items[0].question.plan_at = 1_761_068_280;
  checkPayload(written);
  const leaked = structuredClone(referenced);
  leaked.items[0].card.blob.text = "the card in clear";
  const when = structuredClone(written);
  when.items[0].question.plan_at = "20:18";
  const inRef = structuredClone(referenced);
  inRef.items[0].question.plan.at = 1_761_068_280;
  for (const payload of [leaked, when, inRef]) {
    assert.throws(() => checkPayload(payload), (error) => error.code === "malformed");
  }
});

test("a board with Next, counts, the tasks set aside and each item's agent and reviews passes, and a board without them still does", () => {
  const full = envelopes.cases.find((vector) => vector.name === "board-with-next-opened-by-the-account").expect.payload;
  checkPayload(full);
  assert.deepEqual(full.next.map((row) => [row.rank, row.part]), [[1, "stopped"], [2, "push"]]);
  assert.equal(full.items[0].prompt.options[0].key, "y");
  const plain = envelopes.cases.find((vector) => vector.name === "board-opened-by-a-device").expect.payload;
  for (const key of ["next", "counts", "later"]) assert.equal(Object.hasOwn(plain, key), false);
  for (const key of ["title", "profile", "recipe", "assessment", "second_opinion"]) assert.equal(Object.hasOwn(plain.items[0], key), false);
  checkPayload(plain);
  const wrong = [
    (board) => (board.next[0].part = "later"),
    (board) => (board.next[0].waits_behind = -1),
    (board) => (board.later[0].state = "working"),
    (board) => (board.later[0].path = "/Users/someone"),
    (board) => (board.counts.idle = 1),
    (board) => (board.items[1].second_opinion.points = "two"),
    (board) => (board.items[1].agent.pane = "w1:p9"),
    (board) => (board.items[1].steps.required = "no"),
  ];
  for (const change of wrong) {
    const payload = structuredClone(full);
    change(payload);
    assert.throws(() => checkPayload(payload), (error) => error.code === "malformed", String(change));
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
  const hubSalt = vectors.hub_prf_salt;
  assert.equal(toB64(new Uint8Array(await crypto.subtle.digest("SHA-256", new TextEncoder().encode(hubSalt.domain)))), hubSalt.salt);
  assert.equal(toB64(await accountWrapKey(fromB64(hubSalt.prf_output), fromB64(hubSalt.salt))), hubSalt.wrap_key);
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

test("blobs open or fail as the vectors say", async () => {
  const blobs = load("blobs.json");
  for (const vector of blobs.cases) {
    const ciphertext = fromB64(vector.ciphertext);
    if (vector.error) assert.equal(await failure(openBlob(vector.blob, ciphertext)), vector.error, vector.name);
    else assert.equal(new TextDecoder().decode(await openBlob(vector.blob, ciphertext)), blobs.plaintext, vector.name);
  }
});

async function unlockedAccount() {
  const vectors = load("accounts.json");
  const party = parties[vectors.account];
  const secrets = { signing: fromB64(party.signing_secret), encryption: fromB64(party.encryption_secret) };
  return { vectors, account: await accountIdentity(vectors.key, secrets, vectors.key.account) };
}

test("a device list signs and verifies as the Rust vectors say, and nothing else verifies", async () => {
  const { vectors, account } = await unlockedAccount();
  const lists = vectors.devices;
  assert.equal(new TextDecoder().decode(fromB64(lists.statement).slice(0, lists.domain.length)), lists.domain);
  assert.equal(toB64(deviceListStatement(account.id, lists.list)), lists.statement);
  assert.equal(await signDeviceList(account, lists.list), lists.signature);
  assert.equal(await verifyDeviceList(account, { list: lists.list, signature: lists.signature }), true);
  assert.equal(await verifyDeviceList(account, { list: lists.removed, signature: lists.removed_signature }), true);
  assert.equal(await verifyDeviceList(account, { list: lists.removed, signature: lists.signature }), false, "another list's signature");
  assert.equal(await verifyDeviceList(account, { list: { ...lists.list, counter: lists.removed.counter }, signature: lists.signature }), false, "the counter is signed");
  assert.equal(await verifyDeviceList(account, { list: { ...lists.list, devices: [...lists.list.devices].reverse() }, signature: lists.signature }), false, "out of order");
  const promoted = structuredClone(lists.list);
  promoted.devices[0].role = "control";
  assert.equal(await verifyDeviceList(account, { list: promoted, signature: lists.signature }), false, "the role is signed");
  const renamed = structuredClone(lists.list);
  renamed.devices[1].name = "Intruder";
  assert.equal(await verifyDeviceList(account, { list: renamed, signature: lists.signature }), false, "the name is signed");
  const other = await accountIdentity(vectors.key, { signing: fromB64(parties.intruder.signing_secret), encryption: fromB64(parties.intruder.encryption_secret) }, vectors.key.account).catch(() => null);
  assert.equal(other, null, "another key is not this account's identity");
  const intruder = { id: account.id, verifying: (await importSender(parties.intruder.id, parties.intruder.public)).signing };
  assert.equal(await verifyDeviceList(intruder, { list: lists.list, signature: lists.signature }), false);
  const trusted = parties[lists.trusted];
  const gone = parties[lists.removed_device];
  assert.ok(deviceListTrusts(lists.list, gone.id, gone.public) && !deviceListTrusts(lists.removed, gone.id, gone.public));
  assert.ok(!deviceListTrusts(lists.removed, trusted.id, parties.intruder.public));
  const [tablet, phone] = lists.list.devices;
  const rebuilt = deviceListWith(deviceListWith(emptyDeviceList(), phone), tablet);
  assert.deepEqual(rebuilt, lists.list);
  assert.deepEqual(deviceListWithout(rebuilt, gone.id), lists.removed);
  assert.throws(() => deviceListStatement(account.id, { counter: 1, devices: [{ ...phone, device: parties.machine.id }] }));
  assert.throws(() => deviceListStatement(account.id, { counter: 1, devices: [{ ...phone, role: "admin" }] }));
});

test("a command id is a UUIDv7 with the time it was sealed", () => {
  const id = commandId(1_761_069_600_000, new Uint8Array(10).fill(0xff));
  assert.match(id, /^cmd_[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/);
  assert.equal(id.slice(4, 17).replace("-", ""), (1_761_069_600_000).toString(16).padStart(12, "0"));
  const vector = envelopes.cases.find((candidate) => candidate.name === "answer-opened-by-the-machine");
  assert.equal(commandId(vector.envelope.at * 1000).slice(0, 17), vector.envelope.id.slice(0, 17));
});

test("a command sealed by a fresh device key opens for the machine and for the device, and for nobody else", async () => {
  const keys = await newDeviceKeys();
  assert.equal(keys.signing.extractable, false);
  assert.equal(keys.encryption.privateKey.extractable, false);
  const device = { id: parties.phone.id, ...keys };
  const machine = { id: parties.machine.id, keys: parties.machine.public };
  const at = envelopes.now;
  const payload = { type: "push", address: "refunds@shop", head: "9f3c2a1e5b7d4c6a8b0e2f4d6c8a0b2e4f6d8c0a", card_sha: null };
  const envelope = await sealCommand({ device, machine, payload, at });
  checkShape(envelope);
  assert.deepEqual([envelope.kind, envelope.from, envelope.machine, envelope.at, envelope.exp], ["push", device.id, machine.id, at, at + COMMAND_TTL]);
  assert.equal(COMMAND_TTL, 3600);
  assert.match(envelope.id, /^cmd_[0-9a-f]{8}-[0-9a-f]{4}-7/);
  assert.deepEqual(Object.keys(envelope.keys).sort(), [device.id, machine.id].sort());
  const fromDevice = { id: device.id, signing: keys.verifying };
  assert.deepEqual(await openEnvelope(envelope, await identity("machine"), fromDevice, at), payload);
  assert.deepEqual(await openEnvelope(envelope, device, fromDevice, at), payload);
  assert.equal(await failure(openEnvelope(envelope, await identity("tablet"), fromDevice, at)), "not-a-recipient");
  assert.equal(await failure(openEnvelope(envelope, await identity("machine"), await sender("phone"), at)), "bad-signature");
  assert.equal(await failure(sealCommand({ device, machine, payload: { type: "answer", address: "a@b", question: "q", choice: "first", comment: "" }, at })), "malformed");
});

test("a question option may say it needs a note, and nothing else", () => {
  const board = envelopes.cases.find((vector) => vector.name === "board-opened-by-a-device").expect.payload;
  const asked = structuredClone(board);
  const question = asked.items.find((item) => item.question).question;
  question.options[0].comment = true;
  checkPayload(asked);
  question.options[0].comment = "yes";
  assert.throws(() => checkPayload(asked));
});
