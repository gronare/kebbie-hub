import { importSender, openEnvelope } from "../js/protocol.js";

export const FALLBACK = { title: "kelpie", body: "Något på boardet har ändrats.", tag: "kelpie", retract: null, url: "/#/board" };

export async function openNotice(envelope, identity, machines, now) {
  const machine = machines.find((candidate) => candidate.id === envelope?.machine);
  if (!machine || envelope.kind !== "notice") return FALLBACK;
  try {
    const notice = await openEnvelope(envelope, identity, await importSender(machine.id, machine.keys), now);
    const url = notice.address ? `/#/task/${encodeURIComponent(machine.id)}/${encodeURIComponent(notice.address)}` : "/#/board";
    return { title: notice.title, body: notice.question ?? notice.body, tag: envelope.collapse ?? envelope.id, retract: notice.retract ?? null, url };
  } catch {
    return FALLBACK;
  }
}
