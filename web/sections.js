import { esc, icon, laterRow, nextList, routineLines, taskRow } from "./screens.js";

const plural = (count, one, many) => `${count} ${count === 1 ? one : many}`;
const slug = (key) => key.toLowerCase().replace(/\s+/g, "-");

export function boardSections(view) {
  const next = view.next.length ? [{ key: "next", title: "Next", tone: "peach", count: view.next.reduce((sum, list) => sum + list.rows.length, 0), kind: "next" }] : [];
  const groups = view.groups.map((group) => ({ key: slug(group.key), title: group.title, tone: group.tone, count: group.rows.length, kind: "group", rows: group.rows }));
  const later = view.later.map((group) => ({ key: group.key, title: group.title, tone: "muted", count: group.rows.length, kind: "later", rows: group.rows }));
  const routines = view.routines.length ? [{ key: "routines", title: "Routines", tone: "muted", count: view.routines.length, kind: "routines" }] : [];
  return [...next, ...groups, ...later, ...routines];
}

export const sectionHref = (key) => `#/section/${encodeURIComponent(key)}`;

export function firstTaskOf(view, key) {
  const section = boardSections(view).find((candidate) => candidate.key === key);
  if (section?.kind === "next") {
    const row = view.next[0]?.rows.filter((candidate) => candidate.item).sort((a, b) => a.rank - b.rank)[0];
    return row ? [row.machine, row.address] : null;
  }
  const row = section?.kind === "group" ? section.rows[0] : null;
  return row ? [row.machine, row.item.address] : null;
}

function sectionBody(view, section, now, selected) {
  if (section.kind === "next") return nextList(view.next, now, selected);
  if (section.kind === "group") return `<div class="col">${section.rows.map((row) => taskRow(row, selected)).join("")}</div>`;
  if (section.kind === "later") return `<div class="col">${section.rows.map((row) => laterRow(row, now)).join("")}</div>`;
  return routineLines(view.routines, now);
}

export function sectionView(view, key, now, { selected = null, nextLine = "" } = {}) {
  const section = boardSections(view).find((candidate) => candidate.key === key);
  if (!section) return "";
  const line = section.kind === "next" ? nextLine : section.kind === "routines" ? plural(section.count, "routine", "routines") : plural(section.count, "task", "tasks");
  return `<h1 class="sec-title">${esc(section.title)}</h1>${line ? `<p class="lead">${line}</p>` : ""}${sectionBody(view, section, now, selected)}`;
}

const dot = (tone) => `<span class="tone-dot g-${esc(tone)}" aria-hidden="true"></span>`;

export function sectionPills(sections) {
  const pills = sections.filter((section) => section.key !== "next").map((section) => `<a class="pill" href="${sectionHref(section.key)}">${dot(section.tone)}${esc(section.title)}<span class="n">${esc(section.count)}</span></a>`);
  return pills.length ? `<nav class="pills" aria-label="Sections">${pills.join("")}</nav>` : "";
}

export const sectionsButton = (open) => `<button type="button" class="round" data-action="sections" aria-label="Show sections" aria-expanded="${open}">${icon("sections")}</button>`;

export function sectionSheet(sections, current) {
  const items = sections.map((section) => {
    const on = section.key === current;
    return `<a class="item${on ? " on" : ""}" href="${sectionHref(section.key)}"${on ? ' aria-current="page"' : ""}><span>${dot(section.tone)}${esc(section.title)}</span><span class="n">${esc(section.count)}</span></a>`;
  });
  return `<button type="button" class="scrim" data-action="close-sections" aria-label="Hide sections"></button><nav class="sheet" aria-label="Sections"><div class="head"><b>Board</b><button type="button" class="round" data-action="close-sections" aria-label="Hide sections">${icon("sections")}</button></div>${items.join("")}</nav>`;
}

export const backHref = (name, from) => (name === "task" || name === "card" ? `#/${from}` : "#/board");
