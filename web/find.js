export const FIND_KEY = "kebbie-backlog-find";
export const SORTS = ["priority", "newest"];
export const STATES = [
  { key: "to-write", title: "to write" },
  { key: "drafting", title: "drafting" },
  { key: "ready", title: "ready" },
];
const PRIORITY_KEYS = ["high", "medium", "low", "none"];

export const emptyFind = () => ({ q: "", repos: [], priorities: [], states: [], found: false, sort: "priority" });

const strings = (value, allowed = null) => (Array.isArray(value) ? [...new Set(value.filter((one) => typeof one === "string" && one !== "" && (!allowed || allowed.includes(one))))] : []);

export function readFind(raw) {
  const value = raw && typeof raw === "object" ? raw : {};
  return {
    q: typeof value.q === "string" ? value.q.slice(0, 200) : "",
    repos: strings(value.repos),
    priorities: strings(value.priorities, PRIORITY_KEYS),
    states: strings(value.states, STATES.map((one) => one.key)),
    found: value.found === true,
    sort: SORTS.includes(value.sort) ? value.sort : "priority",
  };
}

export function loadFind(storage) {
  try {
    return readFind(JSON.parse(storage?.getItem(FIND_KEY) ?? "null"));
  } catch {
    return emptyFind();
  }
}

export function keepFind(storage, find) {
  try {
    storage?.setItem(FIND_KEY, JSON.stringify(find));
  } catch {}
}

export const filtering = (find) => Boolean(find.q.trim() || find.repos.length || find.priorities.length || find.states.length || find.found);

export function toggled(find, kind, value) {
  if (kind === "found") return { ...find, found: !find.found };
  const key = { repo: "repos", priority: "priorities", state: "states" }[kind];
  if (!key) return find;
  const list = find[key];
  return { ...find, [key]: list.includes(value) ? list.filter((one) => one !== value) : [...list, value] };
}

export const cleared = (find) => ({ ...emptyFind(), q: find.q, sort: find.sort });

const escapeRegExp = (text) => text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
export const pattern = (q) => (q.trim() ? new RegExp(escapeRegExp(q.trim()), "giu") : null);

const hits = (text, re) => {
  if (!re || !text) return false;
  re.lastIndex = 0;
  return re.test(text);
};

export function bodyTexts(body) {
  if (!body) return [];
  return [...body.sections.map((section) => section.text), ...body.questions.map((question) => question.text)].filter((text) => text && text.trim() !== "");
}

const isFoundRow = (entry) => entry.row.home?.kind === "found";

function passes(entry, find, re, textOf, skip) {
  const { row } = entry;
  if (re && !hits(row.title, re) && !textOf(entry).some((text) => hits(text, re))) return false;
  if (skip !== "repo" && find.repos.length && !find.repos.includes(row.repo)) return false;
  if (skip !== "priority" && find.priorities.length && (isFoundRow(entry) || !find.priorities.includes(row.priority ?? "none"))) return false;
  if (skip !== "state" && find.states.length && (isFoundRow(entry) || !find.states.includes(row.state))) return false;
  if (skip !== "found" && find.found && !isFoundRow(entry)) return false;
  return true;
}

export const textsOf = (bodyOf) => (entry) => [entry.row.goal, ...bodyTexts(bodyOf(entry))].filter(Boolean);

export function chipsFor(entries, find, textOf, priorities) {
  const re = pattern(find.q);
  const count = (skip, test) => entries.filter((entry) => passes(entry, find, re, textOf, skip) && test(entry)).length;
  const tally = new Map();
  for (const entry of entries) if (entry.row.repo) tally.set(entry.row.repo, (tally.get(entry.row.repo) ?? 0) + 1);
  for (const repo of find.repos) if (!tally.has(repo)) tally.set(repo, 0);
  const repos = [...tally.entries()].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0])).map(([repo]) => repo);
  const ordered = [...priorities.filter((one) => one.key !== "none"), ...priorities.filter((one) => one.key === "none")];
  return [
    repos.map((repo) => ({ kind: "repo", value: repo, label: repo, on: find.repos.includes(repo), n: count("repo", (entry) => entry.row.repo === repo) })),
    ordered.map((one) => ({ kind: "priority", value: one.key, label: one.title, on: find.priorities.includes(one.key), n: count("priority", (entry) => !isFoundRow(entry) && (entry.row.priority ?? "none") === one.key) })),
    STATES.map((one) => ({ kind: "state", value: one.key, label: one.title, on: find.states.includes(one.key), n: count("state", (entry) => !isFoundRow(entry) && entry.row.state === one.key) })),
    [{ kind: "found", value: "found", label: "Found by tasks", on: find.found, n: count("found", isFoundRow) }],
  ];
}

export function findIn(entries, find, textOf) {
  const re = pattern(find.q);
  return entries.filter((entry) => passes(entry, find, re, textOf, null));
}

const esc = (value) => String(value ?? "").replace(/[&<>"']/g, (char) => `&#${char.charCodeAt(0)};`);

export function marked(text, q) {
  const re = pattern(q);
  if (!re) return esc(text);
  let out = "";
  let at = 0;
  for (const match of String(text).matchAll(re)) {
    out += `${esc(text.slice(at, match.index))}<mark>${esc(match[0])}</mark>`;
    at = match.index + match[0].length;
  }
  return out + esc(text.slice(at));
}

const BEFORE = 32;
const AFTER = 64;

export function snippet(texts, q) {
  const re = pattern(q);
  if (!re) return null;
  for (const raw of texts) {
    const text = raw.replace(/\s+/g, " ").trim();
    re.lastIndex = 0;
    const match = re.exec(text);
    if (!match) continue;
    let start = Math.max(0, match.index - BEFORE);
    if (start > 0) start = text.indexOf(" ", start) + 1 || start;
    if (start > match.index) start = match.index;
    let end = Math.min(text.length, match.index + match[0].length + AFTER);
    if (end < text.length) end = text.lastIndexOf(" ", end) > match.index + match[0].length ? text.lastIndexOf(" ", end) : end;
    return `${start > 0 ? "…" : ""}${marked(text.slice(start, end), q)}${end < text.length ? "…" : ""}`;
  }
  return null;
}

export function pacer({ load, held, burst = 30, perSecond = 1.5, concurrency = 3, failed = () => false, pause = 30_000, clock = () => performance.now(), sleep = (ms) => new Promise((done) => setTimeout(done, ms)) }) {
  let queue = [];
  const queued = new Set();
  let running = 0;
  let tokens = burst;
  let filledAt = clock();
  let round = 0;
  const take = () => {
    const now = clock();
    tokens = Math.min(burst, tokens + ((now - filledAt) / 1000) * perSecond);
    filledAt = now;
    if (tokens < 1) return Math.ceil(((1 - tokens) / perSecond) * 1000);
    tokens -= 1;
    return 0;
  };
  async function work(mine) {
    running += 1;
    while (queue.length && mine === round) {
      const wait = take();
      if (wait) {
        await sleep(wait);
        continue;
      }
      const ref = queue.shift();
      const result = await load(ref).catch(() => null);
      if (mine === round) queued.delete(ref.id);
      if (failed(result)) {
        tokens = 0;
        await sleep(pause);
      }
    }
    if (mine === round) running -= 1;
  }
  return {
    want(refs) {
      for (const ref of refs) {
        if (queued.has(ref.id) || held(ref.id)) continue;
        queued.add(ref.id);
        queue.push(ref);
      }
      while (running < concurrency && queue.length) work(round);
    },
    pending: () => queued.size,
    stop() {
      round += 1;
      queue = [];
      queued.clear();
      running = 0;
    },
  };
}
