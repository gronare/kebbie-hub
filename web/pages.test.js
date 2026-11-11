import assert from "node:assert/strict";
import { test } from "node:test";

import { isAuthScreen } from "./email.js";
import { startPage } from "./pages.js";

function page(path, { signedIn = true, hub = {} } = {}) {
  const asked = [];
  const listeners = [];
  globalThis.location = new URL(`http://hub.test${path}`);
  globalThis.history = { replaceState() {}, pushState() {} };
  globalThis.document = { activeElement: null, title: "", getElementById: () => null, addEventListener() {}, visibilityState: "visible" };
  globalThis.fetch = async (url) => {
    asked.push(String(url));
    if (String(url) === "/v1/a/account" && !signedIn) return new Response(JSON.stringify({ error: "unauthorized" }), { status: 401 });
    const body = String(url) === "/v1/a/account" ? { account: "acc_1", handle: "k0123456789ab", key_access: true } : String(url) === "/v1/hub" ? hub : { email: null, verified: false, sent: false };
    return new Response(JSON.stringify(body), { status: 200, headers: { "Content-Type": "application/json" } });
  };
  const root = {
    innerHTML: "",
    contains: () => false,
    querySelector: () => null,
    addEventListener(type, listener, options) {
      listeners.push({ type, listener, signal: options?.signal });
    },
    click(target) {
      const event = { target, defaultPrevented: false, preventDefault() { this.defaultPrevented = true; } };
      for (const { type, listener, signal } of listeners) if (type === "click" && !signal?.aborted) listener(event);
      return event;
    },
  };
  return { root, asked };
}

const link = (href) => ({ closest: (selector) => (selector === `a[href="${href}"]` ? { href } : null) });
const unlocked = { state: "unlocked", me: { account: "acc_1", key_access: true }, identity: { id: "acc_1" }, until: Date.now() + 60_000 };

test("an account page opened from the unlocked app hands the same unlocked session back, whatever the device store kept", async () => {
  const { root, asked } = page("/account");
  let backs = 0;
  const opened = startPage(root, { given: { info: {}, me: unlocked.me, session: unlocked, address: null }, back: () => backs++ });
  await opened.started;
  assert.match(root.innerHTML, /data-action="sign-out"/);
  assert.ok(!asked.includes("/v1/a/account"), "the page trusts the app's session instead of signing in again");
  assert.equal(root.click(link("/")).defaultPrevented, true);
  assert.equal(backs, 1);
  const left = opened.leave();
  assert.equal(left.session, unlocked);
  assert.equal(root.click(link("/")).defaultPrevented, false, "a left page stops listening");
});

test("an account page loaded on its own still signs in from the hub and leaves the back link to the browser", async () => {
  const { root, asked } = page("/account");
  const opened = startPage(root);
  await opened.started;
  assert.ok(asked.includes("/v1/a/account"));
  assert.equal(root.click(link("/")).defaultPrevented, false);
  assert.equal(opened.leave().session.state, "locked", "without a kept unlock a fresh load can only show the boards locked");
});

test("the account page asks a signed-out visitor to sign in on the sign-in layout, on every kind of hub", async () => {
  for (const hub of [{}, { email_signin: true }]) {
    const { root } = page("/account", { signedIn: false, hub });
    const opened = startPage(root);
    await opened.started;
    const main = root.innerHTML.replace(/^<main[^>]*>/, "");
    assert.ok(isAuthScreen(main), main.slice(0, 80));
    assert.match(main, /See your account/);
    opened.leave();
  }
});
