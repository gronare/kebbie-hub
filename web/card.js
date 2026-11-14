import { foundStatus } from "./backlog.js";
import { clock, settled, speaksFor, takes } from "./decide.js";
import { esc } from "./screens.js";

export const DECIDERS = { you: "you", lead: "lead", investigator: "investigator", task: "task" };
const SOURCES = { question: "question", method: "question of method", obstacle: "obstacle", plan: "approved with the plan", message: "message", "through-lead": "through the lead" };
const CHOSEN = { subtask: "Added as a subtask", task: "Started as a task", kept: "Kept in the backlog" };
export const NOTICED_ACTIONS = [
  { action: "card-subtask", kind: "found-add", label: "Add as a subtask" },
  { action: "card-task", kind: "found-start", label: "New task" },
  { action: "card-keep", kind: "found-keep", label: "Keep in backlog" },
];
export const NOTICED_KINDS = NOTICED_ACTIONS.map((one) => one.kind);

export const inline = (text) => esc(text).replace(/`([^`]+)`/g, "<code>$1</code>");

export function cardChanges(card) {
  const previous = card.previous;
  if (!previous) return [];
  const parts = [
    ["Built", previous.built, card.built],
    ["Decisions", previous.decisions.map((one) => one.text), card.decisions.map((one) => one.text)],
    ["Also noticed", previous.noticed.map((one) => one.text), card.noticed.map((one) => one.text)],
  ];
  return parts.flatMap(([part, before, after]) => [...before.filter((line) => !after.includes(line)).map((text) => ({ part, mark: "−", text })), ...after.filter((line) => !before.includes(line)).map((text) => ({ part, mark: "+", text }))]);
}

export function changedNotice(card, seen) {
  if (!card.previous || (seen ?? 0) >= card.version) return "";
  const changes = cardChanges(card);
  const when = card.changed_at ? ` at ${esc(clock(card.changed_at))}` : "";
  const summary = ["Built", "Decisions", "Also noticed"].map((part) => {
    const lines = changes.filter((one) => one.part === part);
    const added = lines.filter((one) => one.mark === "+").length;
    const removed = lines.length - added;
    return lines.length ? `${part}: ${[added ? `${added} new` : "", removed ? `${removed} gone` : ""].filter(Boolean).join(", ")}` : "";
  }).filter(Boolean).join(" · ");
  const lines = changes.map((one) => `<div class="change ${one.mark === "+" ? "added" : "removed"}"><span class="mark">${one.mark}</span><span><small>${esc(one.part)}</small> ${inline(one.text)}</span></div>`).join("");
  const what = changes.length ? `<span>${esc(summary)}</span><details class="card-diff"><summary>What changed</summary>${lines}</details>` : `<span>Its lines read the same; a choice or an order moved.</span>`;
  return `<div class="sent wait card-changed" role="status"><b>The card changed${when} · version ${esc(card.previous.version)} → ${esc(card.version)}</b>${what}<button type="button" class="btn small" data-action="card-seen" data-version="${esc(card.version)}">Seen</button></div>`;
}

function decisionRow(decision, card) {
  const replacement = decision.replaced_by === undefined || decision.replaced_by === null ? null : card.decisions[decision.replaced_by];
  const meta = [decision.source ? SOURCES[decision.source] : "", decision.at ? clock(decision.at) : ""].filter(Boolean).map(esc).join(" · ");
  const replaced = replacement ? `<div class="meta">replaced by: ${inline(replacement.text)}</div>` : "";
  return `<div class="dec${replacement ? " replaced" : ""}"><div class="dec-text">${inline(decision.text)}</div><div class="dec-meta"><span class="who who-${esc(decision.by)}">${esc(DECIDERS[decision.by])}</span>${meta ? ` <span class="meta">${meta}</span>` : ""}</div>${replaced}</div>`;
}

export function noticedArea(line, { access, accepts, sent = null, busy = false, error = null, now, recurring = 0 }) {
  const seen = recurring >= 2 ? `<div class="meta"><span class="g-yellow">seen by ${esc(recurring)} tasks</span></div>` : "";
  if (!line.found) return seen;
  if (line.chosen) return `${seen}<div class="meta">${esc(CHOSEN[line.chosen])}</div>`;
  const status = sent ? foundStatus(sent.sent, sent.state, { standing: sent.standing, now }) : "";
  if (sent && !settled(sent.state)) return seen + status;
  if (access !== "ready") return seen + status;
  const offered = NOTICED_ACTIONS.filter((one) => takes(accepts, one.kind));
  if (!offered.length) return seen + status;
  const buttons = offered.map((one) => `<button type="button" class="btn" data-action="${one.action}" data-item="${esc(line.found)}"${busy ? " disabled" : ""}>${esc(one.label)}</button>`).join("");
  const failed = error ? `<div class="center warn" role="alert">${esc(error)}</div>` : "";
  return `${seen}${status}<div class="row-btns found-acts noticed-acts">${buttons}</div>${failed}`;
}

export function cardParts(card, { noticed = () => "" } = {}) {
  const built = card.built.length ? `<h3 class="sec">Built</h3><div class="md card-built">${card.built.map((line) => `<p>${inline(line)}</p>`).join("")}</div>` : "";
  const decisions = card.decisions.length ? `<h3 class="sec">Decisions · ${card.decisions.length}</h3><div class="decs">${card.decisions.map((one) => decisionRow(one, card)).join("")}</div>` : "";
  const lines = card.noticed.map((line, index) => `<div class="noticed"><div class="dec-text">${inline(line.text)}</div>${noticed(line, index)}</div>`).join("");
  const seen = card.noticed.length ? `<h3 class="sec">Also noticed · ${card.noticed.length}</h3><div class="decs">${lines}</div>` : "";
  return built + decisions + seen;
}

export function assessmentBlock(item, { access, accepts, sent: maybe = null, busy = false, error = null, status = () => "" }) {
  const judged = item.assessment;
  if (!judged || typeof judged === "string" || judged.word !== "recommended") return "";
  const when = judged.at ? `<div class="meta">assessed ${esc(clock(judged.at))}</div>` : "";
  const box = `<div class="assess"><b>A full review is recommended</b><span>${inline(judged.reason)}</span>${when}</div>`;
  if (item.verdict?.running) return `${box}<div class="meta pad">The full review is running.</div>`;
  const sent = maybe && speaksFor(maybe.sent, maybe.state, (one) => (one.assessed ?? null) === (judged.at ?? null)) ? maybe : null;
  const shown = sent ? status(sent) : "";
  if (sent && !settled(sent.state)) return box + shown;
  if (access !== "ready" || !takes(accepts, "review-send")) return box + shown;
  const failed = error ? `<div class="center warn" role="alert">${esc(error)}</div>` : "";
  return `${box}${shown}<button type="button" class="btn primary fit" data-action="review-send"${busy ? " disabled" : ""}>Send to review</button>${failed}`;
}
