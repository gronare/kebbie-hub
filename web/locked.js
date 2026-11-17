import { fromB64, verifyMachineList } from "../js/protocol.js";
import { keepAccount, labelsOf, nameOf } from "./accounts.js";
import { NoPasskeyOffered, signInWith } from "./signin.js";
import { keptUnlock, unlockAfter } from "./unlock.js";

export const noPasskeyText = (name) => `This phone has no passkey for ${name}. Unlock ${name} here with a passkey that belongs to it, such as one saved in your password manager.`;

export class NoPasskeyHere extends Error {
  constructor(name) {
    super(noPasskeyText(name));
    this.code = "no-passkey-here";
  }
}

export class OtherAccount extends Error {
  constructor(name) {
    super(`That passkey belongs to another account than ${name}. Nothing changed.`);
    this.code = "other-account";
  }
}

// An account signed in by email or code is kept on the phone locked; a phone that cannot keep it ends the session, so no account is held that the phone does not list.
export async function keepAdded(store, calls, me, { emails = true } = {}) {
  try {
    const address = emails ? await calls.email().catch(() => null) : null;
    return await keepAccount(store, me.account, { ...labelsOf(me, address), signedOut: false });
  } catch (error) {
    await calls.logout().catch(() => undefined);
    throw error;
  }
}

// A session signed in without a passkey cannot read the boards, so an unlock kept from before stays unused.
export const heldSession = async (me, store, now, storage) => (me.key_access === false ? null : await keptUnlock(me, store, now, storage)) ?? { state: "locked", me };

async function passkeyIds(calls) {
  const listed = await calls.passkeys();
  return (listed?.passkeys ?? []).map((one) => one.passkey).filter((id) => typeof id === "string" && id);
}

export async function unlockHeld(hub, held, { remembered = null, storage, store, clock } = {}) {
  const name = nameOf(held);
  const allow = await passkeyIds(hub.account);
  if (!remembered && !allow.length) throw new NoPasskeyHere(name);
  let ceremony;
  try {
    ceremony = await signInWith(hub).withPasskey(remembered, {}, allow);
  } catch (error) {
    throw error instanceof NoPasskeyOffered ? new NoPasskeyHere(name) : error;
  }
  if (ceremony.signedIn.account !== held.account) {
    ceremony.prf?.fill(0);
    throw new OtherAccount(name);
  }
  return unlockAfter(hub, ceremony, storage, store, clock);
}

async function verifying(key) {
  try {
    return await crypto.subtle.importKey("raw", fromB64(key.keys.signing), { name: "Ed25519" }, false, ["verify"]);
  } catch {
    return null;
  }
}

export async function lockedMachines(key, listing) {
  if (!key?.keys?.signing || !listing?.list) return [];
  const verify = await verifying(key);
  if (!verify || !(await verifyMachineList({ id: key.account, verifying: verify }, listing.list))) return [];
  const names = new Map((listing.boards ?? []).map((entry) => [entry.machine, entry.name]));
  return listing.list.list.machines.map(({ machine }) => ({ machine, name: names.get(machine) || machine.slice(-6) }));
}
