import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import { LEAD_THREAD_MEDIA_TYPE, MAX_LEAD_MESSAGE_CHARS, MAX_LEAD_PAGE_TURNS, MAX_LEAD_TURN_CHARS, readLeadThread, CARD_MEDIA_TYPE, MAX_ASSESSMENT_REASON_CHARS, MAX_BUILT_LINES, MAX_CARD_LINE_CHARS, MAX_DECISION_CHARS, MAX_DECISIONS, MAX_NOTICED, readCard, CLOSE_OPTIONS, KEPT_ANSWER_TTL, MAX_KEBBIE_QUESTION_CHARS, MAX_KEBBIE_QUESTION_ID_CHARS, MAX_KEBBIE_QUESTIONS, MAX_MEANING_CHARS, MAX_STAGE_DETAIL_CHARS, MAX_STAGE_NOTE_CHARS, MAX_STAGE_TITLE_CHARS, MAX_STAGES, VERIFY_OPTIONS, MAX_SUBTASK_ID_CHARS, MAX_ADDED_REASON_CHARS, MAX_OBSTACLE_CHARS, MAX_MOVED_REASON_CHARS, pushableOf, MAX_FOUND_SOURCES, MAX_ITEM_TITLE_CHARS, MAX_SECTION_CHARS, MAX_ITEM_EDIT_BYTES, MAX_HEADING_CHARS, textHash, REVIEW_MEDIA_TYPE, MAX_REVIEW_COMMENTS, MAX_REVIEW_COMMENT_CHARS, MAX_REVIEW_COMMENTS_BYTES, MAX_RULE_NAMES, MAX_RULE_NAME_CHARS, checkReview, readReview, payloadOf, subtaskHead, subtaskOf, DEFAULT_NOTICE_CATEGORIES, MAX_NOTICE_BODY_CHARS, MAX_NOTICE_TITLE_CHARS, NOTICE_CATEGORIES, NOTICE_COLLAPSE_PREFIXES, noticeCategoryOf, fitsCollapse, openNotice, BACKLOG_BODY_MEDIA_TYPE, checkBacklogBody, isItemKey, KINDS, MAX_IDEA_CHARS, MAX_ITEM_ANSWER_CHARS, START_ITEM_TTL, commandId, COMMAND_TTL, PROMPT_ANSWER_TTL, sealCommand, deviceCertStatement, deviceFingerprint, deviceId, newDeviceKey, openCommand, signDeviceCert, verifyCommand, verifyDeviceCert, accountCommitment, accountFingerprints, accountIdentity, accountWrapKey, checkPayload, checkShape, machineListStatement, machineListTrusts, machineListWith, machineListWithout, emptyMachineList, newAccountSecrets, signMachineList, signStatement, unwrapAccountSecrets, verifyMachineList, wrapAccountSecrets, wrapAssociatedData, wrapperStatement, decryptEnvelope, fromB64, headerBytes, importIdentity, importSender, machineFingerprint, openBlob, openEnvelope, handoffNonce, parseConnectFragment, ProtocolError, sealEnvelope, toB64 } from "./protocol.js";

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

test("a board item's subtasks, push order and pushable run carry only their own fields and hold together as Rust's validation says", () => {
  const planned = envelopes.cases.find((vector) => vector.name === "board-with-subtasks-opened-by-the-account").expect.payload;
  checkPayload(planned);
  const plain = envelopes.cases.find((vector) => vector.name === "board-opened-by-a-device").expect.payload;
  assert.equal(Object.hasOwn(plain.items[0], "subtasks"), false);
  checkPayload(plain);
  const item = planned.items[0];
  assert.deepEqual(item.subtasks.map((one) => [one.id, one.state, one.commits.length]), [["docs", "pushed", 1], ["currency", "accepted", 1], ["gateway", "ready", 2], ["tests", "accepted", 1], ["new-gateway", "in-progress", 0], ["remove-old", "planned", 0]]);
  assert.deepEqual(item.order.map((row) => row.subtask), ["currency", "gateway", "tests", "gateway"], "a subtask's commits may sit apart in the push order");
  assert.equal(subtaskHead(subtaskOf(item, "gateway")), item.head, "a subtask's head is its last commit");
  assert.deepEqual(item.pushable, { count: 2, through: item.order[1].sha, subtasks: ["currency", "gateway"] }, "the run passes gateway's accepted commit and stops before the group it would split");
  assert.deepEqual(pushableOf(item.order), item.pushable, "the run is what the rule computes from the push order");
  assert.deepEqual(item.order.map((row) => [row.accepted, row.group ?? null, row.tests.state]), [[true, null, "passed"], [true, null, "passed"], [true, "ci-gateway", "running"], [false, "ci-gateway", "not-run"]]);
  assert.equal(item.order[0].moved.from, 2);
  assert.equal(subtaskOf(item, "currency").added.reason, "Amounts carry no currency the gateway can read");
  const plans = load("review.json").plans;
  assert.deepEqual([plans.max_subtask_id_chars, plans.max_added_reason_chars, plans.max_obstacle_chars, plans.max_moved_reason_chars], [MAX_SUBTASK_ID_CHARS, MAX_ADDED_REASON_CHARS, MAX_OBSTACLE_CHARS, MAX_MOVED_REASON_CHARS]);
  assert.ok(plans.cases.length > 20);
  for (const { name, item: candidate, valid } of plans.cases) {
    const passes = (() => {
      try {
        checkPayload({ ...planned, items: [candidate] });
        return true;
      } catch (error) {
        if (error.code !== "malformed") throw error;
        return false;
      }
    })();
    assert.equal(passes, valid, name);
  }
});

test("a task in stages pushes nothing past a stage whose stages before it are not met, as Rust's validation and run say", () => {
  const stages = load("stages.json");
  assert.deepEqual(
    [stages.max_stages, stages.max_stage_title_chars, stages.max_stage_detail_chars, stages.max_stage_note_chars, stages.max_kebbie_questions, stages.max_kebbie_question_id_chars, stages.max_kebbie_question_chars, stages.max_meaning_chars, stages.verify_options, stages.close_options],
    [MAX_STAGES, MAX_STAGE_TITLE_CHARS, MAX_STAGE_DETAIL_CHARS, MAX_STAGE_NOTE_CHARS, MAX_KEBBIE_QUESTIONS, MAX_KEBBIE_QUESTION_ID_CHARS, MAX_KEBBIE_QUESTION_CHARS, MAX_MEANING_CHARS, VERIFY_OPTIONS, CLOSE_OPTIONS],
  );
  const staged = envelopes.cases.find((vector) => vector.name === "board-with-stages-and-a-verification-opened-by-the-account").expect.payload;
  assert.deepEqual(staged, stages.board);
  checkPayload(staged);
  const item = staged.items[0];
  assert.deepEqual(item.stages.map((one) => [one.state, one.requirement.kind]), [["met", "deploy"], ["waiting", "verified"], ["locked", "deploy"]]);
  assert.equal(pushableOf(item.order, item.stages), null, "both commits are accepted, and stage 3 waits on stage 2");
  assert.notEqual(pushableOf(item.order), null, "without its stages the same order would go out");
  assert.deepEqual(item.kebbie_questions.map((one) => [one.kind, one.stage, one.options.map((option) => option.label)]), [["verify", "backfill", VERIFY_OPTIONS]]);
  assert.ok(stages.cases.length > 25);
  for (const { name, item: candidate, valid } of stages.cases) {
    const passes = (() => {
      try {
        checkPayload({ ...staged, items: [candidate] });
        return true;
      } catch (error) {
        if (error.code !== "malformed") throw error;
        return false;
      }
    })();
    assert.equal(passes, valid, name);
    if (valid) assert.deepEqual(pushableOf(candidate.order ?? [], candidate.stages), candidate.pushable ?? null, `${name}: the run is what the rule computes`);
  }
});

test("an answer to kebbie's question is the task's answer command, kept a week for a machine that sleeps", () => {
  const stages = load("stages.json");
  assert.deepEqual([stages.kept_answer_ttl, envelopes.max_ttl.answer, KINDS.answer.maxTtl, KINDS.answer.ttl], [KEPT_ANSWER_TTL, KEPT_ANSWER_TTL, KEPT_ANSWER_TTL, COMMAND_TTL]);
  for (const { name, payload } of stages.answers) checkPayload(payload, name);
  const kept = load("authorize.json").cases.find((vector) => vector.name === "answer-to-kebbies-question-collected-six-days-later-is-applied");
  assert.equal(kept.envelope.exp - kept.envelope.at, KEPT_ANSWER_TTL);
  checkShape(kept.envelope);
  assert.throws(() => checkShape({ ...kept.envelope, exp: kept.envelope.at + KEPT_ANSWER_TTL + 1 }), (error) => error.code === "malformed");
});

test("steps from a machine not yet updated open as Rust opens them: subtasks named by their numbers, in order, with the run of accepted steps pushable", async () => {
  const [one, many] = load("review.json").steps_from_a_machine_not_yet_updated;
  for (const { board, opens_as } of [one, many]) {
    checkPayload(board);
    assert.deepEqual(payloadOf(board), opens_as);
    assert.deepEqual(payloadOf(opens_as), opens_as, "a board already in the new shape opens as it is");
  }
  assert.deepEqual(one.opens_as.items[0].subtasks[0], { id: "2", title: "Charge through a PaymentGateway", state: "ready", commits: [one.board.items[0].head], rules: { state: "ok" } });
  const item = many.opens_as.items[0];
  assert.deepEqual(item.order.map((row) => row.subtask), ["2", "3", "3"]);
  assert.deepEqual([item.pushable.count, item.pushable.subtasks, item.require_accept], [1, ["2"], true]);
  const both = structuredClone(many.board);
  both.items[0].subtasks = [];
  assert.throws(() => checkPayload(both), (error) => error.code === "malformed", "subtasks or steps, never both");
  const planned = envelopes.cases.find((vector) => vector.name === "board-with-subtasks-opened-by-the-account");
  const opened = await openEnvelope(planned.envelope, await identity("account"), await sender("machine"), envelopes.now);
  assert.deepEqual(opened, planned.expect.payload);
});

test("a push order from a machine not yet updated opens as Rust opens it: each commit accepted with its subtask", () => {
  const { board, opens_as } = load("review.json").subtasks_from_a_machine_not_yet_updated;
  checkPayload(board);
  assert.deepEqual(payloadOf(board), opens_as);
  assert.deepEqual(opens_as.items[0].order.map((row) => row.accepted), [true, false, true, false]);
  assert.deepEqual(pushableOf(opens_as.items[0].order), opens_as.items[0].pushable);
});

test("the pushable run is the longest one from the top where every commit is accepted and no group is split", () => {
  const row = (sha, accepted, group) => ({ sha: sha.repeat(40), subject: sha, subtask: sha, accepted, ...(group ? { group } : {}) });
  assert.equal(pushableOf([]), null);
  assert.equal(pushableOf([row("a", false)]), null);
  assert.deepEqual(pushableOf([row("a", true), row("b", true), row("c", false)]), { count: 2, through: "b".repeat(40), subtasks: ["a", "b"] });
  assert.equal(pushableOf([row("a", true, "g"), row("b", false, "g")]), null, "a group goes out whole or not at all");
  assert.equal(pushableOf([row("a", true), row("b", true, "g"), row("c", true, "g"), row("d", false)]).count, 3, "an accepted group goes out whole");
  assert.equal(pushableOf([row("a", true), row("b", true, "g"), row("c", true, "g"), row("d", false, "g")]).count, 1);
});

test("a task says where its obstacle stands, when it was raised and, once decided or asked, the ruling", () => {
  const next = envelopes.cases.find((vector) => vector.name === "board-with-next-opened-by-the-account").expect.payload;
  const held = next.items.find((one) => one.obstacle);
  assert.deepEqual([held.obstacle.id, held.obstacle.state], ["o1", "investigating"]);
  checkPayload(next);
  assert.equal(held.obstacle.at, envelopes.now - 400 - 1500);
  for (const [key, value] of [["state", "decided"], ["state", "raised"], ["text", ""], ["id", "a b"], ["text", "x".repeat(MAX_OBSTACLE_CHARS + 1)], ["by", "task"], ["at", "14:41"], ["decision", { kind: "ask", by: "investigator", text: "x" }]]) {
    const board = structuredClone(next);
    board.items.find((one) => one.obstacle).obstacle[key] = value;
    assert.throws(() => checkPayload(board), (error) => error.code === "malformed", `${key}: ${value}`);
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
    (board) => (board.items[1].require_accept = "no"),
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
    [{ exp: good.at + KEPT_ANSWER_TTL + 1 }, "malformed"],
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
  const misnamed = await sealEnvelope({ ...header, collapse: "r-refunds" }, new TextEncoder().encode(JSON.stringify(noticeFor())), machine, [{ id: device.id, encryption: key.encryptionPublic }]);
  assert.equal(await failure(openNotice(misnamed, device, listed, at)), "category-mismatch", "a tag that names another category than the notice inside does not open");
});

test("a collapse tag names its notice's category as the vectors say", () => {
  const vectors = load("notices.json");
  assert.deepEqual(Object.fromEntries(vectors.prefixes.map((row) => [row.prefix, row.category])), NOTICE_COLLAPSE_PREFIXES);
  for (const row of vectors.tags) assert.equal(noticeCategoryOf(row.tag), row.category, row.tag);
  for (const row of vectors.fits) assert.equal(fitsCollapse({ category: row.category }, row.tag), row.fits, row.tag);
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
  assert.deepEqual(NOTICE_CATEGORIES, ["questions", "new-device", "idea-questions", "ready", "ci-failed", "started", "lead-answered"]);
  assert.deepEqual(DEFAULT_NOTICE_CATEGORIES, ["questions", "new-device", "idea-questions", "ready", "ci-failed", "lead-answered"]);
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

test("an answer to kebbie's question is sealed to live a week, and no command lives past what its kind allows", async () => {
  const machine = { id: parties.machine.id, keys: parties.machine.public };
  const device = await certifiedPhone();
  const verified = load("stages.json").answers[0].payload;
  const kept = await sealCommand({ device, account: devices.key, machine, payload: verified, at: envelopes.now, ttl: KEPT_ANSWER_TTL });
  assert.equal(kept.exp - kept.at, KEPT_ANSWER_TTL);
  assert.deepEqual((await openCommand(kept, await identity("machine"), devices.key, envelopes.now + 6 * 86_400)).payload, verified);
  const plain = await sealCommand({ device, account: devices.key, machine, payload: verified, at: envelopes.now });
  assert.equal(plain.exp - plain.at, COMMAND_TTL, "an answer to the task's own question keeps its hour");
  const push = { type: "push", address: "refunds@shop", head: "a".repeat(40), card_sha: null };
  await assert.rejects(sealCommand({ device, account: devices.key, machine, payload: push, at: envelopes.now, ttl: KEPT_ANSWER_TTL }), (error) => error.code === "ttl");
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
  assert.deepEqual(full.backlog.map((row) => [row.item, row.home.kind, row.state]), [["vault:Projects/refund-rounding.md", "vault", "ready"], ["local:01JB7Q2M3N8K", "local", "drafting"], ["kept:idea-4411", "kept", "to-write"], ["found:f3", "found", "ready"]]);
  assert.deepEqual(full.backlog[3].found, { sources: ["payments@api", "refunds@shop"], count: 3 });
  assert.deepEqual(full.accepts.slice(-7), ["idea", "item-answer", "start-item", "item-edit", "found-start", "found-add", "found-dismiss"]);
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

test("what tasks found is started, added to a task or dismissed by its key, and its row says who found it, as Rust's validation says", async () => {
  const backlog = load("backlog.json");
  assert.equal(backlog.max_found_sources, MAX_FOUND_SOURCES);
  const passes = (payload) => {
    try {
      checkPayload(payload);
      return true;
    } catch (error) {
      if (error.code !== "malformed") throw error;
      return false;
    }
  };
  for (const { name, payload, valid } of backlog.found) assert.equal(passes(payload), valid, name);
  const board = envelopes.cases.find((vector) => vector.name === "board-with-backlog-opened-by-the-account").expect.payload;
  for (const { name, row, valid } of backlog.found_rows) assert.equal(passes({ ...board, backlog: [row] }), valid, name);
  assert.deepEqual([KINDS["found-start"].ttl, KINDS["found-start"].maxTtl, KINDS["found-add"].maxTtl, KINDS["found-dismiss"].maxTtl], [START_ITEM_TTL, envelopes.max_ttl["found-start"], envelopes.max_ttl["found-add"], envelopes.max_ttl["found-dismiss"]]);
  const device = await certifiedPhone();
  const machine = { id: parties.machine.id, keys: parties.machine.public };
  for (const name of ["found-start-opened-by-the-machine", "found-add-opened-by-the-machine", "found-dismiss-opened-by-the-machine"]) {
    const vector = envelopes.cases.find((candidate) => candidate.name === name);
    const envelope = await sealCommand({ device, account: devices.key, machine, payload: vector.expect.payload, at: envelopes.now });
    assert.equal(envelope.exp - envelope.at, KINDS[vector.expect.payload.type].ttl, name);
    assert.deepEqual((await openCommand(envelope, await identity("machine"), devices.key, envelopes.now)).payload, vector.expect.payload, name);
  }
});

test("a sign-off card opens from its blob as data, and its commands and assessments hold as Rust's validation says", async () => {
  const cards = load("cards.json");
  assert.equal(cards.media_type, CARD_MEDIA_TYPE);
  assert.deepEqual(
    [cards.max_built_lines, cards.max_card_line_chars, cards.max_decisions, cards.max_decision_chars, cards.max_noticed, cards.max_assessment_reason_chars],
    [MAX_BUILT_LINES, MAX_CARD_LINE_CHARS, MAX_DECISIONS, MAX_DECISION_CHARS, MAX_NOTICED, MAX_ASSESSMENT_REASON_CHARS],
  );
  const opened = readCard(JSON.parse(new TextDecoder().decode(await openBlob(cards.blob, fromB64(cards.ciphertext)))));
  assert.deepEqual(opened, cards.card);
  assert.deepEqual(opened.decisions.map((decision) => decision.by), ["lead", "you", "investigator", "you", "task"]);
  assert.equal(opened.decisions[0].replaced_by, 3);
  assert.equal(opened.previous.version, 1);
  const reads = (candidate) => {
    try {
      readCard(candidate);
      return true;
    } catch (error) {
      if (error.code !== "malformed") throw error;
      return false;
    }
  };
  assert.ok(cards.cards.length > 25);
  for (const { name, card, valid } of cards.cards) assert.equal(reads(card), valid, name);
  const passes = (payload) => {
    try {
      checkPayload(payload);
      return true;
    } catch (error) {
      if (error.code !== "malformed") throw error;
      return false;
    }
  };
  const board = envelopes.cases.find((vector) => vector.name === "board-with-a-sign-off-card-and-an-assessment-opened-by-the-account").expect.payload;
  assert.deepEqual(board, cards.board);
  for (const { name, item, valid } of cards.assessments) assert.equal(passes({ ...board, items: [item] }), valid, name);
  for (const { name, payload, valid } of cards.payloads) assert.equal(passes(payload), valid, name);
  assert.ok(passes(cards.board_from_a_machine_not_yet_updated), "a markdown card and a one-line assessment still open");
  assert.deepEqual([KINDS["found-keep"].maxTtl, KINDS["review-send"].maxTtl], [envelopes.max_ttl["found-keep"], envelopes.max_ttl["review-send"]]);
  const device = await certifiedPhone();
  const machine = { id: parties.machine.id, keys: parties.machine.public };
  for (const name of ["found-keep-opened-by-the-machine", "review-send-opened-by-the-machine"]) {
    const vector = envelopes.cases.find((candidate) => candidate.name === name);
    const envelope = await sealCommand({ device, account: devices.key, machine, payload: vector.expect.payload, at: envelopes.now });
    assert.deepEqual((await openCommand(envelope, await identity("machine"), devices.key, envelopes.now)).payload, vector.expect.payload, name);
  }
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
  for (const kind of ["review-open", "review-comments", "review-read", "accept-step", "accept-subtask"]) assert.deepEqual([KINDS[kind].command, KINDS[kind].ttl, KINDS[kind].maxTtl, envelopes.max_ttl[kind]], [true, 3600, 3600, 3600], kind);
  for (const { name, payload, valid } of review.payloads) assert.equal((await failure(Promise.resolve().then(() => checkPayload(payload)))) === null, valid, name);
  const device = await certifiedPhone();
  const machine = { id: parties.machine.id, keys: parties.machine.public };
  for (const name of ["review-open-opened-by-the-machine", "review-comments-opened-by-the-machine", "review-read-opened-by-the-machine", "accept-step-opened-by-the-machine", "accept-subtask-opened-by-the-machine"]) {
    const vector = envelopes.cases.find((candidate) => candidate.name === name);
    const envelope = await sealCommand({ device, account: devices.key, machine, payload: vector.expect.payload, at: envelopes.now });
    assert.deepEqual((await openCommand(envelope, await identity("machine"), devices.key, envelopes.now)).payload, vector.expect.payload, name);
  }
});

test("a task and each subtask carry a rules check that passes or fails as Rust's validation says, and an approved flag from an older machine is ignored", () => {
  const review = load("review.json");
  assert.deepEqual([review.max_rule_names, review.max_rule_name_chars], [MAX_RULE_NAMES, MAX_RULE_NAME_CHARS]);
  const stepped = envelopes.cases.find((vector) => vector.name === "board-with-subtasks-opened-by-the-account").expect.payload;
  checkPayload(stepped);
  assert.deepEqual([stepped.items[0].git.rules, stepped.items[0].subtasks[2].rules], [{ state: "checking" }, { state: "ok" }]);
  const withRules = (rules, onStep) => {
    const board = structuredClone(stepped);
    if (onStep) board.items[0].subtasks[2].rules = rules;
    else board.items[0].git.rules = rules;
    return board;
  };
  const passes = (board) => {
    try {
      checkPayload(board);
      return true;
    } catch (error) {
      if (error.code !== "malformed") throw error;
      return false;
    }
  };
  assert.ok(review.rules.length > 0);
  for (const { name, rules, valid } of review.rules) for (const onStep of [false, true]) assert.equal(passes(withRules(rules, onStep)), valid, `${name}${onStep ? " on a subtask" : " on the task"}`);
  const legacy = review.board_from_a_machine_not_yet_updated;
  assert.equal(legacy.items[0].git.approved, true);
  assert.equal(legacy.items[0].git.rules, undefined);
  assert.equal(passes(legacy), true, "a board from a machine not yet updated still opens");
  assert.equal(passes({ ...legacy, items: [{ ...legacy.items[0], git: { ...legacy.items[0].git, approved: "yes" } }] }), false);
});

test("a push names the last commit of the run it pushes, and a push through a step from an app not yet updated still passes", () => {
  const review = load("review.json");
  const run = review.payloads.find((vector) => vector.name === "a push of the pushable run").payload;
  assert.equal(Object.hasOwn(run, "through_step"), false);
  checkPayload(run);
  const older = review.payloads.find((vector) => vector.name === "a push through a step from an app not yet updated").payload;
  checkPayload(older);
  assert.throws(() => checkPayload({ ...older, through_step: 0 }), (error) => error.code === "malformed");
  assert.throws(() => checkPayload({ ...older, through_step: "2" }), (error) => error.code === "malformed");
  const applied = load("authorize.json").cases.find((vector) => vector.name === "device-pushes-a-tasks-pushable-run");
  assert.ok(applied.accepts.includes("push"));
  assert.deepEqual(applied.expect.payload, run);
});

test("a review opens from the blob its result names, checked as Rust checks it, and only a result that is done names a blob", async () => {
  const review = load("review.json");
  const body = JSON.parse(new TextDecoder().decode(await openBlob(review.blob, fromB64(review.ciphertext))));
  assert.deepEqual(body, review.review);
  for (const { name, body: candidate, valid } of review.bodies) assert.equal((await failure(Promise.resolve().then(() => checkReview(candidate)))) === null, valid, name);
  assert.deepEqual(readReview(review.review), review.review, "a review in the new shape opens as it is");
  for (const older of review.reviews_from_a_machine_not_yet_updated) assert.deepEqual(readReview(older.body), older.opens_as, "an older machine's review of a step opens as a subtask, as Rust opens it");
  assert.deepEqual(review.reviews_from_a_machine_not_yet_updated.map((older) => [older.opens_as.subtask.id, older.opens_as.subtask.commits.length]), [["2", 1], ["2", 2]]);
  const [first, last] = review.review.subtask.commits;
  assert.deepEqual([first.files.map((file) => [file.path, file.read]), last.files.map((file) => [file.path, file.read])], [[["src/gateway.rs", true]], [["src/gateway.rs", false], ["src/orders.rs", true], ["assets/logo.png", false]]]);
  assert.notEqual(first.files[0].content, review.review.subtask.files[0].content, "a file read in one commit and changed again in the next names new content on the subtask");
  const ready = envelopes.cases.find((vector) => vector.name === "review-result-naming-its-blob-opened-by-the-account");
  const opened = await openEnvelope(ready.envelope, await identity("account"), await sender("machine"), envelopes.now);
  assert.deepEqual(opened.blob, review.blob);
  assert.throws(() => checkPayload({ ...opened, outcome: "refused", reason: "not-allowed" }), (error) => error.code === "malformed");
  const offering = envelopes.cases.find((vector) => vector.name === "board-offering-a-review-opened-by-the-account").expect.payload;
  checkPayload(offering);
  assert.deepEqual(offering.items[0].review, { subtask: "gateway", files: 3 });
  assert.throws(() => checkPayload({ ...offering, items: [{ ...offering.items[0], review: { subtask: "gateway", files: 3, diff: "+x" } }] }), (error) => error.code === "malformed");
  assert.throws(() => checkPayload({ ...offering, items: [{ ...offering.items[0], review: { subtask: "gateway", step: 2, files: 3 } }] }), (error) => error.code === "malformed");
  const older = { ...offering, items: [{ ...offering.items[0], review: { step: 2, files: 3 } }] };
  checkPayload(older);
  assert.deepEqual(payloadOf(older).items[0].review, { subtask: "2", files: 3 }, "an older machine's offer names the step's number as its subtask");
});

test("the lead's thread opens page by page from its blobs, and its commands hold as Rust's validation says", async () => {
  const lead = load("lead.json");
  assert.deepEqual([lead.media_type, lead.max_page_turns, lead.max_turn_chars, lead.max_message_chars], [LEAD_THREAD_MEDIA_TYPE, MAX_LEAD_PAGE_TURNS, MAX_LEAD_TURN_CHARS, MAX_LEAD_MESSAGE_CHARS]);
  const page = async (blob, ciphertext) => readLeadThread(JSON.parse(new TextDecoder().decode(await openBlob(blob, fromB64(ciphertext)))));
  assert.deepEqual(lead.board.lead.thread, lead.blob);
  const newest = await page(lead.board.lead.thread, lead.ciphertext);
  assert.deepEqual(newest, lead.page);
  assert.deepEqual(newest.turns.map((turn) => [turn.by, turn.device ? "phone" : "computer", turn.delivery?.state ?? null]), [
    ["you", "computer", null],
    ["lead", "computer", null],
    ["you", "phone", "delivered"],
    ["lead", "computer", null],
    ["you", "phone", "waiting-turn"],
    ["you", "phone", null],
    ["lead", "computer", null],
  ]);
  assert.deepEqual(newest.older, lead.older_blob);
  const older = await page(newest.older, lead.older_ciphertext);
  assert.deepEqual(older, lead.older_page);
  assert.equal(older.older, null);
  const holds = (check) => (value) => {
    try {
      check(value);
      return true;
    } catch (error) {
      if (error.code !== "malformed") throw error;
      return false;
    }
  };
  assert.ok(lead.pages.length > 25);
  for (const { name, page: candidate, valid } of lead.pages) assert.equal(holds(readLeadThread)(candidate), valid, name);
  for (const { name, lead: candidate, valid } of lead.leads) assert.equal(holds(checkPayload)({ ...lead.board, lead: candidate }), valid, name);
  for (const { name, payload, valid } of lead.payloads) assert.equal(holds(checkPayload)(payload), valid, name);
  for (const kind of ["lead-message", "away"]) assert.deepEqual([KINDS[kind].ttl, KINDS[kind].maxTtl, KINDS[kind].command], [...lead.ttl[kind], true]);
  assert.equal(noticeCategoryOf(lead.collapse), "lead-answered");
  assert.ok(holds(checkPayload)(lead.board_from_a_machine_not_yet_updated), "a board without a lead still opens");
});
