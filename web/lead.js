import { isAsleep } from "./board.js";
import { clock, sentState } from "./decide.js";

const esc = (value) => String(value ?? "").replace(/[&<>"']/g, (char) => `&#${char.charCodeAt(0)};`);
const svg = (cls, body) => `<svg class="${cls}" viewBox="0 0 24 24" aria-hidden="true">${body}</svg>`;
const CHEV = svg("chev", `<path d="M7 10l5 5 5-5"/>`);
const SEND = svg("send-i", `<path d="M12 19V5M6 11l6-6 6 6"/>`);
const DESK = svg("src-i", `<rect x="4" y="5" width="16" height="11" rx="1.5"/><path d="M2.5 19h19"/>`);
const PHONE = svg("src-i", `<rect x="7" y="3" width="10" height="18" rx="2"/><path d="M11 18h2"/>`);
const CLOCK = svg("src-i", `<circle cx="12" cy="12" r="8.5"/><path d="M12 7.5V12l3 2"/>`);
const CHECK = svg("chk", `<path d="M5 12.5l4.5 4.5L19 7.5"/>`);
const WEEK = 7 * 86_400;

export const leadHref = (machine) => `#/lead/${encodeURIComponent(machine)}`;

export const takesLead = (board) => Boolean(board?.lead) || (board?.accepts ?? []).includes("lead-message");

export const leadMachines = (opened, now = Date.now() / 1000) =>
  opened.filter((machine) => machine.board && !machine.problem && takesLead(machine.board)).sort((a, b) => Number(asleepNow(a, now)) - Number(asleepNow(b, now)));

export const asleepNow = (machine, now) => Boolean(machine.expired) || isAsleep({ lastSeen: machine.lastSeen ?? null }, now);

export function chosenLead(machines, wanted, now) {
  return machines.find((machine) => machine.machine === wanted) ?? machines.find((machine) => !asleepNow(machine, now)) ?? machines[0] ?? null;
}

const sameDay = (a, b) => new Date(a * 1000).toDateString() === new Date(b * 1000).toDateString();

export function dayLabel(seconds, now) {
  if (sameDay(seconds, now)) return "Today";
  if (sameDay(seconds, now - 86_400)) return "Yesterday";
  return new Date(seconds * 1000).toLocaleDateString("en-GB", { weekday: "short", day: "numeric", month: "short" });
}

export function shortWhen(seconds, now) {
  if (!seconds) return "";
  if (sameDay(seconds, now)) return clock(seconds);
  if (sameDay(seconds, now - 86_400)) return "Yesterday";
  return new Date(seconds * 1000).toLocaleDateString("en-GB", { day: "numeric", month: "short" });
}

export function leadStatus(machine, now) {
  if (asleepNow(machine, now)) return `asleep since ${sameDay(machine.lastSeen ?? 0, now) ? clock(machine.lastSeen) : shortWhen(machine.lastSeen, now) || "a while"}`;
  return machine.board?.lead?.working_since ? "working" : "idle";
}

export function unseenReplies(machines, seenOf) {
  return machines.filter((machine) => (machine.board?.lead?.replied_at ?? 0) > (seenOf(machine.machine) ?? 0)).length;
}

export function threadOf(lead, peek, depth = 0) {
  if (!lead) return { turns: [], older: null, missing: null, failed: false };
  const pages = [];
  let ref = lead.thread;
  for (let index = 0; index <= depth && ref; index++) {
    const entry = peek(ref.id);
    if (!entry) return { turns: pages.flatMap((page) => page.turns), older: null, missing: ref, failed: false };
    if (!entry.lead) return { turns: pages.flatMap((page) => page.turns), older: null, missing: null, failed: true };
    pages.unshift(entry.lead);
    ref = entry.lead.older;
  }
  return { turns: pages.flatMap((page) => page.turns), older: ref ?? null, missing: null, failed: false };
}

export function pendingMessages(sentList, machine, turnIds, now) {
  return sentList
    .filter((sent) => sent.kind === "lead-message" && sent.machine === machine.machine && !turnIds.has(sent.id) && now - sent.at < WEEK + 86_400)
    .map((sent) => ({ sent, state: sentState(sent, { now, asleep: asleepNow(machine, now) }) }))
    .sort((a, b) => a.sent.at - b.sent.at);
}

export function awayOn(sentList, opened) {
  const latest = sentList.filter((sent) => sent.kind === "away").sort((a, b) => b.at - a.at)[0];
  const boards = opened.filter((machine) => machine.board);
  const applied = (machine) => (latest.on ? Boolean(machine.board.away_since) : !machine.board.away_since);
  if (latest && boards.some((machine) => machine.machine === latest.machine && !applied(machine))) return Boolean(latest.on);
  return boards.some((machine) => Boolean(machine.board.away_since));
}

const firstLine = (text) => String(text ?? "").split("\n").map((line) => line.replace(/^[#>*\-\s]+/, "").replace(/`|\*\*|__/g, "").trim()).find(Boolean) ?? "";

export function previewOf(turns, pending) {
  const lastSent = pending.at(-1)?.sent;
  const last = turns.at(-1);
  if (lastSent && (!last || lastSent.at >= last.at)) return `You: ${firstLine(lastSent.text)}`;
  if (!last) return "";
  if (last.by === "lead") return firstLine(last.text);
  return last.away === undefined ? `You: ${firstLine(last.text)}` : `Away mode ${last.away ? "on" : "off"}`;
}

const from = (device, nameOf) => (device ? `${PHONE}from ${esc(nameOf(device))}` : `${DESK}at the computer`);

function yourTurn(turn, nameOf) {
  const delivery = turn.delivery;
  if (delivery?.state === "waiting-turn") return mine(turn.text, `<div class="msg-meta wait">${CLOCK}Waiting for the lead to finish its turn</div>`);
  if (delivery?.state === "waiting-draft") return mine(turn.text, `<div class="msg-meta wait">${CLOCK}Waiting: a draft is in the lead's input</div>`);
  const delivered = delivery?.state === "delivered" ? ` · delivered ${esc(clock(delivery.at))}` : "";
  return mine(turn.text, `<div class="msg-meta">${from(turn.device, nameOf)} · ${esc(clock(turn.at))}${delivered}</div>`);
}

const mine = (text, meta) => `<div class="msg mine"><div class="bubble">${esc(text)}</div>${meta}</div>`;

const leadTurn = (turn) => `<div class="msg theirs"><div class="bubble md" data-markdown>${esc(turn.text)}</div><div class="msg-meta">Lead · ${esc(clock(turn.at))}</div></div>`;

const awayTurn = (turn, nameOf) => `<div class="sys"><b>Away mode ${turn.away ? "on" : "off"}</b> · ${turn.device ? `from ${esc(nameOf(turn.device))}` : "at the computer"} · ${esc(clock(turn.at))}</div>`;

function pendingTurn({ sent, state }, machine) {
  const name = esc(machine.name);
  if (state === "waiting") return mine(sent.text, `<div class="msg-meta wait">${CLOCK}Waiting for ${name} to wake</div>`);
  if (state === "done") return mine(sent.text, `<div class="msg-meta">${PHONE}from ${esc(sent.deviceName ?? "this device")} · ${esc(clock(sent.at))} · delivered</div>`);
  if (state === "expired") return mine(sent.text, `<div class="msg-meta refused">Not delivered: ${name} stayed asleep for a week</div>`);
  if (state === "refused" || state === "not-approved" || state === "denied") return mine(sent.text, `<div class="msg-meta refused">Not delivered: ${esc(sent.result?.text || (state === "refused" ? "the machine refused it" : `${machine.name} didn't approve this device`))}</div>`);
  if (state === "approval") return mine(sent.text, `<div class="msg-meta wait">${CLOCK}Waiting for ${name} to approve this device</div>`);
  return mine(sent.text, `<div class="msg-meta">${PHONE}from ${esc(sent.deviceName ?? "this device")} · ${esc(clock(sent.at))} · sending…</div>`);
}

export function threadHtml({ machine, thread, pending, now, nameOf }) {
  const items = [...thread.turns.map((turn) => ({ at: turn.at, turn })), ...pending.map((one) => ({ at: one.sent.at, one }))].sort((a, b) => a.at - b.at);
  const parts = [];
  if (thread.older) parts.push(`<button type="button" class="btn fit older" data-action="lead-older">Earlier messages</button>`);
  let day = null;
  for (const item of items) {
    const label = dayLabel(item.at, now);
    if (label !== day) parts.push(`<div class="day">${esc(label)}</div>`);
    day = label;
    if (item.one) parts.push(pendingTurn(item.one, machine));
    else if (item.turn.by === "lead") parts.push(leadTurn(item.turn));
    else if (item.turn.away !== undefined) parts.push(awayTurn(item.turn, nameOf));
    else parts.push(yourTurn(item.turn, nameOf));
  }
  if (thread.missing) parts.push(`<div class="sys">Opening the thread…</div>`);
  if (thread.failed) parts.push(`<div class="sys">The thread could not be opened here; it is still on ${esc(machine.name)}.</div>`);
  if (items.length === 0 && !thread.missing && !thread.failed) parts.push(`<div class="sys">No messages with ${esc(machine.name)}'s lead yet.</div>`);
  if (machine.board.lead?.working_since && !asleepNow(machine, now)) parts.push(`<div class="working"><span class="spinner"></span>Working…</div>`);
  if (pending.some((one) => one.state === "waiting")) parts.push(`<div class="sent wait waiting" role="status"><b>${CLOCK}Held in the hub</b><span>${esc(machine.name)} has been asleep since ${esc(shortWhen(machine.lastSeen, now) || "a while")}. The lead gets your message when the machine wakes.</span></div>`);
  return `<div class="thread">${parts.join("")}</div>`;
}

export const awaySwitch = (on) => `<label class="away-mini"><span>Away</span><input type="checkbox" class="switch" name="away" data-action="lead-away"${on ? " checked" : ""} aria-label="Away mode"></label>`;

export const awayNote = `<div class="away-on"><span><b>Away mode is on.</b> Leads settle questions about method themselves and keep to what you agreed. Questions about what you want wait in Questions.</span></div>`;

const statusLine = (machine, now) => `<span class="pick-s"><span class="dot${asleepNow(machine, now) ? "" : " awake"}"></span>Lead · ${esc(leadStatus(machine, now))}</span>`;

export function pickRow(machine, { now, selected, when, preview = null, unseen = false, pop = false }) {
  const right = pop && selected ? CHECK : `<span class="t">${esc(when)}</span>`;
  const badge = unseen ? `<em class="badge" aria-label="new reply">1</em>` : "";
  const cls = pop ? `mpick${selected ? " on" : ""}` : `task mpick${selected ? " sel" : ""}`;
  return `<a class="${cls}" href="${esc(leadHref(machine.machine))}"${selected ? ' aria-current="true"' : ""}><span class="dot${asleepNow(machine, now) ? "" : " awake"}"></span><span class="n">${esc(machine.name)}${badge}</span>${right}<span class="s">Lead · ${esc(leadStatus(machine, now))}</span>${preview === null ? "" : `<span class="p">${esc(preview)}</span>`}</a>`;
}

export function leadHead(machine, { now, away, picking, canPick }) {
  const pick = canPick
    ? `<button type="button" class="lead-pick${picking ? " open" : ""}" data-action="lead-pick" aria-expanded="${picking}"><span class="pick-t">${esc(machine.name)}${CHEV}</span>${statusLine(machine, now)}</button>`
    : `<div class="lead-pick"><span class="pick-t">${esc(machine.name)}</span>${statusLine(machine, now)}</div>`;
  return `<div class="lead-head"><div class="lead-row">${pick}${awaySwitch(away)}</div>${away ? awayNote : ""}</div>`;
}

export const panelHead = (machine, now) => `<div class="lead-head"><h2>${esc(machine.name)}</h2>${statusLine(machine, now)}</div>`;

export function composerHtml(machine, { draft, busy, error, takes }) {
  if (!takes) return `<div class="composer off" role="note">${esc(machine.name)}'s kebbie can't take messages from the app yet.</div>`;
  const problem = error ? `<div class="composer-error" role="alert">${esc(error)}</div>` : "";
  return `<div class="composer">${problem}<textarea id="lead-say" rows="1" placeholder="Message the lead" aria-label="Message the lead on ${esc(machine.name)}" enterkeyhint="send"${busy ? " disabled" : ""}>${esc(draft)}</textarea><button type="button" class="send" data-action="lead-send" aria-label="Send"${busy || draft.trim() === "" ? " disabled" : ""}>${SEND}</button></div>`;
}

export const popHtml = (rows) => `<div class="pop-scrim" data-action="lead-pick"></div><div class="pop" role="menu">${rows.join("")}</div>`;
