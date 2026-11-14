const DAY = 86_400;
const ENDING_DAYS = 3;

const esc = (value) => String(value ?? "").replace(/[&<>"']/g, (char) => `&#${char.charCodeAt(0)};`);
const CHECK = `<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M5 12.5l4.5 4.5L19 7.5"/></svg>`;
export const EXTERNAL = `<svg class="ext" viewBox="0 0 24 24" aria-hidden="true"><path d="M14 5h5v5M19 5l-8 8M18 14v5H5V6h5"/></svg>`;

const GIVES = {
  work: ["Push, accept and comment from your phone", "Start tasks and talk to the lead", "Every machine you run, 90 days of history"],
  machine: ["Every machine you run, on one board", "Push, accept and comment from your phone", "90 days of history instead of 7"],
};

const ACTIONS = {
  push: { title: "Push", goes: "Your push goes ahead", back: "push", started: "this push" },
  "accept-subtask": { title: "Accepting", goes: "Your accept goes ahead", back: "accept", started: "this accept" },
  "accept-step": { title: "Accepting", goes: "Your accept goes ahead", back: "accept", started: "this accept" },
  "review-comments": { title: "Commenting on a review", goes: "Your comments go ahead", back: "send them", started: "these comments" },
  "review-send": { title: "Sending a review", goes: "The review goes ahead", back: "send it", started: "this review" },
  start: { title: "Starting a task", goes: "The task starts", back: "start it", started: "this task" },
  "start-item": { title: "Starting a task", goes: "The task starts", back: "start it", started: "this task" },
  "found-start": { title: "Starting a task", goes: "The task starts", back: "start it", started: "this task" },
  idea: { title: "Filing an idea", goes: "Your idea goes ahead", back: "file it", started: "this idea" },
  "item-edit": { title: "Editing an item", goes: "Your edit goes ahead", back: "save it", started: "this edit" },
  "found-add": { title: "Adding a finding to a task", goes: "It goes ahead", back: "add it", started: "this finding" },
  "found-keep": { title: "Keeping a finding", goes: "It goes ahead", back: "keep it", started: "this finding" },
  "found-dismiss": { title: "Dismissing a finding", goes: "It goes ahead", back: "dismiss it", started: "this finding" },
  "lead-message": { title: "Talking to the lead", goes: "Your message goes ahead", back: "send it", started: "this message" },
  away: { title: "Away", goes: "Away turns on", back: "turn it on", started: "away" },
  routine: { title: "Running a routine", goes: "The routine runs", back: "run it", started: "this routine" },
};

const actionOf = (kind) => ACTIONS[kind] ?? { title: "This", goes: "It goes ahead", back: "send it", started: "this" };

export const daysLeft = (until, now) => Math.max(1, Math.ceil((until - now) / DAY));

export function dateOf(seconds) {
  return new Date(seconds * 1000).toLocaleDateString("en-GB", { day: "numeric", month: "long" });
}

export const currentPlan = (plan, now) => (plan?.state === "trial" && plan.until <= now ? { ...plan, state: "expired", trial: true } : plan);

const entitled = (plan) => !plan || plan.state === "pro" || plan.state === "trial";

export const needsPro = (plan, kind) => !entitled(plan) && Array.isArray(plan.pro) && plan.pro.includes(kind);

export const machineNeedsPro = (plan, machines) => !entitled(plan) && typeof plan.machines === "number" && machines >= plan.machines;

export function proCard({ plan, kind, machine = null, busy = false, error = null }) {
  const action = actionOf(kind);
  const title = kind === "machine" ? "A second machine is part of Pro" : `${action.title} is part of Pro`;
  const gives = GIVES[kind === "machine" ? "machine" : "work"];
  const name = machine ?? "This machine";
  const days = plan?.trial_days ?? 14;
  const ended = plan?.state === "expired";
  const sub = ended ? `<p class="pro-sub">Your Pro ${plan.trial ? "trial" : "plan"} ended on ${esc(dateOf(plan.until))}.</p>` : "";
  const host = plan?.billing ? new URL(plan.billing, globalThis.location?.href ?? "https://hub.kebbie.dev").host : "the hub";
  const fine = ended
    ? `Pro is bought on ${esc(host)} in your browser, monthly or yearly. Come back and ${kind === "machine" ? `connect ${esc(name)}` : esc(action.back)}.`
    : `Free for ${days} days, then monthly or yearly. ${kind === "machine" ? `${esc(name)} connects` : esc(action.goes)} as soon as the trial starts.`;
  const primary = ended
    ? `<a class="btn primary fit" href="${esc(plan.billing ?? "#")}" target="_blank" rel="noopener" data-action="pro-continue">Continue with Pro${EXTERNAL}</a>`
    : `<button type="button" class="btn primary fit" data-action="pro-trial"${busy ? " disabled" : ""}>Try Pro free for ${days} days</button>`;
  return `<div class="pro-scrim" data-action="pro-close"></div><div class="pro-sheet" role="dialog" aria-modal="true" aria-labelledby="pro-title"><span class="grab" aria-hidden="true"></span><span class="pro-eyebrow">Hub Pro</span><h2 class="pro-title" id="pro-title">${esc(title)}</h2>${sub}<ul class="pro-gives">${gives.map((line) => `<li>${CHECK}<span>${esc(line)}</span></li>`).join("")}</ul><p class="pro-fine">${fine}</p>${error ? `<p class="pro-fine warn" role="alert">${esc(error)}</p>` : ""}<div class="pro-acts"><button type="button" class="btn fit" data-action="pro-close">Not now</button>${primary}</div></div>`;
}

export function trialLine({ kind, until, now }) {
  return `<div class="trial-line"><span class="chip trial">Pro trial · ${daysLeft(until, now)} days left</span><span>Your trial started with ${esc(actionOf(kind).started)}.</span></div>`;
}

export function endingChip(plan, now) {
  if (plan?.state !== "trial" || plan.until - now > ENDING_DAYS * DAY || !plan.billing) return "";
  const days = daysLeft(plan.until, now);
  return `<a class="chip trial" href="${esc(plan.billing)}" target="_blank" rel="noopener">Trial · ${days} ${days === 1 ? "day" : "days"} left · Buy</a>`;
}

function planChip(plan, now) {
  if (plan.state === "trial") {
    const days = daysLeft(plan.until, now);
    return `<span class="chip trial">Trial · ${days} ${days === 1 ? "day" : "days"} left</span>`;
  }
  if (plan.state === "expired") return `<span class="chip">${plan.trial ? "Trial ended" : "Ended"}</span>`;
  return `<span class="chip">${plan.state === "pro" ? "Pro" : "Free"}</span>`;
}

function planMeta(plan) {
  if (plan.state === "trial") return `Ends ${esc(dateOf(plan.until))}. After that the board stays readable, and pushing and the other Pro actions ask again.`;
  if (plan.state === "pro") return "Every machine you run, pushing and leading work from here, 90 days of history.";
  if (plan.state === "expired") return `Ended ${esc(dateOf(plan.until))}. The board stays readable, and answers go through as before.`;
  return `The board, answers and one machine. The first Pro action starts a ${plan.trial_days ?? 14}-day trial, no card needed.`;
}

export function planGroup(plan, now) {
  if (!plan) return "";
  const ending = plan.state === "trial" && plan.until - now <= ENDING_DAYS * DAY;
  const buy = plan.billing && (ending || plan.state === "expired") ? `<a class="task row-link" href="${esc(plan.billing)}" target="_blank" rel="noopener"><span class="name">Buy Pro</span><span class="meta">monthly or yearly</span>${EXTERNAL.replace('class="ext"', 'class="chev"')}</a>` : "";
  return `<h2 class="group g-muted">Plan</h2><section class="task plan-row"><div class="head"><span class="name">Hub Pro</span>${planChip(plan, now)}</div><div class="meta">${planMeta(plan)}</div></section>${buy}`;
}

export function placeSheet(root, scope = globalThis) {
  const sheet = root.querySelector(".pro-sheet");
  if (!sheet) return;
  const pane = root.querySelector("aside.panel");
  const classes = scope.document?.documentElement.classList;
  const halves = classes?.contains("pose-split") || classes?.contains("pose-land");
  const left = pane ? pane.getBoundingClientRect().left : halves ? scope.innerWidth / 2 : 0;
  sheet.style.left = `${Math.round(left)}px`;
}

export function showTrialLine(root, line, now) {
  const scope = root.querySelector("aside.panel") ?? root.querySelector("main.screen");
  if (!scope || !line) return;
  const receipts = scope.querySelectorAll(".sent");
  const html = trialLine({ ...line, now });
  if (receipts.length) receipts[receipts.length - 1].insertAdjacentHTML("afterend", html);
  else scope.insertAdjacentHTML("beforeend", html);
}
