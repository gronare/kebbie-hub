import assert from "node:assert/strict";
import { test } from "node:test";

import { startPage } from "./pages.js";

const kept = new Map();
globalThis.indexedDB = {
  open() {
    const request = {};
    request.result = {
      transaction() {
        const transaction = {};
        const store = { get: (key) => ({ result: kept.get(key) }), put: (value, key) => (kept.set(key, value), { result: key }), delete: (key) => (kept.delete(key), { result: undefined }) };
        transaction.objectStore = () => store;
        queueMicrotask(() => transaction.oncomplete?.());
        return transaction;
      },
    };
    queueMicrotask(() => request.onsuccess?.());
    return request;
  },
};

const CODE = "K7QF-M2XD";
const VIEW = { code: CODE, name: "work-mac", state: "pending", expires_at: 1_900_000_000 };

function hostedHub({ sessions, ended = [], keyed = false }) {
  const calls = [];
  const json = (body, status = 200) => new Response(JSON.stringify(body), { status, headers: { "Content-Type": "application/json" } });
  const me = (account) => ({ account, handle: "k0123456789ab", key_access: keyed, key: keyed ? { account, version: 1 } : null });
  globalThis.fetch = async (url, options = {}) => {
    const named = options.headers?.["Kebbie-Account"] ?? null;
    const path = String(url);
    const signedIn = named ? sessions.includes(named) && named : sessions.length === 1 && sessions[0];
    const call = { method: options.method ?? "GET", path, named, status: 200 };
    calls.push(call);
    const answer = (body, status = 200) => ((call.status = status), json(body, status));
    if (path === "/v1/hub") return answer({ email: true, email_signin: true, signup: "open" });
    if (path === `/v1/connect?code=${CODE}`) return answer(VIEW);
    if (path === "/v1/a/email/code") return answer({ email: "new@example.com", expires_in: 600 });
    if (path === "/v1/a/email/verify") {
      sessions.push("acc_new");
      return answer({ account: me("acc_new"), created: true });
    }
    if (sessions.length > 1 && !named) return answer({ error: "account-required" }, 400);
    if (!signedIn || ended.includes(path)) return answer({ error: "unauthorized" }, 401);
    if (path === "/v1/a/account") return answer(me(signedIn));
    if (path === "/v1/a/email") return answer({ email: "new@example.com", verified: true, sent: false });
    if (path === `/v1/a/connect?code=${CODE}`) return answer(VIEW);
    if (path === "/v1/a/boards") return answer({ error: "passkey-required" }, 403);
    return answer(null, 204);
  };
  return calls;
}

function connectPage() {
  globalThis.location = new URL(`http://hub.test/connect?code=${CODE}`);
  globalThis.history = { replaceState() {}, pushState() {} };
  globalThis.document = { activeElement: null, title: "", getElementById: () => null, addEventListener() {}, visibilityState: "visible", documentElement: { classList: { contains: () => false } } };
  globalThis.scrollTo = () => {};
  globalThis.PublicKeyCredential = function PublicKeyCredential() {};
  Object.defineProperty(globalThis, "navigator", { value: { userAgent: "Mozilla/5.0 (Macintosh)", credentials: {} }, configurable: true, writable: true });
  const typed = { "#email": { value: "new@example.com", focus() {} }, "#code": { value: "123456", focus() {} } };
  const listeners = [];
  const root = {
    innerHTML: "",
    contains: () => false,
    querySelector: (selector) => typed[selector] ?? null,
    addEventListener(type, listener, options) {
      listeners.push({ type, listener, signal: options?.signal });
    },
    click(dataset) {
      const target = { dataset };
      for (const { type, listener, signal } of listeners) if (type === "click" && !signal?.aborted) listener({ target: { closest: (selector) => (selector === "[data-action]" ? target : null) }, preventDefault() {} });
    },
  };
  return { root, page: startPage(root) };
}

const until = async (check) => {
  for (let tries = 0; tries < 200 && !check(); tries++) await new Promise((resolve) => setTimeout(resolve, 5));
  assert.ok(check(), "the page never got there");
};

test("an email sign-up on the connect page carries on as the new account in a browser that remembers one whose session is gone", async () => {
  kept.clear();
  kept.set("accounts", [{ account: "acc_gone", name: "Home", colour: "blue" }]);
  const calls = hostedHub({ sessions: [] });
  const { root, page } = connectPage();
  await page.started;
  assert.match(root.innerHTML, /data-action="send-code"/);
  root.click({ action: "send-code" });
  await until(() => calls.some((call) => call.path === "/v1/a/email/code"));
  root.click({ action: "enter-code" });
  await until(() => root.innerHTML.includes("Create a passkey"));
  const after = calls.slice(calls.findIndex((call) => call.path === "/v1/a/email/verify") + 1);
  assert.deepEqual(after.filter((call) => call.status === 401), [], "no call after the code names an account this browser holds no session for");
  assert.ok(after.some((call) => call.path === `/v1/a/connect?code=${CODE}` && call.named === "acc_new"), JSON.stringify(after));
  assert.doesNotMatch(root.innerHTML, /used or expired|signed out/);
  page.leave();
});

test("a session that ends on the connect page reads as signed out, not as a used or expired link", async () => {
  kept.clear();
  const calls = hostedHub({ sessions: ["acc_home"], ended: [`/v1/a/connect?code=${CODE}`] });
  const { root, page } = connectPage();
  await page.started;
  assert.ok(calls.some((call) => call.path === `/v1/a/connect?code=${CODE}` && call.status === 401));
  assert.match(root.innerHTML, /You were signed out\. Sign in again\./);
  assert.doesNotMatch(root.innerHTML, /used or expired/);
  page.leave();
});

test("the connect page offers only the accounts with a live session, and marks the one whose session is gone as signed out", async () => {
  kept.clear();
  kept.set("accounts", [{ account: "acc_gone", colour: "blue" }, { account: "acc_home", colour: "peach" }]);
  hostedHub({ sessions: ["acc_home"], keyed: true });
  const { root, page } = connectPage();
  await page.started;
  assert.doesNotMatch(root.innerHTML, /Connect it to/, "one live account means no picker");
  assert.deepEqual(kept.get("accounts").map((one) => [one.account, one.signedOut === true]), [["acc_gone", true], ["acc_home", false]]);
  page.leave();

  kept.set("accounts", [{ account: "acc_gone", colour: "blue" }, { account: "acc_home", colour: "peach" }]);
  hostedHub({ sessions: ["acc_gone", "acc_home"], keyed: true });
  const both = connectPage();
  await both.page.started;
  assert.equal((both.root.innerHTML.match(/class="pill" href="\/connect/g) ?? []).length, 2, "two live accounts give a picker of two");
  both.page.leave();
});
