import { esc } from "./screens.js";

export function clock(seconds) {
  return new Date(seconds * 1000).toLocaleTimeString("en-GB", { hour: "2-digit", minute: "2-digit" });
}

export const until = (seconds, now) => `${clock(seconds)}${new Date(seconds * 1000).toDateString() === new Date(now * 1000).toDateString() ? "" : " tomorrow"}`;

export function gate({ info, refused, allowed }) {
  if (!info?.commands) return "off";
  if (refused) return "refused";
  if (!allowed) return "view-only";
  return "ready";
}

export const takes = (accepts, kind) => Array.isArray(accepts) && accepts.includes(kind);

export function machinePolicy(accepts) {
  if (takes(accepts, "answer") && takes(accepts, "push")) return "takes answers and pushes";
  if (takes(accepts, "answer")) return "takes answers, not pushes";
  return "takes nothing from the hub";
}

export const viewOnlyBlock = () => `<div class="pro">View only on this device. Allow changes in <a class="link inline" href="/account">Account</a></div>`;

export function notTakenBlock(kind, machineName, accepts) {
  const machine = esc(machineName);
  if (kind === "push") return `<div class="pro">Not pushed: ${machine} ${takes(accepts, "answer") ? "takes answers, not pushes" : "takes nothing from the hub"}. Push at the machine, or allow pushes in its kebbie settings.</div>`;
  if (kind === "prompt-answer") return `<div class="pro">Not chosen: ${machine} takes nothing from the hub. Choose at the machine, or allow answers in its kebbie settings.</div>`;
  return `<div class="pro">Not answered: ${machine} takes nothing from the hub. Answer at the machine, or allow answers in its kebbie settings.</div>`;
}

export function sentState(sent, { now, asleep, standing = null }) {
  if (sent.result?.reason === "not-approved") return "not-approved";
  if (sent.result?.reason === "denied") return "denied";
  if (sent.result) return sent.result.outcome === "done" ? "done" : "refused";
  if (standing?.state === "denied") return "denied";
  if (standing?.state === "waiting") return now >= sent.exp ? "not-approved" : "approval";
  if (now >= sent.exp) return "expired";
  return asleep ? "waiting" : "sending";
}

export const settled = (state) => state === "expired" || state === "refused" || state === "not-approved" || state === "denied";

export function standingOn(board, device) {
  const standing = device ? board?.devices?.[device.id] : null;
  return standing && standing.fingerprint === device.fingerprint ? standing : null;
}

export function latestSent(sentList, { machine, address, kind, question = null, prompt = null }) {
  const mine = sentList.filter((one) => one.machine === machine && one.address === address && one.kind === kind && (question === null || one.question === question) && (prompt === null || one.prompt === prompt));
  return mine.sort((a, b) => b.at - a.at)[0] ?? null;
}

export function refusal(fallback, text) {
  const said = String(text ?? "").trim();
  if (!said.toLowerCase().startsWith(fallback.toLowerCase())) return [esc(fallback), esc(said)];
  const cut = said.indexOf(". ");
  return cut < 0 ? [esc(said), ""] : [esc(said.slice(0, cut + 1)), esc(said.slice(cut + 2))];
}

export const from = (sent) => `from ${sent.deviceName ? esc(sent.deviceName) : "this device"} at ${esc(clock(sent.at))}`;
export const hours = (seconds) => `${Math.max(1, Math.floor(seconds / 3600))}\u00a0h`;
const thisDevice = (sent) => (sent.deviceName ? esc(sent.deviceName) : "device");
const approvedAt = (sent, standing) => (standing?.state === "approved" && standing.at >= sent.at ? ` · approved on ${esc(sent.machineName)} at ${esc(clock(standing.at))}` : "");

export function approvalStatus(sent, state, { standing, now, what }) {
  const machine = esc(sent.machineName);
  const device = thisDevice(sent);
  if (state === "denied") return block("refused", `${machine} doesn't take changes from this ${device}`, `Approve it in kebbie on ${machine}.`, `Sent ${from(sent)}`);
  if (state === "not-approved") return block("refused", `Not delivered: ${machine} didn't approve this ${device} within the hour`, `Approve it in kebbie on ${machine}, then ${what.again}.`, `Sent ${from(sent)} · expired ${esc(clock(sent.exp))}`);
  const auto = standing?.until ? `, or it is approved automatically in ${hours(standing.until - now)}` : ` on ${machine}`;
  return block("wait", `Waiting for ${machine} to approve this ${device}`, `Approve it in kebbie${auto}. ${what.goes} if ${machine} approves before ${esc(until(sent.exp, now))}.`, `Sent ${from(sent)} · expires ${esc(until(sent.exp, now))}`);
}

export const APPROVAL_STATES = ["approval", "not-approved", "denied"];

export function block(tone, title, body, meta) {
  return `<div class="sent ${tone}" role="status"><b>${title}</b>${body ? `<span>${body}</span>` : ""}<span class="meta">${meta}</span></div>`;
}

export function answerStatus(sent, state, { standing = null, now = Math.floor(Date.now() / 1000) } = {}) {
  const machine = esc(sent.machineName);
  const said = sent.label ? `"${esc(sent.label)}"` : "your answer";
  if (APPROVAL_STATES.includes(state)) return approvalStatus(sent, state, { standing, now, what: { goes: `Your answer ${said} goes to the task`, again: "answer again" } });
  if (state === "done") return block("done", `✓ Answered: ${esc(sent.label ?? "your own answer")}`, `${esc(sent.address)} has your answer and carries on.`, `${from(sent)}${approvedAt(sent, standing)} · the lead got a copy`);
  if (state === "refused") return block("refused", ...refusal("Not answered", sent.result.text), from(sent));
  if (state === "expired") return block("refused", `Not delivered: ${machine} stayed asleep past the hour`, `Answer again when ${machine} is awake.`, `Sent ${from(sent)} · expired ${esc(clock(sent.exp))}`);
  if (state === "waiting") return block("wait", `Waiting for ${machine}`, `Your answer ${said} goes to the task when ${machine} wakes up.`, `Sent ${from(sent)} · expires ${esc(clock(sent.exp))}`);
  return block("wait", `Sent to ${machine}`, `Your answer ${said} is on its way to the task.`, `Sent ${from(sent)} · expires ${esc(clock(sent.exp))}`);
}

export function pushStatus(sent, state, { standing = null, now = Math.floor(Date.now() / 1000) } = {}) {
  const machine = esc(sent.machineName);
  const sha = esc(sent.short);
  if (APPROVAL_STATES.includes(state)) return approvalStatus(sent, state, { standing, now, what: { goes: `Your push of ${sha} goes out`, again: "push again" } });
  if (state === "done") return block("done", `✓ Pushed ${sha} to main`, esc(sent.result.text), `${from(sent)}${approvedAt(sent, standing)}`);
  if (state === "refused") return block("refused", ...refusal("Not pushed", sent.result.text), from(sent));
  if (state === "expired") return block("refused", `Not delivered: ${machine} stayed asleep past the hour`, `Push again when ${machine} is awake.`, `Sent ${from(sent)} · expired ${esc(clock(sent.exp))}`);
  if (state === "waiting") return block("wait", `Waiting for ${machine}`, `Your push of ${sha} goes out when ${machine} wakes up.`, `Sent ${from(sent)} · expires ${esc(clock(sent.exp))}`);
  return block("wait", `Sent to ${machine}`, `${machine} rebases ${sha} onto main and pushes.`, `Sent ${from(sent)} · expires ${esc(clock(sent.exp))}`);
}

export function promptStatus(sent, state, { standing = null, now = Math.floor(Date.now() / 1000) } = {}) {
  const machine = esc(sent.machineName);
  const label = esc(sent.label);
  if (APPROVAL_STATES.includes(state)) return approvalStatus(sent, state, { standing, now, what: { goes: `"${label}" is chosen while the prompt still shows`, again: "choose again" } });
  if (state === "done") return block("done", `✓ Chose: ${label}`, `${esc(sent.address)} carries on.`, `${from(sent)}${approvedAt(sent, standing)} · the lead got a copy`);
  if (state === "refused") return block("refused", ...refusal("Not chosen", sent.result.text), from(sent));
  if (state === "expired") return block("refused", `Not delivered: ${machine} didn't take it within ten minutes`, `A prompt on the screen doesn't wait. If it still asks, choose again.`, `Sent ${from(sent)} · expired ${esc(clock(sent.exp))}`);
  if (state === "waiting") return block("wait", `Waiting for ${machine}`, `"${label}" is chosen if ${machine} wakes within ten minutes and the prompt still shows.`, `Sent ${from(sent)} · expires ${esc(clock(sent.exp))}`);
  return block("wait", `Sent to ${machine}`, `${machine} chooses "${label}" if the prompt still shows.`, `Sent ${from(sent)} · expires ${esc(clock(sent.exp))}`);
}

export const takesPromptAnswers = (found) => takes(found?.accepts, "prompt-answer");

export const choosable = (option) => Boolean(option) && !option.takes_text && typeof option.key === "string" && option.key !== "";

function promptCard(option, index, { picked, pickable }) {
  const press = pickable && choosable(option);
  const classes = ["opt", press ? "pick" : "", picked === index ? "picked" : ""].filter(Boolean).join(" ");
  const attributes = press ? ` role="button" tabindex="0" data-action="pick" data-index="${index}" aria-pressed="${picked === index}"` : "";
  const meaning = choosable(option) ? "" : "answer at the machine";
  return `<div class="${classes}"${attributes}><b>${esc(option.label)}</b>${meaning ? `<small>${meaning}</small>` : ""}</div>`;
}

export function promptAnswerBlock({ prompt, ui, sent, state, standing = null }) {
  const live = sent && !settled(state);
  const sentAt = live ? prompt.options.findIndex((option) => option.key === sent.key) : -1;
  const picked = live ? (sentAt < 0 ? null : sentAt) : ui.picked;
  const cards = prompt.options.map((option, index) => promptCard(option, index, { picked, pickable: !live && !ui.busy }));
  const options = `<div class="options${live || picked !== null ? " chosen" : ""}">${cards.join("")}</div>`;
  const status = sent ? promptStatus(sent, state, { standing }) : "";
  if (live) return options + status;
  const error = ui.error ? `<div class="center warn" role="alert">${esc(ui.error)}</div>` : "";
  const option = picked === null ? undefined : prompt.options[picked];
  if (choosable(option)) return options + status + `<div class="answer-row"><button type="button" class="btn primary" data-action="choose"${ui.busy ? " disabled" : ""}>Choose: ${esc(option.label)}</button></div>` + error;
  return options + status + error;
}

function optionCard(option, index, { recommend, picked, pickable }) {
  const recommended = Boolean(recommend) && index === recommend - 1;
  const classes = ["opt", recommended ? "rec" : "", pickable ? "pick" : "", picked === index ? "picked" : ""].filter(Boolean).join(" ");
  const press = pickable ? ` role="button" tabindex="0" data-action="pick" data-index="${index}" aria-pressed="${picked === index}"` : "";
  return `<div class="${classes}"${press}>${recommended ? `<span class="tag">Recommended</span>` : ""}<b>${esc(option.label)}</b>${option.meaning ? `<small>${esc(option.meaning)}</small>` : ""}</div>`;
}

const noteNeeded = (option) => option?.comment === true;

function noteField(key, label, note) {
  return `<div class="note-field"><label for="note-${esc(key)}">${esc(label)}</label><textarea id="note-${esc(key)}" data-draft="note" rows="2">${esc(note)}</textarea></div>`;
}

export function answerReady(option, ui) {
  if (ui.free) return ui.note.trim() !== "";
  return option !== undefined && (!noteNeeded(option) || ui.note.trim() !== "");
}

export function answerBlock({ key, question, ui, sent, state, standing = null }) {
  const live = sent && !settled(state);
  const picked = live ? (sent.choice ?? null) : ui.picked;
  const chosen = live || picked !== null || ui.free;
  const cards = question.options.map((option, index) => optionCard(option, index, { recommend: question.recommend, picked, pickable: !live && !ui.busy }));
  const options = `<div class="options${chosen ? " chosen" : ""}">${cards.join("")}</div>`;
  const status = sent ? answerStatus(sent, state, { standing }) : "";
  if (live) return options + status;
  const error = ui.error ? `<div class="center warn" role="alert">${esc(ui.error)}</div>` : "";
  const busy = ui.busy ? " disabled" : "";
  if (ui.free) {
    const ready = answerReady(undefined, ui) && !ui.busy;
    return options + status + `<div class="answer-row">${noteField(key, "Your answer", ui.note)}<button type="button" class="btn primary" data-action="answer"${ready ? "" : " disabled"}>Send answer</button><button type="button" class="sub-btn" data-action="pick-instead"${busy}>Pick an option instead</button></div>` + error;
  }
  if (picked !== null) {
    const option = question.options[picked];
    const ready = answerReady(option, ui) && !ui.busy;
    const label = noteNeeded(option) ? "Note for the agent (needed for this answer)" : "Note for the agent (optional)";
    return options + status + `<div class="answer-row">${noteField(key, label, ui.note)}<button type="button" class="btn primary" data-action="answer"${ready ? "" : " disabled"}>Answer: ${esc(option.label)}</button></div>` + error;
  }
  return options + status + `<button type="button" class="sub-btn" data-action="write-own"${busy}>Write your own answer</button>` + error;
}

export function pushReady(item) {
  return /^[0-9a-f]{40}$/.test(item.head ?? "") && (item.group === "READY TO PUSH" || Boolean(item.card));
}

export function pushBlock({ item, machineName, ui, sent, state, standing = null }) {
  const short = item.head.slice(0, 7);
  const commits = item.git?.ahead ?? 0;
  const count = `${commits} ${commits === 1 ? "commit" : "commits"}`;
  const live = sent && !settled(state);
  const status = sent ? pushStatus(sent, state, { standing }) : "";
  if (live) return status;
  const error = ui.error ? `<div class="center warn" role="alert">${esc(ui.error)}</div>` : "";
  if (ui.confirming) {
    const reviewed = item.verdict?.verdict === "ok" && !item.verdict.stale ? ", reviewer passed" : "";
    const busy = ui.busy ? " disabled" : "";
    return status + `<div class="confirm" role="group" aria-label="Confirm push"><p><b>Push ${esc(short)} to main?</b></p><p>${esc(count)}${reviewed}. ${esc(machineName)} rebases onto the latest main first; on a conflict its Claude resolves it before pushing.</p><div class="row2"><button type="button" class="btn" data-action="push-cancel"${busy}>Cancel</button><button type="button" class="btn primary" data-action="push"${busy}>Push</button></div></div>` + error;
  }
  return status + `<button type="button" class="btn primary" data-action="push-ask">Push ${esc(count)} · ${esc(short)}</button>` + error;
}

export function refusedBlock(url) {
  const link = url ? ` <a class="link" href="${esc(url)}">Details</a>` : "";
  return `<div class="pro">Answering and pushing are not enabled for your account on this hub.${link}</div>`;
}

export const COMMAND_TEXT = {
  "commands-off": "This hub doesn't take answers or pushes.",
  "plan-inactive": "Answering and pushing are not enabled for your account on this hub.",
  "unknown-machine": "This machine isn't on your account any more.",
  "queue-full": "Too much waits for that machine. Try again later.",
  "rate-limited": "Too many tries. Wait a minute.",
  "view-only": "View only on this device. Allow changes in Account.",
  "no-device": "This device has no key yet. Turn on Allow changes in Account.",
  "unknown-device": "The hub doesn't know this device. Turn Allow changes off and on in Account.",
  "device-removed": "This device was removed under Account. Send again to add it as a new device.",
  "too-many-devices": "Your account holds as many devices as this hub allows. Remove one you no longer use under Account, then send again.",
};

export const decides = (standing) => standing.state === "waiting" && standing.mine === "approved" && takes(standing.accepts, "device-decision");

export function decisionState(sent, now) {
  if (!sent) return null;
  if (sent.result) return sent.result.outcome === "done" ? "done" : "refused";
  return now >= sent.exp ? "expired" : "sending";
}

function decisionStatus(sent, state, { machine, name, now }) {
  const verb = sent.approve ? "Approval" : "Denial";
  const meta = `Sent ${from(sent)}`;
  if (state === "sending") return block("wait", `${verb} sent to ${machine}`, `${machine} ${sent.approve ? "approves" : "denies"} ${name} when it takes it.`, `${meta} · expires ${esc(until(sent.exp, now))}`);
  if (state === "done") return block("done", `✓ ${sent.approve ? "Approved" : "Denied"} on ${machine}`, "", meta);
  if (state === "expired") return block("refused", `Not delivered: ${machine} didn't take it within the hour`, `Try again when ${machine} is awake.`, `${meta} · expired ${esc(clock(sent.exp))}`);
  const reason = sent.result.reason;
  if (reason === "not-allowed") return block("refused", `${machine} takes approvals only at the machine`, `Approve or deny ${name} in kebbie on ${machine}.`, meta);
  if (reason === "not-approved") return block("refused", `${machine} hasn't approved this device yet`, `Approve this device in kebbie on ${machine} first, then decide here.`, meta);
  if (reason === "denied") return block("refused", `${machine} doesn't take changes from this device`, `Approve or deny ${name} in kebbie on ${machine}.`, meta);
  if (reason === "unknown-device") return block("refused", `${name} no longer waits on ${machine}`, `It was approved or denied already, or it asked again with a new key.`, meta);
  return block("refused", ...refusal("Not decided", sent.result.text), meta);
}

export function decisionBlock({ standing, device, sent = null, self = false, allowed = true, busy = false, now }) {
  if (standing.state !== "waiting") return "";
  const machine = esc(standing.machine);
  const name = esc(device.name);
  if (self) return `<div class="meta">Approve this device in kebbie on ${machine}.</div>`;
  if (!decides(standing)) {
    const why = standing.mine === "approved" ? `${machine} takes approvals only at the machine.` : `Approve or deny it in kebbie on ${machine}.`;
    return `<div class="meta">${why}</div>`;
  }
  const state = decisionState(sent, now);
  const status = state ? decisionStatus(sent, state, { machine, name, now }) : "";
  if (!allowed && !status) return `<div class="meta">Allow changes from this device to approve or deny ${name} here, or decide in kebbie on ${machine}.</div>`;
  const open = allowed && state !== "sending" && state !== "done";
  const off = busy ? " disabled" : "";
  const buttons = open ? `<div class="pair-answer"><button type="button" class="btn" data-action="decide-device" data-machine="${esc(standing.machineId)}" data-approve="false"${off}>Deny</button><button type="button" class="btn primary" data-action="decide-device" data-machine="${esc(standing.machineId)}" data-approve="true"${off}>Approve</button></div>` : "";
  return `<div class="decide">${status}${buttons}</div>`;
}
