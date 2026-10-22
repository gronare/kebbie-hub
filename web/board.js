import { importSender, openEnvelope, ProtocolError } from "../js/protocol.js";

export const SLEEP_AFTER = 180;

export const GROUPS = [
  { key: "NEEDS YOU", title: "Needs you", tone: "peach" },
  { key: "WORKING", title: "Working", tone: "blue" },
  { key: "TO REVIEW", title: "To review", tone: "blue" },
  { key: "READY TO PUSH", title: "Ready to push", tone: "green" },
  { key: "CI RUNNING", title: "CI running", tone: "blue" },
  { key: "WAITING FOR WORK", title: "Waiting for work", tone: "muted" },
];

export function splitAddress(address) {
  const at = address.lastIndexOf("@");
  return at > 0 ? { task: address.slice(0, at), repo: address.slice(at + 1) } : { task: address, repo: "" };
}

export async function openBoards(entries, identity, machines, now, highest = {}) {
  const opened = [];
  for (const entry of entries) {
    const paired = machines.find((machine) => machine.id === entry.machine);
    const base = { machine: entry.machine, name: paired?.name || entry.name || entry.machine.slice(-6), lastSeen: entry.last_seen ?? null, rev: entry.rev, storedAt: entry.stored_at, board: null, problem: null };
    if (!paired) {
      opened.push({ ...base, problem: "unknown-machine" });
      continue;
    }
    if (!entry.envelope) {
      opened.push({ ...base, problem: "not-sealed-to-this-device" });
      continue;
    }
    try {
      const board = await openEnvelope(entry.envelope, identity, await importSender(paired.id, paired.keys), now);
      if (board.rev !== entry.rev) opened.push({ ...base, problem: "revision-differs" });
      else if (board.rev < (highest[entry.machine] ?? 0)) opened.push({ ...base, problem: "rolled-back" });
      else opened.push({ ...base, board });
    } catch (error) {
      if (!(error instanceof ProtocolError)) throw error;
      opened.push({ ...base, problem: error.code });
    }
  }
  return opened;
}

export function highestRevs(opened, highest) {
  const next = { ...highest };
  for (const machine of opened) {
    if (machine.board) next[machine.machine] = Math.max(next[machine.machine] ?? 0, machine.board.rev);
  }
  return next;
}

export const isAsleep = (machine, now) => machine.lastSeen === null || now - machine.lastSeen > SLEEP_AFTER;

const byUrgency = (a, b) => b.item.urgency - a.item.urgency || a.item.address.localeCompare(b.item.address) || a.name.localeCompare(b.name);

export function arrange(opened, now) {
  const awake = opened.filter((machine) => machine.board && !isAsleep(machine, now));
  const rows = awake.flatMap((machine) => machine.board.items.map((item) => ({ machine: machine.machine, name: machine.name, item, ...splitAddress(item.address) })));
  const groups = GROUPS.map((group) => ({ ...group, rows: rows.filter((row) => row.item.group === group.key).sort(byUrgency) })).filter((group) => group.rows.length > 0);
  const sleeping = opened
    .filter((machine) => machine.board && isAsleep(machine, now))
    .map((machine) => ({ machine: machine.machine, name: machine.name, lastSeen: machine.lastSeen ?? machine.storedAt, working: machine.board.items.filter((item) => item.group === "WORKING").length }));
  const problems = opened.filter((machine) => machine.problem).map(({ machine, name, problem }) => ({ machine, name, problem }));
  const questions = rows.filter((row) => row.item.question || row.item.prompt).sort(byUrgency);
  return { groups, sleeping, problems, questions, machines: opened.length };
}

export function findTask(opened, machine, address) {
  const found = opened.find((candidate) => candidate.machine === machine);
  const item = found?.board?.items.find((candidate) => candidate.address === address);
  return item ? { machine, name: found.name, item, ...splitAddress(address) } : null;
}
