import assert from "node:assert/strict";
import { test } from "node:test";

import { fieldAction, hasTextField, keepFieldsAboveKeyboard, keepInView, revealDelta } from "./keyboard.js";

class Box {
  constructor(tag, { top = 0, height = 0, laidOut = true, scrolls = false, id = "", primary = false } = {}) {
    Object.assign(this, { tag, top, height, laidOut, scrolls, id, primary, children: [], parentElement: null, scrolled: 0, listeners: {} });
    this.classes = new Set();
  }
  add(...kids) {
    for (const kid of kids) {
      kid.parentElement = this;
      this.children.push(kid);
    }
    return this;
  }
  get ownerDocument() { return this.doc ?? this.parentElement?.ownerDocument; }
  get offsetParent() { return this.laidOut ? this.parentElement : null; }
  get scrollHeight() { return this.scrolls ? 5000 : this.height; }
  get clientHeight() { return this.height; }
  all() { return this.children.flatMap((kid) => [kid, ...kid.all()]); }
  order() { return this.ownerDocument.body.all(); }
  matches(selector) {
    if (selector.startsWith("button.primary")) return this.tag === "button" && this.primary;
    if (selector.startsWith("button")) return this.tag === "button";
    return this.tag === "textarea" || this.tag === "input";
  }
  querySelectorAll(selector) { return this.all().filter((one) => one.matches(selector) && (selector.startsWith("button") || false)); }
  compareDocumentPosition(other) { const all = this.order(); return all.indexOf(other) > all.indexOf(this) ? 4 : 2; }
  getBoundingClientRect() { const top = this.top - (this.parentScroller()?.scrolled ?? 0); return { top, bottom: top + this.height }; }
  parentScroller() { for (let box = this.parentElement; box; box = box.parentElement) if (box.scrolls) return box; return null; }
  closest(selector) { for (let box = this; box; box = box.parentElement) if (selector === ".composer" && box.classes.has("composer")) return box; return null; }
  contains(other) { return this.all().includes(other); }
  scrollBy(x, y) { this.scrolled += y; }
  addEventListener(name, run) { this.listeners[name] = run; }
}

function page({ scrolls = false } = {}) {
  const doc = { activeElement: null };
  const body = new Box("body");
  body.doc = doc;
  doc.body = body;
  const pane = new Box("main", { top: 0, height: 1000, scrolls });
  body.add(pane);
  const field = new Box("textarea", { top: 700, height: 60, id: "answer-0" });
  const send = new Box("button", { top: 772, height: 44, primary: true });
  const row = new Box("div").add(new Box("div").add(field), send);
  pane.add(new Box("button", { top: 100, height: 44 }), row);
  const win = { scrolled: 0, scrollBy(x, y) { this.scrolled += y; }, getComputedStyle: (box) => ({ overflowY: box.scrolls ? "auto" : "visible" }), setTimeout: (run) => run(), requestAnimationFrame: (run) => run() };
  return { doc, body, pane, field, send, win };
}

const keyboardUp = { offsetTop: 0, height: 560 };

test("a span below the keyboard scrolls up until its bottom clears it", () => {
  assert.equal(revealDelta({ top: 700, bottom: 830 }, { top: 0, bottom: 560 }), 270);
});

test("a span taller than the band stops scrolling once its top reaches the band's top", () => {
  assert.equal(revealDelta({ top: 300, bottom: 1200 }, { top: 0, bottom: 560 }), 300);
});

test("a span above the band comes down to it, and one inside it stays", () => {
  assert.equal(revealDelta({ top: -40, bottom: 100 }, { top: 0, bottom: 560 }), -40);
  assert.equal(revealDelta({ top: 100, bottom: 300 }, { top: 0, bottom: 560 }), 0);
});

test("the action kept with a field is the first laid-out button after it, not one above it", () => {
  const { field, send, body } = page();
  assert.equal(fieldAction(field, body), send);
  send.laidOut = false;
  assert.equal(fieldAction(field, body), null, "a fixed or hidden button is left to the CSS lift");
});

test("a primary button further on wins over a plain one between, as one Send under several questions", () => {
  const { field, body, pane } = page();
  const row = field.parentElement.parentElement;
  const send = row.children[1];
  row.children.splice(1, 1);
  const next = new Box("div").add(new Box("button", { top: 850, height: 30 }));
  const all = new Box("div").add(send);
  pane.add(next, all);
  assert.equal(fieldAction(field, body), send);
});

test("a focused answer under the keyboard scrolls the page until its Send button shows above it", () => {
  const { field, win } = page();
  assert.equal(keepInView(field, keyboardUp, win), 772 + 44 + 12 - 560);
  assert.equal(win.scrolled, 268);
});

test("in a pane that scrolls on its own, the pane scrolls rather than the page", () => {
  const { field, pane, win } = page({ scrolls: true });
  keepInView(field, keyboardUp, win);
  assert.equal(pane.scrolled, 268);
  assert.equal(win.scrolled, 0);
});

test("the visual viewport iOS panned down by offsetTop counts from where it now starts", () => {
  const { field, win } = page();
  assert.equal(keepInView(field, { offsetTop: 200, height: 560 }, win), 68);
});

test("fields lift only while the keyboard is open, and the Lead composer is left to its own rules", () => {
  const { doc, body, field, win } = page();
  const root = body.children[0];
  const html = { classList: new Set() };
  html.classList.contains = html.classList.has;
  const view = { ...keyboardUp, addEventListener(name, run) { this[name] = run; } };
  keepFieldsAboveKeyboard(root, view, html, win);
  doc.activeElement = field;
  root.listeners.focusin();
  assert.equal(win.scrolled, 0, "no keyboard, as on a desktop: nothing moves");
  html.classList.add("kb-open");
  view.resize();
  assert.equal(win.scrolled, 268);
  field.parentElement.classes.add("composer");
  view.resize();
  assert.equal(win.scrolled, 268);
});

test("a Send that appears only once something is typed is brought above the keyboard as the first letter lands", () => {
  const { doc, body, field, send, win } = page();
  const root = body.children[0];
  const html = { classList: new Set(["kb-open"]) };
  html.classList.contains = html.classList.has;
  const view = { ...keyboardUp, addEventListener() {} };
  send.laidOut = false;
  send.top = 900;
  keepFieldsAboveKeyboard(root, view, html, win);
  doc.activeElement = field;
  root.listeners.focusin();
  assert.equal(win.scrolled, 700 + 60 + 12 - 560, "while Send is hidden only the field is brought up");
  send.laidOut = true;
  root.listeners.input();
  assert.equal(win.scrolled, 212 + (900 + 44 + 12 - 560), "typing brings up the Send that appeared");
});

test("every screen with a text field turns the keyboard lift on, and one with only switches does not", () => {
  assert.equal(hasTextField('<textarea id="answer-0"></textarea>'), true);
  assert.equal(hasTextField('<input type="search" id="bl-q">'), true);
  assert.equal(hasTextField('<input id="edit-title" data-draft="item-edit">'), true);
  assert.equal(hasTextField('<input type="checkbox" class="switch">'), false);
  assert.equal(hasTextField("<section>no fields</section>"), false);
});
