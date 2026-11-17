import assert from "node:assert/strict";
import { test } from "node:test";

import { accountOfDevice, dropAccount, heldAccounts, keepAccount, keepNotifyRecord, migrate, withAccount } from "./accounts.js";

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
  assert.deepEqual(list, [{ account: "acc_home", name: "Home", colour: "blue" }]);
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

test("accounts get a name and an unused colour, keep what they were given, and drop everything of theirs", async () => {
  const store = memory();
  await keepAccount(store, "acc_home");
  await keepAccount(store, "acc_work");
  await keepAccount(store, "acc_home", { name: "Hemma" });
  assert.deepEqual(await heldAccounts(store), [{ account: "acc_home", name: "Hemma", colour: "blue" }, { account: "acc_work", name: "Work", colour: "peach" }]);
  await keepNotifyRecord(store, "acc_home", { device: "dev_a" });
  await keepNotifyRecord(store, "acc_work", { device: "dev_b" });
  assert.equal(await accountOfDevice(store, "dev_b"), "acc_work");
  assert.equal(await accountOfDevice(store, "dev_unknown"), null, "with two accounts an unknown device belongs to neither");
  await dropAccount(store, "acc_work");
  assert.equal(store.values.has("notify:acc_work"), false);
  assert.equal(await accountOfDevice(store, "dev_b"), "acc_home", "with one account left an unknown device is that account's");
  assert.deepEqual(withAccount([], "acc_x", { colour: "teal" }), [{ account: "acc_x", name: "Home", colour: "teal" }]);
});
