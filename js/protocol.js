import * as hpke from "./hpke.js";

const subtle = globalThis.crypto.subtle;
const text = new TextEncoder();
const HEADER_DOMAIN = text.encode("kelpie-hub envelope\0");
const HPKE_INFO = text.encode("kelpie-hub content key v1");
const KEY_COMMIT_DOMAIN = text.encode("kelpie-hub key commit v1");
const PAIR_COMMIT_DOMAIN = text.encode("kelpie-pair-commit-v1");
const SAS_DOMAIN = text.encode("kelpie-pair-v1");
const PKCS8_ED25519 = Uint8Array.fromHex("302e020100300506032b657004220420");
const PKCS8_X25519 = Uint8Array.fromHex("302e020100300506032b656e04220420");
const NONCE_LEN = 12;
const DAY = 86_400;
export const PROTOCOL_VERSION = 1;
export const SCHEMA = 1;
export const CLOCK_SKEW = 300;

const command = { prefix: "cmd", maxTtl: DAY, command: true };
export const KINDS = {
  board: { prefix: "brd", maxTtl: 30 * DAY, command: false },
  notice: { prefix: "ntc", maxTtl: DAY, command: false },
  result: { prefix: "cmd", maxTtl: 7 * DAY, command: false },
  answer: command,
  push: command,
  start: command,
  routine: command,
};

const ID = /^(?:(mch|dev|ntc|brd|alr)_[0-7][0-9A-HJKMNP-TV-Z]{25}|(cmd)_[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}|(blb)_[0-9a-f]{64})$/;
const BASE64URL = /^[A-Za-z0-9_-]*$/;

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
const u64 = (value) => {
  const out = new Uint8Array(8);
  new DataView(out.buffer).setBigUint64(0, BigInt(value));
  return out;
};
const field = (value) => {
  const bytes = text.encode(value);
  if (bytes.length > 0xffff) throw new ProtocolError("malformed");
  return concat(u16(bytes.length), bytes);
};
const absent = (value) => value === undefined || value === null;
const optional = (value) => (absent(value) ? new Uint8Array([0]) : concat(new Uint8Array([1]), field(value)));

const prefixOf = (id) => {
  const match = typeof id === "string" ? ID.exec(id) : null;
  return match ? (match[1] ?? match[2] ?? match[3]) : null;
};
const isTime = (value) => Number.isSafeInteger(value) && value >= 0;
const isPlainObject = (value) => typeof value === "object" && value !== null && !Array.isArray(value);

export function checkShape(envelope) {
  if (!isPlainObject(envelope)) throw new ProtocolError("malformed");
  if (envelope.v !== PROTOCOL_VERSION) throw new ProtocolError("unsupported-version");
  if (envelope.schema !== SCHEMA) throw new ProtocolError("unsupported-schema");
  const kind = typeof envelope.kind === "string" && Object.hasOwn(KINDS, envelope.kind) ? KINDS[envelope.kind] : null;
  const fits =
    kind !== null &&
    prefixOf(envelope.id) === kind.prefix &&
    prefixOf(envelope.machine) === "mch" &&
    (kind.command ? prefixOf(envelope.from) === "dev" : envelope.from === envelope.machine) &&
    isTime(envelope.at) &&
    isTime(envelope.exp) &&
    envelope.exp > envelope.at &&
    envelope.exp - envelope.at <= kind.maxTtl &&
    typeof envelope.kc === "string" &&
    /^[A-Za-z0-9_-]{43}$/.test(envelope.kc) &&
    (envelope.kind === "notice" || (absent(envelope.urgency) && absent(envelope.collapse))) &&
    (absent(envelope.urgency) || envelope.urgency === "high" || envelope.urgency === "normal") &&
    (absent(envelope.collapse) || (typeof envelope.collapse === "string" && /^[A-Za-z0-9_-]{1,64}$/.test(envelope.collapse))) &&
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
const u32 = uint(0xffff_ffff);
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
const item = shape({
  address: string,
  group: oneOf("NEEDS YOU", "WORKING", "TO REVIEW", "READY TO PUSH", "CI RUNNING", "WAITING FOR WORK"),
  urgency: u8,
  summary: string,
  next_for_you: optionalField(string),
  agent: optionalField(shape({ status: oneOf("working", "blocked", "idle", "done") })),
  report: optionalField(shape({ state: string, next: optionalField(string), summary: string, at: u64v })),
  question: optionalField(
    shape({
      id: string,
      text: string,
      options: list(shape({ label: string, meaning: string })),
      recommend: optionalField(u8),
      kept_for_user: bool,
      plan: optionalField(blobRef),
      at: u64v,
    }),
  ),
  prompt: optionalField(shape({ id: string, question: string, options: list(shape({ label: string, takes_text: bool })) })),
  git: optionalField(shape({ upstream: string, ahead: u32, behind: u32, uncommitted: u32, approved: bool })),
  head: optionalField(string),
  verdict: optionalField(shape({ verdict: optionalField(oneOf("ok", "changes")), findings: optionalField(u32), sha: optionalField(string), stale: bool, running: bool })),
  push_permission: optionalField(oneOf("task", "repo")),
  push_waits: optionalField(string),
  follows_main: bool,
  ci: optionalField(shape({ state: oneOf("running", "failed"), failure: optionalField(string) })),
  recover: optionalField(shape({ note: string })),
  routine: optionalField(string),
  card: optionalField(shape({ sha: string, current: bool, blob: blobRef })),
});
const REASONS = ["already-settled", "head-moved", "unknown-repo", "not-allowed", "expired", "undecryptable", "bad-request"];
const EVENTS = ["needs-you", "ready", "question", "summary", "ci-failed", "ci-green", "api-error", "routine-findings", "routine-failed"];
const typed = (fields) => shape({ type: string, ...fields });
const PAYLOADS = {
  board: typed({
    rev: u64v,
    kelpie: string,
    away_since: optionalField(u64v),
    checkouts: list(shape({ repo: string, free_worktrees: u32, busy_worktrees: u32 })),
    routines: list(shape({ id: string, title: string, repo: string, mode: string, next_run: optionalField(u64v), last_outcome: optionalField(string) })),
    items: list(item),
    sessions_outside_tasks: u32,
  }),
  notice: typed({
    event: oneOf(...EVENTS),
    address: optionalField(string),
    title: string,
    body: string,
    question: optionalField(string),
    options: list(string),
    recommend: optionalField(u8),
    board_rev: u64v,
    retract: optionalField(string),
  }),
  result: (value) =>
    typed({ command: id, outcome: oneOf("done", "refused", "failed"), reason: optionalField(oneOf(...REASONS)), text: string })(value) &&
    (value.outcome === "refused") === !absent(value.reason),
  answer: typed({ address: string, question: string, choice: optionalField(u8), comment: string }),
  push: typed({ address: string, head: string, card_sha: optionalField(string) }),
  start: typed({ repo: string, name: string, prompt: string }),
  routine: typed({ id: string }),
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
  );
}

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
  return payload;
}

export async function openEnvelope(envelope, recipient, sender, now) {
  await verifyEnvelope(envelope, sender, now);
  return decryptEnvelope(envelope, recipient);
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

export const pairingNonce = () => random(32);

export const pairingCommitment = async (device, deviceNonce, code) => sha256(PAIR_COMMIT_DOMAIN, publicBytes(device), deviceNonce, text.encode(code));

export async function comparisonCode(machine, device, machineNonce, deviceNonce, code) {
  const digest = await sha256(SAS_DOMAIN, publicBytes(machine), publicBytes(device), machineNonce, deviceNonce, text.encode(code));
  const digits = String(new DataView(digest.buffer).getUint32(0) % 1_000_000).padStart(6, "0");
  return `${digits.slice(0, 3)} ${digits.slice(3)}`;
}

export async function machineComparisonCode(commitment, machine, machineNonce, device, deviceNonce, code) {
  const revealed = await pairingCommitment(device, deviceNonce, code);
  if (toB64(revealed) !== toB64(commitment)) throw new ProtocolError("commitment-mismatch");
  return comparisonCode(machine, device, machineNonce, deviceNonce, code);
}
