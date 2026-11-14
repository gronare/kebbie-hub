import assert from "node:assert/strict";
import { test } from "node:test";

import { startPage } from "./pages.js";

const CODE = "K7QF-M2XD";
const FREE = { state: "free", pro: ["push"], machines: 1, trial_days: 14, billing: "https://hub.test/v1/a/billing/checkout" };

function connectPage({ plan = FREE, trial = { status: 200, body: { ...FREE, state: "trial", until: 1_900_000_000, machines: null } } } = {}) {
  const calls = [];
  const passkeys = [];
  globalThis.location = new URL(`http://hub.test/connect?code=${CODE}`);
  globalThis.history = { replaceState() {}, pushState() {} };
  globalThis.document = { activeElement: null, title: "", getElementById: () => null, addEventListener() {}, visibilityState: "visible", documentElement: { classList: { contains: () => false } } };
  globalThis.scrollTo = () => {};
  globalThis.PublicKeyCredential = function PublicKeyCredential() {};
  Object.defineProperty(globalThis, "navigator", {
    value: {
      userAgent: "Mozilla/5.0 (Macintosh)",
      credentials: {
        get: async () => {
          passkeys.push("get");
          throw Object.assign(new Error("cancelled"), { name: "NotAllowedError" });
        },
      },
    },
    configurable: true,
    writable: true,
  });
  globalThis.fetch = async (url, options = {}) => {
    const method = options.method ?? "GET";
    calls.push({ method, url: String(url), body: options.body ? JSON.parse(options.body) : null });
    const json = (body, status = 200) => new Response(JSON.stringify(body), { status, headers: { "Content-Type": "application/json" } });
    if (url === `/v1/a/connect?code=${CODE}`) return json({ code: CODE, name: "Devbox", state: "pending", expires_at: 1_900_000_000 });
    if (url === "/v1/a/boards") return json({ boards: [{ machine: "mch_01MACBOOK", name: "MacBook" }], list: null });
    if (url === "/v1/a/plan/trial") return json(trial.body, trial.status);
    if (url === "/v1/a/login/begin") return json({ ceremony: "c1", options: { publicKey: { challenge: "AAAA", allowCredentials: [] } } });
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
  const me = { account: "acc_0123456789abcdefghij01", key_access: true, plan };
  const page = startPage(root, { given: { info: { commands: true }, me, session: { state: "locked", me }, address: null } });
  return { root, calls, passkeys, page };
}

const until = async (check) => {
  for (let tries = 0; tries < 200 && !check(); tries++) await new Promise((resolve) => setTimeout(resolve, 5));
  assert.ok(check(), "the page never got there");
};

test("on the free plan a second machine shows the Pro card before anything is approved, and Not now stops it for Pro", async () => {
  const { root, calls, passkeys, page } = connectPage();
  await page.started;
  root.click({ action: "connect" });
  await until(() => root.innerHTML.includes("pro-sheet"));
  assert.match(root.innerHTML, /A second machine is part of Pro/);
  assert.match(root.innerHTML, /Devbox connects as soon as the trial starts\./);
  assert.deepEqual(passkeys, [], "no passkey is asked for while the card is open");
  assert.equal(calls.some((call) => call.url.startsWith("/v1/a/connect/approve")), false);

  root.click({ action: "pro-close" });
  await until(() => calls.some((call) => call.url === "/v1/a/connect/deny"));
  assert.deepEqual(calls.find((call) => call.url === "/v1/a/connect/deny").body, { code: CODE, pro: true }, "the waiting machine hears it was stopped for Pro");
  await until(() => root.innerHTML.includes("Cancelled."));
  assert.doesNotMatch(root.innerHTML, /pro-sheet/);
  page.leave();
});

test("Try Pro starts the trial and the connect goes on to the passkey", async () => {
  const { root, calls, passkeys, page } = connectPage();
  await page.started;
  root.click({ action: "connect" });
  await until(() => root.innerHTML.includes("pro-sheet"));
  root.click({ action: "pro-trial" });
  await until(() => passkeys.length > 0);
  assert.deepEqual(calls.filter((call) => call.url === "/v1/a/plan/trial").map((call) => call.method), ["POST"]);
  assert.equal(calls.some((call) => call.url === "/v1/a/connect/deny"), false);
  await until(() => !root.innerHTML.includes("pro-sheet"));
  page.leave();
});

test("an ended trial offers Continue with Pro instead, and a refused trial says so on the card", async () => {
  const ended = { ...FREE, state: "expired", until: 1_795_000_000, trial: true };
  const { root, page } = connectPage({ plan: ended });
  await page.started;
  root.click({ action: "connect" });
  await until(() => root.innerHTML.includes("pro-sheet"));
  assert.match(root.innerHTML, /Your Pro trial ended on/);
  assert.match(root.innerHTML, /data-action="pro-continue">Continue with Pro/);
  page.leave();

  const used = connectPage({ trial: { status: 409, body: { error: "trial-used" } } });
  await used.page.started;
  used.root.click({ action: "connect" });
  await until(() => used.root.innerHTML.includes("pro-sheet"));
  used.root.click({ action: "pro-trial" });
  await until(() => used.root.innerHTML.includes('role="alert"'));
  assert.match(used.root.innerHTML, /Your Pro trial has been used\./);
  used.page.leave();
});

test("with Pro, or on a hub without plans, Connect goes straight on", async () => {
  for (const plan of [{ ...FREE, state: "trial", until: 1_900_000_000, machines: null }, null]) {
    const { root, passkeys, page } = connectPage({ plan });
    await page.started;
    root.click({ action: "connect" });
    await until(() => passkeys.length > 0);
    assert.doesNotMatch(root.innerHTML, /pro-sheet/);
    page.leave();
  }
});
