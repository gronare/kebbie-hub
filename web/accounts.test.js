import assert from "node:assert/strict";
import { test } from "node:test";

import { accountOfDevice, droppedDevice, dropAccount, heldAccounts, keepAccount, keepNotifyRecord, labelsOf, liveAccounts, markSignedOut, migrate, nameOf, titled, withAccount } from "./accounts.js";

function memory(entries = []) {
  const values = new Map(entries);
  return { values, get: async (key) => structuredClone(values.get(key)), put: async (key, value) => void values.set(key, structuredClone(value)), del: async (key) => void values.delete(key) };
}

const before = () => [
  ["notify", { account: { account: "acc_home", version: 1 }, device: "dev_phone" }],
  ["unlock", { identity: { id: "acc_home" }, until: 5 }],
  ["listing", { listing: { boards: [] }, at: 3 }],
  ["notice-failure", { reason: "no-list" }],
  ["device-key:acc_home", { id: "dev_phone" }],
  ["marks:acc_home", { list: 2, machines: {} }],
];

test("today's single-account store becomes account one with its unlock, listing, notify record and failure, and nothing else moves", async () => {
  const store = memory(before());
  const list = await migrate(store);
  assert.deepEqual(list, [{ account: "acc_home", colour: "blue" }]);
  assert.deepEqual(Object.fromEntries(store.values), {
    accounts: list,
    "unlock:acc_home": { identity: { id: "acc_home" }, until: 5 },
    "notify:acc_home": { account: { account: "acc_home", version: 1 }, device: "dev_phone" },
    "notify-devices": { dev_phone: "acc_home" },
    "listing:acc_home": { listing: { boards: [] }, at: 3 },
    "notice-failure:acc_home": { reason: "no-list" },
    "device-key:acc_home": { id: "dev_phone" },
    "marks:acc_home": { list: 2, machines: {} },
  });
  const once = structuredClone(Object.fromEntries(store.values));
  await migrate(store, "acc_home");
  assert.deepEqual(Object.fromEntries(store.values), once, "a second run changes nothing");
});

test("a listing kept alone waits for the signed-in account to say whose it is, and a newer key is never overwritten", async () => {
  const store = memory([["listing", { at: 1 }]]);
  assert.deepEqual(await migrate(store), []);
  assert.deepEqual(store.values.get("listing"), { at: 1 });
  store.values.set("listing:acc_home", { at: 9 });
  await migrate(store, "acc_home");
  assert.deepEqual(store.values.get("listing:acc_home"), { at: 9 });
  assert.equal(store.values.has("listing"), false);
  assert.deepEqual((await heldAccounts(store)).map((one) => one.account), ["acc_home"]);
});

test("accounts get an unused colour, keep what they were given, and drop everything of theirs", async () => {
  const store = memory();
  await keepAccount(store, "acc_home");
  await keepAccount(store, "acc_work");
  await keepAccount(store, "acc_home", { name: "Hemma", named: true });
  assert.deepEqual(await heldAccounts(store), [{ account: "acc_home", name: "Hemma", named: true, colour: "blue" }, { account: "acc_work", colour: "peach" }]);
  await keepNotifyRecord(store, "acc_home", { device: "dev_a" });
  await keepNotifyRecord(store, "acc_work", { device: "dev_b" });
  assert.equal(await accountOfDevice(store, "dev_b"), "acc_work");
  assert.equal(await accountOfDevice(store, "dev_unknown"), null, "with two accounts an unknown device belongs to neither");
  await dropAccount(store, "acc_work");
  assert.equal(store.values.has("notify:acc_work"), false);
  assert.equal(await accountOfDevice(store, "dev_b"), null, "a removed account's device never passes for the account left");
  assert.equal(await droppedDevice(store, "dev_b"), true);
  assert.equal(await accountOfDevice(store, "dev_unknown"), "acc_home", "with one account left a device never recorded is that account's");
  await keepNotifyRecord(store, "acc_work", { device: "dev_b" });
  assert.equal(await droppedDevice(store, "dev_b"), false, "an account added back takes its device back");
  assert.deepEqual(withAccount([], "acc_x", { colour: "teal" }), [{ account: "acc_x", colour: "teal" }]);
});

test("an account is called by its email until the person names it, and the Home and Work the app once gave give way to it", async () => {
  const me = { account: "acc_work", handle: "k0123456789ab", github: null };
  const store = memory();
  await keepAccount(store, "acc_work", labelsOf(me, { email: "carl@work.example", verified: true }));
  assert.equal(nameOf((await heldAccounts(store))[0]), "carl@work.example");
  await keepAccount(store, "acc_work", { name: "Work", named: true });
  assert.equal(nameOf((await heldAccounts(store))[0]), "Work", "a name the person saved stays, even one the app once gave");
  const old = memory([["accounts", [{ account: "acc_a", name: "Home", colour: "blue", email: "carl@work.example" }, { account: "acc_b", name: "Work", colour: "peach" }, { account: "acc_c", name: "Kebbie", colour: "teal" }]]]);
  assert.deepEqual((await heldAccounts(old)).map(nameOf), ["carl@work.example", "Account", "Kebbie"]);
  assert.equal(nameOf({ account: "acc_d", handle: "k0123456789ab" }), "k0123456789ab", "a hub without email names it by its handle");
});

test("the same account is never held twice, and running the migration again after a sign-in adds no second entry", async () => {
  const doubled = memory([["accounts", [{ account: "acc_home", colour: "blue" }, { account: "acc_home", colour: "peach" }]]]);
  assert.deepEqual(await heldAccounts(doubled), [{ account: "acc_home", colour: "blue" }]);
  const store = memory(before());
  await migrate(store);
  await migrate(store, "acc_home");
  await migrate(store);
  await keepAccount(store, "acc_home", { signedOut: false });
  assert.deepEqual((await heldAccounts(store)).map((one) => one.account), ["acc_home"]);
});

test("only accounts with a live session count: a signed-out one leaves the list of live ones and the notice title, and comes back on sign-in", async () => {
  const store = memory();
  await keepAccount(store, "acc_home", { email: "carl@home.example" });
  await keepAccount(store, "acc_work", { email: "carl@work.example" });
  assert.equal(await titled(store, "acc_home", "Ready"), "carl@home.example · Ready");
  await markSignedOut(store, "acc_work", true);
  assert.deepEqual(liveAccounts(await heldAccounts(store)).map((one) => one.account), ["acc_home"]);
  assert.equal(await titled(store, "acc_home", "Ready"), "Ready", "with one live account the title is the notice's own");
  await markSignedOut(store, "acc_gone", true);
  assert.equal((await heldAccounts(store)).length, 2, "marking an account this browser doesn't hold adds nothing");
  await markSignedOut(store, "acc_work", false);
  assert.equal(liveAccounts(await heldAccounts(store)).length, 2);
});
