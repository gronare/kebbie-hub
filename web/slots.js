import { emptyMarks } from "./board.js";

export const freshSlot = (id, calls) => ({ id, calls, me: null, session: null, opened: [], marks: emptyMarks(), keys: new Map(), kept: { cursor: 0, sent: [] }, keptFor: null, allowed: false, device: null, lastMachine: null, fetchedAt: null, offline: false, following: null, collecting: null, retrying: null, ends: null });

export const unlockedOf = (slots, sessionOf) => [...slots.values()].filter((slot) => sessionOf(slot)?.state === "unlocked");

// A machine id belongs to the one account whose signed list or boards hold it; a machine no unlocked account holds belongs to none.
export const holderOf = (open, machine) => open.find((slot) => slot.keys.has(machine) || slot.opened.some((opened) => opened.machine === machine)) ?? null;

export function merged(open) {
  return {
    opened: open.flatMap((slot) => slot.opened.map((opened) => ({ ...opened, account: slot.id }))),
    keys: new Map(open.flatMap((slot) => [...slot.keys])),
    kept: { cursor: 0, sent: open.flatMap((slot) => slot.kept.sent) },
    offline: open.some((slot) => slot.offline),
    fetchedAt: open.some((slot) => slot.fetchedAt) ? Math.min(...open.map((slot) => slot.fetchedAt ?? Infinity)) : null,
    allowed: open.some((slot) => slot.allowed === true),
  };
}
