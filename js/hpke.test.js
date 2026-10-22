import assert from "node:assert/strict";
import { test } from "node:test";

import { Aes128Gcm, CipherSuite, DhkemX25519HkdfSha256, HkdfSha256 } from "@hpke/core";

import { open, seal } from "./hpke.js";

const suite = new CipherSuite({ kem: new DhkemX25519HkdfSha256(), kdf: new HkdfSha256(), aead: new Aes128Gcm() });
const info = new TextEncoder().encode("kelpie-hub content key v1");
const aad = new TextEncoder().encode("header bytes");
const plaintext = crypto.getRandomValues(new Uint8Array(32));

const recipient = async () => {
  const pair = await crypto.subtle.generateKey({ name: "X25519" }, false, ["deriveBits"]);
  return { pair, raw: new Uint8Array(await crypto.subtle.exportKey("raw", pair.publicKey)) };
};

test("what this HPKE seals, @hpke/core opens", async () => {
  const { pair, raw } = await recipient();
  const row = await seal(raw, info, aad, plaintext, crypto.getRandomValues(new Uint8Array(32)));
  const context = await suite.createRecipientContext({ recipientKey: pair, enc: row.slice(0, 32), info });
  assert.deepEqual(new Uint8Array(await context.open(row.slice(32), aad)), plaintext);
});

test("what @hpke/core seals, this HPKE opens with a non-extractable key", async () => {
  const { pair, raw } = await recipient();
  assert.equal(pair.privateKey.extractable, false);
  const context = await suite.createSenderContext({ recipientPublicKey: await suite.kem.deserializePublicKey(raw), info });
  const row = new Uint8Array([...new Uint8Array(context.enc), ...new Uint8Array(await context.seal(plaintext, aad))]);
  assert.deepEqual(await open(pair, info, aad, row), plaintext);
});

test("the same ephemeral input seals the same bytes as @hpke/core", async () => {
  const { raw } = await recipient();
  const ekm = crypto.getRandomValues(new Uint8Array(32));
  const context = await suite.createSenderContext({ recipientPublicKey: await suite.kem.deserializePublicKey(raw), info, ekm });
  const theirs = new Uint8Array([...new Uint8Array(context.enc), ...new Uint8Array(await context.seal(plaintext, aad))]);
  assert.deepEqual(await seal(raw, info, aad, plaintext, ekm), theirs);
});

test("another associated data or another key does not open", async () => {
  const { pair, raw } = await recipient();
  const other = await recipient();
  const row = await seal(raw, info, aad, plaintext, crypto.getRandomValues(new Uint8Array(32)));
  await assert.rejects(open(pair, info, new Uint8Array([1]), row));
  await assert.rejects(open(other.pair, info, aad, row));
});

test("RFC 9180 A.1.1 base mode seals and opens byte for byte", async () => {
  const hex = (value) => Uint8Array.fromHex(value);
  const info9180 = hex("4f6465206f6e2061204772656369616e2055726e");
  const pkRm = hex("3948cfe0ad1ddb695d780e59077195da6c56506b027329794ab02bca80815c4d");
  const skRm = hex("4612c550263fc8ad58375df3f557aac531d26850903e55a9f23f21d8534e8ac8");
  const enc = hex("37fda3567bdbd628e88668c3c8d7e97d1d1253b6d4ea6d44c150f741f1bf4431");
  const pt = hex("4265617574792069732074727574682c20747275746820626561757479");
  const aad0 = hex("436f756e742d30");
  const ct = hex("f938558b5d72f1a23810b4be2ab4f84331acc02fc97babc53a52ae8218a355a96d8770ac83d07bea87e13c512a");
  const sealed = await seal(pkRm, info9180, aad0, pt, hex("7268600d403fce431561aef583ee1613527cff655c1343f29812e66706df3234"));
  assert.deepEqual(sealed, new Uint8Array([...enc, ...ct]));
  const privateKey = await crypto.subtle.importKey("pkcs8", new Uint8Array([...hex("302e020100300506032b656e04220420"), ...skRm]), { name: "X25519" }, false, ["deriveBits"]);
  const publicKey = await crypto.subtle.importKey("raw", pkRm, { name: "X25519" }, true, []);
  assert.deepEqual(await open({ privateKey, publicKey }, info9180, aad0, sealed), pt);
});
