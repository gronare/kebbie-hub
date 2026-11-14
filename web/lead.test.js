import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { awayOn, chosenLead, composerHtml, leadHead, leadMachines, leadStatus, pendingMessages, pickRow, previewOf, threadHtml, threadOf, unseenReplies } from "./lead.js";
import { blobReader } from "./blobs.js";
import { fromB64 } from "../js/protocol.js";

const vectors = JSON.parse(readFileSync(new URL("../vectors/lead.json", import.meta.url), "utf8"));
const NOW = vectors.page.turns.at(-1).at + 60;
const PHONE = vectors.page.turns[2].device;
const nameOf = (device) => (device === PHONE ? "iPhone" : "another device");
const text = (html) => html.replace(/<[^>]+>/g, " ").replace(/\s+/g, " ").trim();
const machine = (overrides = {}) => ({ machine: "mch_a", name: "MacBook", lastSeen: NOW - 10, board: { ...vectors.board, lead: { ...vectors.board.lead } }, ...overrides });
const peekOf = (pages) => (id) => (pages[id] ? { lead: pages[id] } : null);
const both = { [vectors.blob.id]: vectors.page, [vectors.older_blob.id]: vectors.older_page };

test("the thread opens the newest page first and walks to older pages only when asked", () => {
  const newest = threadOf(vectors.board.lead, peekOf(both));
  assert.equal(newest.turns.length, vectors.page.turns.length);
  assert.deepEqual(newest.older, vectors.older_blob);
  const all = threadOf(vectors.board.lead, peekOf(both), 1);
  assert.equal(all.turns.length, vectors.older_page.turns.length + vectors.page.turns.length);
  assert.equal(all.turns[0].id, vectors.older_page.turns[0].id, "older turns come first");
  assert.equal(all.older, null);
  assert.deepEqual(threadOf(vectors.board.lead, peekOf({}), 0).missing, vectors.blob);
  assert.deepEqual(threadOf(vectors.board.lead, peekOf({ [vectors.blob.id]: vectors.page }), 1).missing, vectors.older_blob);
  assert.equal(threadOf(vectors.board.lead, () => ({ failed: true })).failed, true);
});

test("a page opens from its sealed blob through the blob reader as the lead's thread", async () => {
  const reader = blobReader(async (id) => fromB64(id === vectors.blob.id ? vectors.ciphertext : vectors.older_ciphertext));
  assert.deepEqual((await reader.load(vectors.blob)).lead, vectors.page);
  assert.deepEqual((await reader.load(vectors.older_blob)).lead, vectors.older_page);
});

test("each turn says who said it and from where, and a message from a device says how it was delivered", () => {
  const thread = threadOf(vectors.board.lead, peekOf(both));
  const html = threadHtml({ machine: machine(), thread, pending: [], now: NOW, nameOf });
  const said = text(html);
  assert.match(html, /^<div class="thread"><button type="button" class="btn fit older" data-action="lead-older">Earlier messages<\/button>/);
  assert.match(said, /at the computer · \d\d:\d\d/);
  assert.match(said, /from iPhone · \d\d:\d\d · delivered \d\d:\d\d/);
  assert.match(said, /Waiting for the lead to finish its turn/);
  assert.match(said, /Away mode on · from iPhone · \d\d:\d\d/);
  assert.equal((html.match(/class="msg theirs"/g) ?? []).length, 3, "the lead's replies, never a tool call");
  assert.match(html, /<div class="bubble md" data-markdown>Understood\. While you&#39;re away/);
  assert.match(html, /<div class="working"><span class="spinner"><\/span>Working…<\/div>/);
  const draft = { ...thread, turns: thread.turns.map((turn) => (turn.delivery?.state === "waiting-turn" ? { ...turn, delivery: { state: "waiting-draft" } } : turn)) };
  assert.match(text(threadHtml({ machine: machine(), thread: draft, pending: [], now: NOW, nameOf })), /Waiting: a draft is in the lead's input/);
  const idle = machine();
  idle.board.lead.working_since = null;
  assert.doesNotMatch(threadHtml({ machine: idle, thread, pending: [], now: NOW, nameOf }), /Working…/);
  assert.match(threadHtml({ machine: machine(), thread: { turns: [{ by: "lead", id: "t", at: NOW, text: "<img src=x onerror=alert(1)>" }], older: null }, pending: [], now: NOW, nameOf }), /&#60;img src=x/);
});

test("a message the machine has not picked up waits in the hub while it sleeps, and says so", () => {
  const asleep = machine({ lastSeen: NOW - 3600 });
  const sent = { id: "cmd_held", kind: "lead-message", machine: "mch_a", text: "Run the suite when you wake.", at: NOW - 30, exp: NOW - 30 + 7 * 86_400, deviceName: "iPhone", result: null };
  const pending = pendingMessages([sent, { ...sent, id: "cmd_other", machine: "mch_b" }], asleep, new Set(), NOW);
  assert.deepEqual(pending.map((one) => [one.sent.id, one.state]), [["cmd_held", "waiting"]]);
  const said = text(threadHtml({ machine: asleep, thread: { turns: [], older: null }, pending, now: NOW, nameOf }));
  assert.match(said, /Run the suite when you wake\. Waiting for MacBook to wake/);
  assert.match(said, /Held in the hub MacBook has been asleep since \d\d:\d\d\. The lead gets your message when the machine wakes\./);
  assert.deepEqual(pendingMessages([sent], asleep, new Set(["cmd_held"]), NOW), [], "once in the thread, the thread says it");
  const expired = pendingMessages([sent], asleep, new Set(), sent.exp + 1);
  assert.match(text(threadHtml({ machine: asleep, thread: { turns: [], older: null }, pending: expired, now: sent.exp + 1, nameOf })), /Not delivered: MacBook stayed asleep for a week/);
  const refused = pendingMessages([{ ...sent, result: { outcome: "refused", reason: "no-lead", text: "No lead runs on this machine." } }], machine(), new Set(), NOW);
  assert.match(text(threadHtml({ machine: machine(), thread: { turns: [], older: null }, pending: refused, now: NOW, nameOf })), /Not delivered: No lead runs on this machine\./);
});

test("away mode is one switch: on when the latest switch has not reached a machine yet, otherwise as the boards say", () => {
  const off = { ...machine(), board: { ...vectors.board, away_since: null } };
  assert.equal(awayOn([], [off]), false);
  assert.equal(awayOn([], [machine()]), true);
  const sentOn = { kind: "away", machine: "mch_a", on: true, at: NOW };
  assert.equal(awayOn([sentOn], [off]), true, "sent but not yet applied");
  assert.equal(awayOn([{ ...sentOn, on: false }], [machine()]), false);
  assert.match(leadHead(machine(), { now: NOW, away: true, picking: false, canPick: true }), /<input type="checkbox" class="switch" name="away" data-action="lead-away" checked aria-label="Away mode"><\/label><\/div><div class="away-on">/);
});

test("the machine picker lists each machine with a lead, awake first, with its status, its last word and a mark for an unread reply", () => {
  const asleep = machine({ machine: "mch_b", name: "Devbox", lastSeen: NOW - 7200 });
  const none = { machine: "mch_c", name: "Old", lastSeen: NOW, board: { ...vectors.board, lead: undefined, accepts: ["answer"] } };
  const listed = leadMachines([asleep, none, machine()], NOW);
  assert.deepEqual(listed.map((one) => one.machine), ["mch_a", "mch_b"]);
  assert.equal(chosenLead(listed, null, NOW).machine, "mch_a");
  assert.equal(chosenLead(listed, "mch_b", NOW).machine, "mch_b");
  assert.equal(leadStatus(machine(), NOW), "working");
  assert.match(leadStatus(asleep, NOW), /^asleep since /);
  assert.equal(unseenReplies(listed, (id) => (id === "mch_a" ? vectors.board.lead.replied_at : 0)), 1);
  const row = pickRow(asleep, { now: NOW, selected: false, when: "Yesterday", preview: "The suite is green", unseen: true });
  assert.equal(text(row), "Devbox 1 Yesterday Lead · asleep since " + leadStatus(asleep, NOW).slice("asleep since ".length) + " The suite is green");
  assert.match(row, /^<a class="task mpick" href="#\/lead\/mch_b">/);
  assert.equal(previewOf([{ by: "lead", at: 1, text: "## Done\n`email-column` is at **step 3**" }], []), "Done");
  assert.equal(previewOf([{ by: "you", at: 1, text: "Go" }], [{ sent: { at: 2, text: "And hold the push" } }]), "You: And hold the push");
});

test("the composer is a 16px field and a round Send that stays off until there is something to send", () => {
  assert.match(composerHtml(machine(), { draft: "", busy: false, error: null, takes: true }), /<textarea id="lead-say" rows="1" placeholder="Message the lead" aria-label="Message the lead on MacBook" enterkeyhint="send"><\/textarea><button type="button" class="send" data-action="lead-send" aria-label="Send" disabled>/);
  assert.doesNotMatch(composerHtml(machine(), { draft: "Go", busy: false, error: null, takes: true }), /disabled/);
  assert.match(composerHtml(machine(), { draft: "Go", busy: false, error: "Not sent", takes: true }), /<div class="composer-error" role="alert">Not sent<\/div>/);
  assert.equal(text(composerHtml(machine(), { draft: "", busy: false, error: null, takes: false })), "MacBook's kebbie can't take messages from the app yet.");
  const css = readFileSync(new URL("./app.css", import.meta.url), "utf8");
  assert.match(css, /\.composer textarea \{[^}]*font: 16px/, "16px keeps iOS from zooming into the field");
});
