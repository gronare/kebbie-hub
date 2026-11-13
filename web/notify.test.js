import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import { accountIdentity, deviceId, emptyMachineList, importIdentity, machineListWith, newAccountSecrets, newDeviceKey, sealEnvelope, signMachineList } from "../js/protocol.js";
import { CATEGORY_GROUPS, closeSettled, deviceWord, failureLine, GENERIC, isSettled, keptCategories, LISTING, NOTICE_FAILURE, NOTIFY_RECORD, notifyPhase, notifyText, pushedNotice, routeOf, showPushed, switchable, withCategory } from "./notify.js";

const { parties } = JSON.parse(readFileSync(new URL("../vectors/keys.json", import.meta.url), "utf8"));
const NOW = 1_761_070_000;

test("the switch row says where notifications stand, in the words of the mock", () => {
  const ios = { ios: true, word: "iPhone" };
  assert.equal(notifyText("off", ios), "Off");
  assert.equal(notifyText("asking", ios), "Asking iOS for permission…");
  assert.equal(notifyText("on", { ...ios, machines: ["MacBook", "work-mac"] }), "On for this iPhone. MacBook and work-mac send here.");
  assert.equal(notifyText("on", { ...ios, machines: ["MacBook"] }), "On for this iPhone. MacBook sends here.");
  assert.equal(notifyText("on", { ...ios, machines: ["a", "b", "c"] }), "On for this iPhone. a, b and c send here.");
  assert.equal(notifyText("on", ios), "On for this iPhone.");
  assert.equal(notifyText("denied", ios), "Turned off for kebbie in iOS. Turn them on in Settings › Notifications › kebbie.");
  assert.equal(notifyText("install", ios), "Add kebbie to your Home Screen to get notifications. Share, then Add to Home Screen.");
  assert.equal(notifyText("asking", { word: "browser" }), "Asking for permission…");
  assert.equal(deviceWord("Mozilla/5.0 (iPhone; CPU iPhone OS 18_2 like Mac OS X)"), "iPhone");
  assert.equal(deviceWord("Mozilla/5.0 (Macintosh; Intel Mac OS X 14_0) Chrome/130"), "browser");
});

test("Safari in a tab asks for the Home Screen, a refusal in iOS greys the switch, and only a granted subscription is on", () => {
  assert.equal(notifyPhase({ ios: true, standalone: false, supported: false }), "install");
  assert.equal(notifyPhase({ ios: true, standalone: true, permission: "denied", subscribed: true }), "denied");
  assert.equal(notifyPhase({ ios: true, standalone: true, permission: "granted", subscribed: true }), "on");
  assert.equal(notifyPhase({ ios: true, standalone: true, permission: "default", subscribed: true }), "off");
  assert.equal(notifyPhase({ ios: true, standalone: true, permission: "default", asking: true }), "asking");
  assert.equal(notifyPhase({ supported: false }), "unsupported");
  assert.deepEqual(["off", "asking", "on", "denied", "install", "unsupported"].map(switchable), [true, true, true, false, false, false]);
});

test("categories keep the cards' order, start with everything but a start going through, and ignore a corrupt kept list", () => {
  assert.deepEqual(CATEGORY_GROUPS.flatMap((group) => group.rows.map((row) => row.category)), ["questions", "new-device", "idea-questions", "ready", "ci-failed", "started"]);
  assert.deepEqual(keptCategories(null), ["questions", "new-device", "idea-questions", "ready", "ci-failed"]);
  assert.deepEqual(keptCategories(["ready", "questions"]), ["questions", "ready"]);
  assert.deepEqual(keptCategories(["ready", "summary"]), ["questions", "new-device", "idea-questions", "ready", "ci-failed"]);
  assert.deepEqual(withCategory(["questions", "ready"], "started", true), ["questions", "ready", "started"]);
  assert.deepEqual(withCategory(["questions", "ready"], "questions", false), ["ready"]);
});

test("a tap opens the task, the item at its questions or the device's page", () => {
  assert.equal(routeOf("mch_1", { to: "task", address: "refunds@shop" }), "/#/task/mch_1/refunds%40shop");
  assert.equal(routeOf("mch_1", { to: "item", item: "local:retry.md" }), `/#/item/${encodeURIComponent("mch_1|local:retry.md")}`);
  assert.equal(routeOf("mch_1", { to: "item", item: "vault:Projects/retry" }), `/#/item/${encodeURIComponent("vault:Projects/retry")}`);
  assert.equal(routeOf("mch_1", { to: "device", device: "dev_1" }), "/account#device/dev_1");
  assert.equal(routeOf("mch_1", undefined), "/#/board");
});

const board = {
  items: [
    { address: "asks@shop", group: "NEEDS YOU", question: { id: "q" }, prompt: null },
    { address: "answered@shop", group: "WORKING", question: null, prompt: null },
    { address: "prompt@shop", group: "NEEDS YOU", question: null, prompt: { id: "p" } },
    { address: "ready@shop", group: "READY TO PUSH" },
    { address: "red@shop", group: "CI RUNNING", ci: { state: "failed" } },
  ],
  devices: { dev_wait: { state: "waiting" }, dev_done: { state: "approved" } },
  backlog: [{ item: "local:open.md", questions: 2 }, { item: "local:closed.md", questions: 0 }],
};
const opened = [{ machine: "mch_1", board }, { machine: "mch_asleep", board: null }];

test("a notification is settled when the board no longer holds what it was about, and kept while its machine can't tell", () => {
  const on = (category, open, machine = "mch_1") => isSettled({ machine, category, open }, opened);
  assert.equal(on("questions", { to: "task", address: "asks@shop" }), false);
  assert.equal(on("questions", { to: "task", address: "prompt@shop" }), false);
  assert.equal(on("questions", { to: "task", address: "answered@shop" }), true);
  assert.equal(on("questions", { to: "task", address: "gone@shop" }), true);
  assert.equal(on("ready", { to: "task", address: "ready@shop" }), false);
  assert.equal(on("ready", { to: "task", address: "answered@shop" }), true);
  assert.equal(on("ci-failed", { to: "task", address: "red@shop" }), false);
  assert.equal(on("ci-failed", { to: "task", address: "ready@shop" }), true);
  assert.equal(on("started", { to: "task", address: "answered@shop" }), false);
  assert.equal(on("new-device", { to: "device", device: "dev_wait" }), false);
  assert.equal(on("new-device", { to: "device", device: "dev_done" }), true);
  assert.equal(on("idea-questions", { to: "item", item: "local:open.md" }), false);
  assert.equal(on("idea-questions", { to: "item", item: "local:closed.md" }), true);
  assert.equal(on("questions", { to: "task", address: "answered@shop" }, "mch_asleep"), false);
  assert.equal(isSettled(null, opened), false);
});

test("opening the app closes only the settled notifications", async () => {
  const closed = [];
  const shown = (tag, data) => ({ tag, data, close: () => closed.push(tag) });
  const registration = { getNotifications: async () => [shown("a", { machine: "mch_1", category: "questions", open: { to: "task", address: "answered@shop" } }), shown("b", { machine: "mch_1", category: "questions", open: { to: "task", address: "asks@shop" } }), shown("c", { url: "/#/board" })] };
  assert.equal(await closeSettled(registration, opened), 1);
  assert.deepEqual(closed, ["a"]);
});

async function world() {
  const fresh = await newAccountSecrets("acc_0123456789abcdefghijkl");
  const identity = await accountIdentity(fresh.key, fresh.secrets, fresh.key.account);
  const machine = await importIdentity(parties.machine);
  const list = machineListWith(emptyMachineList(), machine.id, parties.machine.public);
  const signed = { list, signature: await signMachineList(identity, list) };
  const key = await newDeviceKey();
  const device = { id: deviceId(), encryption: key.encryption, encryptionPublic: key.encryptionPublic };
  const kv = new Map([
    [NOTIFY_RECORD, { account: fresh.key, device: device.id }],
    [`device-key:${fresh.key.account}`, { id: device.id, encryption: device.encryption }],
    [LISTING, { listing: { list: signed, boards: [{ machine: machine.id, name: "MacBook" }] }, at: NOW }],
  ]);
  const seal = async (payload, recipient = device, collapse = "q-refunds") => {
    const header = { v: 1, id: "ntc_01K85Y2D8G0000000000000000", kind: "notice", from: machine.id, machine: machine.id, at: NOW, exp: NOW + 3600, schema: 1, urgency: "high", collapse };
    return sealEnvelope(header, new TextEncoder().encode(JSON.stringify(payload)), machine, [{ id: recipient.id, encryption: recipient.encryptionPublic }]);
  };
  return { kv, get: async (name) => kv.get(name), put: async (name, value) => void kv.set(name, value), del: async (name) => void kv.delete(name), seal, machine, device, identity, fresh, signed };
}

const notice = { type: "notice", category: "questions", title: "refunds · shop", body: "Keep the old endpoint for a release?", open: { to: "task", address: "refunds@shop" }, retract: ["q-old"] };

test("a pushed notice opens with the device's key after the account-signed list vouches for its machine", async () => {
  const { get, seal, machine } = await world();
  const shown = await pushedNotice(await seal(notice), { get, now: NOW });
  assert.deepEqual(shown, { title: "refunds · shop", body: "Keep the old endpoint for a release?", tag: "q-refunds", data: { machine: machine.id, name: "MacBook", category: "questions", open: notice.open, url: `/#/task/${machine.id}/refunds%40shop` }, retract: ["q-old"], failure: null });
});

test("a notice that can't be opened still names its machine and its kind, and says why", async () => {
  const { kv, get, seal, device } = await world();
  const why = async (envelope, options = {}) => {
    const shown = await pushedNotice(envelope, { get, now: NOW, ...options });
    return { title: shown.title, body: shown.body, tag: shown.tag, url: shown.data.url, reason: shown.failure?.reason, detail: shown.failure?.detail ?? null };
  };
  const plain = (reason, extra = {}) => ({ title: "MacBook", body: "A question or a prompt waits for you.", tag: "q-refunds", url: "/#/board", reason, detail: null, ...extra });
  const envelope = await seal(notice);
  const other = await newDeviceKey();
  const elsewhere = await pushedNotice(await seal(notice, { id: deviceId(), encryptionPublic: other.encryptionPublic }), { get, now: NOW });
  assert.equal(elsewhere.failure.reason, "not-a-recipient");
  assert.equal(elsewhere.failure.detail, device.id);
  assert.deepEqual([elsewhere.failure.machine, elsewhere.failure.device, elsewhere.failure.name, elsewhere.failure.at], [envelope.machine, device.id, "MacBook", NOW]);
  assert.notEqual(elsewhere.failure.recipient, device.id);
  assert.deepEqual(await why({ ...envelope, ct: envelope.ct.slice(4) }), plain("bad-signature"));
  assert.deepEqual(await why(await seal(notice, device, "r-refunds")), plain("category-mismatch", { body: "A task is ready to push.", tag: "r-refunds" }));
  assert.deepEqual(await why(null), { title: "kebbie", body: GENERIC.body, tag: "kebbie", url: "/#/board", reason: "no-envelope", detail: null });
  const listing = kv.get(LISTING);
  kv.set(LISTING, { ...listing, listing: { ...listing.listing, list: { ...listing.listing.list, signature: listing.listing.list.signature.replace(/^./, (char) => (char === "A" ? "B" : "A")) } } });
  assert.deepEqual(await why(envelope), plain("list-unsigned"), "a machine list the account didn't sign vouches for nothing");
  kv.set(LISTING, listing);
  kv.set(`marks:${kv.get(NOTIFY_RECORD).account.account}`, { list: listing.listing.list.list.counter + 1, machines: {} });
  assert.deepEqual(await why(envelope), plain("list-older"), "a signed list older than one this browser has seen vouches for nothing");
  kv.delete(`marks:${kv.get(NOTIFY_RECORD).account.account}`);
  const record = kv.get(NOTIFY_RECORD);
  const earlier = await newDeviceKey();
  const earlierId = deviceId();
  kv.set(NOTIFY_RECORD, { ...record, device: earlierId });
  assert.equal((await why(envelope)).reason, undefined, "a notice sealed to the key this browser holds opens whatever the record names");
  assert.deepEqual(await why(await seal(notice, { id: earlierId, encryptionPublic: earlier.encryptionPublic })), plain("device-changed", { detail: earlierId }), "a notice sealed to the earlier device the record still names");
  kv.set(NOTIFY_RECORD, record);
  const failing = async (name) => {
    if (name.startsWith("device-key:")) throw new DOMException("The object could not be cloned.", "DataCloneError");
    return kv.get(name);
  };
  assert.deepEqual(await why(envelope, { get: failing }), plain("error", { detail: "DataCloneError: The object could not be cloned." }), "a key the worker can't read says what the browser threw");
  kv.delete(LISTING);
  assert.deepEqual(await why(envelope), plain("no-list", { title: "kebbie" }));
  assert.deepEqual(await why(envelope, { get: async () => undefined }), plain("no-record", { title: "kebbie" }));
  assert.equal((await why(await seal(notice, device, "refunds"))).body, GENERIC.body, "a tag that names no kind keeps the plain words");
});

test("a machine missing from the kept list is looked up on the hub once, and kept when the account signed the list", async () => {
  const { kv, get, put, seal, identity, machine, signed } = await world();
  const envelope = await seal(notice);
  const empty = { counter: 0, machines: [] };
  const stale = { list: { list: empty, signature: await signMachineList(identity, empty) }, boards: [] };
  const grown = { counter: signed.list.counter + 1, machines: signed.list.machines };
  const fresh = { list: { list: grown, signature: await signMachineList(identity, grown) }, boards: [{ machine: machine.id, name: "MacBook" }] };
  kv.set(LISTING, { listing: stale, at: NOW - 60 });
  let asked = 0;
  const shown = await pushedNotice(envelope, { get, put, now: NOW, boards: async () => (asked++, fresh) });
  assert.deepEqual([shown.title, shown.failure, asked], [notice.title, null, 1]);
  assert.deepEqual(kv.get(LISTING), { listing: fresh, at: NOW });
  assert.equal((await pushedNotice(envelope, { get, put, now: NOW, boards: async () => (asked++, fresh) })).failure, null);
  assert.equal(asked, 1, "a machine on the kept list needs no lookup");

  kv.set(LISTING, { listing: stale, at: NOW - 60 });
  const signedOut = Object.assign(new Error("401 unauthenticated"), { status: 401 });
  assert.equal((await pushedNotice(envelope, { get, put, now: NOW, boards: async () => { throw signedOut; } })).failure.reason, "signed-out");
  const forged = { ...fresh, list: { ...fresh.list, signature: stale.list.signature } };
  assert.equal((await pushedNotice(envelope, { get, put, now: NOW, boards: async () => forged })).failure.reason, "list-unsigned");
  assert.deepEqual(kv.get(LISTING), { listing: stale, at: NOW - 60 }, "a list the account didn't sign is never kept");
  assert.equal((await pushedNotice(envelope, { get, put, now: NOW, boards: async () => stale })).failure.reason, "unknown-machine");
});

test("the worker keeps why the last notice didn't open, and forgets it once one does", async () => {
  const { kv, get, put, del, seal } = await world();
  const registration = { getNotifications: async () => [], showNotification: async () => undefined };
  const envelope = await seal(notice);
  const other = await newDeviceKey();
  await showPushed(registration, await seal(notice, { id: deviceId(), encryptionPublic: other.encryptionPublic }), { get, put, del, now: NOW });
  assert.deepEqual(Object.keys(kv.get(NOTICE_FAILURE)).sort(), ["at", "detail", "device", "machine", "name", "reason", "recipient"]);
  assert.equal(kv.get(NOTICE_FAILURE).reason, "not-a-recipient");
  await showPushed(registration, envelope, { get, put, del, now: NOW });
  assert.equal(kv.has(NOTICE_FAILURE), false);
});

test("Account says in one line when and from where the last notice came, and why it didn't open", () => {
  const at = Math.floor(new Date(2026, 10, 13, 10, 1).getTime() / 1000);
  const line = (failure, now = at + 60) => failureLine({ at, name: "MacBook", detail: null, ...failure }, now);
  assert.equal(line({ reason: "device-changed" }), "The last notification, at 10:01 from MacBook, couldn't be opened: it was sealed to an earlier key of this device.");
  assert.equal(line({ reason: "error", detail: "DataCloneError: The object could not be cloned." }), "The last notification, at 10:01 from MacBook, couldn't be opened: this browser failed while opening it (DataCloneError: The object could not be cloned.).");
  assert.equal(line({ reason: "unsupported-schema", name: null }), "The last notification, at 10:01, couldn't be opened: this app can't read it (unsupported-schema).");
  assert.equal(line({ reason: "expired" }, at + 2 * 86_400), "The last notification, at 13 Nov 10:01 from MacBook, couldn't be opened: it had expired when it arrived.");
  for (const reason of ["no-envelope", "no-record", "no-key", "device-changed", "not-a-recipient", "no-list", "list-older", "list-unsigned", "signed-out", "unknown-machine", "expired", "not-yet-valid", "bad-signature", "undecryptable", "key-mismatch", "category-mismatch"]) assert.doesNotMatch(line({ reason }), /can't read it/, reason);
  assert.equal(failureLine(null, at), null);
});

test("the service worker closes what the notice retracts and shows the notice under its collapse tag", async () => {
  const { get, seal } = await world();
  const closed = [];
  const shown = [];
  const registration = {
    getNotifications: async () => ["q-old", "ci-web"].map((tag) => ({ tag, close: () => closed.push(tag) })),
    showNotification: async (title, options) => shown.push({ title, ...options }),
  };
  await showPushed(registration, await seal(notice), { get, now: NOW });
  assert.deepEqual(closed, ["q-old"]);
  assert.equal(shown.length, 1);
  assert.deepEqual([shown[0].title, shown[0].body, shown[0].tag], ["refunds · shop", "Keep the old endpoint for a release?", "q-refunds"]);
});

test("the service worker's shell holds every module the app and the worker import", async () => {
  const { readdirSync } = await import("node:fs");
  const shell = readFileSync(new URL("./sw.js", import.meta.url), "utf8");
  const files = JSON.parse(/const FILES = (\[[^\]]*\]);/.exec(shell)[1]);
  const modules = readdirSync(new URL(".", import.meta.url)).filter((name) => name.endsWith(".js") && !name.endsWith(".test.js") && name !== "sw.js");
  for (const name of modules) assert.ok(files.includes(`/${name}`), name);
  assert.ok(files.includes("/js/protocol.js") && files.includes("/js/hpke.js"));
});
