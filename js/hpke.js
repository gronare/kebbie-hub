const subtle = globalThis.crypto.subtle;
const text = new TextEncoder();
const PKCS8_X25519 = Uint8Array.fromHex("302e020100300506032b656e04220420");
const KEM_SUITE = concat(text.encode("KEM"), new Uint8Array([0x00, 0x20]));
const HPKE_SUITE = concat(text.encode("HPKE"), new Uint8Array([0x00, 0x20, 0x00, 0x01, 0x00, 0x01]));
const VERSION = text.encode("HPKE-v1");
const HASH_LEN = 32;
const KEY_LEN = 16;
const NONCE_LEN = 12;
export const ENC_LEN = 32;

function concat(...parts) {
  const out = new Uint8Array(parts.reduce((sum, part) => sum + part.length, 0));
  let offset = 0;
  for (const part of parts) {
    out.set(part, offset);
    offset += part.length;
  }
  return out;
}

const hmac = async (key, data) => {
  const hmacKey = await subtle.importKey("raw", key.length === 0 ? new Uint8Array(HASH_LEN) : key, { name: "HMAC", hash: "SHA-256" }, false, ["sign"]);
  return new Uint8Array(await subtle.sign("HMAC", hmacKey, data));
};

const labeledExtract = (suite, salt, label, ikm) => hmac(salt, concat(VERSION, suite, text.encode(label), ikm));

const labeledExpand = async (suite, prk, label, info, length) => {
  if (length > HASH_LEN) throw new Error("hpke: expand longer than one block");
  const block = await hmac(prk, concat(new Uint8Array([length >> 8, length & 0xff]), VERSION, suite, text.encode(label), info, new Uint8Array([1])));
  return block.slice(0, length);
};

const sharedSecret = async (dh, enc, recipientPublic) => {
  const prk = await labeledExtract(KEM_SUITE, new Uint8Array(), "eae_prk", dh);
  return labeledExpand(KEM_SUITE, prk, "shared_secret", concat(enc, recipientPublic), HASH_LEN);
};

const keySchedule = async (shared, info) => {
  const empty = new Uint8Array();
  const context = concat(new Uint8Array([0]), await labeledExtract(HPKE_SUITE, empty, "psk_id_hash", empty), await labeledExtract(HPKE_SUITE, empty, "info_hash", info));
  const secret = await labeledExtract(HPKE_SUITE, shared, "secret", empty);
  const key = await subtle.importKey("raw", await labeledExpand(HPKE_SUITE, secret, "key", context, KEY_LEN), "AES-GCM", false, ["encrypt", "decrypt"]);
  return { key, nonce: await labeledExpand(HPKE_SUITE, secret, "base_nonce", context, NONCE_LEN) };
};

const x25519 = async (privateKey, publicBytes) => {
  const publicKey = await subtle.importKey("raw", publicBytes, { name: "X25519" }, true, []);
  return new Uint8Array(await subtle.deriveBits({ name: "X25519", public: publicKey }, privateKey, 256));
};

async function deriveKeyPair(ikm) {
  const prk = await labeledExtract(KEM_SUITE, new Uint8Array(), "dkp_prk", ikm);
  const secret = await labeledExpand(KEM_SUITE, prk, "sk", new Uint8Array(), 32);
  const privateKey = await subtle.importKey("pkcs8", concat(PKCS8_X25519, secret), { name: "X25519" }, true, ["deriveBits"]);
  const { x } = await subtle.exportKey("jwk", privateKey);
  return { privateKey, publicBytes: Uint8Array.fromBase64(x, { alphabet: "base64url" }) };
}

export async function seal(recipientPublic, info, aad, plaintext, ikm) {
  const ephemeral = await deriveKeyPair(ikm);
  const shared = await sharedSecret(await x25519(ephemeral.privateKey, recipientPublic), ephemeral.publicBytes, recipientPublic);
  const { key, nonce } = await keySchedule(shared, info);
  const sealed = new Uint8Array(await subtle.encrypt({ name: "AES-GCM", iv: nonce, additionalData: aad }, key, plaintext));
  return concat(ephemeral.publicBytes, sealed);
}

export async function open(recipient, info, aad, row) {
  const enc = row.slice(0, ENC_LEN);
  const recipientPublic = new Uint8Array(await subtle.exportKey("raw", recipient.publicKey));
  const shared = await sharedSecret(await x25519(recipient.privateKey, enc), enc, recipientPublic);
  const { key, nonce } = await keySchedule(shared, info);
  return new Uint8Array(await subtle.decrypt({ name: "AES-GCM", iv: nonce, additionalData: aad }, key, row.slice(ENC_LEN)));
}
