export const ACCOUNTS = "accounts";
export const NOTIFY_DEVICES = "notify-devices";
export const UNKNOWN = "*";
export const unlockKey = (account) => `unlock:${account}`;
export const listingKey = (account) => `listing:${account}`;
export const notifyKey = (account) => `notify:${account}`;
export const failureKey = (account) => `notice-failure:${account ?? UNKNOWN}`;
export const marksKey = (account) => `marks:${account}`;

export const COLOURS = ["blue", "peach", "teal", "green", "yellow"];

const LEGACY = { unlock: "unlock", listing: "listing", notify: "notify", failure: "notice-failure" };

export async function heldAccounts(store) {
  try {
    const list = await store.get(ACCOUNTS);
    return Array.isArray(list) ? list.filter((one) => typeof one?.account === "string") : [];
  } catch {
    return [];
  }
}

export const defaultName = (index) => (index === 0 ? "Home" : index === 1 ? "Work" : `Account ${index + 1}`);

export function withAccount(list, account, change = {}) {
  const at = list.findIndex((one) => one.account === account);
  if (at >= 0) return list.map((one, index) => (index === at ? { ...one, ...change } : one));
  const used = new Set(list.map((one) => one.colour));
  return [...list, { account, name: defaultName(list.length), colour: COLOURS.find((colour) => !used.has(colour)) ?? COLOURS[list.length % COLOURS.length], ...change }];
}

export async function keepAccount(store, account, change = {}) {
  const list = withAccount(await heldAccounts(store), account, change);
  await store.put(ACCOUNTS, list);
  return list;
}

export async function dropAccount(store, account) {
  const list = (await heldAccounts(store)).filter((one) => one.account !== account);
  await store.put(ACCOUNTS, list);
  const index = (await store.get(NOTIFY_DEVICES).catch(() => null)) ?? {};
  await store.put(NOTIFY_DEVICES, Object.fromEntries(Object.entries(index).filter(([, owner]) => owner !== account)));
  for (const key of [unlockKey(account), listingKey(account), notifyKey(account), failureKey(account)]) await store.del(key).catch(() => undefined);
  return list;
}

// A notice sealed to a device this browser never recorded belongs to the only account here, when there is one, so it can still say why it didn't open.
export async function accountOfDevice(store, device) {
  const index = (await store.get(NOTIFY_DEVICES).catch(() => null)) ?? {};
  if (Object.hasOwn(index, device)) return index[device];
  const owners = new Set(Object.values(index));
  return owners.size === 1 ? [...owners][0] : null;
}

export async function keepNotifyRecord(store, account, record) {
  await store.put(notifyKey(account), record);
  const index = (await store.get(NOTIFY_DEVICES).catch(() => null)) ?? {};
  await store.put(NOTIFY_DEVICES, { ...index, [record.device]: account });
}

export const notifyRecord = (store, account) => store.get(notifyKey(account)).catch(() => null);

export async function forgetNotifyRecord(store, account) {
  await store.del(notifyKey(account)).catch(() => undefined);
}

async function moved(store, from, to) {
  const value = await store.get(from);
  if (value === undefined) return false;
  if ((await store.get(to)) === undefined) await store.put(to, value);
  await store.del(from);
  return true;
}

// The store before several accounts kept one unlock, listing, notify record and notice failure; they move under the account they belong to, read from the records themselves, or from the signed-in account when only the listing is left.
export async function migrate(store, signedIn = null) {
  const notify = await store.get(LEGACY.notify).catch(() => undefined);
  const unlock = await store.get(LEGACY.unlock).catch(() => undefined);
  const owner = notify?.account?.account ?? unlock?.identity?.id ?? signedIn;
  if (!owner) return heldAccounts(store);
  if (unlock !== undefined) await moved(store, LEGACY.unlock, unlockKey(unlock?.identity?.id ?? owner));
  if (notify?.device) await keepNotifyRecord(store, notify.account?.account ?? owner, notify);
  if (notify !== undefined) await store.del(LEGACY.notify);
  await moved(store, LEGACY.listing, listingKey(owner));
  await moved(store, LEGACY.failure, failureKey(owner));
  const list = await heldAccounts(store);
  return list.some((one) => one.account === owner) ? list : keepAccount(store, owner);
}
