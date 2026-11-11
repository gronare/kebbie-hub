import { esc } from "./screens.js";

export function clock(seconds) {
  return new Date(seconds * 1000).toLocaleTimeString("en-GB", { hour: "2-digit", minute: "2-digit" });
}

export function gate({ info, refused, device }) {
  if (!info?.commands) return "off";
  if (refused) return "refused";
  if (!device?.listed) return "setup";
  return "ready";
}

export function sentState(sent, { now, asleep }) {
  if (sent.result) return sent.result.outcome === "done" ? "done" : "refused";
  if (now >= sent.exp) return "expired";
  return asleep ? "waiting" : "sending";
}

export const settled = (state) => state === "expired" || state === "refused";

export function latestSent(sentList, { machine, address, kind, question = null, prompt = null }) {
  const mine = sentList.filter((one) => one.machine === machine && one.address === address && one.kind === kind && (question === null || one.question === question) && (prompt === null || one.prompt === prompt));
  return mine.sort((a, b) => b.at - a.at)[0] ?? null;
}

function refusal(fallback, text) {
  const said = String(text ?? "").trim();
  return said.toLowerCase().startsWith(fallback.toLowerCase()) ? [esc(said), ""] : [esc(fallback), esc(said)];
}

const from = (sent) => `from ${esc(sent.device)} at ${esc(clock(sent.at))}`;

function block(tone, title, body, meta) {
  return `<div class="sent ${tone}" role="status"><b>${title}</b>${body ? `<span>${body}</span>` : ""}<span class="meta">${meta}</span></div>`;
}

export function answerStatus(sent, state) {
  const machine = esc(sent.machineName);
  const said = sent.label ? `"${esc(sent.label)}"` : "your answer";
  if (state === "done") return block("done", `✓ Answered: ${esc(sent.label ?? "your own answer")}`, `${esc(sent.address)} has your answer and carries on.`, `${from(sent)} · the lead got a copy`);
  if (state === "refused") return block("refused", ...refusal("Not answered", sent.result.text), from(sent));
  if (state === "expired") return block("refused", `Not delivered: ${machine} stayed asleep past the hour`, `Answer again when ${machine} is awake.`, `Sent ${from(sent)} · expired ${esc(clock(sent.exp))}`);
  if (state === "waiting") return block("wait", `Waiting for ${machine}`, `Your answer ${said} goes to the task when ${machine} wakes up.`, `Sent ${from(sent)} · expires ${esc(clock(sent.exp))}`);
  return block("wait", `Sent to ${machine}`, `Your answer ${said} is on its way to the task.`, `Sent ${from(sent)} · expires ${esc(clock(sent.exp))}`);
}

export function pushStatus(sent, state) {
  const machine = esc(sent.machineName);
  const sha = esc(sent.short);
  if (state === "done") return block("done", `✓ Pushed ${sha} to main`, esc(sent.result.text), from(sent));
  if (state === "refused") return block("refused", ...refusal("Not pushed", sent.result.text), from(sent));
  if (state === "expired") return block("refused", `Not delivered: ${machine} stayed asleep past the hour`, `Push again when ${machine} is awake.`, `Sent ${from(sent)} · expired ${esc(clock(sent.exp))}`);
  if (state === "waiting") return block("wait", `Waiting for ${machine}`, `Your push of ${sha} goes out when ${machine} wakes up.`, `Sent ${from(sent)} · expires ${esc(clock(sent.exp))}`);
  return block("wait", `Sent to ${machine}`, `${machine} rebases ${sha} onto main and pushes.`, `Sent ${from(sent)} · expires ${esc(clock(sent.exp))}`);
}

export function promptStatus(sent, state) {
  const machine = esc(sent.machineName);
  const label = esc(sent.label);
  if (state === "done") return block("done", `✓ Chose: ${label}`, `${esc(sent.address)} carries on.`, `${from(sent)} · the lead got a copy`);
  if (state === "refused") return block("refused", ...refusal("Not chosen", sent.result.text), from(sent));
  if (state === "expired") return block("refused", `Not delivered: ${machine} didn't take it within ten minutes`, `A prompt on the screen doesn't wait. If it still asks, choose again.`, `Sent ${from(sent)} · expired ${esc(clock(sent.exp))}`);
  if (state === "waiting") return block("wait", `Waiting for ${machine}`, `"${label}" is chosen if ${machine} wakes within ten minutes and the prompt still shows.`, `Sent ${from(sent)} · expires ${esc(clock(sent.exp))}`);
  return block("wait", `Sent to ${machine}`, `${machine} chooses "${label}" if the prompt still shows.`, `Sent ${from(sent)} · expires ${esc(clock(sent.exp))}`);
}

export const takesPromptAnswers = (found) => Array.isArray(found?.accepts) && found.accepts.includes("prompt-answer");

export const choosable = (option) => Boolean(option) && !option.takes_text && typeof option.key === "string" && option.key !== "";

function promptCard(option, index, { picked, pickable }) {
  const press = pickable && choosable(option);
  const classes = ["opt", press ? "pick" : "", picked === index ? "picked" : ""].filter(Boolean).join(" ");
  const attributes = press ? ` role="button" tabindex="0" data-action="pick" data-index="${index}" aria-pressed="${picked === index}"` : "";
  const meaning = choosable(option) ? "" : "answer at the machine";
  return `<div class="${classes}"${attributes}><b>${esc(option.label)}</b>${meaning ? `<small>${meaning}</small>` : ""}</div>`;
}

export function promptAnswerBlock({ prompt, ui, sent, state }) {
  const live = sent && !settled(state);
  const sentAt = live ? prompt.options.findIndex((option) => option.key === sent.key) : -1;
  const picked = live ? (sentAt < 0 ? null : sentAt) : ui.picked;
  const cards = prompt.options.map((option, index) => promptCard(option, index, { picked, pickable: !live && !ui.busy }));
  const options = `<div class="options${live || picked !== null ? " chosen" : ""}">${cards.join("")}</div>`;
  const status = sent ? promptStatus(sent, state) : "";
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

export function answerBlock({ key, question, ui, sent, state }) {
  const live = sent && !settled(state);
  const picked = live ? (sent.choice ?? null) : ui.picked;
  const chosen = live || picked !== null || ui.free;
  const cards = question.options.map((option, index) => optionCard(option, index, { recommend: question.recommend, picked, pickable: !live && !ui.busy }));
  const options = `<div class="options${chosen ? " chosen" : ""}">${cards.join("")}</div>`;
  const status = sent ? answerStatus(sent, state) : "";
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

export function pushBlock({ item, machineName, ui, sent, state }) {
  const short = item.head.slice(0, 7);
  const commits = item.git?.ahead ?? 0;
  const count = `${commits} ${commits === 1 ? "commit" : "commits"}`;
  const live = sent && !settled(state);
  const status = sent ? pushStatus(sent, state) : "";
  if (live) return status;
  const error = ui.error ? `<div class="center warn" role="alert">${esc(ui.error)}</div>` : "";
  if (ui.confirming) {
    const reviewed = item.verdict?.verdict === "ok" && !item.verdict.stale ? ", reviewer passed" : "";
    const busy = ui.busy ? " disabled" : "";
    return status + `<div class="confirm" role="group" aria-label="Confirm push"><p><b>Push ${esc(short)} to main?</b></p><p>${esc(count)}${reviewed}. ${esc(machineName)} rebases onto the latest main first; on a conflict its Claude resolves it before pushing.</p><div class="row2"><button type="button" class="btn" data-action="push-cancel"${busy}>Cancel</button><button type="button" class="btn primary" data-action="push"${busy}>Push</button></div></div>` + error;
  }
  return status + `<button type="button" class="btn primary" data-action="push-ask">Push ${esc(count)} · ${esc(short)}</button>` + error;
}

export function setupBlock({ busy = false, error = null } = {}) {
  const warn = error ? `<div class="center warn" role="alert">${esc(error)}</div>` : "";
  return `<div class="pro setup"><span>Answer questions and approve pushes from here. Your machines trust this <span class="phone-only">phone</span><span class="desk-only">browser</span> once you confirm with your passkey.</span><button type="button" class="btn primary" data-action="enable-device"${busy ? " disabled" : ""}><span>Let this <span class="phone-only">phone</span><span class="desk-only">browser</span> answer and push</span></button></div>${warn}`;
}

export function refusedBlock(url) {
  const link = url ? ` <a class="link" href="${esc(url)}">Details</a>` : "";
  return `<div class="pro">Answering and pushing are not enabled for your account on this hub.${link}</div>`;
}

export const COMMAND_TEXT = {
  "commands-off": "This hub doesn't take answers or pushes.",
  "plan-inactive": "Answering and pushing are not enabled for your account on this hub.",
  "unknown-machine": "This machine doesn't trust this device yet. Let it answer and push again.",
  "queue-full": "Too much waits for that machine. Try again later.",
  "rate-limited": "Too many tries. Wait a minute.",
  "unsigned-device-list": "The hub's list of your devices is not signed by your account. Nothing was changed.",
  "rolled-back-device-list": "The hub served an older list of your devices than this device has seen. Nothing was changed.",
  "stale-list": "Another device changed your devices at the same time. Try again.",
  "unknown-device": "The hub doesn't hold this device's keys. Nothing was changed.",
};

export function deviceRows(list, mine) {
  if (list.devices.length === 0) return `<div class="center">No device answers or pushes yet.</div>`;
  return list.devices
    .map((listed) => {
      const facts = [listed.role === "control" ? "answers and pushes" : "reads", listed.device === mine ? "this device" : null].filter(Boolean).join(" · ");
      return `<div class="mrow"><div class="about"><b>${esc(listed.name)}</b><small>${esc(facts)}</small></div><button type="button" class="btn small" data-action="remove-device" data-id="${esc(listed.device)}" data-name="${esc(listed.name)}">Remove</button></div>`;
    })
    .join("");
}
