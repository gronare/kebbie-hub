import assert from "node:assert/strict";
import { test } from "node:test";

import { freshSlot, holderOf, merged, unlockedOf } from "./slots.js";

function two() {
  const home = { ...freshSlot("acc_home", "home calls"), session: { state: "unlocked" }, opened: [{ machine: "mch_devbox", name: "devbox" }], keys: new Map([["mch_devbox", "home keys"]]), kept: { cursor: 4, sent: [{ id: "cmd_1", machine: "mch_devbox" }] }, allowed: true, fetchedAt: 100 };
  const work = { ...freshSlot("acc_work", "work calls"), session: { state: "unlocked" }, opened: [{ machine: "mch_laptop", name: "laptop" }], keys: new Map([["mch_laptop", "work keys"]]), kept: { cursor: 9, sent: [{ id: "cmd_2", machine: "mch_laptop" }] }, fetchedAt: 90 };
  return new Map([[home.id, home], [work.id, work]]);
}

test("each machine resolves to the account that holds it, so its commands go out through that account's calls only", () => {
  const slots = two();
  const open = unlockedOf(slots, (slot) => slot.session);
  assert.equal(holderOf(open, "mch_devbox").calls, "home calls");
  assert.equal(holderOf(open, "mch_laptop").calls, "work calls");
  assert.equal(holderOf(open, "mch_elsewhere"), null);
  slots.get("acc_work").session = { state: "locked" };
  assert.equal(holderOf(unlockedOf(slots, (slot) => slot.session), "mch_laptop"), null, "a locked account's machines take no command");
});

test("the merged view tags every machine with its account and keeps each account's sent commands", () => {
  const view = merged(unlockedOf(two(), (slot) => slot.session));
  assert.deepEqual(view.opened.map((one) => [one.account, one.machine]), [["acc_home", "mch_devbox"], ["acc_work", "mch_laptop"]]);
  assert.deepEqual([...view.keys.keys()], ["mch_devbox", "mch_laptop"]);
  assert.deepEqual(view.kept.sent.map((one) => one.id), ["cmd_1", "cmd_2"]);
  assert.equal(view.allowed, true);
  assert.equal(view.fetchedAt, 90);
  assert.deepEqual(merged([]), { opened: [], keys: new Map(), kept: { cursor: 0, sent: [] }, offline: false, fetchedAt: null, allowed: false });
});
