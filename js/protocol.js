import * as hpke from "./hpke.js";

const subtle = globalThis.crypto.subtle;
const text = new TextEncoder();
const HEADER_DOMAIN = text.encode("kebbie-hub envelope\0");
const HPKE_INFO = text.encode("kebbie-hub content key v1");
const KEY_COMMIT_DOMAIN = text.encode("kebbie-hub key commit v1");
const CONNECT_DOMAIN = text.encode("kebbie-connect-machine-v1");
const WRAP_KEY_INFO = text.encode("kebbie-hub account key v1");
const WRAP_AD_DOMAIN = text.encode("kebbie-hub account key wrap v1");
const WRAPPER_SIGN_DOMAIN = text.encode("kebbie-hub account key wrapper v1");
const MACHINE_LIST_DOMAIN = text.encode("kebbie-hub account machines v1");
const SIGNING_FINGERPRINT_DOMAIN = text.encode("kebbie-hub account signing key v1");
const ENCRYPTION_FINGERPRINT_DOMAIN = text.encode("kebbie-hub account encryption key v1");
const ACCOUNT_COMMIT_DOMAIN = text.encode("kebbie-connect-account-commit-v1");
const DEVICE_CERT_DOMAIN = text.encode("kebbie-hub device certificate v2");
const DEVICE_FINGERPRINT_DOMAIN = text.encode("kebbie-hub device key v1");
export const MAX_DEVICE_NAME_CHARS = 64;
export const DEVICE_KINDS = ["web"];
const BLOB_DOMAIN = text.encode("kebbie-hub blob v1");
const BLOB_OVERHEAD = 12 + 16;
export const WRAP_NONCE_LEN = 12;
const WRAPPED_LEN = WRAP_NONCE_LEN + 64 + 16;
const PKCS8_ED25519 = Uint8Array.fromHex("302e020100300506032b657004220420");
const PKCS8_X25519 = Uint8Array.fromHex("302e020100300506032b656e04220420");
const NONCE_LEN = 12;
const DAY = 86_400;
export const PROTOCOL_VERSION = 1;
export const SCHEMA = 1;
export const CLOCK_SKEW = 300;

export const COMMAND_TTL = 3600;
export const PROMPT_ANSWER_TTL = 600;
export const START_ITEM_TTL = DAY;
export const MAX_IDEA_CHARS = 2000;
export const MAX_ITEM_ANSWER_CHARS = 2000;
export const MAX_QUESTION_CHARS = 4000;
export const MAX_ITEM_TITLE_CHARS = 200;
export const MAX_SECTION_CHARS = 8000;
export const MAX_ITEM_EDIT_BYTES = 24 * 1024;
export const MAX_HEADING_CHARS = 120;
export const MAX_NOTICE_TITLE_CHARS = 120;
export const MAX_NOTICE_BODY_CHARS = 600;
export const MAX_NOTICE_BYTES = 2048;
export const MAX_RETRACTS = 8;
export const NOTICE_CATEGORIES = ["questions", "new-device", "idea-questions", "ready", "ci-failed", "started"];
export const DEFAULT_NOTICE_CATEGORIES = NOTICE_CATEGORIES.filter((category) => category !== "started");
export const NOTICE_COLLAPSE_PREFIXES = { q: "questions", p: "questions", d: "new-device", i: "idea-questions", r: "ready", c: "ci-failed", s: "started" };
export function noticeCategoryOf(tag) {
  const at = typeof tag === "string" ? tag.indexOf("-") : -1;
  return at < 0 ? null : (Object.hasOwn(NOTICE_COLLAPSE_PREFIXES, tag.slice(0, at)) ? NOTICE_COLLAPSE_PREFIXES[tag.slice(0, at)] : null);
}
export const fitsCollapse = (notice, tag) => [null, notice.category].includes(noticeCategoryOf(tag));
export const MAX_LABEL_BYTES = 256;
export const BACKLOG_BODY_MEDIA_TYPE = "application/vnd.kebbie.backlog-item+json";
export const REVIEW_MEDIA_TYPE = "application/vnd.kebbie.review+json";
export const MAX_REVIEW_COMMENTS = 50;
export const MAX_REVIEW_COMMENT_CHARS = 2000;
export const MAX_REVIEW_COMMENTS_BYTES = 24 * 1024;
const MAX_SINCE_CHARS = 256;
const command = { prefix: "cmd", maxTtl: DAY, ttl: COMMAND_TTL, command: true };
export const KINDS = {
  board: { prefix: "brd", maxTtl: DAY, command: false },
  notice: { prefix: "ntc", maxTtl: DAY, command: false },
  result: { prefix: "cmd", maxTtl: 7 * DAY, command: false },
  answer: command,
  push: command,
  start: command,
  routine: command,
  "prompt-answer": { ...command, maxTtl: PROMPT_ANSWER_TTL, ttl: PROMPT_ANSWER_TTL },
  idea: { ...command, maxTtl: COMMAND_TTL },
  "item-answer": { ...command, maxTtl: COMMAND_TTL },
  "start-item": { ...command, ttl: START_ITEM_TTL },
  "item-edit": { ...command, maxTtl: COMMAND_TTL },
  "device-decision": { ...command, maxTtl: COMMAND_TTL },
  "review-open": { ...command, maxTtl: COMMAND_TTL },
  "review-comments": { ...command, maxTtl: COMMAND_TTL },
  "review-read": { ...command, maxTtl: COMMAND_TTL },
  "accept-step": { ...command, maxTtl: COMMAND_TTL },
  "accept-subtask": { ...command, maxTtl: COMMAND_TTL },
  "found-start": { ...command, ttl: START_ITEM_TTL },
  "found-add": { ...command, maxTtl: COMMAND_TTL },
  "found-dismiss": { ...command, maxTtl: COMMAND_TTL },
};

const ID = /^(?:(mch|dev|ntc|brd|alr)_[0-7][0-9A-HJKMNP-TV-Z]{25}|(cmd)_[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}|(blb)_[0-9a-f]{64}|(acc)_[A-Za-z0-9_-]{22})$/;
const BASE64URL = /^[A-Za-z0-9_-]*$/;
const COLLAPSE = /^[A-Za-z0-9_-]{1,64}$/;
const FINGERPRINT = /^[0-9A-F]{4}-[0-9A-F]{4}$/;

export class ProtocolError extends Error {
  constructor(code) {
    super(code);
    this.code = code;
  }
}

export function fromB64(value) {
  if (typeof value !== "string" || !BASE64URL.test(value) || value.length % 4 === 1) throw new ProtocolError("malformed");
  try {
    return Uint8Array.fromBase64(value + "=".repeat((4 - (value.length % 4)) % 4), { alphabet: "base64url", lastChunkHandling: "strict" });
  } catch {
    throw new ProtocolError("malformed");
  }
}

export const toB64 = (bytes) => new Uint8Array(bytes).toBase64({ alphabet: "base64url", omitPadding: true });

const concat = (...parts) => {
  const out = new Uint8Array(parts.reduce((sum, part) => sum + part.length, 0));
  let offset = 0;
  for (const part of parts) {
    out.set(part, offset);
    offset += part.length;
  }
  return out;
};

const sha256 = async (...parts) => new Uint8Array(await subtle.digest("SHA-256", concat(...parts)));

const u16 = (value) => new Uint8Array([value >> 8, value & 0xff]);
const u32 = (value) => {
  const out = new Uint8Array(4);
  new DataView(out.buffer).setUint32(0, value);
  return out;
};
const u64 = (value) => {
  const out = new Uint8Array(8);
  new DataView(out.buffer).setBigUint64(0, BigInt(value));
  return out;
};
const bytesField = (bytes) => {
  if (bytes.length > 0xffff) throw new ProtocolError("malformed");
  return concat(u16(bytes.length), bytes);
};
const field = (value) => bytesField(text.encode(value));
const absent = (value) => value === undefined || value === null;
const optional = (value) => (absent(value) ? new Uint8Array([0]) : concat(new Uint8Array([1]), field(value)));

const prefixOf = (id) => {
  const match = typeof id === "string" ? ID.exec(id) : null;
  return match ? (match[1] ?? match[2] ?? match[3] ?? match[4]) : null;
};
const isTime = (value) => Number.isSafeInteger(value) && value >= 0;
const isPlainObject = (value) => typeof value === "object" && value !== null && !Array.isArray(value);
const hasLength = (value, length) => {
  try {
    return fromB64(value).length === length;
  } catch {
    return false;
  }
};
const CERT_FIELDS = ["account", "device", "name", "kind", "signing", "encryption", "created_at", "sig"];
export const isDeviceName = (name) => typeof name === "string" && [...name].length >= 1 && [...name].length <= MAX_DEVICE_NAME_CHARS && !/\p{Cc}/u.test(name);
const isCert = (cert) =>
  isPlainObject(cert) &&
  Object.keys(cert).every((key) => CERT_FIELDS.includes(key)) &&
  prefixOf(cert.account) === "acc" &&
  prefixOf(cert.device) === "dev" &&
  isDeviceName(cert.name) &&
  DEVICE_KINDS.includes(cert.kind) &&
  hasLength(cert.signing, 32) &&
  hasLength(cert.encryption, 32) &&
  isTime(cert.created_at) &&
  hasLength(cert.sig, 64);

export function checkShape(envelope) {
  if (!isPlainObject(envelope)) throw new ProtocolError("malformed");
  if (envelope.v !== PROTOCOL_VERSION) throw new ProtocolError("unsupported-version");
  if (envelope.schema !== SCHEMA) throw new ProtocolError("unsupported-schema");
  const kind = typeof envelope.kind === "string" && Object.hasOwn(KINDS, envelope.kind) ? KINDS[envelope.kind] : null;
  const fits =
    kind !== null &&
    prefixOf(envelope.id) === kind.prefix &&
    prefixOf(envelope.machine) === "mch" &&
    (kind.command ? isCert(envelope.cert) && prefixOf(envelope.from) === "dev" && envelope.cert.device === envelope.from : absent(envelope.cert) && envelope.from === envelope.machine) &&
    isTime(envelope.at) &&
    isTime(envelope.exp) &&
    envelope.exp > envelope.at &&
    envelope.exp - envelope.at <= kind.maxTtl &&
    typeof envelope.kc === "string" &&
    /^[A-Za-z0-9_-]{43}$/.test(envelope.kc) &&
    (envelope.kind === "notice" ? !absent(envelope.collapse) : absent(envelope.urgency) && absent(envelope.collapse)) &&
    (absent(envelope.urgency) || envelope.urgency === "high" || envelope.urgency === "normal") &&
    (absent(envelope.collapse) || (typeof envelope.collapse === "string" && COLLAPSE.test(envelope.collapse))) &&
    isPlainObject(envelope.keys) &&
    Object.entries(envelope.keys).every(([id, row]) => prefixOf(id) !== null && typeof row === "string") &&
    typeof envelope.ct === "string" &&
    typeof envelope.sig === "string";
  if (!fits) throw new ProtocolError("malformed");
}

const string = (value) => typeof value === "string";
const bool = (value) => typeof value === "boolean";
const uint = (max) => (value) => Number.isSafeInteger(value) && value >= 0 && value <= max;
const u8 = uint(0xff);
const u32v = uint(0xffff_ffff);
const u64v = uint(Number.MAX_SAFE_INTEGER);
const id = (value) => prefixOf(value) !== null;
const oneOf = (...values) => (value) => values.includes(value);
const maybe = (check) => (value) => absent(value) || check(value);
const list = (check) => (value) => Array.isArray(value) && value.every(check);
const shape = (fields) => (value) =>
  isPlainObject(value) &&
  Object.keys(value).every((key) => Object.hasOwn(fields, key)) &&
  Object.entries(fields).every(([key, check]) => (Object.hasOwn(value, key) ? check(value[key]) : check.optional === true));
const optionalField = (check) => Object.assign(maybe(check), { optional: true });

const blobRef = shape({ id, key: string, media_type: string, bytes: u64v });
export const MAX_RULE_NAMES = 32;
export const MAX_RULE_NAME_CHARS = 200;
const rulesCheck = (rules) =>
  shape({ state: oneOf("checking", "ok", "breaks"), breaks: optionalField(u32v), names: optionalField(list(string)), override: optionalField(shape({ by: oneOf("user", "lead") })) })(rules) &&
  (rules.state === "breaks" ? !absent(rules.breaks) && rules.breaks >= 1 : absent(rules.breaks) && absent(rules.names)) &&
  (absent(rules.names) || (rules.names.length <= MAX_RULE_NAMES && rules.names.length <= rules.breaks && rules.names.every((name) => isLine(MAX_RULE_NAME_CHARS)(name)))) &&
  (absent(rules.override) || rules.state !== "ok");
export const MAX_SUBTASK_ID_CHARS = 32;
export const MAX_ADDED_REASON_CHARS = 500;
export const MAX_OBSTACLE_CHARS = 2000;
export const MAX_FOUND_SOURCES = 16;
const SUBTASK_STATES = ["planned", "in-progress", "ready", "accepted", "pushed"];
const COMMITS_HELD = { planned: (n) => n === 0, "in-progress": () => true, ready: (n) => n >= 1, accepted: (n) => n >= 1, pushed: (n) => n >= 1 };
export const isSubtaskId = (value) => typeof value === "string" && new RegExp(`^[A-Za-z0-9][A-Za-z0-9_-]{0,${MAX_SUBTASK_ID_CHARS - 1}}$`).test(value);
const subtask = shape({ id: isSubtaskId, title: string, state: oneOf(...SUBTASK_STATES), commits: list(string), added: optionalField(shape({ at: u64v, reason: string })), rules: optionalField(rulesCheck) });
const orderRow = shape({ sha: string, subject: string, subtask: string });
const pushable = shape({ count: u32v, through: string, subtasks: list(string) });
const obstacleText = (value) => isText(MAX_OBSTACLE_CHARS)(value) && !/[^\P{Cc}\n]/u.test(value);
const obstacleDecision = shape({ kind: oneOf("workaround", "subtask", "task", "ask"), by: oneOf("investigator", "lead", "you"), text: obstacleText });
const obstacle = (value) =>
  shape({ id: isSubtaskId, state: oneOf("investigating", "asked", "with-lead", "decided"), text: obstacleText, at: u64v, decision: optionalField(obstacleDecision) })(value) &&
  (absent(value.decision) ? value.state !== "decided" : value.state !== "investigating" && (value.decision.kind === "ask") === (value.state === "asked"));
const stepCommit = shape({ sha: string, subject: string });
const boardStep = shape({ number: (value) => u32v(value) && value >= 1, title: string, state: oneOf(...SUBTASK_STATES, "committed"), commits: optionalField(list(stepCommit)), sha: optionalField(string), rules: optionalField(rulesCheck) });
const boardSteps = (value) => shape({ pushed: u32v, items: list(boardStep), required: optionalField(bool) })(value) && value.items.every((step) => absent(step.commits) || absent(step.sha));
const reviewOffer = (value) => shape({ subtask: optionalField(isSubtaskId), step: optionalField((step) => u32v(step) && step >= 1), files: u32v })(value) && (absent(value.subtask) || absent(value.step));
const runOf = (order, count) => [...new Set(order.slice(0, count).map((row) => row.subtask))];
const PLAN_KEYS = ["subtasks", "order", "pushable", "require_accept"];
function subtasksOf(steps) {
  const commitsOf = (step) => step.commits ?? (absent(step.sha) ? [] : [{ sha: step.sha, subject: step.title }]);
  const subtasks = steps.items.map((step) => {
    const one = { id: String(step.number), title: step.title, state: step.state === "committed" ? "ready" : step.state, commits: commitsOf(step).map((commit) => commit.sha) };
    return absent(step.rules) ? one : { ...one, rules: step.rules };
  });
  const order = steps.items.filter((step) => step.state !== "pushed").flatMap((step) => commitsOf(step).map((commit) => ({ sha: commit.sha, subject: commit.subject, subtask: String(step.number) })));
  const accepted = new Set(subtasks.filter((one) => one.state === "accepted").map((one) => one.id));
  const count = order.findIndex((row) => !accepted.has(row.subtask));
  const run = count === -1 ? order.length : count;
  const plan = { subtasks, order };
  if (run > 0) plan.pushable = { count: run, through: order[run - 1].sha, subtasks: runOf(order, run) };
  if (!absent(steps.required)) plan.require_accept = steps.required;
  return plan;
}
function itemOf(item) {
  let out = item;
  if (!absent(item.steps)) {
    const { steps, ...rest } = item;
    out = { ...rest, ...subtasksOf(steps) };
  }
  if (!absent(out.review)) out = { ...out, review: { subtask: out.review.subtask ?? (absent(out.review.step) ? null : String(out.review.step)), files: out.review.files } };
  return out;
}
export const subtaskHead = (part) => {
  const last = part.commits.at(-1);
  return typeof last === "string" ? last : (last?.sha ?? null);
};
export const subtaskOf = (item, id) => (item.subtasks ?? []).find((one) => one.id === id) ?? null;
export function payloadOf(payload) {
  if (payload.type !== "board") return payload;
  return { ...payload, items: payload.items.map(itemOf) };
}
function planHolds(item) {
  if (absent(item.subtasks)) return absent(item.order) && absent(item.pushable);
  if (absent(item.order) && !absent(item.pushable)) return false;
  const ids = new Set();
  const shas = new Set();
  for (const one of item.subtasks) {
    if (ids.has(one.id) || !one.commits.every(isFullSha) || !COMMITS_HELD[one.state](one.commits.length)) return false;
    if (!absent(one.added) && !isLine(MAX_ADDED_REASON_CHARS)(one.added.reason)) return false;
    ids.add(one.id);
    for (const sha of one.commits) {
      if (shas.has(sha)) return false;
      shas.add(sha);
    }
  }
  const order = item.order ?? [];
  const placed = new Set();
  for (const row of order) {
    const owner = subtaskOf(item, row.subtask);
    if (!isFullSha(row.sha) || placed.has(row.sha) || !owner || !owner.commits.includes(row.sha) || owner.state === "pushed") return false;
    placed.add(row.sha);
  }
  if (absent(item.pushable)) return true;
  const { count, through, subtasks } = item.pushable;
  const run = runOf(order, count);
  return count >= 1 && count <= order.length && order[count - 1].sha === through && run.length === subtasks.length && run.every((id, index) => subtasks[index] === id) && run.every((id) => subtaskOf(item, id).state === "accepted");
}
const itemHolds = (raw) => (absent(raw.steps) || PLAN_KEYS.every((key) => absent(raw[key]))) && planHolds(itemOf(raw));
const item = shape({
  address: string,
  group: oneOf("NEEDS YOU", "WORKING", "TO REVIEW", "READY TO PUSH", "CI RUNNING", "WAITING FOR WORK"),
  urgency: u8,
  summary: string,
  next_for_you: optionalField(string),
  agent: optionalField(shape({ status: oneOf("working", "blocked", "idle", "done"), kind: optionalField(string), model: optionalField(string) })),
  report: optionalField(shape({ state: string, next: optionalField(string), summary: string, at: u64v })),
  question: optionalField(
    shape({
      id: string,
      text: string,
      options: list(shape({ label: string, meaning: string, comment: optionalField(bool) })),
      recommend: optionalField(u8),
      kept_for_user: bool,
      plan: optionalField(blobRef),
      plan_at: optionalField(u64v),
      at: u64v,
    }),
  ),
  prompt: optionalField(shape({ id: string, question: string, options: list(shape({ label: string, takes_text: bool, key: optionalField(string) })) })),
  git: optionalField(shape({ upstream: string, ahead: u32v, behind: u32v, uncommitted: u32v, rules: optionalField(rulesCheck), approved: optionalField(bool) })),
  head: optionalField(string),
  verdict: optionalField(shape({ verdict: optionalField(oneOf("ok", "changes")), findings: optionalField(u32v), sha: optionalField(string), stale: bool, running: bool, held: optionalField(bool) })),
  push_permission: optionalField(oneOf("task", "repo")),
  push_waits: optionalField(string),
  follows_main: bool,
  ci: optionalField(shape({ state: oneOf("running", "failed"), failure: optionalField(string) })),
  recover: optionalField(shape({ note: string })),
  routine: optionalField(string),
  card: optionalField(shape({ sha: string, current: bool, blob: blobRef })),
  subtasks: optionalField(list(subtask)),
  order: optionalField(list(orderRow)),
  pushable: optionalField(pushable),
  require_accept: optionalField(bool),
  obstacle: optionalField(obstacle),
  steps: optionalField(boardSteps),
  title: optionalField(string),
  profile: optionalField(string),
  recipe: optionalField(string),
  assessment: optionalField(string),
  second_opinion: optionalField(shape({ by: string, points: optionalField(u32v), running: bool })),
  review: optionalField(reviewOffer),
});
const boardItem = (value) => item(value) && itemHolds(value);
export const isHomeLabel = (label) =>
  typeof label === "string" &&
  text.encode(label).length >= 1 &&
  text.encode(label).length <= MAX_LABEL_BYTES &&
  !/^[/~]/.test(label) &&
  !/[\p{Cc}\\:]/u.test(label) &&
  label.split("/").every((segment) => segment !== "" && segment !== "." && segment !== "..");
const LOCAL_ID = /^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$/;
const homeOf = (key) => key.slice(0, key.indexOf(":"));
export function isItemKey(key) {
  if (typeof key !== "string" || !key.includes(":")) return false;
  const id = key.slice(key.indexOf(":") + 1);
  const home = homeOf(key);
  if (home === "found") return /^f[1-9][0-9]{0,8}$/.test(id);
  return home === "vault" ? isHomeLabel(id) : (home === "local" || home === "kept") && LOCAL_ID.test(id);
}
const isText = (max) => (value) => typeof value === "string" && value.trim() !== "" && [...value].length <= max;
const backlogRow = (row) =>
  shape({
    item: isItemKey,
    title: string,
    priority: oneOf("high", "medium", "low", "none"),
    repo: optionalField(string),
    state: oneOf("to-write", "drafting", "ready"),
    filed: u64v,
    home: shape({ kind: oneOf("vault", "local", "kept", "found"), label: isHomeLabel }),
    questions: u32v,
    blocking: u32v,
    goal: optionalField(string),
    body: optionalField(blobRef),
    found: optionalField(shape({ sources: list(isAddress), count: u32v })),
  })(row) &&
  homeOf(row.item) === row.home.kind &&
  row.blocking <= row.questions &&
  (row.home.kind === "found") === !absent(row.found) &&
  (absent(row.found) || (row.found.sources.length >= 1 && row.found.sources.length <= MAX_FOUND_SOURCES && new Set(row.found.sources).size === row.found.sources.length && row.found.count >= row.found.sources.length));
const backlog = (rows) => list(backlogRow)(rows) && new Set(rows.map((row) => row.item)).size === rows.length;
export const isSectionKey = (value) => typeof value === "string" && /^[a-z0-9][a-z0-9_-]{0,63}$/.test(value);
const bodySection = shape({ key: isSectionKey, heading: (value) => isLine(MAX_HEADING_CHARS)(value), text: string, required: bool });
const backlogBody = (body) =>
  shape({ sections: list(bodySection), questions: list(shape({ text: string, blocks: bool, pending_answer: optionalField(string) })), questions_key: optionalField(isSectionKey) })(body) &&
  new Set(body.sections.map((section) => section.key)).size === body.sections.length &&
  (absent(body.questions_key) || body.sections.some((section) => section.key === body.questions_key));

export function checkBacklogBody(body) {
  if (!backlogBody(body)) throw new ProtocolError("malformed");
}

const FULL_SHA = /^[0-9a-f]{40}$/;
const isFullSha = (value) => typeof value === "string" && FULL_SHA.test(value);
export const isContentHash = (value) => typeof value === "string" && /^(?:[0-9a-f]{40}|[0-9a-f]{64})$/.test(value);
const count = uint(0xffff_ffff);
const stepNumber = (value) => count(value) && value >= 1;
const LINE_NUMBERS = { context: (line) => count(line.old) && count(line.new), add: (line) => absent(line.old) && count(line.new), del: (line) => count(line.old) && absent(line.new) };
const diffLine = (line) => shape({ kind: oneOf("context", "add", "del"), old: optionalField(count), new: optionalField(count), text: string })(line) && LINE_NUMBERS[line.kind](line);
const reviewFile = (file) =>
  shape({
    path: isHomeLabel,
    old_path: optionalField(isHomeLabel),
    status: oneOf("added", "modified", "deleted", "renamed"),
    add: count,
    del: count,
    content: isContentHash,
    read: bool,
    omitted: bool,
    hunks: list(shape({ head: string, lines: list(diffLine) })),
  })(file) &&
  (file.status === "renamed") === !absent(file.old_path) &&
  (file.omitted ? file.hunks.length === 0 : countsFit(file));
const countsFit = (file) => {
  const lines = file.hunks.flatMap((hunk) => hunk.lines);
  return lines.filter((line) => line.kind === "add").length === file.add && lines.filter((line) => line.kind === "del").length === file.del;
};
const reviewFiles = (files) => list(reviewFile)(files) && new Set(files.map((file) => file.path)).size === files.length;
const reviewCommit = shape({ sha: isFullSha, subject: string, body: string, files: reviewFiles });
const oneCommitStep = shape({ number: count, title: string, sha: string, subject: string, body: string });
const commitsStep = shape({ number: count, title: string, commits: list(reviewCommit), files: reviewFiles });
const reviewSubtask = shape({ id: string, title: string, commits: list(reviewCommit), files: reviewFiles });
const reviewShape = (body) =>
  shape({
    address: isAddress,
    since: (value) => isText(MAX_SINCE_CHARS)(value) && !/\p{Cc}/u.test(value),
    head: isFullSha,
    subtask: optionalField(reviewSubtask),
    step: optionalField((step) => commitsStep(step) || oneCommitStep(step)),
    guide: list(shape({ path: isHomeLabel, text: string })),
    left_out: optionalField(string),
    files: reviewFiles,
  })(body) &&
  (absent(body.subtask) || absent(body.step)) &&
  (absent(body.step) || body.step.number >= 1);
const reviewOf = (body) => {
  const { address, since, head, subtask, step, guide, left_out, files } = body;
  const opened = (part, rest) => ({ address, since, head, subtask: part, guide, left_out: left_out ?? null, files: rest });
  if (!absent(subtask)) return opened({ id: subtask.id, title: subtask.title, commits: subtask.commits, files: subtask.files }, files);
  if (absent(step)) return opened(null, files);
  if (Object.hasOwn(step, "commits")) return opened({ id: String(step.number), title: step.title, commits: step.commits, files: step.files }, files);
  const commit = { sha: step.sha, subject: step.subject, body: step.body, files };
  return opened({ id: String(step.number), title: step.title, commits: [commit], files }, []);
};
const subtaskReview = (body) => {
  if (absent(body.subtask)) return true;
  const { subtask } = body;
  return body.files.length === 0 && isSubtaskId(subtask.id) && subtask.commits.length >= 1 && subtask.commits.every((commit) => isFullSha(commit.sha)) && subtask.commits.at(-1).sha === body.head && new Set(subtask.commits.map((commit) => commit.sha)).size === subtask.commits.length;
};

export function readReview(body) {
  if (!reviewShape(body)) throw new ProtocolError("malformed");
  const opened = reviewOf(body);
  if (!subtaskReview(opened)) throw new ProtocolError("malformed");
  return opened;
}

export function checkReview(body) {
  readReview(body);
}

const reviewComment = shape({ path: isHomeLabel, line: stepNumber, side: oneOf("old", "new"), sha: optionalField(isFullSha), text: (value) => isText(MAX_REVIEW_COMMENT_CHARS)(value) && !/[^\P{Cc}\n\t]/u.test(value) });
const namesOne = (value) => absent(value.subtask) || absent(value.step);
const reviewComments = (value) =>
  namesOne(value) &&
  typed({ address: isAddress, subtask: optionalField(isSubtaskId), step: optionalField(stepNumber), comments: (comments) => Array.isArray(comments) && comments.length >= 1 && comments.length <= MAX_REVIEW_COMMENTS && comments.every(reviewComment) })(value) &&
  text.encode(JSON.stringify({ ...value, type: undefined })).length <= MAX_REVIEW_COMMENTS_BYTES;

const isTextHash = (value) => typeof value === "string" && /^[0-9a-f]{64}$/.test(value);
export async function textHash(value) {
  return new Uint8Array(await subtle.digest("SHA-256", text.encode(value))).toHex();
}
const itemEdit = (value) =>
  typed({ item: isItemKey, section: optionalField(isSectionKey), text: string, base: isTextHash })(value) &&
  (absent(value.section) ? isLine(MAX_ITEM_TITLE_CHARS)(value.text) : [...value.text].length <= MAX_SECTION_CHARS && !/[^\P{Cc}\n\t]/u.test(value.text)) &&
  text.encode(JSON.stringify({ item: value.item, section: value.section ?? null, text: value.text, base: value.base })).length <= MAX_ITEM_EDIT_BYTES;

const nextRow = shape({ part: oneOf("stopped", "push", "read"), address: string, rank: u32v, line: string, since: optionalField(u64v), waits_behind: u32v });
const later = shape({ address: string, title: optionalField(string), state: oneOf("queued", "parked"), line: string, until: optionalField(u64v), question: bool });
const STANDINGS = {
  waiting: shape({ state: string, fingerprint: string, until: optionalField(u64v) }),
  approved: shape({ state: string, fingerprint: string, at: u64v }),
  denied: shape({ state: string, fingerprint: string, at: u64v }),
};
const standing = (value) => isPlainObject(value) && string(value.state) && Object.hasOwn(STANDINGS, value.state) && STANDINGS[value.state](value);
const devices = (value) => isPlainObject(value) && Object.entries(value).every(([key, entry]) => prefixOf(key) === "dev" && standing(entry));
const REASONS = ["already-settled", "head-moved", "unknown-repo", "not-allowed", "expired", "undecryptable", "bad-request", "not-approved", "denied", "unknown-item", "question-changed", "questions-open", "already-started", "unknown-device", "section-changed"];
const isAddress = (value) => typeof value === "string" && /^[A-Za-z0-9][a-z0-9._-]{0,63}@[A-Za-z0-9][a-z0-9._-]{0,63}$/.test(value);
const isLine = (max) => (value) => isText(max)(value) && !/\p{Cc}/u.test(value);
const TARGETS = {
  task: shape({ to: string, address: isAddress }),
  item: shape({ to: string, item: isItemKey }),
  device: shape({ to: string, device: (value) => prefixOf(value) === "dev" }),
};
const target = (value) => isPlainObject(value) && string(value.to) && Object.hasOwn(TARGETS, value.to) && TARGETS[value.to](value);
const notice = (value) =>
  typed({
    category: oneOf(...NOTICE_CATEGORIES),
    title: isLine(MAX_NOTICE_TITLE_CHARS),
    body: (body) => isText(MAX_NOTICE_BODY_CHARS)(body) && !/[^\P{Cc}\n]/u.test(body),
    open: target,
    retract: optionalField((tags) => Array.isArray(tags) && tags.length <= MAX_RETRACTS && tags.every((tag) => typeof tag === "string" && COLLAPSE.test(tag))),
  })(value) && text.encode(JSON.stringify(value)).length <= MAX_NOTICE_BYTES;
const typed = (fields) => shape({ type: string, ...fields });
const isToken = (value) => typeof value === "string" && /^[A-Za-z0-9][a-z0-9._-]{0,63}$/.test(value);
const isFoundKey = (value) => isItemKey(value) && homeOf(value) === "found";
const PAYLOADS = {
  board: typed({
    rev: u64v,
    kebbie: string,
    away_since: optionalField(u64v),
    checkouts: list(shape({ repo: string, free_worktrees: u32v, busy_worktrees: u32v })),
    routines: list(shape({ id: string, title: string, repo: string, mode: string, next_run: optionalField(u64v), last_outcome: optionalField(string) })),
    items: list(boardItem),
    sessions_outside_tasks: u32v,
    next: optionalField(list(nextRow)),
    counts: optionalField(shape({ working: u32v, queued: u32v, parked: u32v })),
    later: optionalField(list(later)),
    accepts: optionalField(list(string)),
    devices: optionalField(devices),
    backlog: optionalField(backlog),
  }),
  notice,
  result: (value) =>
    typed({ command: id, device: (value) => prefixOf(value) === "dev", outcome: oneOf("done", "refused", "failed"), reason: optionalField(oneOf(...REASONS)), blocking: optionalField(uint(0xffff_ffff)), text: string, blob: optionalField(blobRef) })(value) &&
    (value.outcome === "refused") === !absent(value.reason) &&
    (absent(value.blob) || value.outcome === "done") &&
    (value.reason === "questions-open") === (!absent(value.blocking) && value.blocking > 0),
  answer: typed({ address: string, question: string, choice: optionalField(u8), comment: string }),
  push: typed({ address: isAddress, head: isFullSha, card_sha: optionalField(isFullSha), through_step: optionalField(stepNumber) }),
  start: typed({ repo: string, name: string, prompt: string }),
  routine: typed({ id: string }),
  "prompt-answer": typed({ address: string, prompt: string, key: string }),
  idea: typed({ text: isText(MAX_IDEA_CHARS), repo: optionalField(string) }),
  "item-answer": typed({ item: isItemKey, index: u32v, question: isText(MAX_QUESTION_CHARS), answer: isText(MAX_ITEM_ANSWER_CHARS) }),
  "start-item": typed({ item: isItemKey, anyway: optionalField(bool) }),
  "item-edit": itemEdit,
  "device-decision": typed({ device: (value) => prefixOf(value) === "dev", fingerprint: (value) => typeof value === "string" && FINGERPRINT.test(value), approve: bool }),
  "review-open": (value) => namesOne(value) && typed({ address: isAddress, subtask: optionalField(isSubtaskId), step: optionalField(stepNumber) })(value),
  "review-comments": reviewComments,
  "review-read": typed({ address: isAddress, path: isHomeLabel, read: bool, content: isContentHash, sha: optionalField(isFullSha) }),
  "accept-step": typed({ address: isAddress, step: stepNumber, head: isFullSha }),
  "accept-subtask": typed({ address: isAddress, subtask: isSubtaskId, head: isFullSha }),
  "found-start": typed({ item: isFoundKey, name: optionalField(isToken) }),
  "found-add": typed({ item: isFoundKey, task: isAddress }),
  "found-dismiss": typed({ item: isFoundKey }),
};

export function checkPayload(payload) {
  if (!isPlainObject(payload) || !string(payload.type) || !Object.hasOwn(PAYLOADS, payload.type)) throw new ProtocolError("malformed");
  if (!PAYLOADS[payload.type](payload)) throw new ProtocolError("malformed");
}

export function headerBytes(header) {
  return concat(
    HEADER_DOMAIN,
    u16(header.v),
    field(header.id),
    field(header.kind),
    field(header.from),
    field(header.machine),
    u64(header.at),
    u64(header.exp),
    u16(header.schema),
    field(header.kc),
    optional(header.urgency),
    optional(header.collapse),
    absent(header.cert) ? new Uint8Array([0]) : concat(new Uint8Array([1]), deviceCertBody(header.cert), fromB64(header.cert.sig)),
  );
}

const deviceCertBody = (cert) => concat(field(cert.account), field(cert.device), field(cert.name), field(cert.kind), fromB64(cert.signing), fromB64(cert.encryption), u64(cert.created_at));

export function deviceCertStatement(cert) {
  if (!isCert({ sig: toB64(new Uint8Array(64)), ...cert })) throw new ProtocolError("malformed");
  return concat(DEVICE_CERT_DOMAIN, deviceCertBody(cert));
}

export async function signDeviceCert(account, { device, name, kind = "web", signing, encryption, created_at }) {
  const cert = { account: account.id, device, name, kind, signing, encryption, created_at };
  return { ...cert, sig: toB64(await signStatement(account, deviceCertStatement(cert))) };
}

export async function verifyDeviceCert(key, cert) {
  try {
    if (!isCert(cert) || cert.account !== key.account) return false;
    const verifying = await subtle.importKey("raw", fromB64(key.keys.signing), { name: "Ed25519" }, false, ["verify"]);
    return await subtle.verify({ name: "Ed25519" }, verifying, fromB64(cert.sig), deviceCertStatement(cert));
  } catch {
    return false;
  }
}

export async function deviceFingerprint(signing) {
  const hex = (await sha256(DEVICE_FINGERPRINT_DOMAIN, fromB64(signing))).slice(0, 4).toHex().toUpperCase();
  return `${hex.slice(0, 4)}-${hex.slice(4)}`;
}

const CROCKFORD = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";

export function deviceId(millis = Date.now(), randomBytes = random(10)) {
  let value = (BigInt(millis) & 0xffff_ffff_ffffn) << 80n;
  randomBytes.forEach((byte, index) => (value |= BigInt(byte) << BigInt(8 * (9 - index))));
  let body = "";
  for (let digit = 25; digit >= 0; digit--) body += CROCKFORD[Number((value >> BigInt(5 * digit)) & 31n)];
  return `dev_${body}`;
}

export async function newDeviceKey() {
  const pair = await subtle.generateKey({ name: "Ed25519" }, false, ["sign", "verify"]);
  const generated = await subtle.generateKey({ name: "X25519" }, true, ["deriveBits"]);
  const encryptionPkcs8 = new Uint8Array(await subtle.exportKey("pkcs8", generated.privateKey));
  const privateKey = await subtle.importKey("pkcs8", encryptionPkcs8, { name: "X25519" }, false, ["deriveBits"]);
  return {
    signing: pair.privateKey,
    verifying: pair.publicKey,
    public: toB64(new Uint8Array(await subtle.exportKey("raw", pair.publicKey))),
    encryption: { privateKey, publicKey: generated.publicKey },
    encryptionPublic: toB64(new Uint8Array(await subtle.exportKey("raw", generated.publicKey))),
    encryptionPkcs8,
  };
}

export const encryptionPkcs8 = (secret) => concat(PKCS8_X25519, secret);

export const keyCommitment = async (contentKey) => toB64(await sha256(KEY_COMMIT_DOMAIN, contentKey));

export async function importIdentity(party) {
  const signing = await subtle.importKey("pkcs8", concat(PKCS8_ED25519, fromB64(party.signing_secret)), { name: "Ed25519" }, false, ["sign"]);
  const privateKey = await subtle.importKey("pkcs8", concat(PKCS8_X25519, fromB64(party.encryption_secret)), { name: "X25519" }, false, ["deriveBits"]);
  const publicKey = await subtle.importKey("raw", fromB64(party.public.encryption), { name: "X25519" }, true, []);
  return { id: party.id, signing, encryption: { privateKey, publicKey } };
}

export async function importSender(id, publicKeys) {
  return { id, signing: await subtle.importKey("raw", fromB64(publicKeys.signing), { name: "Ed25519" }, false, ["verify"]) };
}

export async function verifyEnvelope(envelope, sender, now) {
  checkShape(envelope);
  if (envelope.from !== sender.id) throw new ProtocolError("wrong-sender");
  const ct = fromB64(envelope.ct);
  const sig = fromB64(envelope.sig);
  if (sig.length !== 64) throw new ProtocolError("malformed");
  if (!(await subtle.verify({ name: "Ed25519" }, sender.signing, sig, concat(headerBytes(envelope), ct)))) throw new ProtocolError("bad-signature");
  if (now >= envelope.exp) throw new ProtocolError("expired");
  if (envelope.at > now + CLOCK_SKEW) throw new ProtocolError("not-yet-valid");
}

export async function verifyCommand(envelope, key, now) {
  checkShape(envelope);
  if (!KINDS[envelope.kind].command) throw new ProtocolError("malformed");
  if (envelope.cert.account !== key.account) throw new ProtocolError("wrong-account");
  if (!(await verifyDeviceCert(key, envelope.cert))) throw new ProtocolError("bad-certificate");
  await verifyEnvelope(envelope, await importSender(envelope.cert.device, { signing: envelope.cert.signing }), now);
  return envelope.cert;
}

export async function openCommand(envelope, recipient, key, now) {
  const device = await verifyCommand(envelope, key, now);
  return { device, payload: await decryptEnvelope(envelope, recipient) };
}

export async function openBlob(blob, ciphertext) {
  const digest = await sha256(ciphertext);
  if (`blb_${digest.toHex()}` !== blob.id) throw new ProtocolError("wrong-id");
  if (ciphertext.length !== blob.bytes + BLOB_OVERHEAD) throw new ProtocolError("wrong-size");
  const raw = fromB64(blob.key);
  if (raw.length !== 32) throw new ProtocolError("bad-key");
  try {
    const key = await subtle.importKey("raw", raw, "AES-GCM", false, ["decrypt"]);
    return new Uint8Array(await subtle.decrypt({ name: "AES-GCM", iv: ciphertext.slice(0, NONCE_LEN), additionalData: BLOB_DOMAIN }, key, ciphertext.slice(NONCE_LEN)));
  } catch {
    throw new ProtocolError("undecryptable");
  }
}

export async function decryptEnvelope(envelope, recipient) {
  const aad = headerBytes(envelope);
  if (!Object.hasOwn(envelope.keys, recipient.id)) throw new ProtocolError("not-a-recipient");
  const row = fromB64(envelope.keys[recipient.id]);
  if (row.length <= hpke.ENC_LEN) throw new ProtocolError("malformed");
  let contentKey;
  try {
    contentKey = await hpke.open(recipient.encryption, HPKE_INFO, aad, row);
  } catch {
    throw new ProtocolError("undecryptable");
  }
  if ((await keyCommitment(contentKey)) !== envelope.kc) throw new ProtocolError("key-mismatch");
  const ct = fromB64(envelope.ct);
  if (ct.length <= NONCE_LEN) throw new ProtocolError("malformed");
  let plaintext;
  try {
    const key = await subtle.importKey("raw", contentKey, "AES-GCM", false, ["decrypt"]);
    plaintext = await subtle.decrypt({ name: "AES-GCM", iv: ct.slice(0, NONCE_LEN), additionalData: aad }, key, ct.slice(NONCE_LEN));
  } catch {
    throw new ProtocolError("undecryptable");
  }
  let payload;
  try {
    payload = JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(plaintext));
  } catch {
    throw new ProtocolError("malformed");
  }
  checkPayload(payload);
  if (payload.type !== envelope.kind) throw new ProtocolError("kind-mismatch");
  return payloadOf(payload);
}

export async function openEnvelope(envelope, recipient, sender, now) {
  await verifyEnvelope(envelope, sender, now);
  return decryptEnvelope(envelope, recipient);
}

export async function openNotice(envelope, device, machine, now) {
  checkShape(envelope);
  if (envelope.kind !== "notice" || envelope.machine !== machine.id) throw new ProtocolError("malformed");
  const notice = await openEnvelope(envelope, device, await importSender(machine.id, machine.keys), now);
  if (!fitsCollapse(notice, envelope.collapse)) throw new ProtocolError("category-mismatch");
  return notice;
}

const random = (length) => globalThis.crypto.getRandomValues(new Uint8Array(length));

export async function sealEnvelope(header, plaintext, sender, recipients, randomness = undefined) {
  const contentKey = randomness?.contentKey ?? random(32);
  const nonce = randomness?.nonce ?? random(NONCE_LEN);
  const sealedHeader = { ...header, kc: await keyCommitment(contentKey) };
  const aad = headerBytes(sealedHeader);
  const keys = {};
  if (recipients.length === 0) throw new ProtocolError("no-recipients");
  if (new Set(recipients.map((recipient) => recipient.id)).size !== recipients.length) throw new ProtocolError("duplicate-recipient");
  for (const recipient of [...recipients].sort((a, b) => (a.id < b.id ? -1 : 1))) {
    const ekm = randomness?.ephemeral[recipient.id] ?? random(32);
    keys[recipient.id] = toB64(await hpke.seal(fromB64(recipient.encryption), HPKE_INFO, aad, contentKey, ekm));
  }
  const key = await subtle.importKey("raw", contentKey, "AES-GCM", false, ["encrypt"]);
  const sealed = new Uint8Array(await subtle.encrypt({ name: "AES-GCM", iv: nonce, additionalData: aad }, key, plaintext));
  const ct = concat(nonce, sealed);
  const sig = await subtle.sign({ name: "Ed25519" }, sender.signing, concat(aad, ct));
  const envelope = { ...sealedHeader, keys, ct: toB64(ct), sig: toB64(sig) };
  checkShape(envelope);
  return envelope;
}

const publicBytes = (keys) => concat(fromB64(keys.signing), fromB64(keys.encryption));

export const handoffNonce = () => random(32);

export const machineFingerprint = async (machine) => toB64(await sha256(CONNECT_DOMAIN, publicBytes(machine)));

export function parseConnectFragment(fragment) {
  const values = new URLSearchParams(String(fragment ?? "").replace(/^#/, ""));
  const [code, fingerprint] = [values.get("k"), values.get("m")];
  try {
    if (fromB64(code ?? "").length !== 32 || fromB64(fingerprint ?? "").length !== 32) return null;
  } catch {
    return null;
  }
  return { code, fingerprint };
}

export async function accountWrapKey(prfOutput, salt) {
  const ikm = await subtle.importKey("raw", prfOutput, "HKDF", false, ["deriveBits"]);
  return new Uint8Array(await subtle.deriveBits({ name: "HKDF", hash: "SHA-256", salt, info: WRAP_KEY_INFO }, ikm, 256));
}

export const wrapAssociatedData = (account, credential, version) => concat(WRAP_AD_DOMAIN, field(account), bytesField(credential), u32(version));

async function wrapCipher(unlock, usage) {
  return subtle.importKey("raw", await accountWrapKey(unlock.prfOutput, unlock.salt), "AES-GCM", false, [usage]);
}

export async function wrapAccountSecrets(unlock, secrets, nonce = random(WRAP_NONCE_LEN)) {
  const plaintext = concat(secrets.signing, secrets.encryption);
  try {
    const sealed = await subtle.encrypt({ name: "AES-GCM", iv: nonce, additionalData: wrapAssociatedData(unlock.account, unlock.credential, unlock.version) }, await wrapCipher(unlock, "encrypt"), plaintext);
    return concat(nonce, new Uint8Array(sealed));
  } finally {
    plaintext.fill(0);
  }
}

export async function unwrapAccountSecrets(unlock, wrapped) {
  if (wrapped.length !== WRAPPED_LEN) throw new ProtocolError("wrong-account-key");
  let plaintext;
  try {
    plaintext = new Uint8Array(await subtle.decrypt({ name: "AES-GCM", iv: wrapped.slice(0, WRAP_NONCE_LEN), additionalData: wrapAssociatedData(unlock.account, unlock.credential, unlock.version) }, await wrapCipher(unlock, "decrypt"), wrapped.slice(WRAP_NONCE_LEN)));
  } catch {
    throw new ProtocolError("wrong-account-key");
  }
  return { signing: plaintext.slice(0, 32), encryption: plaintext.slice(32), wipe: () => plaintext.fill(0) };
}

const sameBytes = (a, b) => a.length === b.length && a.every((byte, index) => byte === b[index]);

async function holdsPair(signing, verifying, privateKey, publicKey) {
  const probe = random(32);
  const signed = await subtle.verify({ name: "Ed25519" }, verifying, await subtle.sign({ name: "Ed25519" }, signing, probe), probe);
  const other = await subtle.generateKey({ name: "X25519" }, false, ["deriveBits"]);
  try {
    const mine = new Uint8Array(await subtle.deriveBits({ name: "X25519", public: other.publicKey }, privateKey, 256));
    const theirs = new Uint8Array(await subtle.deriveBits({ name: "X25519", public: publicKey }, other.privateKey, 256));
    return signed && sameBytes(mine, theirs);
  } catch {
    return false;
  }
}

export async function accountIdentity(key, secrets, account) {
  if (typeof account !== "string" || key?.account !== account) throw new ProtocolError("wrong-account");
  const signingPkcs8 = concat(PKCS8_ED25519, secrets.signing);
  const encryptionPkcs8 = concat(PKCS8_X25519, secrets.encryption);
  try {
    const signing = await subtle.importKey("pkcs8", signingPkcs8, { name: "Ed25519" }, false, ["sign"]);
    const privateKey = await subtle.importKey("pkcs8", encryptionPkcs8, { name: "X25519" }, false, ["deriveBits"]);
    const publicKey = await subtle.importKey("raw", fromB64(key.keys.encryption), { name: "X25519" }, true, []);
    const verifying = await subtle.importKey("raw", fromB64(key.keys.signing), { name: "Ed25519" }, false, ["verify"]);
    if (!(await holdsPair(signing, verifying, privateKey, publicKey))) throw new ProtocolError("key-mismatch");
    return { id: key.account, key: structuredClone(key), signing, verifying, encryption: { privateKey, publicKey } };
  } finally {
    signingPkcs8.fill(0);
    encryptionPkcs8.fill(0);
  }
}

async function generated(algorithm, usages, prefix) {
  const pair = await subtle.generateKey({ name: algorithm }, true, usages);
  const pkcs8 = new Uint8Array(await subtle.exportKey("pkcs8", pair.privateKey));
  try {
    if (pkcs8.length !== prefix.length + 32 || !sameBytes(pkcs8.slice(0, prefix.length), prefix)) throw new ProtocolError("malformed");
    return { secret: pkcs8.slice(prefix.length), public: toB64(new Uint8Array(await subtle.exportKey("raw", pair.publicKey))) };
  } finally {
    pkcs8.fill(0);
  }
}

export async function newAccountSecrets(account, version = 1) {
  const signing = await generated("Ed25519", ["sign", "verify"], PKCS8_ED25519);
  const encryption = await generated("X25519", ["deriveBits"], PKCS8_X25519);
  const secrets = { signing: signing.secret, encryption: encryption.secret };
  return { secrets, key: { account, version, keys: { signing: signing.public, encryption: encryption.public } }, wipe: () => (secrets.signing.fill(0), secrets.encryption.fill(0)) };
}

const accountKeyBytes = (key) => concat(fromB64(key.keys.signing), fromB64(key.keys.encryption));

export const wrapperStatement = async (key, credential, wrapped) => concat(WRAPPER_SIGN_DOMAIN, field(key.account), bytesField(credential), u32(key.version), accountKeyBytes(key), await sha256(wrapped));

const canonical = (list) => list.machines.every((listed, index) => prefixOf(listed.machine) === "mch" && (index === 0 || list.machines[index - 1].machine < listed.machine));

export function machineListStatement(account, list) {
  if (!Number.isSafeInteger(list?.counter) || list.counter < 0 || !Array.isArray(list.machines) || !canonical(list)) throw new ProtocolError("malformed");
  return concat(MACHINE_LIST_DOMAIN, field(account), u64(list.counter), u32(list.machines.length), ...list.machines.map((listed) => concat(field(listed.machine), publicBytes(listed.keys))));
}

export const emptyMachineList = () => ({ counter: 0, machines: [] });

export const machineListWithout = (list, machine) => ({ counter: list.counter + 1, machines: list.machines.filter((listed) => listed.machine !== machine) });

export function machineListWith(list, machine, keys) {
  const next = machineListWithout(list, machine);
  next.machines = [...next.machines, { machine, keys: structuredClone(keys) }].sort((a, b) => (a.machine < b.machine ? -1 : 1));
  return next;
}

export const machineListTrusts = (list, machine, keys) => list.machines.some((listed) => listed.machine === machine && listed.keys.signing === keys?.signing && listed.keys.encryption === keys?.encryption);

export async function signMachineList(identity, list) {
  return toB64(await signStatement(identity, machineListStatement(identity.id, list)));
}

export async function verifyMachineList(identity, signed) {
  try {
    const bytes = fromB64(signed?.signature ?? "");
    return bytes.length === 64 && (await subtle.verify({ name: "Ed25519" }, identity.verifying, bytes, machineListStatement(identity.id, signed.list)));
  } catch {
    return false;
  }
}

export const signStatement = async (identity, statement) => new Uint8Array(await subtle.sign({ name: "Ed25519" }, identity.signing, statement));

export async function accountFingerprints(key) {
  const print = async (domain, bytes) => toB64(await sha256(domain, field(key.account), u32(key.version), bytes));
  return { signing: await print(SIGNING_FINGERPRINT_DOMAIN, fromB64(key.keys.signing)), encryption: await print(ENCRYPTION_FINGERPRINT_DOMAIN, fromB64(key.keys.encryption)) };
}

export const accountCommitment = async (key, nonce, code) => sha256(ACCOUNT_COMMIT_DOMAIN, field(key.account), u32(key.version), accountKeyBytes(key), nonce, text.encode(code));

export function commandId(millis = Date.now(), randomBytes = random(10)) {
  const bytes = new Uint8Array(16);
  new DataView(bytes.buffer).setBigUint64(0, BigInt(millis) << 16n);
  bytes.set(randomBytes, 6);
  bytes[6] = 0x70 | (bytes[6] & 0x0f);
  bytes[8] = 0x80 | (bytes[8] & 0x3f);
  const hex = bytes.toHex();
  return `cmd_${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}

export async function sealCommand({ device, account, machine, payload, at, id = commandId(at * 1000) }) {
  checkPayload(payload);
  if (device.cert.device !== device.id || !(await verifyDeviceCert(account, device.cert))) throw new ProtocolError("cert-mismatch");
  const header = { v: PROTOCOL_VERSION, id, kind: payload.type, from: device.id, machine: machine.id, at, exp: at + KINDS[payload.type].ttl, schema: SCHEMA, cert: structuredClone(device.cert) };
  const recipients = [{ id: machine.id, encryption: machine.keys.encryption }, { id: account.account, encryption: account.keys.encryption }];
  return sealEnvelope(header, text.encode(JSON.stringify(payload)), device, recipients);
}
