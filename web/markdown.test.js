import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import { markdown } from "./markdown.js";

const escape = (text) => String(text).replace(/[&<>"]/g, (char) => `&#${char.charCodeAt(0)};`);

function node(name) {
  return {
    name,
    attributes: [],
    children: [],
    className: "",
    append(...children) {
      this.children.push(...children.map((child) => (typeof child === "string" ? { text: child } : child)));
    },
    setAttribute(key, value) {
      this.attributes.push([key, String(value)]);
    },
  };
}

const doc = { createElement: node, createTextNode: (text) => ({ text }) };

function html(nodes) {
  return nodes
    .map((item) => {
      if ("text" in item) return escape(item.text);
      const attributes = [...(item.className ? [["class", item.className]] : []), ...item.attributes].map(([key, value]) => (value === "" ? ` ${key}` : ` ${key}="${escape(value)}"`)).join("");
      return item.name === "input" || item.name === "hr" ? `<${item.name}${attributes}>` : `<${item.name}${attributes}>${html(item.children)}</${item.name}>`;
    })
    .join("");
}

const render = (text) => html(markdown(text, doc));

function elements(nodes) {
  return nodes.flatMap((item) => ("text" in item ? [] : [item, ...elements(item.children)]));
}

test("paragraphs split on blank lines and a single newline is a space", () => {
  assert.equal(render("One line\nand the next.\n\nA second paragraph."), "<p>One line and the next.</p><p>A second paragraph.</p>");
});

test("a top heading renders as h2 inside the card, the third level as h3", () => {
  assert.equal(render("# Plan\n## Steps\n### Look\n#### Deeper ##"), "<h2>Plan</h2><h2>Steps</h2><h3>Look</h3><h3>Deeper</h3>");
});

test("ordered and unordered lists keep their numbering and one nested level", () => {
  assert.equal(render("1. Add the gateway\n2. Book the entry\n   - on confirmation\n   - not on request\n3. Point the button"), "<ol><li>Add the gateway</li><li>Book the entry<ul><li>on confirmation</li><li>not on request</li></ul></li><li>Point the button</li></ol>");
  assert.equal(render("- one\n* two\n\n3. three\n4. four"), "<ul><li>one</li><li>two</li></ul><ol start=\"3\"><li>three</li><li>four</li></ol>");
  assert.equal(render("- a wrapped\n  item\n- next"), "<ul><li>a wrapped item</li><li>next</li></ul>");
});

test("task lists show disabled checkboxes, ticked where the text says so", () => {
  assert.equal(render("- [ ] Open\n- [x] Done with `code`"), '<ul class="md-check"><li><input type="checkbox" disabled><span>Open</span></li><li><input type="checkbox" disabled checked><span>Done with <code>code</code></span></li></ul>');
});

test("inline code, bold and italic, and snake_case stays as written", () => {
  assert.equal(render("Use `refunds.create` with **one** key, *not* two, __bold__ _too_: idempotency_key_name"), "<p>Use <code>refunds.create</code> with <strong>one</strong> key, <em>not</em> two, <strong>bold</strong> <em>too</em>: idempotency_key_name</p>");
  assert.equal(render("a * b and 2 * 3 and \\*escaped\\*"), "<p>a * b and 2 * 3 and *escaped*</p>");
});

test("a fenced block keeps its text exactly, markup and all", () => {
  assert.equal(render("```json\n{ \"a\": 1 }\n  <b>**not bold**</b>\n```\nafter"), "<pre><code>{ &#34;a&#34;: 1 }\n  &#60;b&#62;**not bold**&#60;/b&#62;</code></pre><p>after</p>");
  assert.equal(render("~~~\nunclosed"), "<pre><code>unclosed</code></pre>");
});

test("a pipe table renders inside a scrolling wrapper", () => {
  assert.equal(render("| Path | Today |\n| --- | :-: |\n| `/a\\|b` | books |\n| /refunds |"), '<div class="md-table"><table><thead><tr><th>Path</th><th>Today</th></tr></thead><tbody><tr><td><code>/a|b</code></td><td>books</td></tr><tr><td>/refunds</td><td></td></tr></tbody></table></div>');
});

test("a blockquote holds its prose directly, and blocks when it has them", () => {
  assert.equal(render("> DNS changes reach every resolver\n> within an hour."), "<blockquote>DNS changes reach every resolver within an hour.</blockquote>");
  assert.equal(render("> one\n>\n> - two"), "<blockquote><p>one</p><ul><li>two</li></ul></blockquote>");
});

test("only https links become links; every other scheme stays text", () => {
  assert.equal(render("[docs](https://example.com/a?b=1)"), '<p><a href="https://example.com/a?b=1" target="_blank" rel="noopener noreferrer">docs</a></p>');
  for (const target of ["javascript:alert(1)", "http://example.com", "data:text/html,x", "JAVASCRIPT:alert(1)", "/relative", "vbscript:x"]) {
    const out = markdown(`[x](${target})`, doc);
    assert.equal(html(out), `<p>${escape(`[x](${target})`)}</p>`, target);
    assert.equal(elements(out).some((item) => item.name === "a"), false, target);
  }
});

test("HTML in the text is inert wherever it appears", () => {
  const hostile = [
    "<script>alert(1)</script>",
    "<img src=x onerror=alert(1)>",
    "[x](javascript:alert(1))",
    "# <script>alert(1)</script>",
    "- <img src=x onerror=alert(1)>",
    "| <script>x</script> | b |\n| - | - |\n| <img src=x onerror=alert(1)> | c |",
    "> <iframe src=javascript:alert(1)>",
    "**<svg onload=alert(1)>**",
    "`<script>alert(1)</script>`",
    "[<img src=x onerror=alert(1)>](https://example.com)",
  ].join("\n\n");
  const out = markdown(hostile, doc);
  const all = elements(out);
  const allowed = new Set(["p", "h2", "h3", "ul", "li", "div", "table", "thead", "tbody", "tr", "th", "td", "blockquote", "strong", "code", "a"]);
  assert.deepEqual(all.filter((item) => !allowed.has(item.name)).map((item) => item.name), []);
  assert.deepEqual(all.flatMap((item) => item.attributes.map(([key]) => key)).filter((key) => key !== "href" && key !== "target" && key !== "rel"), []);
  assert.deepEqual(all.filter((item) => item.name === "a").map((item) => item.attributes[0][1]), ["https://example.com/"]);
  const said = html(out);
  for (const raw of ["<script", "<img", "<iframe", "<svg"]) assert.equal(said.includes(raw), false, raw);
  assert.match(said, /&#60;script&#62;alert\(1\)&#60;\/script&#62;/);
  assert.match(said, /\[x\]\(javascript:alert\(1\)\)/);
});

test("the sample plan renders as the approved reading", () => {
  const plan = JSON.parse(readFileSync(new URL("../vectors/blobs.json", import.meta.url), "utf8")).plaintext;
  assert.equal(
    render(plan),
    "<p>Move refunds from the old charge endpoint to the gateway's <code>refunds.create</code>, so each refund books its own ledger entry when the gateway confirms it.</p>" +
      "<h2>Steps</h2><ol><li>Add <code>Gateway#refund</code> with one idempotency key per refund.</li><li>Book the refund ledger entry on the gateway's confirmation, not on the request.</li><li>Point the portal's refund button at the new path.</li><li>Keep <code>/charges/:id/refund</code> answering for one release and log every call.</li></ol>" +
      "<h2>What the customer sees</h2><p>Nothing new. A refund shows on the billing page the minute the gateway confirms it.</p>" +
      "<h2>Risks</h2><ul><li>An app version older than 2.14 still calls the old endpoint.</li><li>A double tap sends two requests; the idempotency key stops the second refund.</li></ul>" +
      '<h2>Paths</h2><div class="md-table"><table><thead><tr><th>Path</th><th>Today</th><th>After</th></tr></thead><tbody><tr><td><code>/charges/:id/refund</code></td><td>books on request</td><td>logs, then forwards</td></tr><tr><td><code>/refunds</code></td><td>none</td><td>books on confirm</td></tr></tbody></table></div>' +
      "<h2>Request</h2><pre><code>POST /v1/refunds\n{ &#34;charge&#34;: &#34;ch_3P9xQ2&#34;, &#34;amount&#34;: 12500,\n  &#34;idempotency_key&#34;: &#34;refund-8812-1&#34; }</code></pre>" +
      '<h2>Tests</h2><ul class="md-check"><li><input type="checkbox" disabled><span>A second refund with the same key books nothing</span></li><li><input type="checkbox" disabled><span>The old endpoint logs and forwards</span></li><li><input type="checkbox" disabled><span>The ledger entry is dated on confirmation</span></li></ul>',
  );
});
