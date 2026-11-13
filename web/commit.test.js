import assert from "node:assert/strict";
import { test } from "node:test";

import { commitBlocks, commitBody } from "./commit.js";

test("lines hard-wrapped within a paragraph join, and a blank line starts the next paragraph", () => {
  assert.deepEqual(commitBlocks("Orders charge through the gateway trait,\nso the new provider slots in next.\n\nThe old client stays for one release."), [
    { kind: "text", text: "Orders charge through the gateway trait, so the new provider slots in next." },
    { kind: "text", text: "The old client stays for one release." },
  ]);
  assert.deepEqual(commitBlocks("One line\r\nwrapped\r\n"), [{ kind: "text", text: "One line wrapped" }]);
  assert.deepEqual(commitBlocks(""), []);
});

test("each list item starts its own line and its wrapped lines join it", () => {
  assert.deepEqual(commitBlocks("Why:\n- the runner drops\n  the push on a timeout\n* retries were silent\n1. first\n2) second"), [
    { kind: "text", text: "Why:" },
    { kind: "item", text: "- the runner drops the push on a timeout" },
    { kind: "item", text: "* retries were silent" },
    { kind: "item", text: "1. first" },
    { kind: "item", text: "2) second" },
  ]);
});

test("code keeps its lines, indented after a blank line or fenced", () => {
  assert.deepEqual(commitBlocks("Run it:\n\n    cargo test\n      --workspace\n\nThen push."), [
    { kind: "text", text: "Run it:" },
    { kind: "code", text: "cargo test\n  --workspace" },
    { kind: "text", text: "Then push." },
  ]);
  assert.deepEqual(commitBlocks("Before:\n```\nfn a() {\n\n    b()\n}\n```\nAfter"), [
    { kind: "text", text: "Before:" },
    { kind: "code", text: "fn a() {\n\n    b()\n}" },
    { kind: "text", text: "After" },
  ]);
});

test("trailers keep one per line", () => {
  assert.deepEqual(commitBlocks("Body text.\n\nSigned-off-by: A <a@x>\nCo-authored-by: B <b@x>"), [
    { kind: "text", text: "Body text." },
    { kind: "lines", lines: ["Signed-off-by: A <a@x>", "Co-authored-by: B <b@x>"] },
  ]);
});

test("the body renders as escaped paragraphs, list lines and code", () => {
  assert.match(commitBody("1. first\n2) second"), /<span class="cb-m">1\.<\/span><span>first<\/span>.*<span class="cb-m">2\)<\/span><span>second<\/span>/);
  assert.equal(commitBody("A <b>\nwrapped\n\n- one\n\n    x < y"), `<p class="cb-p">A &#60;b&#62; wrapped</p><p class="cb-li"><span class="cb-m">•</span><span>one</span></p><pre class="cb-code">x &#60; y</pre>`);
});
