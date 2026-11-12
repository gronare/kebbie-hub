import assert from "node:assert/strict";
import { test } from "node:test";

import { backlogList, chooseMachine, ideaEntries, ideaForm, itemBody, listAction, mergeBacklog, startsAnyway } from "./backlog.js";

const text = (html) => html.replace(/<span[^>]*aria-hidden="true"[^>]*>[^<]*<\/span>/g, " ").replace(/<[^>]+>/g, " ").replace(/&#(\d+);/g, (_, code) => String.fromCharCode(Number(code))).replace(/\s+/g, " ").trim();
const top = (title, aside) => `<h1>${title}</h1><span>${aside}</span>`;
const NOW = 1_762_956_540;
const DAY = 86_400;
const ALL = ["answer", "push", "idea", "item-answer", "start-item"];

const row = (item, fields = {}) => ({ item, title: item, priority: "medium", repo: "kebbie", state: "ready", filed: NOW - DAY, home: { kind: item.startsWith("vault:") ? "vault" : "local", label: item.split(":")[1] }, questions: 0, blocking: 0, goal: null, body: null, ...fields });
const machine = (id, name, backlog, { lastSeen = NOW - 20, accepts = ALL, repos = ["kebbie"] } = {}) => ({ machine: id, name, lastSeen, storedAt: lastSeen, board: { accepts, backlog, checkouts: repos.map((repo) => ({ repo, free_worktrees: 1, busy_worktrees: 0 })), items: [] } });

const session = row("vault:Projects/session.md", { title: "Session drops on reload", priority: "high", repo: "greenhouse", state: "drafting", questions: 3, blocking: 2, body: { id: "blb_1", key: "k", media_type: "application/vnd.kebbie.backlog-item+json", bytes: 900 } });
const body = {
  background: "Users are signed out on reload.",
  goal: "You stay signed in.",
  what_we_know: "- The cookie lasts ten minutes.",
  questions: [
    { text: "Should a reload renew the session?", blocks: true, pending_answer: null },
    { text: "Does a password change sign out every device?", blocks: true, pending_answer: null },
    { text: "Is 14 days right?", blocks: false, pending_answer: null },
  ],
  out_of_scope: "The operator app.",
  source: "From the board.",
};
const opened = [
  machine("mch_mac", "MacBook", [session, row("local:faster", { title: "Faster board", goal: "The board shows the push within a second." }), row("local:logs", { title: "Archive logs", state: "to-write", filed: NOW - 7200 })], { repos: ["kebbie"] }),
  machine("mch_work", "work-mac", [session, row("vault:Projects/round.md", { title: "Round invoices", priority: "high", repo: "billing" })], { repos: ["greenhouse", "billing"] }),
];
const ui = (fields = {}) => ({ picked: null, text: "", busy: false, error: null, choosing: false, machine: null, visit: NOW - 60, ...fields });
const sentAt = (fields) => ({ id: "cmd_1", machine: "mch_work", machineName: "work-mac", device: "dev_1", deviceName: "iPhone 16", at: NOW - 120, exp: NOW - 120 + 3600, result: null, ...fields });

test("a vault note seen on two machines is one row with the vault chip, and the list groups by priority with its questions counted", () => {
  const entries = mergeBacklog(opened, NOW);
  assert.equal(entries.length, 4);
  const shared = entries.find((entry) => entry.key === "vault:Projects/session.md");
  assert.deepEqual(shared.machines.map((one) => one.name), ["MacBook", "work-mac"]);
  const said = text(backlogList({ entries, now: NOW, top }));
  assert.match(said, /^Backlog 4 items · 2 questions wait High · 2/);
  assert.match(said, /Session drops on reload vault greenhouse · drafting · 2 questions · 1 d you: answer them on its card/);
  assert.match(said, /Faster board MacBook kebbie · 1 d The board shows the push within a second\./);
  assert.match(said, /Archive logs MacBook kebbie · to write · 2 h the lead writes it out/);
  assert.ok(said.indexOf("High") < said.indexOf("Medium"));
});

test("a start goes to the machine that has the repo, else the one used last, and can be changed", () => {
  const shared = mergeBacklog(opened, NOW).find((entry) => entry.vault && entry.row.repo === "greenhouse");
  assert.equal(chooseMachine(shared.machines, { repo: "greenhouse" }).name, "work-mac");
  assert.equal(chooseMachine(shared.machines, { repo: "nowhere", lastUsed: "mch_mac" }).why, "you used it last");
  assert.equal(chooseMachine(shared.machines, { repo: "greenhouse", chosen: "mch_mac" }).name, "MacBook");
  const html = itemBody(shared, { body, access: "ready", ui: ui({ choosing: true }), candidates: shared.machines, chosen: chooseMachine(shared.machines, { repo: "greenhouse" }), now: NOW });
  assert.match(text(html), /Starts on work-mac it has greenhouse Done MacBook awake work-mac awake Save to backlog Start The lead on work-mac picks the recipe and queues it\./);
});

test("the card shows its facts, its sections and its questions, blocking ones marked, and which machines see it", () => {
  const shared = mergeBacklog(opened, NOW)[0];
  const chosen = chooseMachine(shared.machines, { repo: "greenhouse" });
  const said = text(itemBody(shared, { body, access: "ready", ui: ui(), candidates: shared.machines, chosen, now: NOW }));
  assert.match(said, /priority high repo greenhouse state drafting: 2 questions wait on you filed \d+ Nov note Projects\/session\.md · seen on MacBook and work-mac/);
  assert.match(said, /Background Users are signed out on reload\. Goal You stay signed in\. What we know - The cookie lasts ten minutes\. Open questions 2 wait on you before it starts/);
  assert.match(said, /Is 14 days right\? the work can settle it Out of scope/);
  const html = itemBody(shared, { body, access: "ready", ui: ui(), candidates: shared.machines, chosen, now: NOW });
  assert.equal((html.match(/data-action="pick-question"/g) ?? []).length, 3);
  assert.equal((html.match(/qmark warn/g) ?? []).length, 2);
});

test("an answer sent shows with the lead, and once the body carries it the question stops counting", () => {
  const shared = mergeBacklog(opened, NOW)[0];
  const chosen = chooseMachine(shared.machines, { repo: "greenhouse" });
  const picked = itemBody(shared, { body, access: "ready", ui: ui({ picked: 0, text: "Renew on every request" }), candidates: shared.machines, chosen, now: NOW });
  assert.match(text(picked), /Your answer Renew on every request Send to the lead/);
  const done = { sent: sentAt({ kind: "item-answer", index: 0, text: "Renew on every request", result: { outcome: "done", reason: null, text: "ok", at: NOW - 60 } }), state: "done", standing: null, live: true };
  const sentFor = (index) => (index === 0 ? done : null);
  const answered = text(itemBody(shared, { body, access: "ready", ui: ui(), sentFor, candidates: shared.machines, chosen, now: NOW }));
  assert.match(answered, /state drafting: 1 question waits on you · 1 answer with the lead/);
  assert.match(answered, /With the lead: "Renew on every request" The lead folds your answer into the item and takes the question out\. from iPhone 16 at \d\d:\d\d · on work-mac/);
  const folded = { ...body, questions: body.questions.map((question, index) => (index === 0 ? { ...question, pending_answer: "Renew on every request" } : question)) };
  const html = itemBody(shared, { body: folded, access: "ready", ui: ui(), candidates: shared.machines, chosen, now: NOW });
  assert.equal((html.match(/data-action="pick-question"/g) ?? []).length, 2, "a question with the lead is not picked again");
  assert.match(text(html), /1 waits on you before it starts/);
});

test("a start refused for open questions says how many, and a second Start in the same visit goes anyway", () => {
  const shared = mergeBacklog(opened, NOW)[0];
  const chosen = chooseMachine(shared.machines, { repo: "greenhouse" });
  const start = { sent: sentAt({ kind: "start-item", at: NOW - 30, exp: NOW - 30 + DAY, result: { outcome: "refused", reason: "questions-open", blocking: 2, text: "Not started: 2 questions wait on you.", at: NOW - 20 } }), state: "refused", standing: null, live: false };
  const said = text(itemBody(shared, { body, access: "ready", ui: ui(), start, candidates: shared.machines, chosen, now: NOW }));
  assert.match(said, /Not started: 2 questions wait on you\. Answer them on its card, or press Start again to start anyway\. .* Save to backlog Start/);
  assert.equal(startsAnyway(start, NOW - 60), true);
  assert.equal(startsAnyway(start, NOW), false, "a refusal from an earlier visit does not start it anyway");
  assert.equal(startsAnyway({ ...start, sent: { ...start.sent, result: { ...start.sent.result, reason: "already-started" } } }, NOW - 60), false);
});

test("a start waiting for an asleep machine, one waiting for this device's approval and one started each say so", () => {
  const shared = mergeBacklog(opened, NOW)[0];
  const chosen = chooseMachine(shared.machines, { repo: "greenhouse" });
  const card = (state, fields = {}, standing = null) => text(itemBody(shared, { body, access: "ready", ui: ui(), start: { sent: sentAt({ kind: "start-item", exp: NOW - 120 + DAY, ...fields }), state, standing, live: state !== "refused" }, candidates: shared.machines, chosen, now: NOW, taskHref: "#/task/mch_work/session@greenhouse" }));
  assert.match(card("waiting"), /Waiting for work-mac The lead starts Session drops on reload when work-mac wakes up\. Sent from iPhone 16 at \d\d:\d\d · goes out when it wakes · expires in 23 h$/);
  assert.match(card("approval", {}, { state: "waiting", fingerprint: "AB", until: NOW + 23 * 3600 }), /Waiting for work-mac to approve this iPhone 16 Approve it in kebbie, or it is approved automatically in 23 h\. Your start goes out if work-mac approves before/);
  const started = itemBody(shared, { body, access: "ready", ui: ui(), start: { sent: sentAt({ kind: "start-item", result: { outcome: "done", reason: null, text: "Started session@greenhouse: the lead chose the recipe small-fix and queued it.", at: NOW } }), state: "done", standing: null, live: true }, candidates: shared.machines, chosen, now: NOW, taskHref: "#/task/mch_work/session@greenhouse" });
  assert.match(text(started), /✓ Started Session drops on reload Started session@greenhouse: the lead chose the recipe small-fix and queued it\. .* Open the task$/);
  assert.match(started, /href="#\/task\/mch_work\/session@greenhouse"/);
  assert.doesNotMatch(started, /data-action="start-item"/);
});

test("behind the plan gate the backlog and its cards read, with Hub Pro lines where the actions would be", () => {
  const entries = mergeBacklog(opened, NOW);
  const list = text(backlogList({ entries, now: NOW, top, action: listAction({ access: "refused", refusedUrl: "/plan", ideaMachines: [{}] }) }));
  assert.match(list, /Hub Pro Adding ideas and starting them from this device come with Hub Pro\. Details$/);
  const html = itemBody(entries[0], { body, access: "refused", refusedUrl: "/plan", ui: ui(), candidates: entries[0].machines, chosen: entries[0].machines[0], now: NOW });
  assert.doesNotMatch(html, /data-action="pick-question"|data-action="start-item"/);
  assert.match(text(html), /Answering its questions from this device comes with Hub Pro\. Details/);
  assert.match(text(html), /Starting it from this device comes with Hub Pro\. Details$/);
  assert.equal(listAction({ access: "ready", ideaMachines: [{}] }).includes('href="#/idea"'), true);
  assert.equal(listAction({ access: "ready", ideaMachines: [] }), "");
});

test("a machine that does not take starts from the backlog says so instead of a Start button", () => {
  const entries = mergeBacklog([machine("mch_mac", "MacBook", [row("local:faster", { title: "Faster board" })], { accepts: ["answer"] })], NOW);
  const said = text(itemBody(entries[0], { access: "ready", ui: ui(), candidates: [], chosen: null, now: NOW }));
  assert.match(said, /Not started from here: MacBook takes no starts from the backlog\./);
});

test("a new idea shows at once under New until its machine posts a board after filing it", () => {
  const idea = sentAt({ id: "cmd_idea", kind: "idea", machine: "mch_mac", machineName: "MacBook", text: "ci push retry flaky", address: null, at: NOW - 5 });
  const showing = (sent, state, storedAt = NOW - 20) => ideaEntries([sent], [{ ...opened[0], storedAt }], NOW, () => state);
  const [entry] = showing(idea, "sending");
  const said = text(backlogList({ entries: [entry, ...mergeBacklog(opened, NOW)], now: NOW, top }));
  assert.match(said, /New · 1 ci push retry flaky MacBook to write · now the lead on MacBook writes it out High/);
  assert.match(text(backlogList({ entries: showing(idea, "waiting"), now: NOW, top })), /waits in the hub for MacBook · goes out when it wakes/);
  const filed = { ...idea, result: { outcome: "done", text: "Filed", at: NOW - 2 } };
  assert.equal(showing(filed, "done", NOW - 20).length, 1);
  assert.equal(showing(filed, "done", NOW - 1).length, 0, "the board after the result carries the item");
  assert.match(text(itemBody(entry, { now: NOW })), /state to write: the lead on MacBook writes it out filed today, from iPhone 16/);
});

test("the idea composer goes to a chosen machine and adds only a written idea", () => {
  const candidates = [{ machine: "mch_mac", name: "MacBook", asleep: false, repos: [] }, { machine: "mch_work", name: "work-mac", asleep: false, repos: [] }];
  const chosen = chooseMachine(candidates, { lastUsed: "mch_mac" });
  assert.match(text(ideaForm({ text: "", candidates, chosen })), /Goes to the lead on MacBook you used it last Change Add to the backlog/);
  assert.match(ideaForm({ text: "", candidates, chosen }), /data-action="add-idea" disabled/);
  assert.doesNotMatch(ideaForm({ text: "retry", candidates, chosen }), /data-action="add-idea" disabled/);
  assert.match(text(ideaForm({ text: "x", candidates: [], chosen: null })), /No machine takes ideas from the hub/);
});

test("a vault note's row shows the furthest state and the fewest blocking questions any machine has, and a ready item without a body shows its goal", () => {
  const answered = { ...session, blocking: 1 };
  const [shared] = mergeBacklog([machine("mch_mac", "MacBook", [session]), machine("mch_work", "work-mac", [answered])], NOW);
  assert.equal(shared.row.blocking, 1);
  const ready = mergeBacklog([machine("mch_mac", "MacBook", [row("local:faster", { title: "Faster board", goal: "The board shows the push within a second." })])], NOW)[0];
  const said = text(itemBody(ready, { access: "ready", ui: ui(), candidates: ready.machines, chosen: ready.machines[0], now: NOW }));
  assert.match(said, /Goal The board shows the push within a second\. Save to backlog Start/);
  assert.doesNotMatch(said, /writes it out/);
});
