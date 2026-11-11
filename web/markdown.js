const FENCE = /^ {0,3}(`{3,}|~{3,})/;
const HEADING = /^ {0,3}(#{1,6})[ \t]+(.*?)(?:[ \t]+#+)?[ \t]*$/;
const ITEM = /^( *)([-*+]|\d{1,9}[.)])[ \t]+(.*)$/;
const QUOTE = /^ {0,3}> ?(.*)$/;
const RULE = /^ {0,3}([-*_])(?:[ \t]*\1){2,}[ \t]*$/;
const DELIMITER = /^ *\|? *:?-+:? *(?:\| *:?-+:? *)*\|? *$/;
const TASK = /^\[([ xX])\][ \t]+(.*)$/;

const blank = (line) => line.trim() === "";
const isTableStart = (lines, at) => lines[at].includes("|") && at + 1 < lines.length && lines[at + 1].includes("-") && DELIMITER.test(lines[at + 1]);
const startsBlock = (lines, at) => FENCE.test(lines[at]) || HEADING.test(lines[at]) || ITEM.test(lines[at]) || QUOTE.test(lines[at]) || RULE.test(lines[at]) || isTableStart(lines, at);

function element(doc, name, children = [], className = "") {
  const node = doc.createElement(name);
  if (className) node.className = className;
  for (const child of children) node.append(child);
  return node;
}

function cells(line) {
  const trimmed = line.trim().replace(/^\|/, "").replace(/(?<!\\)\|$/, "");
  const out = [];
  let cell = "";
  let ticks = false;
  for (let index = 0; index < trimmed.length; index += 1) {
    const char = trimmed[index];
    if (char === "\\" && trimmed[index + 1] === "|") {
      cell += "|";
      index += 1;
    } else if (char === "`") {
      ticks = !ticks;
      cell += char;
    } else if (char === "|" && !ticks) {
      out.push(cell.trim());
      cell = "";
    } else cell += char;
  }
  out.push(cell.trim());
  return out;
}

function safeHref(target) {
  try {
    const url = new URL(target);
    return url.protocol === "https:" ? url.href : null;
  } catch {
    return null;
  }
}

function closing(text, from, marker) {
  let index = text.indexOf(marker, from);
  while (index !== -1 && text[index - 1] === "\\") index = text.indexOf(marker, index + 1);
  return index;
}

const wordChar = (char) => char !== undefined && /[\p{L}\p{N}]/u.test(char);

function inline(doc, text) {
  const nodes = [];
  let buffer = "";
  const flush = () => {
    if (buffer) nodes.push(doc.createTextNode(buffer));
    buffer = "";
  };
  let index = 0;
  while (index < text.length) {
    const char = text[index];
    if (char === "\\" && index + 1 < text.length && /[!-/:-@[-`{-~]/.test(text[index + 1])) {
      buffer += text[index + 1];
      index += 2;
      continue;
    }
    if (char === "\n") {
      buffer += " ";
      index += 1;
      continue;
    }
    if (char === "`") {
      const run = text.slice(index).match(/^`+/)[0];
      const end = text.indexOf(run, index + run.length);
      if (end !== -1) {
        flush();
        const code = text.slice(index + run.length, end).replace(/\n/g, " ");
        nodes.push(element(doc, "code", [code.length > 2 && code.startsWith(" ") && code.endsWith(" ") ? code.slice(1, -1) : code]));
        index = end + run.length;
        continue;
      }
    }
    if (char === "[") {
      const link = text.slice(index).match(/^\[([^\]\n]*)\]\(([^()\s]*)\)/);
      if (link) {
        const href = safeHref(link[2]);
        if (href) {
          flush();
          const anchor = element(doc, "a", inline(doc, link[1] || link[2]));
          anchor.setAttribute("href", href);
          anchor.setAttribute("target", "_blank");
          anchor.setAttribute("rel", "noopener noreferrer");
          nodes.push(anchor);
        } else buffer += link[0];
        index += link[0].length;
        continue;
      }
    }
    if (char === "*" || char === "_") {
      const strong = text[index + 1] === char;
      const marker = strong ? char + char : char;
      const opensWord = char === "_" && wordChar(text[index - 1]);
      const next = text[index + marker.length];
      if (!opensWord && next !== undefined && next !== " " && next !== char) {
        const end = closing(text, index + marker.length, marker);
        const after = text[end + marker.length];
        if (end > index + marker.length && text[end - 1] !== " " && !(char === "_" && wordChar(after))) {
          flush();
          nodes.push(element(doc, strong ? "strong" : "em", inline(doc, text.slice(index + marker.length, end))));
          index = end + marker.length;
          continue;
        }
      }
    }
    buffer += char;
    index += 1;
  }
  flush();
  return nodes;
}

function fence(doc, lines, at) {
  const opening = lines[at].match(FENCE)[1];
  const body = [];
  let index = at + 1;
  while (index < lines.length && !new RegExp(`^ {0,3}${opening[0] === "`" ? "`" : "~"}{${opening.length},}[ \\t]*$`).test(lines[index])) {
    body.push(lines[index]);
    index += 1;
  }
  return { node: element(doc, "pre", [element(doc, "code", [body.join("\n")])]), next: index + 1 };
}

function table(doc, lines, at) {
  const head = cells(lines[at]);
  const rows = [];
  let index = at + 2;
  while (index < lines.length && !blank(lines[index]) && lines[index].includes("|")) {
    rows.push(cells(lines[index]));
    index += 1;
  }
  const row = (values, tag) => element(doc, "tr", head.map((_, column) => element(doc, tag, inline(doc, values[column] ?? ""))));
  const grid = element(doc, "table", [element(doc, "thead", [row(head, "th")]), element(doc, "tbody", rows.map((values) => row(values, "td")))]);
  return { node: element(doc, "div", [grid], "md-table"), next: index };
}

function quote(doc, lines, at) {
  const inner = [];
  let index = at;
  while (index < lines.length && QUOTE.test(lines[index])) {
    inner.push(lines[index].match(QUOTE)[1]);
    index += 1;
  }
  const prose = inner.every((line, row) => !blank(line) && !startsBlock(inner, row));
  return { node: element(doc, "blockquote", prose ? inline(doc, inner.join("\n")) : blocks(doc, inner)), next: index };
}

function list(doc, lines, at) {
  const [, indent, marker] = lines[at].match(ITEM);
  const ordered = /\d/.test(marker);
  const items = [];
  let index = at;
  while (index < lines.length) {
    const match = lines[index].match(ITEM);
    if (!match || match[1].length !== indent.length || /\d/.test(match[2]) !== ordered) break;
    const offset = match[1].length + match[2].length + 1;
    const body = [match[3]];
    index += 1;
    while (index < lines.length) {
      const line = lines[index];
      if (blank(line)) {
        const resumes = index + 1 < lines.length && /^ +\S/.test(lines[index + 1]) && lines[index + 1].search(/\S/) > indent.length;
        if (!resumes) break;
        body.push("");
      } else if (line.search(/\S/) > indent.length) body.push(line.slice(Math.min(offset, line.search(/\S/))));
      else if (!startsBlock(lines, index) && !blank(lines[index - 1])) body.push(line.trim());
      else break;
      index += 1;
    }
    items.push(body);
    while (index < lines.length && blank(lines[index]) && index + 1 < lines.length && (lines[index + 1].match(ITEM)?.[1].length ?? -1) === indent.length) index += 1;
  }
  const tasks = items.map((body) => body[0].match(TASK));
  const node = element(doc, ordered ? "ol" : "ul", [], tasks.some(Boolean) ? "md-check" : "");
  const start = ordered ? parseInt(marker, 10) : 1;
  if (start !== 1) node.setAttribute("start", String(start));
  items.forEach((body, number) => {
    const task = tasks[number];
    const lead = task ? [task[2], ...body.slice(1)] : body;
    const split = lead.findIndex((line, row) => row > 0 && (blank(line) || startsBlock(lead, row)));
    const text = split === -1 ? lead : lead.slice(0, split);
    const rest = split === -1 ? [] : lead.slice(split);
    const item = element(doc, "li");
    if (task) {
      const box = doc.createElement("input");
      box.setAttribute("type", "checkbox");
      box.setAttribute("disabled", "");
      if (task[1] !== " ") box.setAttribute("checked", "");
      item.append(box, element(doc, "span", inline(doc, text.join("\n"))));
    } else item.append(...inline(doc, text.join("\n")));
    item.append(...blocks(doc, rest));
    node.append(item);
  });
  return { node, next: index };
}

function paragraph(doc, lines, at) {
  const text = [lines[at].trim()];
  let index = at + 1;
  while (index < lines.length && !blank(lines[index]) && !startsBlock(lines, index)) {
    text.push(lines[index].trim());
    index += 1;
  }
  return { node: element(doc, "p", inline(doc, text.join("\n"))), next: index };
}

function blocks(doc, lines) {
  const nodes = [];
  let index = 0;
  while (index < lines.length) {
    const line = lines[index];
    if (blank(line)) {
      index += 1;
      continue;
    }
    let block;
    const heading = line.match(HEADING);
    if (FENCE.test(line)) block = fence(doc, lines, index);
    else if (heading) block = { node: element(doc, heading[1].length <= 2 ? "h2" : "h3", inline(doc, heading[2])), next: index + 1 };
    else if (RULE.test(line)) block = { node: doc.createElement("hr"), next: index + 1 };
    else if (isTableStart(lines, index)) block = table(doc, lines, index);
    else if (QUOTE.test(line)) block = quote(doc, lines, index);
    else if (ITEM.test(line)) block = list(doc, lines, index);
    else block = paragraph(doc, lines, index);
    nodes.push(block.node);
    index = block.next;
  }
  return nodes;
}

export function markdown(text, doc = globalThis.document) {
  return blocks(doc, String(text).replace(/\r\n?/g, "\n").replace(/\t/g, "    ").split("\n"));
}
