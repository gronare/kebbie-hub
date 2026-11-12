import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import { accountIdentity, deviceId, emptyMachineList, importIdentity, machineListWith, newAccountSecrets, newDeviceKey, sealEnvelope, signMachineList } from "../js/protocol.js";
import { CATEGORY_GROUPS, closeSettled, deviceWord, GENERIC, isSettled, keptCategories, LISTING, NOTIFY_RECORD, notifyPhase, notifyText, pushedNotice, routeOf, showPushed, switchable, withCategory } from "./notify.js";

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
  const seal = async (payload, recipient = device) => {
    const header = { v: 1, id: "ntc_01K85Y2D8G0000000000000000", kind: "notice", from: machine.id, machine: machine.id, at: NOW, exp: NOW + 3600, schema: 1, urgency: "high", collapse: "q-refunds" };
    return sealEnvelope(header, new TextEncoder().encode(JSON.stringify(payload)), machine, [{ id: recipient.id, encryption: recipient.encryptionPublic }]);
  };
  return { kv, get: async (name) => kv.get(name), seal, machine, device };
}

const notice = { type: "notice", category: "questions", title: "refunds · shop", body: "Keep the old endpoint for a release?", open: { to: "task", address: "refunds@shop" }, retract: ["q-old"] };

test("a pushed notice opens with the device's key after the account-signed list vouches for its machine", async () => {
  const { get, seal, machine } = await world();
  const shown = await pushedNotice(await seal(notice), { get, now: NOW });
  assert.deepEqual(shown, { title: "refunds · shop", body: "Keep the old endpoint for a release?", tag: "q-refunds", data: { machine: machine.id, name: "MacBook", category: "questions", open: notice.open, url: `/#/task/${machine.id}/refunds%40shop` }, retract: ["q-old"] });
});

test("a notice that can't be verified or opened still shows, as a plain one that names nothing", async () => {
  const plain = (tag = "q-refunds") => ({ title: GENERIC.title, body: GENERIC.body, tag, data: { url: "/#/board" }, retract: [] });
  const { kv, get, seal } = await world();
  const envelope = await seal(notice);
  const other = await newDeviceKey();
  assert.deepEqual(await pushedNotice(await seal(notice, { id: deviceId(), encryptionPublic: other.encryptionPublic }), { get, now: NOW }), plain());
  assert.deepEqual(await pushedNotice({ ...envelope, ct: envelope.ct.slice(1) }, { get, now: NOW }), plain());
  assert.deepEqual(await pushedNotice(null, { get, now: NOW }), plain("kebbie"));
  const listing = kv.get(LISTING);
  kv.set(LISTING, { ...listing, listing: { ...listing.listing, list: { ...listing.listing.list, signature: listing.listing.list.signature.replace(/^./, (char) => (char === "A" ? "B" : "A")) } } });
  assert.deepEqual(await pushedNotice(envelope, { get, now: NOW }), plain(), "a machine list the account didn't sign vouches for nothing");
  kv.delete(LISTING);
  assert.deepEqual(await pushedNotice(envelope, { get, now: NOW }), plain());
  assert.deepEqual(await pushedNotice(envelope, { get: async () => undefined, now: NOW }), plain());
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
