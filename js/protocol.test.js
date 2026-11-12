import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import { MAX_ITEM_TITLE_CHARS, MAX_SECTION_CHARS, MAX_ITEM_EDIT_BYTES, MAX_HEADING_CHARS, textHash, REVIEW_MEDIA_TYPE, MAX_REVIEW_COMMENTS, MAX_REVIEW_COMMENT_CHARS, MAX_REVIEW_COMMENTS_BYTES, checkReview, DEFAULT_NOTICE_CATEGORIES, MAX_NOTICE_BODY_CHARS, MAX_NOTICE_TITLE_CHARS, NOTICE_CATEGORIES, openNotice, BACKLOG_BODY_MEDIA_TYPE, checkBacklogBody, isItemKey, KINDS, MAX_IDEA_CHARS, MAX_ITEM_ANSWER_CHARS, START_ITEM_TTL, commandId, COMMAND_TTL, PROMPT_ANSWER_TTL, sealCommand, deviceCertStatement, deviceFingerprint, deviceId, newDeviceKey, openCommand, signDeviceCert, verifyCommand, verifyDeviceCert, accountCommitment, accountFingerprints, accountIdentity, accountWrapKey, checkPayload, checkShape, machineListStatement, machineListTrusts, machineListWith, machineListWithout, emptyMachineList, newAccountSecrets, signMachineList, signStatement, unwrapAccountSecrets, verifyMachineList, wrapAccountSecrets, wrapAssociatedData, wrapperStatement, decryptEnvelope, fromB64, headerBytes, importIdentity, importSender, machineFingerprint, openBlob, openEnvelope, handoffNonce, parseConnectFragment, ProtocolError, sealEnvelope, toB64 } from "./protocol.js";

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
  const recipients = ["account", "machine"].map((name) => ({ id: parties[name].id, encryption: parties[name].public.encryption }));
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

test("a connect link names its machine", async () => {
  const vector = load("connect.json");
  const machine = parties[vector.machine].public;
  assert.equal(await machineFingerprint(machine), vector.fingerprint);
  assert.equal(await machineFingerprint(parties.intruder.public), vector.other_fingerprint);
  const parsed = parseConnectFragment(new URL(vector.link).hash);
  assert.deepEqual(parsed, { code: vector.code, fingerprint: vector.fingerprint });
  for (const broken of ["", "#k=AAAA&m=BBBB", `#k=${vector.code}`, `#m=${vector.fingerprint}`, "#k=!!&m=??"]) assert.equal(parseConnectFragment(broken), null, broken);
});

test("a handoff nonce is 32 fresh bytes", () => {
  const [first, second] = [handoffNonce(), handoffNonce()];
  assert.equal(first.length, 32);
  assert.notDeepEqual(first, second);
});

test("sealing refuses a recipient listed twice", async () => {
  const vector = envelopes.cases.find((candidate) => candidate.name === "answer-opened-by-the-machine");
  const machine = { id: parties.machine.id, encryption: parties.machine.public.encryption };
  const account = await identity("account");
  assert.equal(await failure(sealEnvelope(headerOf(vector.envelope), new Uint8Array([123, 125]), account, [machine, machine])), "duplicate-recipient");
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

test("a command id is a UUIDv7 with the time it was sealed", () => {
  const id = commandId(1_761_069_600_000, new Uint8Array(10).fill(0xff));
  assert.match(id, /^cmd_[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/);
  assert.equal(id.slice(4, 17).replace("-", ""), (1_761_069_600_000).toString(16).padStart(12, "0"));
  const vector = envelopes.cases.find((candidate) => candidate.name === "answer-opened-by-the-machine");
  assert.equal(commandId(vector.envelope.at * 1000).slice(0, 17), vector.envelope.id.slice(0, 17));
});

const devices = load("devices.json");

async function certifiedPhone() {
  const phone = await identity("phone");
  return { id: phone.id, signing: phone.signing, cert: devices.certs.find((listed) => listed.device === "phone").cert };
}

test("a device is certified by the account byte for byte as Rust certifies it", async () => {
  const { account } = await unlockedAccount();
  for (const listed of devices.certs) {
    const { sig, ...unsigned } = listed.cert;
    assert.equal(toB64(deviceCertStatement(unsigned)), listed.statement, listed.device);
    assert.deepEqual(await signDeviceCert(account, unsigned), listed.cert, listed.device);
    assert.equal(await verifyDeviceCert(devices.key, listed.cert), true, listed.device);
    assert.equal(await deviceFingerprint(listed.cert.signing), listed.fingerprint, listed.device);
  }
  for (const refusal of devices.refusals) assert.equal(await verifyDeviceCert(devices.key, refusal.cert), false, refusal.name);
  for (const listed of devices.ids) assert.equal(deviceId(listed.millis, fromB64(listed.random)), listed.id);
  const { sig, ...unsigned } = devices.certs[0].cert;
  for (const name of ["", "x".repeat(65), "iPhone\n16"]) assert.throws(() => deviceCertStatement({ ...unsigned, name }), (error) => error.code === "malformed", JSON.stringify(name));
  assert.throws(() => deviceCertStatement({ ...unsigned, kind: "ios" }), (error) => error.code === "malformed");
  assert.throws(() => deviceCertStatement({ ...unsigned, device: parties.machine.id }), (error) => error.code === "malformed");
});

test("a device's commands verify against the account as Rust verifies them", async () => {
  const account = await identity("account");
  for (const vector of devices.commands) {
    assert.equal(toB64(headerBytes(vector.envelope)), vector.header_bytes, vector.name);
    if (vector.expect.device) {
      const device = await verifyCommand(vector.envelope, devices.key, devices.now);
      assert.deepEqual([device.device, await deviceFingerprint(device.signing)], [vector.expect.device, vector.expect.fingerprint], vector.name);
      assert.deepEqual((await openCommand(vector.envelope, account, devices.key, devices.now)).payload, devices.payload, vector.name);
    } else {
      assert.equal(await failure(verifyCommand(vector.envelope, devices.key, devices.now)), vector.expect.error, vector.name);
    }
  }
});

test("a command sealed by a device opens for the machine and the account, names the device, and opens for nobody else", async () => {
  const device = await certifiedPhone();
  const machine = { id: parties.machine.id, keys: parties.machine.public };
  const at = envelopes.now;
  const payload = { type: "push", address: "refunds@shop", head: "9f3c2a1e5b7d4c6a8b0e2f4d6c8a0b2e4f6d8c0a", card_sha: null };
  const envelope = await sealCommand({ device, account: devices.key, machine, payload, at });
  checkShape(envelope);
  assert.deepEqual([envelope.kind, envelope.from, envelope.machine, envelope.at, envelope.exp], ["push", device.id, machine.id, at, at + COMMAND_TTL]);
  assert.equal(COMMAND_TTL, 3600);
  assert.match(envelope.id, /^cmd_[0-9a-f]{8}-[0-9a-f]{4}-7/);
  assert.deepEqual(Object.keys(envelope.keys).sort(), [devices.key.account, machine.id].sort());
  for (const recipient of [await identity("machine"), await identity("account")]) {
    const opened = await openCommand(envelope, recipient, devices.key, at);
    assert.deepEqual([opened.device.device, opened.payload], [device.id, payload]);
  }
  assert.equal(await failure(openCommand(envelope, await identity("phone"), devices.key, at)), "not-a-recipient");
  assert.equal(await failure(verifyCommand(envelope, { ...devices.key, account: parties["other-account"].id }, at)), "wrong-account");
  assert.equal(await failure(verifyCommand(envelope, { ...devices.key, keys: parties["other-account"].public }, at)), "bad-certificate");
  assert.equal(await failure(verifyCommand(envelope, devices.key, at + COMMAND_TTL)), "expired");
  assert.equal(await failure(sealCommand({ device: { ...device, signing: (await identity("tablet")).signing }, account: devices.key, machine, payload, at }).then((forged) => verifyCommand(forged, devices.key, at))), "bad-signature");
  assert.equal(await failure(sealCommand({ device, account: { ...devices.key, keys: parties["other-account"].public }, machine, payload, at })), "cert-mismatch");
  assert.equal(await failure(sealCommand({ device, account: devices.key, machine, payload: { type: "answer", address: "a@b", question: "q", choice: "first", comment: "" }, at })), "malformed");
});

test("a fresh device key never leaves the browser, and the account certifies it for commands", async () => {
  const { account } = await unlockedAccount();
  const key = await newDeviceKey();
  assert.equal(key.signing.extractable, false);
  const id = deviceId();
  assert.match(id, /^dev_[0-7][0-9A-HJKMNP-TV-Z]{25}$/);
  const cert = await signDeviceCert(account, { device: id, name: "iPhone 16", signing: key.public, encryption: key.encryptionPublic, created_at: envelopes.now });
  assert.equal(await verifyDeviceCert(devices.key, cert), true);
  assert.equal(await verifyDeviceCert(devices.key, { ...cert, encryption: parties.tablet.public.encryption }), false);
  const payload = { type: "routine", id: "nightly-deps" };
  const envelope = await sealCommand({ device: { id, signing: key.signing, cert }, account: devices.key, machine: { id: parties.machine.id, keys: parties.machine.public }, payload, at: envelopes.now });
  assert.deepEqual((await openCommand(envelope, await identity("machine"), devices.key, envelopes.now)).payload, payload);
  assert.match(await deviceFingerprint(key.public), /^[0-9A-F]{4}-[0-9A-F]{4}$/);
});

const noticeFor = () => ({ type: "notice", category: "questions", title: "refunds · shop", body: "Keep the old endpoint for a release?", open: { to: "task", address: "refunds@shop" } });

test("a notice a machine seals to a fresh device opens with the device's own key, which never leaves the browser", async () => {
  const key = await newDeviceKey();
  assert.equal(key.encryption.privateKey.extractable, false);
  const device = { id: deviceId(), encryption: key.encryption };
  const machine = await identity("machine");
  const at = envelopes.now;
  const header = { v: 1, id: "ntc_01K85Y2D8G0000000000000000", kind: "notice", from: machine.id, machine: machine.id, at, exp: at + 3600, schema: 1, urgency: "high", collapse: "q-refunds" };
  const envelope = await sealEnvelope(header, new TextEncoder().encode(JSON.stringify(noticeFor())), machine, [{ id: device.id, encryption: key.encryptionPublic }]);
  const listed = { id: machine.id, keys: parties.machine.public };
  assert.deepEqual(await openNotice(envelope, device, listed, at), noticeFor());
  assert.equal(await failure(openNotice(envelope, device, { id: machine.id, keys: parties["other-machine"].public }, at)), "bad-signature");
  assert.equal(await failure(openNotice(envelope, device, { id: parties["other-machine"].id, keys: parties["other-machine"].public }, at)), "malformed");
  assert.equal(await failure(openNotice(envelope, { id: device.id, encryption: (await newDeviceKey()).encryption }, listed, at)), "undecryptable");
  const { collapse, ...uncollapsed } = envelope;
  assert.throws(() => checkShape(uncollapsed), (error) => error.code === "malformed");
});

test("a notice is one line, a plain body and a target inside the app, small enough to push", () => {
  const valid = (payload) => {
    try {
      checkPayload(payload);
      return true;
    } catch {
      return false;
    }
  };
  const notice = noticeFor();
  assert.equal(valid(notice), true);
  assert.equal(valid({ ...notice, body: "Keep it?\nThe lead says keep it.", retract: ["q-refunds"] }), true);
  for (const open of [{ to: "item", item: "kept:idea-1" }, { to: "device", device: parties.tablet.id }]) assert.equal(valid({ ...notice, open }), true, open.to);
  for (const [name, wrong] of [
    ["title on two lines", { title: "a\nb" }],
    ["blank title", { title: " " }],
    ["long title", { title: "x".repeat(MAX_NOTICE_TITLE_CHARS + 1) }],
    ["empty body", { body: "" }],
    ["bell in the body", { body: "a\u0007b" }],
    ["long body", { body: "x".repeat(MAX_NOTICE_BODY_CHARS + 1) }],
    ["unknown category", { category: "summary" }],
    ["a url to open", { open: { to: "url", url: "https://example.com/" } }],
    ["a task without task@repo", { open: { to: "task", address: "refunds" } }],
    ["a device that is the account", { open: { to: "device", device: parties.account.id } }],
    ["a retract that is not a tag", { retract: ["a b"] }],
    ["too many retracts", { retract: Array(9).fill("a") }],
    ["too large to push", { title: "€".repeat(MAX_NOTICE_TITLE_CHARS), body: "€".repeat(MAX_NOTICE_BODY_CHARS), retract: Array(8).fill("a".repeat(64)) }],
  ]) assert.equal(valid({ ...notice, ...wrong }), false, name);
  assert.deepEqual(NOTICE_CATEGORIES, ["questions", "new-device", "idea-questions", "ready", "ci-failed", "started"]);
  assert.deepEqual(DEFAULT_NOTICE_CATEGORIES, ["questions", "new-device", "idea-questions", "ready", "ci-failed"]);
});

test("a device decision names the waiting device by id and fingerprint and lives an hour", async () => {
  const decision = { type: "device-decision", device: parties.tablet.id, fingerprint: "0C11-5E9A", approve: true };
  checkPayload(decision);
  for (const wrong of [{ fingerprint: "0c11-5e9a" }, { fingerprint: "0C115E9A" }, { device: parties.account.id }, { approve: "yes" }, { reason: "x" }]) {
    assert.throws(() => checkPayload({ ...decision, ...wrong }), (error) => error.code === "malformed", JSON.stringify(wrong));
  }
  assert.deepEqual([KINDS["device-decision"].command, KINDS["device-decision"].ttl, KINDS["device-decision"].maxTtl], [true, 3600, 3600]);
  const envelope = await sealCommand({ device: await certifiedPhone(), account: devices.key, machine: { id: parties.machine.id, keys: parties.machine.public }, payload: decision, at: envelopes.now });
  assert.deepEqual((await openCommand(envelope, await identity("machine"), devices.key, envelopes.now)).payload, decision);
});

test("a command without its device's certificate, or from the account, has no valid shape", () => {
  for (const name of ["account-as-command-sender", "command-without-a-certificate", "sender-is-not-the-certified-device", "certificate-on-a-board"]) {
    const vector = envelopes.cases.find((candidate) => candidate.name === name);
    assert.equal(vector.expect.error, "malformed", name);
    assert.throws(() => checkShape(vector.envelope), (error) => error.code === "malformed", name);
  }
});

test("a board says where each device stands on its machine, and a board without it still passes", () => {
  const full = envelopes.cases.find((vector) => vector.name === "board-with-next-opened-by-the-account").expect.payload;
  assert.deepEqual(Object.values(full.devices).map((standing) => standing.state).sort(), ["approved", "denied", "waiting"]);
  checkPayload(full);
  const [phone] = Object.keys(full.devices);
  const wrong = [
    (board) => (board.devices[phone] = { state: "trusted", fingerprint: "7F3A-91C2", at: 1 }),
    (board) => (board.devices[phone] = { state: "approved", fingerprint: "7F3A-91C2", at: 1, until: 2 }),
    (board) => (board.devices[phone] = { state: "denied", fingerprint: "7F3A-91C2" }),
    (board) => (board.devices[parties.account.id] = { state: "approved", fingerprint: "7F3A-91C2", at: 1 }),
    (board) => (board.devices = []),
  ];
  for (const change of wrong) {
    const payload = structuredClone(full);
    change(payload);
    assert.throws(() => checkPayload(payload), (error) => error.code === "malformed", String(change));
  }
});

test("a result names the device whose command it answers", () => {
  const result = envelopes.cases.find((vector) => vector.name === "result-opened-by-the-account").expect.payload;
  assert.equal(result.device, parties.phone.id);
  checkPayload(result);
  checkPayload({ ...result, outcome: "refused", reason: "not-approved" });
  checkPayload({ ...result, outcome: "refused", reason: "denied" });
  assert.throws(() => checkPayload({ ...result, device: parties.account.id }), (error) => error.code === "malformed");
  const { device, ...anonymous } = result;
  assert.throws(() => checkPayload(anonymous), (error) => error.code === "malformed");
});

test("an answer to an agent's prompt lives ten minutes, as Rust's limit for its kind says", async () => {
  assert.equal(envelopes.max_ttl["prompt-answer"], PROMPT_ANSWER_TTL);
  assert.equal(PROMPT_ANSWER_TTL, 600);
  const sealed = envelopes.cases.find((candidate) => candidate.name === "prompt-answer-opened-by-the-machine");
  checkPayload(sealed.expect.payload);
  checkShape(sealed.envelope);
  assert.throws(() => checkShape({ ...sealed.envelope, exp: sealed.envelope.at + 601 }), (error) => error.code === "malformed");
  const payload = { type: "prompt-answer", address: "hub@kebbie", prompt: "3f2a9c1d", key: "y" };
  const envelope = await sealCommand({ device: await certifiedPhone(), account: devices.key, machine: { id: parties.machine.id, keys: parties.machine.public }, payload, at: envelopes.now });
  assert.equal(envelope.exp - envelope.at, 600);
  assert.throws(() => checkPayload({ ...payload, choice: 0 }), (error) => error.code === "malformed");
});

test("a board says which commands its machine takes, and a board without the list still passes", () => {
  const full = envelopes.cases.find((vector) => vector.name === "board-with-next-opened-by-the-account").expect.payload;
  assert.deepEqual(full.accepts, ["answer", "push", "prompt-answer"]);
  checkPayload(full);
  const plain = envelopes.cases.find((vector) => vector.name === "board-opened-by-a-device").expect.payload;
  assert.equal(Object.hasOwn(plain, "accepts"), false);
  assert.throws(() => checkPayload({ ...full, accepts: "prompt-answer" }), (error) => error.code === "malformed");
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

test("a board lists its backlog by item key, each row naming its home and a safe label, and a board without it still passes", () => {
  const full = envelopes.cases.find((vector) => vector.name === "board-with-backlog-opened-by-the-account").expect.payload;
  checkPayload(full);
  assert.deepEqual(full.backlog.map((row) => [row.item, row.home.kind, row.state]), [["vault:Projects/refund-rounding.md", "vault", "ready"], ["local:01JB7Q2M3N8K", "local", "drafting"], ["kept:idea-4411", "kept", "to-write"]]);
  assert.deepEqual(full.accepts.slice(-4), ["idea", "item-answer", "start-item", "item-edit"]);
  const plain = envelopes.cases.find((vector) => vector.name === "board-with-next-opened-by-the-account").expect.payload;
  assert.equal(Object.hasOwn(plain, "backlog"), false);
  const wrong = [
    (board) => (board.backlog[0].home.label = "/Users/someone/vault/Projects/refund-rounding.md"),
    (board) => (board.backlog[0].home.kind = "local"),
    (board) => (board.backlog[0].item = "/Users/someone/vault/Projects/refund-rounding.md"),
    (board) => (board.backlog[0].blocking = 4),
    (board) => (board.backlog[0].priority = "urgent"),
    (board) => (board.backlog[0].state = "started"),
    (board) => (board.backlog[0].path = "Projects/x.md"),
    (board) => (board.backlog[0].body.text = "the body in clear"),
    (board) => board.backlog.push(structuredClone(board.backlog[0])),
  ];
  for (const change of wrong) {
    const payload = structuredClone(full);
    change(payload);
    assert.throws(() => checkPayload(payload), (error) => error.code === "malformed", String(change));
  }
  const stray = envelopes.cases.find((vector) => vector.name === "backlog-row-with-an-absolute-home");
  assert.equal(stray.expect.error, "malformed");
});

test("a backlog item's body opens from its blob as sections and questions, and its keys hold as Rust's do", async () => {
  const backlog = load("backlog.json");
  assert.equal(backlog.media_type, BACKLOG_BODY_MEDIA_TYPE);
  assert.deepEqual([backlog.max_idea_chars, backlog.max_item_answer_chars], [MAX_IDEA_CHARS, MAX_ITEM_ANSWER_CHARS]);
  const body = JSON.parse(new TextDecoder().decode(await openBlob(backlog.blob, fromB64(backlog.ciphertext))));
  assert.deepEqual(body, backlog.body);
  checkBacklogBody(body);
  assert.deepEqual(body.questions.map((question) => [question.blocks, question.pending_answer]), [[true, null], [true, "No, only new ones."], [false, null]]);
  assert.deepEqual(body.sections.map((section) => [section.key, section.required]), [["background", true], ["goal", true], ["what-we-know", false], ["questions", false], ["out-of-scope", false], ["source", false]]);
  assert.equal(body.questions_key, "questions");
  for (const { name, body: candidate, valid } of backlog.bodies) {
    if (valid) checkBacklogBody(candidate);
    else assert.throws(() => checkBacklogBody(candidate), (error) => error.code === "malformed", name);
  }
  for (const { key, valid } of backlog.item_keys) assert.equal(isItemKey(key), valid, key);
});

test("ideas and answers on an item live an hour and a start from an item a day, as Rust's limits say", async () => {
  assert.deepEqual([envelopes.max_ttl.idea, envelopes.max_ttl["item-answer"], envelopes.max_ttl["start-item"]], [KINDS.idea.maxTtl, KINDS["item-answer"].maxTtl, KINDS["start-item"].maxTtl]);
  assert.deepEqual([KINDS.idea.ttl, KINDS["item-answer"].ttl, START_ITEM_TTL], [3600, 3600, 86_400]);
  const device = await certifiedPhone();
  const machine = { id: parties.machine.id, keys: parties.machine.public };
  assert.equal(envelopes.cases.find((candidate) => candidate.name === "start-item-anyway-opened-by-the-machine").expect.payload.anyway, true);
  for (const name of ["idea-opened-by-the-machine", "item-answer-opened-by-the-machine", "start-item-opened-by-the-machine", "start-item-anyway-opened-by-the-machine"]) {
    const vector = envelopes.cases.find((candidate) => candidate.name === name);
    checkPayload(vector.expect.payload);
    const envelope = await sealCommand({ device, account: devices.key, machine, payload: vector.expect.payload, at: envelopes.now });
    assert.equal(envelope.exp - envelope.at, KINDS[vector.expect.payload.type].ttl, name);
    assert.deepEqual((await openCommand(envelope, await identity("machine"), devices.key, envelopes.now)).payload, vector.expect.payload, name);
  }
  const idea = envelopes.cases.find((candidate) => candidate.name === "idea-opened-by-the-machine");
  assert.throws(() => checkShape({ ...idea.envelope, exp: idea.envelope.at + 3601 }), (error) => error.code === "malformed");
  const wrong = [
    { type: "idea", text: "x".repeat(MAX_IDEA_CHARS + 1), repo: null },
    { type: "idea", text: "  ", repo: null },
    { type: "item-answer", item: "vault:Projects/refund-rounding.md", index: 0, question: "Which?", answer: "" },
    { type: "item-answer", item: "vault:Projects/refund-rounding.md", index: -1, question: "Which?", answer: "This" },
    { type: "start-item", item: "local:../etc" },
    { type: "start-item", item: "vault:Projects/x.md", recipe: "small fix" },
    { type: "start-item", item: "vault:Projects/x.md", anyway: "yes" },
  ];
  for (const payload of wrong) assert.throws(() => checkPayload(payload), (error) => error.code === "malformed", JSON.stringify(payload));
});

test("an item edit replaces a section or the title from the hash of the text it edited, lives an hour, and passes or fails as Rust's validation says", async () => {
  const backlog = load("backlog.json");
  assert.deepEqual([backlog.max_item_title_chars, backlog.max_section_chars, backlog.max_item_edit_bytes, backlog.max_heading_chars], [MAX_ITEM_TITLE_CHARS, MAX_SECTION_CHARS, MAX_ITEM_EDIT_BYTES, MAX_HEADING_CHARS]);
  for (const { text, hash } of backlog.text_hashes) assert.equal(await textHash(text), hash, text);
  for (const { name, payload, valid } of backlog.edits) {
    if (valid) checkPayload(payload);
    else assert.throws(() => checkPayload(payload), (error) => error.code === "malformed", name);
  }
  assert.deepEqual([KINDS["item-edit"].command, KINDS["item-edit"].ttl, KINDS["item-edit"].maxTtl, envelopes.max_ttl["item-edit"]], [true, 3600, 3600, 3600]);
  const device = await certifiedPhone();
  const machine = { id: parties.machine.id, keys: parties.machine.public };
  for (const name of ["item-edit-opened-by-the-machine", "title-edit-opened-by-the-machine"]) {
    const vector = envelopes.cases.find((candidate) => candidate.name === name);
    const envelope = await sealCommand({ device, account: devices.key, machine, payload: vector.expect.payload, at: envelopes.now });
    assert.deepEqual((await openCommand(envelope, await identity("machine"), devices.key, envelopes.now)).payload, vector.expect.payload, name);
  }
  const refused = envelopes.cases.find((vector) => vector.name === "edit-refused-for-a-newer-section-opened-by-the-account").expect.payload;
  assert.equal(refused.reason, "section-changed");
  checkPayload(refused);
});

test("a start refused for open questions says how many wait, and no other result carries a count", () => {
  const refused = envelopes.cases.find((vector) => vector.name === "start-refused-for-open-questions-opened-by-the-account").expect.payload;
  assert.deepEqual([refused.reason, refused.blocking], ["questions-open", 2]);
  checkPayload(refused);
  checkPayload({ ...refused, reason: "already-started", blocking: undefined });
  for (const wrong of [{ ...refused, blocking: 0 }, { ...refused, blocking: undefined }, { ...refused, reason: "already-started" }]) {
    assert.throws(() => checkPayload(wrong), (error) => error.code === "malformed", JSON.stringify(wrong));
  }
  for (const name of ["result-counting-questions-for-another-reason", "open-questions-refusal-without-a-count"]) {
    assert.equal(envelopes.cases.find((vector) => vector.name === name).expect.error, "malformed", name);
  }
});

test("review commands hold Rust's limits and lifetimes, and each payload passes or fails as Rust's validation says", async () => {
  const review = load("review.json");
  assert.deepEqual([review.media_type, review.max_comments, review.max_comment_chars, review.max_comments_bytes], [REVIEW_MEDIA_TYPE, MAX_REVIEW_COMMENTS, MAX_REVIEW_COMMENT_CHARS, MAX_REVIEW_COMMENTS_BYTES]);
  for (const kind of ["review-open", "review-comments", "review-read", "accept-step"]) assert.deepEqual([KINDS[kind].command, KINDS[kind].ttl, KINDS[kind].maxTtl, envelopes.max_ttl[kind]], [true, 3600, 3600, 3600], kind);
  for (const { name, payload, valid } of review.payloads) assert.equal((await failure(Promise.resolve().then(() => checkPayload(payload)))) === null, valid, name);
  const device = await certifiedPhone();
  const machine = { id: parties.machine.id, keys: parties.machine.public };
  for (const name of ["review-open-opened-by-the-machine", "review-comments-opened-by-the-machine", "review-read-opened-by-the-machine", "accept-step-opened-by-the-machine"]) {
    const vector = envelopes.cases.find((candidate) => candidate.name === name);
    const envelope = await sealCommand({ device, account: devices.key, machine, payload: vector.expect.payload, at: envelopes.now });
    assert.deepEqual((await openCommand(envelope, await identity("machine"), devices.key, envelopes.now)).payload, vector.expect.payload, name);
  }
});

test("a review opens from the blob its result names, checked as Rust checks it, and only a result that is done names a blob", async () => {
  const review = load("review.json");
  const body = JSON.parse(new TextDecoder().decode(await openBlob(review.blob, fromB64(review.ciphertext))));
  assert.deepEqual(body, review.review);
  for (const { name, body: candidate, valid } of review.bodies) assert.equal((await failure(Promise.resolve().then(() => checkReview(candidate)))) === null, valid, name);
  const ready = envelopes.cases.find((vector) => vector.name === "review-result-naming-its-blob-opened-by-the-account");
  const opened = await openEnvelope(ready.envelope, await identity("account"), await sender("machine"), envelopes.now);
  assert.deepEqual(opened.blob, review.blob);
  assert.throws(() => checkPayload({ ...opened, outcome: "refused", reason: "not-allowed" }), (error) => error.code === "malformed");
  const offering = envelopes.cases.find((vector) => vector.name === "board-offering-a-review-opened-by-the-account").expect.payload;
  checkPayload(offering);
  assert.deepEqual(offering.items[0].review, { step: 2, files: 3 });
  assert.throws(() => checkPayload({ ...offering, items: [{ ...offering.items[0], review: { step: 2, files: 3, diff: "+x" } }] }), (error) => error.code === "malformed");
});
