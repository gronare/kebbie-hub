import { esc } from "./screens.js";

const FENCE = /^\s*(```|~~~)/;
const LIST_ITEM = /^\s*(?:[-*+•]|\d+[.)])\s+/;
const INDENTED = /^(?: {4}|\t)/;
const BULLETS = ["-", "*", "+", "•"];
const TRAILER = /^[A-Za-z][A-Za-z0-9-]*: \S/;

export function commitBlocks(body) {
  const blocks = [];
  let open = null;
  let fence = null;
  const close = () => {
    if (open) blocks.push(open);
    open = null;
  };
  for (const line of (body ?? "").replace(/\r\n?/g, "\n").split("\n")) {
    if (fence) {
      if (line.trim().startsWith(fence)) {
        close();
        fence = null;
      } else open.lines.push(line);
      continue;
    }
    if (FENCE.test(line)) {
      close();
      fence = line.trim().slice(0, 3);
      open = { kind: "code", lines: [] };
      continue;
    }
    if (!line.trim()) {
      if (open?.kind === "code") open.lines.push("");
      else close();
      continue;
    }
    if (open?.kind === "code" && INDENTED.test(line)) {
      open.lines.push(line);
      continue;
    }
    if (LIST_ITEM.test(line)) {
      close();
      open = { kind: "item", lines: [line.trim()] };
      continue;
    }
    if (!open && INDENTED.test(line)) {
      open = { kind: "code", lines: [line] };
      continue;
    }
    if (open?.kind === "code") close();
    if (!open) open = { kind: "text", lines: [] };
    open.lines.push(line.trim());
  }
  close();
  return blocks.map((block) => (block.kind === "code" ? { kind: "code", text: trimCode(block.lines) } : block.kind === "text" && block.lines.every((line) => TRAILER.test(line)) ? { kind: "lines", lines: block.lines } : { kind: block.kind, text: block.lines.join(" ") }));
}

function trimCode(lines) {
  while (lines.length && !lines[lines.length - 1].trim()) lines.pop();
  const indent = Math.min(...lines.filter((line) => line.trim()).map((line) => line.match(/^\s*/)[0].length));
  return lines.map((line) => line.slice(Number.isFinite(indent) ? indent : 0)).join("\n");
}

export function commitBody(body) {
  return commitBlocks(body)
    .map((block) => {
      if (block.kind === "code") return `<pre class="cb-code">${esc(block.text)}</pre>`;
      if (block.kind === "lines") return `<p class="cb-p">${block.lines.map(esc).join("<br>")}</p>`;
      if (block.kind === "item") {
        const [, marker, text] = /^(\S+)\s+([\s\S]*)$/.exec(block.text);
        return `<p class="cb-li"><span class="cb-m">${esc(BULLETS.includes(marker) ? "•" : marker)}</span><span>${esc(text)}</span></p>`;
      }
      return `<p class="cb-p">${esc(block.text)}</p>`;
    })
    .join("");
}
