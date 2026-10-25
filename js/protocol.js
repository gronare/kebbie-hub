import * as hpke from "./hpke.js";

const subtle = globalThis.crypto.subtle;
const text = new TextEncoder();
const HEADER_DOMAIN = text.encode("kelpie-hub envelope\0");
const HPKE_INFO = text.encode("kelpie-hub content key v1");
const KEY_COMMIT_DOMAIN = text.encode("kelpie-hub key commit v1");
const PAIR_COMMIT_DOMAIN = text.encode("kelpie-pair-commit-v1");
const SAS_DOMAIN = text.encode("kelpie-pair-v1");
const CONNECT_DOMAIN = text.encode("kelpie-connect-machine-v1");
const WRAP_KEY_INFO = text.encode("kebbie-hub account key v1");
const WRAP_AD_DOMAIN = text.encode("kebbie-hub account key wrap v1");
const WRAPPER_SIGN_DOMAIN = text.encode("kebbie-hub account key wrapper v1");
const MACHINE_LIST_DOMAIN = text.encode("kebbie-hub account machines v1");
const SIGNING_FINGERPRINT_DOMAIN = text.encode("kebbie-hub account signing key v1");
const ENCRYPTION_FINGERPRINT_DOMAIN = text.encode("kebbie-hub account encryption key v1");
const ACCOUNT_COMMIT_DOMAIN = text.encode("kebbie-connect-account-commit-v1");
export const WRAP_NONCE_LEN = 12;
const WRAPPED_LEN = WRAP_NONCE_LEN + 64 + 16;
const PKCS8_ED25519 = Uint8Array.fromHex("302e020100300506032b657004220420");
const PKCS8_X25519 = Uint8Array.fromHex("302e020100300506032b656e04220420");
const NONCE_LEN = 12;
const DAY = 86_400;
export const PROTOCOL_VERSION = 1;
export const SCHEMA = 1;
export const CLOCK_SKEW = 300;

const command = { prefix: "cmd", maxTtl: DAY, command: true };
export const KINDS = {
  board: { prefix: "brd", maxTtl: DAY, command: false },
  notice: { prefix: "ntc", maxTtl: DAY, command: false },
  result: { prefix: "cmd", maxTtl: 7 * DAY, command: false },
  answer: command,
  push: command,
  start: command,
  routine: command,
};

const ID = /^(?:(mch|dev|ntc|brd|alr)_[0-7][0-9A-HJKMNP-TV-Z]{25}|(cmd)_[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}|(blb)_[0-9a-f]{64}|(acc)_[A-Za-z0-9_-]{22})$/;
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
  git: optionalField(shape({ upstream: string, ahead: u32v, behind: u32v, uncommitted: u32v, approved: bool })),
  head: optionalField(string),
  verdict: optionalField(shape({ verdict: optionalField(oneOf("ok", "changes")), findings: optionalField(u32v), sha: optionalField(string), stale: bool, running: bool })),
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
    checkouts: list(shape({ repo: string, free_worktrees: u32v, busy_worktrees: u32v })),
    routines: list(shape({ id: string, title: string, repo: string, mode: string, next_run: optionalField(u64v), last_outcome: optionalField(string) })),
    items: list(item),
    sessions_outside_tasks: u32v,
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
