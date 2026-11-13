import { emptyMachineList, importSender, machineListTrusts, openEnvelope, ProtocolError, verifyMachineList } from "../js/protocol.js";

export const SLEEP_AFTER = 180;

export const GROUPS = [
  { key: "NEEDS YOU", title: "Needs you", tone: "peach" },
  { key: "WORKING", title: "Working", tone: "blue" },
  { key: "TO REVIEW", title: "In review", tone: "teal" },
  { key: "READY TO PUSH", title: "Ready to push", tone: "green" },
  { key: "CI RUNNING", title: "CI running", tone: "yellow" },
  { key: "WAITING FOR WORK", title: "Waiting for work", tone: "muted" },
];

export function splitAddress(address) {
  const at = address.lastIndexOf("@");
  return at > 0 ? { task: address.slice(0, at), repo: address.slice(at + 1) } : { task: address, repo: "" };
}

export const emptyMarks = () => ({ list: 0, machines: {} });

export function mergeMarks(...all) {
  const merged = emptyMarks();
  for (const marks of all.filter(Boolean)) {
    merged.list = Math.max(merged.list, marks.list ?? 0);
    for (const [machine, mark] of Object.entries(marks.machines ?? {})) {
      const held = merged.machines[machine] ?? { rev: 0, at: 0 };
      merged.machines[machine] = { rev: Math.max(held.rev, mark.rev ?? 0), at: Math.max(held.at, mark.at ?? 0) };
    }
  }
  return merged;
}

export async function readMarks(get, key) {
  try {
    return { marks: (await get(key)) ?? emptyMarks(), failed: false };
  } catch {
    return { marks: emptyMarks(), failed: true };
  }
}

export async function trustedList(listing, identity, marks) {
  if (!listing.list) return marks.list > 0 ? { problem: "rolled-back-list" } : { list: emptyMachineList() };
  if (!(await verifyMachineList(identity, listing.list))) return { problem: "unknown-machine" };
  if (listing.list.list.counter < marks.list) return { problem: "rolled-back-list" };
  return { list: listing.list.list };
}

export async function openBoards(listing, identity, now, marks = emptyMarks()) {
  const trusted = await trustedList(listing, identity, marks);
  const opened = [];
  const next = mergeMarks(marks, trusted.list ? { list: trusted.list.counter } : null);
  for (const entry of listing.boards) {
    const base = { machine: entry.machine, name: entry.name || entry.machine.slice(-6), lastSeen: entry.last_seen ?? null, rev: entry.rev ?? null, storedAt: entry.stored_at ?? null, board: null, problem: null, expired: false };
    if (trusted.problem || !machineListTrusts(trusted.list, entry.machine, entry.machine_keys)) {
      opened.push({ ...base, problem: trusted.problem ?? "unknown-machine" });
      continue;
    }
    if (entry.rev === null || entry.rev === undefined) {
      opened.push(base);
      continue;
    }
    if (!entry.envelope) {
      opened.push({ ...base, problem: "not-sealed-to-this-account" });
      continue;
    }
    try {
      const board = await openEnvelope(entry.envelope, identity, await importSender(entry.machine, entry.machine_keys), now);
      const mark = next.machines[entry.machine] ?? { rev: 0, at: 0 };
      if (board.rev !== entry.rev) opened.push({ ...base, problem: "revision-differs" });
      else if (board.rev < mark.rev || entry.envelope.at < mark.at) opened.push({ ...base, problem: "rolled-back" });
      else {
        next.machines[entry.machine] = { rev: board.rev, at: entry.envelope.at };
        opened.push({ ...base, board });
      }
    } catch (error) {
      if (!(error instanceof ProtocolError)) throw error;
      opened.push(error.code === "expired" ? { ...base, expired: true } : { ...base, problem: error.code });
    }
  }
  const gone = trusted.list ? trusted.list.machines.filter((listed) => !listing.boards.some((entry) => entry.machine === listed.machine)).map((listed) => listed.machine) : [];
  return { opened, marks: next, list: trusted.list ?? null, gone };
}

export const isAsleep = (machine, now) => machine.lastSeen === null || now - machine.lastSeen > SLEEP_AFTER;

export const acceptRequired = (item) => item.steps?.required === true;

export const toAccept = (item) => (acceptRequired(item) ? item.steps.items.filter((step) => step.state === "ready").length : 0);

const byUrgency = (a, b) => a.item.urgency - b.item.urgency || a.item.address.localeCompare(b.item.address) || a.name.localeCompare(b.name);

export const LATER = [
  { key: "queued", title: "Queued" },
  { key: "parked", title: "Parked" },
];

function nextOf(machine) {
  const items = new Map(machine.board.items.map((item) => [item.address, item]));
  const rows = machine.board.next.map((row) => ({ ...row, machine: machine.machine, name: machine.name, item: items.get(row.address) ?? null, ...splitAddress(row.address) }));
  return { machine: machine.machine, name: machine.name, rows, counts: machine.board.counts ?? null };
}

export function arrange(opened, now) {
  const awake = opened.filter((machine) => machine.board && !isAsleep(machine, now));
  const rows = awake.flatMap((machine) => machine.board.items.map((item) => ({ machine: machine.machine, name: machine.name, item, ...splitAddress(item.address) })));
  const groups = GROUPS.map((group) => ({ ...group, rows: rows.filter((row) => row.item.group === group.key).sort(byUrgency) })).filter((group) => group.rows.length > 0);
  const sleeping = opened
    .filter((machine) => machine.expired || (machine.board && isAsleep(machine, now)))
    .map((machine) => ({ machine: machine.machine, name: machine.name, lastSeen: machine.lastSeen ?? machine.storedAt, working: machine.board ? machine.board.items.filter((item) => item.group === "WORKING").length : null }));
  const problems = opened.filter((machine) => machine.problem).map(({ machine, name, problem }) => ({ machine, name, problem }));
  const questions = rows.filter((row) => row.item.question || row.item.prompt).sort(byUrgency);
  const waiting = rows.reduce((sum, row) => sum + toAccept(row.item), 0);
  const next = awake.filter((machine) => Array.isArray(machine.board.next)).map(nextOf);
  const set = awake.flatMap((machine) => (machine.board.later ?? []).map((later) => ({ machine: machine.machine, name: machine.name, later, ...splitAddress(later.address) })));
  const later = LATER.map((group) => ({ ...group, rows: set.filter((row) => row.later.state === group.key) })).filter((group) => group.rows.length > 0);
  const routines = awake.flatMap((machine) => machine.board.routines.map((routine) => ({ machine: machine.machine, name: machine.name, routine })));
  const away = awake.filter((machine) => machine.board.away_since).map((machine) => ({ machine: machine.machine, name: machine.name, since: machine.board.away_since }));
  return { groups, sleeping, problems, questions, toAccept: waiting, machines: opened.length, next, later, routines, away };
}

export function findTask(opened, machine, address) {
  const found = opened.find((candidate) => candidate.machine === machine);
  const item = found?.board?.items.find((candidate) => candidate.address === address);
  return item ? { machine, name: found.name, item, accepts: found.board.accepts ?? null, ...splitAddress(address) } : null;
}
