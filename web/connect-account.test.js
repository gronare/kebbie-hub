import assert from "node:assert/strict";
import { test } from "node:test";

import { startPage } from "./pages.js";

const CODE = "K7QF-M2XD";
const HOME = "acc_0123456789abcdefghij01";
const WORK = "acc_0123456789abcdefghij02";
const HOME_PASSKEYS = ["aG9tZS1vbmU", "aG9tZS10d28"];

function connectPage({ held, passkeyOf = WORK }) {
  const calls = [];
  const asked = [];
  const bytes = (...values) => new Uint8Array(values).buffer;
  globalThis.location = new URL(`http://hub.test/connect?code=${CODE}`);
  globalThis.history = { replaceState() {}, pushState() {} };
  globalThis.document = { activeElement: null, title: "", getElementById: () => null, addEventListener() {}, visibilityState: "visible", documentElement: { classList: { contains: () => false } } };
  globalThis.scrollTo = () => {};
  globalThis.PublicKeyCredential = function PublicKeyCredential() {};
  Object.defineProperty(globalThis, "navigator", {
    value: {
      userAgent: "Mozilla/5.0 (Macintosh)",
      credentials: {
        get: async (options) => {
          asked.push(options);
          return { id: "d29yaw", rawId: bytes(1), type: "public-key", getClientExtensionResults: () => ({}), response: { authenticatorData: bytes(3), clientDataJSON: bytes(4), signature: bytes(5), userHandle: null } };
        },
      },
    },
    configurable: true,
    writable: true,
  });
  globalThis.fetch = async (url, options = {}) => {
    const method = options.method ?? "GET";
    calls.push({ method, url: String(url), named: options.headers?.["Kebbie-Account"] ?? null, body: options.body ? JSON.parse(options.body) : null });
    const json = (body, status = 200) => new Response(JSON.stringify(body), { status, headers: { "Content-Type": "application/json" } });
    if (url === `/v1/a/connect?code=${CODE}`) return json({ code: CODE, name: "Devbox", state: "pending", expires_at: 1_900_000_000 });
    if (url === "/v1/a/passkeys") return json({ passkeys: HOME_PASSKEYS.map((passkey) => ({ passkey, name: "iPhone", created_at: 1, last_used_at: null, unlocks: true })) });
    if (url === "/v1/a/login/begin") return json({ ceremony: "c1", options: { publicKey: { challenge: "AAAA", allowCredentials: [] } } });
    if (url === "/v1/a/login/finish") return json({ account: passkeyOf, handle: "work", key_access: true, key: null, unlock: { credential: "d29yaw", prf_salt: null, wrapped: null } });
    return new Response(null, { status: 204 });
  };
  const listeners = [];
  const root = {
    innerHTML: "",
    contains: () => false,
    querySelector: () => null,
    addEventListener(type, listener, options) {
      listeners.push({ type, listener, signal: options?.signal });
    },
    click(dataset) {
      const target = { dataset };
      for (const { type, listener, signal } of listeners) if (type === "click" && !signal?.aborted) listener({ target: { closest: (selector) => (selector === "[data-action]" ? target : null) }, preventDefault() {} });
    },
  };
  const me = { account: HOME, handle: "home", key_access: true, plan: null };
  const page = startPage(root, { given: { info: { commands: true }, me, session: { state: "locked", me }, address: null, accounts: held } });
  return { root, calls, asked, page };
}

const until = async (check) => {
  for (let tries = 0; tries < 200 && !check(); tries++) await new Promise((resolve) => setTimeout(resolve, 5));
  assert.ok(check(), "the page never got there");
};

const toB64url = (buffer) => Buffer.from(buffer).toString("base64url");

test("Connect offers only the passkeys of the account the picker shows, and refuses another account's passkey before approving", async () => {
  const held = [{ account: HOME, name: "Home", colour: "blue" }];
  const { root, calls, asked, page } = connectPage({ held });
  await page.started;
  root.click({ action: "connect" });
  await until(() => root.innerHTML.includes("That passkey belongs to another account than Home."));
  assert.deepEqual(asked[0].publicKey.allowCredentials.map((one) => toB64url(one.id)), HOME_PASSKEYS);
  assert.equal(calls.find((call) => call.url === "/v1/a/passkeys").named, HOME);
  assert.equal(calls.some((call) => call.url === "/v1/a/connect/approve"), false, "nothing is approved for either account");
  assert.deepEqual(calls.filter((call) => call.url === "/v1/a/logout").map((call) => call.named), [WORK], "the stray session of an account the phone doesn't hold is ended");
  page.leave();
});

test("a passkey of another account the phone holds keeps that account's session, and still connects nothing", async () => {
  const held = [{ account: HOME, name: "Home", colour: "blue" }, { account: WORK, name: "Work", colour: "peach" }];
  const { root, calls, page } = connectPage({ held });
  await page.started;
  root.click({ action: "connect" });
  await until(() => root.innerHTML.includes("That passkey belongs to another account than Home."));
  assert.equal(calls.some((call) => call.url === "/v1/a/connect/approve"), false);
  assert.equal(calls.some((call) => call.url === "/v1/a/logout"), false);
  page.leave();
});
