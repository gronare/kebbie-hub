import assert from "node:assert/strict";
import { test } from "node:test";

import { MAX_ITEM_TITLE_CHARS, MAX_SECTION_CHARS } from "../js/protocol.js";
import { answerableQuestions, answerCommand, answers, answerSents, answersToSend, answersWithLead, backlogList, bodyShown, keepDrafts, readDrafts, sendAllLabel, sendAnswers, sentKey, settleDrafts, startAnswering, strayDrafts, takesBatch, routeMachine, newTargets, ideaMachine, needsYouCount, cleanEdit, editProblem, editReady, ideaEntries, ideaForm, itemBody, newStartStatus, startCommand, listAction, mergeBacklog, mergedInto, startsAnyway, writeArea, backlogRow } from "./backlog.js";

const text = (html) => html.replace(/<span[^>]*aria-hidden="true"[^>]*>[^<]*<\/span>/g, " ").replace(/<[^>]+>/g, " ").replace(/&#(\d+);/g, (_, code) => String.fromCharCode(Number(code))).replace(/\s+/g, " ").trim();
const top = (title, aside) => `<h1>${title}</h1><span>${aside}</span>`;
const NOW = 1_762_956_540;
const DAY = 86_400;
const ALL = ["answer", "push", "idea", "item-answer", "start-item"];

const row = (item, fields = {}) => ({ item, title: item, priority: "medium", repo: "kebbie", state: "ready", filed: NOW - DAY, home: { kind: item.startsWith("vault:") ? "vault" : "local", label: item.split(":")[1] }, questions: 0, blocking: 0, goal: null, body: null, ...fields });
const machine = (id, name, backlog, { lastSeen = NOW - 20, accepts = ALL, repos = ["kebbie"] } = {}) => ({ machine: id, name, lastSeen, storedAt: lastSeen, board: { accepts, backlog, checkouts: repos.map((repo) => ({ repo, free_worktrees: 1, busy_worktrees: 0 })), items: [] } });

const session = row("vault:Projects/session.md", { title: "Session drops on reload", priority: "high", repo: "webapp", state: "drafting", questions: 3, blocking: 2, body: { id: "blb_1", key: "k", media_type: "application/vnd.kebbie.backlog-item+json", bytes: 900 } });
const section = (key, heading, text, required = false) => ({ key, heading, text, required });
const body = {
  sections: [
    section("background", "Background", "Users are signed out on reload.", true),
    section("goal", "Goal", "You stay signed in.", true),
    section("what-we-know", "What we know", "- The cookie lasts ten minutes."),
    section("questions", "Open questions", ""),
    section("out-of-scope", "Out of scope", "The operator app."),
    section("source", "Source", "From the board."),
  ],
  questions: [
    { text: "Should a reload renew the session?", blocks: true, pending_answer: null },
    { text: "Does a password change sign out every device?", blocks: true, pending_answer: null },
    { text: "Is 14 days right?", blocks: false, pending_answer: null },
  ],
  questions_key: "questions",
};
const opened = [
  machine("mch_mac", "MacBook", [session, row("local:faster", { title: "Faster board", goal: "The board shows the push within a second." }), row("local:logs", { title: "Archive logs", state: "to-write", filed: NOW - 7200 })], { repos: ["kebbie"] }),
  machine("mch_work", "work-mac", [session, row("vault:Projects/round.md", { title: "Round invoices", priority: "high", repo: "billing" })], { repos: ["webapp", "billing"] }),
];
const [Q0, Q1, Q2] = body.questions.map((question) => question.text);
const ui = (fields = {}) => ({ answers: new Map(), open: new Set(), answering: { busy: false, error: null }, busy: false, error: null, visit: NOW - 60, ...fields });
const sentAt = (fields) => ({ id: "cmd_1", machine: "mch_work", machineName: "work-mac", device: "dev_1", deviceName: "iPhone 16", at: NOW - 120, exp: NOW - 120 + 3600, result: null, ...fields });

test("a vault note seen on two machines is one row with the vault chip, and the list groups by priority with its questions counted", () => {
  const entries = mergeBacklog(opened, NOW);
  assert.equal(entries.length, 4);
  const shared = entries.find((entry) => entry.key === "vault:Projects/session.md");
  assert.deepEqual(shared.machines.map((one) => one.name), ["MacBook", "work-mac"]);
  const said = text(backlogList({ entries, now: NOW, top }));
  assert.match(said, /^Backlog 4 items · 2 questions wait .* All 4 Priority Newest Needs you · 1 Session drops on reload vault webapp · drafting · 2 questions · 1 d you: answer them on its card High · 1 Round invoices/);
  assert.match(said, /Faster board MacBook kebbie · 1 d The board shows the push within a second\./);
  assert.match(said, /Archive logs MacBook kebbie · to write · 2 h the lead writes it out/);
  assert.ok(said.indexOf("High ·") < said.indexOf("Medium ·"));
});

test("a start goes to the one machine that has the item's repo and names it, with no chooser", () => {
  const shared = mergeBacklog(opened, NOW).find((entry) => entry.vault && entry.row.repo === "webapp");
  const chosen = routeMachine(shared.machines, { repo: "webapp", needsRepo: true });
  assert.deepEqual([chosen.name, chosen.why], ["work-mac", "only work-mac has webapp"]);
  const html = itemBody(shared, { body, access: "ready", ui: ui(), candidates: shared.machines, chosen, now: NOW });
  assert.match(text(html), /Starts on work-mac only work-mac has webapp Save to backlog Start The lead on work-mac picks the recipe and queues it\./);
  assert.doesNotMatch(html, /backlog-choose|backlog-machine/);
});
test("the card shows its facts, its sections and its questions, blocking ones marked, and which machines see it", () => {
  const shared = mergeBacklog(opened, NOW)[0];
  const chosen = routeMachine(shared.machines, { repo: "webapp" });
  const said = text(itemBody(shared, { body, access: "ready", ui: ui(), candidates: shared.machines, chosen, now: NOW }));
  assert.match(said, /priority high repo webapp state drafting: 2 questions wait on you filed \d+ Nov note Projects\/session\.md · seen on MacBook and work-mac/);
  assert.match(said, /Background Users are signed out on reload\. Goal You stay signed in\. What we know - The cookie lasts ten minutes\. Open questions 2 wait on you before it starts/);
  assert.match(said, /Is 14 days right\? the work can settle it Out of scope/);
  const html = itemBody(shared, { body, access: "ready", ui: ui(), candidates: shared.machines, chosen, now: NOW });
  assert.equal((html.match(/data-action="pick-question"/g) ?? []).length, 3);
  assert.equal((html.match(/qmark warn/g) ?? []).length, 2);
});

test("an answer sent shows with the lead, and once the body carries it the question stops counting", () => {
  const shared = mergeBacklog(opened, NOW)[0];
  const chosen = routeMachine(shared.machines, { repo: "webapp" });
  const picked = itemBody(shared, { body, access: "ready", ui: ui({ open: new Set([Q0]), answers: new Map([[Q0, "Renew on every request"]]) }), candidates: shared.machines, chosen, now: NOW });
  assert.match(text(picked), /Your answer Renew on every request Does a password .* Send 1 answer/);
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

test("each open question keeps its own draft across a re-render, and every filled answer goes out with one Send button", () => {
  const shared = mergeBacklog(opened, NOW)[0];
  const chosen = routeMachine(shared.machines, { repo: "webapp" });
  const drafts = ui({ open: new Set([Q0, Q1, Q2]), answers: new Map([[Q0, "Renew on every request"], [Q1, "Yes, every device"]]) });
  const card = () => itemBody(shared, { body, access: "ready", ui: drafts, candidates: shared.machines, chosen, now: NOW });
  const values = (html) => [...html.matchAll(/<textarea id="answer-(\d)"[^>]*>([^<]*)<\/textarea>/g)].map(([, index, value]) => `${index}:${value}`);
  assert.deepEqual(values(card()), ["0:Renew on every request", "1:Yes, every device", "2:"]);
  assert.deepEqual(values(card()), values(card()), "a second render shows the same drafts");
  const two = card();
  assert.match(two, /<div class="answers-send"><button type="button" class="btn primary" data-action="send-item-answers">Send 2 answers<\/button><\/div>/);
  assert.equal((two.match(/data-action="send-item-answers?"/g) ?? []).length, 1, "one button sends them all");
  drafts.answers.delete(Q1);
  assert.match(card(), /data-action="send-item-answers">Send 1 answer</);
  drafts.answers.delete(Q0);
  assert.match(card(), /<div class="answers-send" hidden>/, "nothing to send, nothing to press");
  drafts.answers.set(Q0, "Renew on every request");
  drafts.answering.batch = { total: 2, keys: [Q0, Q1] };
  assert.match(card(), /data-action="send-item-answers" disabled>Sending 2 answers…</, "while it sends it says so and takes no second press");
});

test("the Send button counts what it sends, and says how many while it sends", () => {
  assert.equal(sendAllLabel(1), "Send 1 answer");
  assert.equal(sendAllLabel(3), "Send 3 answers");
  assert.equal(sendAllLabel(3, { total: 3, keys: [] }), "Sending 3 answers…");
  assert.equal(sendAllLabel(1, { total: 1, keys: [] }), "Sending 1 answer…");
  const sent = { sent: sentAt({ kind: "item-answer", index: 0, text: "x" }), state: "sent", standing: null, live: true };
  assert.deepEqual(answerableQuestions(body, (index) => (index === 0 ? sent : null)), [1, 2], "a question with an answer on its way is not sent again");
});

test("a sent answer keeps its draft: hidden while it is on its way, back in its field with one plain line and Send again when it is refused", () => {
  const shared = mergeBacklog(opened, NOW)[0];
  const chosen = routeMachine(shared.machines, { repo: "webapp" });
  const drafts = ui({ open: new Set([Q0]), answers: new Map([[Q0, "Renew on every request"]]) });
  const sent = (state, result = null) => (index) => (index === 0 ? { sent: sentAt({ kind: "item-answer", index: 0, question: Q0, text: "Renew on every request", result }), state, standing: null, live: state === "sent" } : null);
  const going = itemBody(shared, { body, access: "ready", ui: drafts, sentFor: sent("sent"), candidates: shared.machines, chosen, now: NOW });
  assert.doesNotMatch(going, /<textarea id="answer-0"/);
  assert.match(text(going), /Sent to work-mac Your answer "Renew on every request" is on its way to the lead/);
  const refused = itemBody(shared, { body, access: "ready", ui: drafts, sentFor: sent("refused", { outcome: "refused", reason: "question-changed", text: "That question changed on the item since; read it again and answer what it asks now.", at: NOW - 30 }), candidates: shared.machines, chosen, now: NOW });
  assert.match(text(refused), /Should a reload renew the session\? Not delivered: the item changed while you answered\. Your answer is still here\. Your answer Renew on every request Send again/);
  assert.doesNotMatch(text(refused), /Not answered|read it again/, "the machine's raw refusal is not shown");
  assert.match(refused, /<textarea id="answer-0"[^>]*data-again[^>]*>Renew on every request<\/textarea>/, "the text is still there to send again");
  assert.match(refused, /<button type="button" class="btn" data-action="send-item-answers" data-key="Should a reload renew the session\?">Send again<\/button>/);
  assert.doesNotMatch(refused, /Send 1 answer/, "a refused answer has its own Send again, not a second button");
  const expired = text(itemBody(shared, { body, access: "ready", ui: drafts, sentFor: sent("expired"), candidates: shared.machines, chosen, now: NOW }));
  assert.match(expired, /Not delivered: work-mac stayed asleep past the hour\. Your answer is still here\./);
});

test("a refused answer whose draft is gone gets its text back, and one whose question changed stays on the card with its text", () => {
  const refused = { sent: sentAt({ kind: "item-answer", question: Q0, nth: 0, text: "Renew on every request", result: { outcome: "refused", reason: "question-changed", text: "x", at: NOW } }), state: "refused", standing: null, live: false };
  const drafts = ui();
  assert.equal(settleDrafts(drafts, body, (key) => (key === Q0 ? refused : null)), true);
  assert.equal(drafts.answers.get(Q0), "Renew on every request");
  const changed = ui({ answers: new Map([["Renew the session on reload?", "Every request"], [Q1, "Every device"]]) });
  const strays = strayDrafts(changed, body, () => null);
  assert.deepEqual(strays, [{ key: "Renew the session on reload?", question: "Renew the session on reload?", text: "Every request" }]);
  const shared = mergeBacklog(opened, NOW)[0];
  const said = text(itemBody(shared, { body, access: "ready", ui: changed, strays, candidates: shared.machines, chosen: routeMachine(shared.machines, { repo: "webapp" }), now: NOW }));
  assert.match(said, /Renew the session on reload\? no longer on the item Not delivered: the item changed while you answered\. Your answer is still here\. Copy it into the question as it reads now\. Every request Remove/);
  const delivered = { ...refused, sent: { ...refused.sent, result: { outcome: "done", reason: null, text: "ok", at: NOW } }, state: "done", live: true };
  const sentOnce = ui({ answers: new Map([[Q0, "Renew on every request"]]) });
  settleDrafts(sentOnce, body, (key) => (key === Q0 ? delivered : null));
  assert.equal(sentOnce.answers.has(Q0), false, "a delivered answer clears its draft");
});

test("answer drafts are kept per item and question across a reload, and a storage that throws loses nothing on screen", () => {
  const held = new Map();
  const storage = { getItem: (key) => held.get(key) ?? null, setItem: (key, value) => held.set(key, value), removeItem: (key) => held.delete(key) };
  keepDrafts(storage, "vault:Projects/session.md", new Map([[Q0, "Renew on every request"], [Q1, "  "]]));
  assert.deepEqual([...readDrafts(storage, "vault:Projects/session.md")], [[Q0, "Renew on every request"]]);
  assert.deepEqual([...readDrafts(storage, "vault:Projects/other.md")], []);
  keepDrafts(storage, "vault:Projects/session.md", new Map());
  assert.equal(held.size, 0, "nothing left to send leaves nothing stored");
  const broken = { getItem: () => { throw new Error("blocked"); }, setItem: () => { throw new Error("blocked"); }, removeItem: () => { throw new Error("blocked"); } };
  assert.deepEqual([...readDrafts(broken, "x")], []);
  assert.doesNotThrow(() => keepDrafts(broken, "x", new Map([[Q0, "y"]])));
  storage.setItem("kebbie-answers:x", "[not json");
  assert.deepEqual([...readDrafts(storage, "x")], []);
});

test("a click while answers are sending is never dropped unseen: the button says Sending and a press with no machine says why", () => {
  const shared = mergeBacklog(opened, NOW)[0];
  const chosen = routeMachine(shared.machines, { repo: "webapp" });
  const drafts = ui({ open: new Set([Q0, Q1]), answers: new Map([[Q0, "Renew on every request"], [Q1, "Every device"]]) });
  const going = answersToSend(body, drafts, [0, 1]);
  assert.deepEqual(startAnswering(drafts, { chosen, answers: going }), { total: 2, keys: [Q0, Q1] });
  assert.equal(startAnswering(drafts, { chosen, answers: going }), null, "a second press starts nothing");
  const card = itemBody(shared, { body, access: "ready", ui: drafts, candidates: shared.machines, chosen, now: NOW });
  assert.match(card, /data-action="send-item-answers" disabled>Sending 2 answers…</, "and the button shows what the first press is doing");
  const none = ui({ open: new Set([Q0]), answers: new Map([[Q0, "Renew"]]) });
  assert.equal(startAnswering(none, { chosen: null, answers: answersToSend(body, none, [0]) }), null);
  assert.match(text(itemBody(shared, { body, access: "ready", ui: none, candidates: shared.machines, chosen, now: NOW })), /Not sent: no machine takes answers from the hub right now\. Your answer is still here\./);
});

test("to a machine that takes item-answers every answer goes in one command; to an older one each goes alone and a refusal stops none", async () => {
  const shared = mergeBacklog(opened, NOW)[0];
  const drafts = ui({ open: new Set([Q0, Q1, Q2]), answers: new Map([[Q2, "Yes"], [Q0, " Renew on every request "], [Q1, "Every device"]]) });
  const going = answersToSend(body, drafts, [0, 1, 2]);
  assert.equal(takesBatch({ accepts: [...ALL, "item-answers"] }, going), true);
  assert.equal(takesBatch({ accepts: ALL }, going), false);
  const commands = [];
  assert.deepEqual(await sendAnswers(shared, going, { batch: true, send: async (command) => commands.push(command) }), []);
  assert.equal(commands.length, 1, "one message for every answer");
  assert.deepEqual(commands[0].payload, {
    type: "item-answers",
    item: "vault:Projects/session.md",
    answers: [
      { question: Q0, answer: "Renew on every request" },
      { question: Q1, answer: "Every device" },
      { question: Q2, answer: "Yes" },
    ],
  });
  assert.deepEqual(commands[0].meta.answers.map((one) => one.text), ["Renew on every request", "Every device", "Yes"]);
  assert.deepEqual(await sendAnswers(shared, going, { batch: true, send: async () => null }), [Q0, Q1, Q2], "a batch the hub did not take keeps every answer");
  const singles = [];
  assert.deepEqual(await sendAnswers(shared, going, { batch: false, send: async (command) => (singles.push(command.payload), command.payload.index !== 1) }), [Q1]);
  assert.deepEqual(singles.map((one) => [one.type, one.index, one.answer]), [["item-answer", 0, "Renew on every request"], ["item-answer", 1, "Every device"], ["item-answer", 2, "Yes"]], "the answer after a refused one still goes out");
});

test("one item-answers command shows With the lead on each question it answered", () => {
  const shared = mergeBacklog(opened, NOW)[0];
  const batch = sentAt({ kind: "item-answers", address: shared.key, machine: "mch_work", answers: [{ question: Q0, nth: 0, text: "Renew on every request" }, { question: Q1, nth: 0, text: "Every device" }], result: { outcome: "done", reason: null, text: "ok", at: NOW - 60 } });
  const each = answerSents([batch]);
  assert.deepEqual(each.map((one) => [one.kind, sentKey(one), one.text]), [["item-answer", Q0, "Renew on every request"], ["item-answer", Q1, "Every device"]]);
  const sentFor = (index, questions) => {
    const one = each.find((sent) => answers(sent, questions, index));
    return one ? { sent: one, state: "done", standing: null, live: true } : null;
  };
  const said = text(itemBody(shared, { body, access: "ready", ui: ui(), sentFor, candidates: shared.machines, chosen: routeMachine(shared.machines, { repo: "webapp" }), now: NOW }));
  assert.match(said, /Should a reload renew the session\? With the lead: "Renew on every request".*Does a password change sign out every device\? With the lead: "Every device"/);
  assert.equal(answersWithLead({ ...shared, row: { ...shared.row, blocking: 2 } }, [batch], { shown: () => ({ state: "done", live: true }), storedAt: () => 0, body }), 2);
});

test("the rule sends work to the one machine with the repo, else the ideas machine, and an asleep one's work to an awake machine", () => {
  const candidates = [
    { machine: "mch_mac", name: "MacBook", asleep: false, repos: ["kebbie", "shop"] },
    { machine: "mch_work", name: "work-mac", asleep: false, repos: ["webapp", "shop"] },
    { machine: "mch_box", name: "devbox", asleep: false, repos: ["kebbie", "shop"], ideas: true },
  ];
  const said = (one) => [one.name, one.why];
  assert.deepEqual(said(routeMachine(candidates, { repo: "webapp" })), ["work-mac", "only work-mac has webapp"], "a repo on one machine wins over the ideas machine");
  assert.deepEqual(said(routeMachine(candidates, { repo: "shop" })), ["devbox", "ideas machine"], "a repo on several machines goes to the ideas machine");
  assert.deepEqual(said(routeMachine(candidates)), ["devbox", "ideas machine"], "no repo goes to the ideas machine");
  assert.deepEqual(said(routeMachine(candidates, { repo: "nowhere" })), ["devbox", "ideas machine"]);
  const boxAsleep = candidates.map((one) => (one.ideas ? { ...one, asleep: true } : one));
  assert.deepEqual(said(routeMachine(boxAsleep)), ["MacBook", "devbox is asleep, sent to MacBook"]);
  const workAsleep = candidates.map((one) => (one.machine === "mch_work" ? { ...one, asleep: true } : one));
  assert.deepEqual(said(routeMachine(workAsleep, { repo: "webapp" })), ["devbox", "work-mac is asleep, sent to devbox"], "an awake ideas machine takes the asleep machine's work first");
  assert.deepEqual(said(routeMachine(workAsleep, { repo: "webapp", needsRepo: true })), ["work-mac", "only work-mac has webapp, asleep now"], "a start waits for the only machine with its repo");
  assert.deepEqual(said(routeMachine(candidates.map((one) => ({ ...one, asleep: true })))), ["devbox", "ideas machine, asleep now"]);
  const plain = candidates.map(({ ideas, ...one }) => one);
  assert.deepEqual(said(routeMachine(plain, { repo: "shop" })), ["MacBook", "awake now"], "with no ideas machine an awake machine takes it");
  assert.deepEqual(said(routeMachine([{ ...plain[1], asleep: true }, plain[0]], { repo: "kebbie", needsRepo: true })), ["MacBook", "only MacBook has kebbie"]);
  assert.deepEqual(said(routeMachine(plain, { repo: "gone", needsRepo: true })), ["MacBook", "awake now"], "a start whose repo no machine has goes out for the machine to refuse");
  assert.equal(routeMachine([]), null);
  assert.equal(ideaMachine(candidates), "mch_box");
  assert.equal(ideaMachine(plain), null, "a kebbie without ideas.machine names none");
});

test("an item's write-out and answers go to the ideas machine its board names, not to another machine that also has the repo", () => {
  const box = machine("mch_box", "devbox", [session], { repos: ["kebbie", "webapp"] });
  box.board.idea_machine = true;
  const shared = mergeBacklog([...opened, box], NOW).find((entry) => entry.key === session.item);
  assert.deepEqual(shared.machines.map((one) => [one.name, one.ideas]), [["MacBook", false], ["work-mac", false], ["devbox", true]]);
  const chosen = routeMachine(shared.machines, { repo: shared.row.repo });
  assert.deepEqual([chosen.name, chosen.why], ["devbox", "ideas machine"]);
});

test("New sends an idea to the ideas machine and a start to the machine the rule picks for its repo, each named without a chooser", () => {
  const mac = { machine: "mch_mac", name: "MacBook", accepts: ["idea", "start"], asleep: false, repos: ["shop", "kebbie"], starts: true };
  const box = { machine: "mch_box", name: "devbox", accepts: ["idea", "start"], asleep: false, repos: ["kebbie"], starts: true, ideas: true };
  const targets = newTargets([mac, box], null);
  assert.deepEqual([targets.adds.name, targets.starter.name, targets.repo, targets.repos], ["devbox", "devbox", "kebbie", ["kebbie", "shop"]], "the ideas machine's repos come first");
  const shop = newTargets([mac, box], "shop");
  assert.deepEqual([shop.adds.name, shop.starter.name, shop.starter.why], ["devbox", "MacBook", "only MacBook has shop"]);
  const form = ideaForm({ text: "x", candidates: [mac, box], ...shop });
  assert.match(text(form), /Goes to the lead on devbox ideas machine Starts in kebbie shop Starts on MacBook only MacBook has shop Start now on MacBook Add to the backlog$/);
  assert.doesNotMatch(form, /backlog-choose|backlog-machine/);
  assert.equal(newTargets([mac, box], "gone").repo, "kebbie", "a repo no machine has falls back to the first");
  assert.notEqual(newTargets([mac, box], "kebbie").starter.name, newTargets([mac, box], "shop").starter.name, "picking another repo can move the start to another machine");
});

test("when the lead folds one answer in and its question leaves, every other draft and sent answer stays on its own question", () => {
  const shared = mergeBacklog(opened, NOW)[0];
  const chosen = routeMachine(shared.machines, { repo: "webapp" });
  const drafts = ui({ open: new Set([Q0, Q1, Q2]), answers: new Map([[Q0, "Renew on every request"], [Q1, "Every device"], [Q2, "Yes"]]) });
  const [first, second] = answersToSend(body, drafts, [0, 1]).map((one) => answerCommand(shared, one).meta);
  const kept = [sentAt({ kind: "item-answer", ...first, result: { outcome: "done", reason: null, text: "ok", at: NOW - 60 } }), sentAt({ kind: "item-answer", ...second, id: "cmd_2" })];
  const sentFor = (index, questions) => {
    const sent = kept.filter((one) => answers(one, questions, index)).at(-1);
    return sent ? { sent, state: sent.result ? "done" : "sent", standing: null, live: true } : null;
  };
  const folded = { ...body, questions: body.questions.slice(1) };
  const html = itemBody(shared, { body: folded, access: "ready", ui: drafts, sentFor, candidates: shared.machines, chosen, now: NOW });
  const [upToThird, third] = text(html).split(Q2);
  assert.match(upToThird, /Does a password change sign out every device\? Sent to work-mac Your answer "Every device" is on its way to the lead/);
  assert.doesNotMatch(third, /Sent to|With the lead/, "the third question carries no one else's status");
  assert.match(html, /<textarea id="answer-1" data-draft="item-answer" data-index="1" data-key="Is 14 days right\?" rows="2" maxlength="2000">Yes<\/textarea>/, "its draft moved with it");
  assert.doesNotMatch(html, /Renew on every request/, "the folded answer left with its question");
  assert.equal(answersWithLead({ ...shared, row: { ...shared.row, blocking: 1 } }, kept.map((one) => ({ ...one, machine: "mch_work", address: shared.key })), { shown: () => ({ state: "sent", live: true }), storedAt: () => 0, body: folded }), 1, "the answer on its way counts against the question it answers");
});

test("a body that changes keeps the one shown until the new one is read, so the page keeps its height and scroll", () => {
  assert.deepEqual(bodyShown(null, body), { body, bodyState: "ready" });
  assert.deepEqual(bodyShown(null, null), { body: null, bodyState: "loading" });
  const fresh = { ...body, questions: [] };
  assert.deepEqual(bodyShown({ body: fresh }, body), { body: fresh, bodyState: "ready" });
  assert.deepEqual(bodyShown({ body: null }, body), { body: null, bodyState: "failed" });
});

test("a start refused for open questions says how many, and a second Start in the same visit goes anyway", () => {
  const shared = mergeBacklog(opened, NOW)[0];
  const chosen = routeMachine(shared.machines, { repo: "webapp" });
  const start = { sent: sentAt({ kind: "start-item", at: NOW - 30, exp: NOW - 30 + DAY, result: { outcome: "refused", reason: "questions-open", blocking: 2, text: "Not started: 2 questions wait on you.", at: NOW - 20 } }), state: "refused", standing: null, live: false };
  const said = text(itemBody(shared, { body, access: "ready", ui: ui(), start, candidates: shared.machines, chosen, now: NOW }));
  assert.match(said, /Not started: 2 questions wait on you\. Answer them on its card, or press Start again to start anyway\. .* Save to backlog Start/);
  assert.equal(startsAnyway(start, NOW - 60), true);
  assert.equal(startsAnyway(start, NOW), false, "a refusal from an earlier visit does not start it anyway");
  assert.equal(startsAnyway({ ...start, sent: { ...start.sent, result: { ...start.sent.result, reason: "already-started" } } }, NOW - 60), false);
});

test("a start waiting for an asleep machine, one waiting for this device's approval and one started each say so", () => {
  const shared = mergeBacklog(opened, NOW)[0];
  const chosen = routeMachine(shared.machines, { repo: "webapp" });
  const card = (state, fields = {}, standing = null) => text(itemBody(shared, { body, access: "ready", ui: ui(), start: { sent: sentAt({ kind: "start-item", exp: NOW - 120 + DAY, ...fields }), state, standing, live: state !== "refused" }, candidates: shared.machines, chosen, now: NOW, taskHref: "#/task/mch_work/session@webapp" }));
  assert.match(card("waiting"), /Waiting for work-mac The lead starts Session drops on reload when work-mac wakes up\. Sent from iPhone 16 at \d\d:\d\d · goes out when it wakes · expires in 23 h$/);
  assert.match(card("approval", {}, { state: "waiting", fingerprint: "AB", until: NOW + 23 * 3600 }), /Waiting for work-mac to approve this iPhone 16 Approve it in kebbie, or it is approved automatically in 23 h\. Your start goes out if work-mac approves before/);
  const started = itemBody(shared, { body, access: "ready", ui: ui(), start: { sent: sentAt({ kind: "start-item", result: { outcome: "done", reason: null, text: "Started session@webapp: the lead chose the recipe small-fix and queued it.", at: NOW } }), state: "done", standing: null, live: true }, candidates: shared.machines, chosen, now: NOW, taskHref: "#/task/mch_work/session@webapp" });
  assert.match(text(started), /✓ Started Session drops on reload Started session@webapp: the lead chose the recipe small-fix and queued it\. .* Open the task$/);
  assert.match(started, /href="#\/task\/mch_work\/session@webapp"/);
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
  assert.match(listAction({ access: "ready", ideaMachines: [{}] }), /^<a class="fab" href="#\/idea" aria-label="New idea">/);
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

test("the idea composer names where the idea goes and adds only a written idea", () => {
  const candidates = [{ machine: "mch_mac", name: "MacBook", accepts: ["idea"], asleep: false, repos: [] }, { machine: "mch_work", name: "work-mac", accepts: ["idea"], asleep: false, repos: [], ideas: true }];
  const targets = newTargets(candidates);
  assert.match(text(ideaForm({ text: "", candidates, ...targets })), /Goes to the lead on work-mac ideas machine Add to the backlog/);
  assert.match(ideaForm({ text: "", candidates, ...targets }), /data-action="add-idea" disabled/);
  assert.doesNotMatch(ideaForm({ text: "retry", candidates, ...targets }), /data-action="add-idea" disabled/);
  assert.match(text(ideaForm({ text: "x", candidates: [], ...newTargets([]) })), /No machine takes ideas from the hub/);
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

test("the card shows whatever template the item uses, in its order, with the open questions under the section the template names", () => {
  const shared = mergeBacklog(opened, NOW)[0];
  const chosen = routeMachine(shared.machines, { repo: shared.row.repo });
  const other = { sections: [section("problem", "Problem", "Reloads sign you out.", true), section("risks", "Risks", "")], questions: body.questions, questions_key: null };
  const said = text(itemBody(shared, { body: other, access: "ready", ui: ui(), candidates: shared.machines, chosen, now: NOW }));
  assert.match(said, /Problem Reloads sign you out\. Risks Empty\. Open questions 2 wait on you before it starts/);
  assert.doesNotMatch(said, /Background|Goal|Out of scope/);
  const under = text(itemBody(shared, { body: { ...other, questions_key: "problem" }, access: "ready", ui: ui(), candidates: shared.machines, chosen, now: NOW }));
  assert.match(under, /Problem Reloads sign you out\. 2 wait on you before it starts .* Risks Empty\./);
});

test("each section and the title get an Edit where a machine takes edits, and Save waits for a change a required section can hold", () => {
  const shared = mergeBacklog(opened, NOW)[0];
  const chosen = routeMachine(shared.machines, { repo: shared.row.repo });
  const card = (fields) => itemBody(shared, { body, access: "ready", ui: ui(), candidates: shared.machines, chosen, now: NOW, edits: { machine: chosen, editing: null, sentFor: () => null, ...fields } });
  assert.equal((card({}).match(/data-action="item-edit"/g) ?? []).length, body.sections.length + 1);
  assert.doesNotMatch(card({ machine: null }), /data-action="item-edit"/);
  assert.doesNotMatch(itemBody(shared, { body, access: "view-only", ui: ui(), candidates: shared.machines, chosen, now: NOW, edits: { machine: chosen, editing: null, sentFor: () => null } }), /data-action="item-edit"/);
  const editing = { section: "goal", text: "You stay signed in.", original: "You stay signed in.", required: true, busy: false, error: null };
  const open = card({ editing });
  assert.match(open, /<textarea id="edit-goal" data-draft="item-edit"[^>]*>You stay signed in\.<\/textarea>/);
  assert.match(text(open), /Goal · markdown You stay signed in\. Goal can't be empty: the template requires it\. Cancel Save/);
  assert.equal(editReady(editing, true), false, "nothing changed");
  assert.equal(editReady({ ...editing, text: " " }, true), false, "a required section is never saved empty");
  assert.equal(editReady({ ...editing, section: "source", text: "" }, false), true, "an optional section can be emptied");
  assert.equal(editReady({ ...editing, section: null, text: "" }, false), false, "a title is never empty");
  assert.match(card({ editing: { ...editing, section: null, text: "Session drops", original: "Session drops on reload" } }), /<input id="edit-title" data-draft="item-edit" maxlength="200"[^>]*value="Session drops">/);
});

test("a saved edit shows Saving with its text until the board carries it, and an edit made on an older text says the machine has a newer one", () => {
  const shared = mergeBacklog(opened, NOW)[0];
  const chosen = routeMachine(shared.machines, { repo: shared.row.repo });
  const edit = (shown) => (key) => (key === "goal" ? shown : null);
  const card = (shown) => text(itemBody(shared, { body, access: "ready", ui: ui(), candidates: shared.machines, chosen, now: NOW, edits: { machine: chosen, editing: null, sentFor: edit(shown) } }));
  const sent = sentAt({ kind: "item-edit", section: "goal", text: "You stay signed in across a reload." });
  assert.match(card({ sent, state: "sending", standing: null, live: true, saving: true, recent: true }), /Goal You stay signed in across a reload\. Saving… What we know/);
  const refused = { ...sent, result: { outcome: "refused", reason: "section-changed", text: "Not saved: it changed on the machine since you opened it.", at: NOW } };
  assert.match(card({ sent: refused, state: "refused", standing: null, live: false, saving: false, recent: true }), /Goal Edit You stay signed in\. work-mac has a newer version of this section; reload and edit again from iPhone 16/);
  assert.match(card({ sent: refused, state: "refused", standing: null, live: false, saving: false, recent: false }), /Goal Edit You stay signed in\. What we know/, "a refusal from an earlier visit is not shown again");
  const title = (shown) => text(itemBody(shared, { body, access: "ready", ui: ui(), candidates: shared.machines, chosen, now: NOW, edits: { machine: chosen, editing: null, sentFor: (key) => (key === null ? shown : null) } }));
  assert.match(title({ sent: { ...sent, section: null, text: "Session survives a reload" }, state: "sending", standing: null, live: true, saving: true, recent: true }), /title: Session survives a reload Saving…/);
});

test("an edit loses control characters the machine refuses, says what is too long before anything is sealed, and a kept edit without its words still shows Saving", () => {
  assert.equal(cleanEdit({ section: "goal", text: "Stay\u001b[2J signed in\n\tacross a reload" }), "Stay[2J signed in\n\tacross a reload");
  assert.equal(cleanEdit({ section: null, text: "Session\tdrops" }), "Sessiondrops");
  const payload = (section, text) => ({ item: "vault:Projects/session.md", section, text, base: "0".repeat(64) });
  assert.equal(editProblem(payload("goal", "x".repeat(MAX_SECTION_CHARS))), null);
  assert.equal(editProblem(payload("goal", "x".repeat(MAX_SECTION_CHARS + 1))), "This section is 8,001 characters; a section holds at most 8,000.");
  assert.equal(editProblem(payload(null, "x".repeat(MAX_ITEM_TITLE_CHARS + 1))), "This title is 201 characters; a title holds at most 200.");
  assert.equal(editProblem(payload("goal", "😀".repeat(MAX_SECTION_CHARS))), "This section comes to 32 kB, and an edit holds at most 24 kB. Shorten it before saving.");
  const shared = mergeBacklog(opened, NOW)[0];
  const chosen = routeMachine(shared.machines, { repo: shared.row.repo });
  const { text: _, ...kept } = sentAt({ kind: "item-edit", section: "goal", result: { outcome: "done", at: NOW } });
  const card = (key) => text(itemBody(shared, { body, access: "ready", ui: ui(), candidates: shared.machines, chosen, now: NOW, edits: { machine: chosen, editing: null, sentFor: (one) => (one === key ? { sent: kept, state: "done", standing: null, live: true, saving: true, recent: true } : null) } }));
  assert.match(card("goal"), /Goal Saving…/);
  assert.match(card(null), /Saving…/);
});

test("what tasks found sits in its own group after the filed items, each row saying which tasks found it and how often", () => {
  const found = row("found:f3", { title: "The client can't tell a missing log from an outage", priority: "none", repo: "mindin", home: { kind: "found", label: "found by tasks" }, found: { sources: ["lint-index@mindin", "split-notes@mindin"], count: 3 } });
  const entries = mergeBacklog([machine("mch_mac", "MacBook", [found, row("local:faster", { title: "Faster board", priority: "none" })])], NOW);
  const said = text(backlogList({ entries, now: NOW, top }));
  assert.match(said, /New · 1 Faster board MacBook kebbie · 1 d Found by tasks · 1 The client can't tell a missing log from an outage MacBook mindin · found 3× · by lint-index, split-notes · 1 d$/);
  const once = mergeBacklog([machine("mch_mac", "MacBook", [{ ...found, found: { sources: ["lint-index@mindin"], count: 1 } }])], NOW);
  assert.match(text(backlogList({ entries: once, now: NOW, top })), /mindin · by lint-index · 1 d$/);
});

test("a found item's card names who found it and offers Start a task, Add to each task that found it, and Dismiss", () => {
  const found = row("found:f3", { title: "Logs repeat the token", priority: "none", repo: "mindin", home: { kind: "found", label: "found by tasks" }, found: { sources: ["lint-index@mindin"], count: 2 } });
  const [entry] = mergeBacklog([machine("mch_mac", "MacBook", [found], { accepts: [...ALL, "found-start", "found-add", "found-dismiss"] })], NOW);
  const chosen = { ...entry.machines[0], why: "awake now" };
  const options = { access: "ready", ui: ui(), now: NOW, found: { chosen, tasks: [{ address: "lint-index@mindin", name: "Lint from the index" }], sent: null } };
  const html = itemBody(entry, options);
  assert.match(text(html), /found found by tasks · on MacBook Found by lint-index@mindin found 2 times outside their own work Start a task Add to Lint from the index Dismiss$/);
  assert.match(html, /<button type="button" class="btn" data-action="found-add" data-task="lint-index@mindin">Add to Lint from the index<\/button>/);
  assert.doesNotMatch(html, /Edit title|data-action="start-item"/);
  assert.match(text(itemBody(entry, { ...options, found: { ...options.found, chosen: null } })), /MacBook takes no actions on found items from the hub\. Settle it in kebbie there\.$/);
  const sent = { kind: "found-dismiss", machineName: "MacBook", deviceName: "iPhone", at: NOW - 30, exp: NOW + 3570, result: null };
  assert.match(text(itemBody(entry, { ...options, found: { ...options.found, sent: { sent, state: "waiting" } } })), /Waiting for MacBook It leaves the backlog when MacBook wakes up\./);
  assert.doesNotMatch(itemBody(entry, { ...options, found: { ...options.found, sent: { sent, state: "waiting" } } }), /data-action="found-start"/, "nothing to press while a choice is on its way");
  assert.match(text(itemBody(entry, { ...options, access: "view-only" })), /View only/);
});

const list = (entries, find, textOf) => text(backlogList({ entries, now: NOW, top, find: { q: "", repos: [], priorities: [], states: [], found: false, sort: "priority", ...find }, textOf }));
const asked = row("vault:Projects/asked.md", { title: "Lock screen notice", priority: "low", repo: "hub", state: "drafting", questions: 2, blocking: 1, filed: NOW - 2 * DAY });
const listed = mergeBacklog([machine("mch_mac", "MacBook", [session, asked, row("local:faster", { title: "Faster board", priority: "high", goal: "The board shows the notice within a second." }), row("local:logs", { title: "Archive logs", priority: "low", state: "to-write", filed: NOW - 7200 }), row("found:f1", { title: "Notices double up", priority: "none", home: { kind: "found", label: "found by tasks" }, found: { sources: ["notify@hub"], count: 2 } })], { repos: ["kebbie"] })], NOW);

test("items whose questions wait on you sit once, newest first, in Needs you above every sort and filter", () => {
  const said = list(listed);
  assert.match(said, /All 5 Priority Newest Needs you · 2 Session drops on reload vault webapp · drafting · 2 questions · 1 d you: answer them on its card Lock screen notice vault hub · drafting · 1 question · 2 d you: answer it on its card High · 1 Faster board .* Low · 1 Archive logs .* Found by tasks · 1 Notices double up/);
  assert.equal(said.match(/Session drops on reload/g).length, 1);
  assert.match(list(listed, { sort: "newest" }), /Needs you · 2 Session drops on reload .* Lock screen notice .* Newest first · 3 Archive logs MacBook low · kebbie · to write · 2 h .* Faster board MacBook high · kebbie · 1 d .* Notices double up MacBook kebbie · found 2× · by notify/);
  assert.doesNotMatch(list(listed, { sort: "newest" }), /High · |Low · /);
  assert.match(text(backlogList({ entries: listed, now: NOW, top, find: { q: "", repos: [], priorities: [], states: [], found: false, sort: "newest" } })), /Archive logs MacBook low · kebbie/);
  assert.match(backlogList({ entries: listed, now: NOW, top, find: { q: "", repos: [], priorities: [], states: [], found: false, sort: "newest" } }), /<span class="g-muted">low<\/span> · kebbie/);
  const filtered = list(listed, { repos: ["hub"] });
  assert.match(filtered, /1 of 5 .*Needs you · 1 Lock screen notice/);
  assert.doesNotMatch(filtered, /Session drops/);
  const answered = listed.map((one) => (one.key === "vault:Projects/asked.md" ? { ...one, waiting: 0 } : one));
  assert.match(list(answered), /^Backlog 5 items · 2 questions wait .*Needs you · 1 Session drops on reload .* Low · 2 Archive logs .* Lock screen notice vault hub · writing… · 2 d the lead is writing it out/);
  assert.equal(needsYouCount(listed), 2);
  assert.equal(needsYouCount(answered), 1);
});

test("the toolbar counts what shows, puts active chips first after Clear, and parts the chip kinds", () => {
  const html = backlogList({ entries: listed, now: NOW, top, find: { q: "", repos: ["hub"], priorities: [], states: ["drafting"], found: false, sort: "priority" } });
  const chips = [...html.matchAll(/<button type="button" class="pill[^"]*"[^>]*>(.*?)<\/button>|<span class="sep"/g)].map((match) => (match[1] === undefined ? "|" : text(match[1])));
  assert.deepEqual(chips, ["Clear", "hub 1", "drafting 1", "|", "kebbie 0", "webapp 1", "|", "High 0", "Medium 0", "Low 1", "New 0", "|", "to write 0", "ready 0", "|", "Found by tasks 0"]);
  assert.match(html, /aria-pressed="true">hub <span class="n">1<\/span>/);
  assert.match(text(html), /1 of 5 Priority Newest/);
  assert.match(html, /<input type="search" id="bl-q" placeholder="Search titles and text"[^>]*value=""[^>]*><button type="button" class="clear" data-action="find-q-clear" aria-label="Clear search" hidden>/);
  assert.match(html, /data-action="find-sort" data-sort="priority" aria-pressed="true">Priority/);
  assert.match(text(backlogList({ entries: [], now: NOW, top })), /^Backlog No machine has a backlog on its board yet\.$/);
  assert.match(list(listed, { q: "nothing like this" }), /0 of 5 .* Nothing in the backlog matches\./);
});

test("a search marks the title, and a match only in the item's text shows a snippet line", () => {
  const bodies = { blb_1: { sections: [section("goal", "Goal", "Every notice keeps you signed in.")], questions: [], questions_key: null } };
  const textOf = (entry) => [entry.row.goal, ...(bodies[entry.row.body?.id]?.sections.map((one) => one.text) ?? [])].filter(Boolean);
  const html = backlogList({ entries: listed, now: NOW, top, find: { q: "notice", repos: [], priorities: [], states: [], found: false, sort: "priority" }, textOf });
  assert.match(text(html), /4 of 5/);
  assert.match(html, /<span class="name">Lock screen <mark>notice<\/mark><\/span>/);
  assert.match(html, /<span class="name">Session drops on reload<\/span>.*?<div class="hit">Every <mark>notice<\/mark> keeps you signed in\.<\/div><\/a>/);
  assert.match(html, /<div>The board shows the <mark>notice<\/mark> within a second\.<\/div><\/a>/, "a goal the row shows is marked in place, with no snippet");
  assert.match(html, /<mark>Notice<\/mark>s double up/);
  assert.match(html, /class="clear" data-action="find-q-clear" aria-label="Clear search">/);
});

test("an answer sent from this device takes its question out of the count until the board carries it", () => {
  const entry = listed.find((one) => one.key === "vault:Projects/session.md");
  const sent = (index, fields = {}) => sentAt({ kind: "item-answer", address: entry.key, machine: "mch_mac", index, ...fields });
  const storedAt = () => NOW - 60;
  const live = (sent) => ({ state: sent.result ? "done" : "sending", live: true });
  assert.equal(answersWithLead(entry, [sent(0), sent(0, { id: "cmd_2" }), sent(1, { machine: "mch_other" })], { shown: live, storedAt }), 1);
  assert.equal(answersWithLead(entry, [sent(0, { result: { outcome: "done", at: NOW - 30 } })], { shown: live, storedAt }), 1, "done but not yet on the stored board");
  assert.equal(answersWithLead(entry, [sent(0, { result: { outcome: "done", at: NOW - 90 } })], { shown: live, storedAt }), 0, "the board already counts it");
  assert.equal(answersWithLead(entry, [sent(0)], { shown: () => ({ state: "refused", live: false }), storedAt }), 0);
  assert.equal(answersWithLead(entry, [sent(2)], { shown: live, storedAt, body }), 0, "a question that does not block never counted");
  assert.equal(answersWithLead(entry, [sent(1)], { shown: live, storedAt, body }), 1);
});

test("an item to write offers Write it out where its machine takes it, reads writing… while the lead writes it, and offers it again after a day", () => {
  const toWrite = mergeBacklog([machine("mch_mac", "MacBook", [row("local:logs", { title: "Archive logs", state: "to-write" })], { accepts: [...ALL, "item-write"] })], NOW)[0];
  const write = { chosen: toWrite.machines[0], sent: null };
  const shown = (entry, fields = {}) => itemBody(entry, { access: "ready", ui: ui(), candidates: entry.machines, chosen: entry.machines[0], now: NOW, write, ...fields });
  assert.match(shown(toWrite), /<button type="button" class="btn primary" data-action="write-item">Write it out<\/button>/);
  assert.doesNotMatch(shown(toWrite, { write: { chosen: null, sent: null } }), /write-item/, "a machine on an older kebbie takes no write-out, so none is offered");
  assert.doesNotMatch(shown(toWrite, { access: "view-only" }), /write-item/);
  const asked = { ...toWrite, row: { ...toWrite.row, state: "drafting", writing: NOW - 2 * 3600 } };
  assert.doesNotMatch(shown(asked), /write-item/, "an item being written is not asked again");
  assert.match(text(shown(asked)), /state writing… 2 h/);
  const left = { ...toWrite, row: { ...toWrite.row, state: "drafting", writing: NOW - DAY } };
  assert.match(shown(left), /data-action="write-item">Write it out again</);
  assert.match(text(backlogRow(asked, null, NOW)), /writing… 2 h · /);
  const refused = { sent: sentAt({ kind: "item-write", machine: "mch_mac", machineName: "MacBook", result: { outcome: "refused", reason: "bad-request", text: "Not sent: it is being written out already (writing… 2 h)\nThe lead has it.", at: NOW } }), state: "refused", standing: null, live: false };
  const said = writeArea(toWrite, { access: "ready", write: { ...write, sent: refused }, ui: ui(), now: NOW });
  assert.match(said, /<div class="center warn" role="alert">Not sent: it is being written out already \(writing… 2 h\)<\/div>/, "a refusal reads as its one line");
  const going = { ...refused, sent: { ...refused.sent, result: null }, state: "sending", live: true };
  assert.match(text(writeArea(toWrite, { access: "ready", write: { ...write, sent: going }, ui: ui(), now: NOW })), /^Sent to MacBook The lead writes it out in place\./);
});

test("a row kept after its idea was merged into a note opens that note's card", () => {
  const merged = row("kept:a1b2c3", { title: "Native app · merged into kebbie-ios", home: { kind: "kept", label: "Projects/kebbie-ios.md" } });
  assert.equal(mergedInto(merged), "vault:Projects/kebbie-ios.md");
  assert.equal(mergedInto(row("kept:idea-1", { home: { kind: "kept", label: "kept by kebbie" } })), null);
  assert.equal(mergedInto(row("vault:Projects/x.md")), null);
  const entry = { ...mergeBacklog([machine("mch_mac", "MacBook", [merged])], NOW)[0], opens: "#/item/vault%3AProjects%2Fkebbie-ios.md" };
  assert.match(backlogRow(entry, null, NOW), /href="#\/item\/vault%3AProjects%2Fkebbie-ios\.md"/);
});

test("the same vault note in two accounts stays two rows, and each row's machines are its own account's only", () => {
  const note = row("vault:Projects/round.md", { title: "Round invoices", priority: "high" });
  const tagged = (account, made) => ({ ...made, account });
  const entries = mergeBacklog([tagged("acc_home", machine("mch_mac", "MacBook", [note])), tagged("acc_home", machine("mch_mini", "mini", [note])), tagged("acc_work", machine("mch_work", "work-mac", [note]))], NOW);
  assert.deepEqual(entries.map((entry) => [entry.id, entry.account, entry.machines.map((one) => one.machine)]), [
    ["vault:Projects/round.md", "acc_home", ["mch_mac", "mch_mini"]],
    ["acc_work|vault:Projects/round.md", "acc_work", ["mch_work"]],
  ]);
  assert.equal(entries[1].key, "vault:Projects/round.md", "the command still names the note itself");
});

test("New offers Start now on the rule's machine and Add to the backlog, starts in the first repo unless another is picked, and sends a start the machine names", () => {
  const mac = { machine: "mch_mac", name: "MacBook", accepts: ["idea", "start"], asleep: false, repos: ["shop", "kebbie"], starts: true };
  const both = ideaForm({ text: "Fix the rounding in invoices", candidates: [mac], ...newTargets([mac]) });
  assert.match(text(both), /What do you want done\? Start it now, or add it to the backlog .* Goes to the lead on MacBook .* Starts in shop kebbie Starts on MacBook .* Start now on MacBook Add to the backlog$/);
  assert.match(both, /data-action="start-now">Start now on MacBook</);
  assert.match(both, /<option value="shop" selected>/);
  assert.match(ideaForm({ text: "", candidates: [mac], ...newTargets([mac]) }), /data-action="start-now" disabled/);
  assert.equal(newTargets([mac], "kebbie").repo, "kebbie");
  assert.equal(newTargets([{ ...mac, repos: [] }]).starter, null);
  const ideasOnly = { ...mac, accepts: ["idea"], starts: false };
  assert.doesNotMatch(ideaForm({ text: "x", candidates: [ideasOnly], ...newTargets([ideasOnly]) }), /start-now/, "an older kebbie that takes no starts offers only the backlog");
  const startsOnly = { ...mac, accepts: ["start"] };
  assert.doesNotMatch(ideaForm({ text: "x", candidates: [startsOnly], ...newTargets([startsOnly]) }), /add-idea/);
  assert.doesNotMatch(ideaForm({ text: "x", candidates: [{ ...mac, repos: [] }], ...newTargets([{ ...mac, repos: [] }]) }), /start-now/, "with no repo on its board there is nowhere to start");
  assert.deepEqual(startCommand({ text: "  Fix the rounding in invoices \n", repo: "shop" }).payload, { type: "start", repo: "shop", name: "", prompt: "Fix the rounding in invoices" });
});
test("a start from New says it is on its way, then the machine's result: started as its name, or the refusal with the words to try again", () => {
  const sent = { id: "cmd_s", kind: "start", machine: "mch_mac", machineName: "MacBook", repo: "shop", text: "Fix it", at: NOW - 5, exp: NOW + 3_000, deviceName: "iPhone 16" };
  assert.match(text(newStartStatus(sent, "sending", { now: NOW })), /Sent to MacBook The task starts in shop\./);
  const done = { ...sent, result: { outcome: "done", text: "Started as fix-it@shop. It shows under Work in a moment.", at: NOW } };
  assert.match(text(newStartStatus(done, "done", { now: NOW })), /✓ Started on MacBook Started as fix-it@shop\./);
  const refused = { ...sent, result: { outcome: "refused", reason: "not-allowed", text: "Not done: MacBook takes no start from the hub", at: NOW } };
  const said = newStartStatus(refused, "refused", { now: NOW });
  assert.match(text(said), /MacBook takes no start from the hub/);
  assert.match(said, /data-action="start-again"/);
});

test("with two accounts New names the account in its question and its button, with one it reads as before", () => {
  const candidates = [{ machine: "mch_devbox", name: "devbox", accepts: ["idea"], asleep: false, repos: [], ideas: true }];
  const targets = newTargets(candidates);
  const plain = ideaForm({ text: "Mail a summary", candidates, ...targets });
  assert.match(text(plain), /Add to the backlog$/);
  const work = ideaForm({ text: "Mail a summary", candidates, ...targets, picker: "<div>PICKER</div>", account: "Work" });
  assert.match(text(work), /^PICKER What do you want done\? .* Add to Work's backlog$/);
});
