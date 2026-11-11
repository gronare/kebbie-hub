import { test } from "node:test";
import assert from "node:assert/strict";
import { addEmailScreen, address, codeScreen, connectEmailScreen, createPasskeyScreen, emailSignInScreen, emailText, keylessUnlockScreen, passkeySignInScreen, signedInAs, signedInStep, signedOutStep, typedCode } from "./email.js";
import { HubError } from "./hub.js";

const text = (html) => html.replace(/<[^>]+>/g, " ").replace(/&#39;/g, "'").replace(/\s+/g, " ").trim();
const actionsOf = (html) => html.slice(html.indexOf('<div class="auth-actions">'));
const primaries = (html) => [...html.matchAll(/class="btn primary" data-action="([^"]+)"/g)].map((match) => match[1]);

test("a hub that does not advertise email sign-in never reaches an email screen", () => {
  for (const info of [null, {}, { email: true, github: true }, { email_signin: false }]) {
    assert.equal(signedOutStep({ info, remembered: "cred", step: "code", chosen: true }), "passkey-only");
    const me = { account: "acc_x", key: null, key_access: false, github: "octocat" };
    assert.equal(signedInStep({ info, me, session: { state: "locked" }, address: { email: null, verified: false } }), null);
  }
});

test("without a remembered passkey the sign-in page offers email and GitHub and no passkey", () => {
  const info = { email_signin: true };
  assert.equal(signedOutStep({ info }), "email");
  const html = emailSignInScreen({ github: true });
  assert.equal(text(html), "kebbie hub See your boards Your machines' boards, end-to-end encrypted. Sign in with your email or GitHub. Email Send code or Continue with GitHub New to kebbie? Start in your terminal: kebbie setup");
  assert.doesNotMatch(text(html), /passkey/i);
  assert.deepEqual(primaries(html), ["send-code"]);
  assert.match(html, /autocomplete="email"/);
  assert.match(html, /href="\/auth\/github"/);
  assert.doesNotMatch(text(emailSignInScreen({ github: false })), /GitHub/);
});

test("a remembered passkey is the one main action and email waits behind a muted line", () => {
  const info = { email_signin: true };
  assert.equal(signedOutStep({ info, remembered: "cred" }), "passkey");
  assert.equal(signedOutStep({ info, remembered: "cred", chosen: true }), "email");
  const html = passkeySignInScreen({});
  assert.equal(text(html), "kebbie hub See your boards Your machines' boards, end-to-end encrypted. Sign in with your passkey Can't use your passkey? Sign in with email instead");
  assert.deepEqual(primaries(html), ["sign-in"]);
  assert.match(html, /class="link inline" data-action="email-instead"/);
});

test("the code screen takes one-time-code autofill and says where the code went", () => {
  assert.equal(signedOutStep({ info: { email_signin: true }, remembered: "cred", step: "code" }), "code");
  const html = codeScreen({ email: "<ada>@example.com" });
  assert.match(html, /autocomplete="one-time-code"/);
  assert.match(html, /inputmode="numeric"/);
  assert.match(html, /<b>&#60;ada&#62;@example.com<\/b>/);
  assert.equal(text(codeScreen({ email: "carl@example.com" })), "kebbie hub Check your email Enter the 6-digit code sent to carl@example.com . 6-digit code The code works for 10 minutes. Sign in Nothing arrived? Send a new code Use another email");
  assert.match(text(codeScreen({ purpose: "confirm", email: "carl@example.com" })), /^kebbie hub Confirm your email .* Confirm email Nothing arrived/);
  assert.match(text(codeScreen({ purpose: "connect", email: "carl@example.com", name: "studio-mac" })), /minutes\. studio-mac waits in its terminal\. Next you create a passkey, then connect it\. Continue/);
  assert.deepEqual(primaries(codeScreen({ purpose: "confirm", email: "a@b.c" })), ["confirm-email"]);
});

test("an email or GitHub session sees its account, and the boards wait for a passkey", () => {
  const info = { email_signin: true };
  const keyed = { account: "acc_x", key: { version: 1 }, key_access: false };
  assert.equal(signedInStep({ info, me: keyed, session: { state: "locked" } }), "unlock");
  assert.equal(signedInStep({ info, me: { ...keyed, key: null } }), "create-passkey");
  const unlock = keylessUnlockScreen({ who: "carl@example.com" });
  assert.match(text(unlock), /^kebbie hub ✓ Signed in as carl@example.com Unlock your boards Email and GitHub open your account, not your boards\./);
  assert.deepEqual(primaries(unlock), ["use-passkey"]);
  assert.match(unlock, /href="\/account">Go to your account/);
  const create = createPasskeyScreen({ who: "carl@example.com" });
  assert.match(text(create), /Create a passkey to unlock your boards .* Create passkey$/);
  assert.doesNotMatch(text(create), /Not now/, "no skip");
  assert.equal(signedInAs({ github: "octocat" }, null), "octocat");
  assert.equal(signedInAs({ github: "octocat" }, { email: "a@b.c" }), "a@b.c");
});

test("an account without a confirmed email adds one after its passkey unlocks, with no skip", () => {
  const info = { email_signin: true };
  const me = { account: "acc_x", key: { version: 1 }, key_access: true };
  const unlocked = { state: "unlocked" };
  assert.equal(signedInStep({ info, me, session: { state: "locked" }, address: { email: null, verified: false } }), null, "the passkey comes first");
  assert.equal(signedInStep({ info, me, session: unlocked, address: { email: null, verified: false } }), "add-email");
  assert.equal(signedInStep({ info, me, session: unlocked, address: { email: "a@b.c", verified: false } }), "add-email");
  assert.equal(signedInStep({ info, me, session: unlocked, address: { email: null }, emailStep: { email: "a@b.c" } }), "confirm-email");
  assert.equal(signedInStep({ info, me, session: unlocked, address: { email: "a@b.c", verified: true } }), null);
  assert.equal(signedInStep({ info, me, session: unlocked, address: null }), null, "an unread address blocks nothing");
  const html = addEmailScreen({ email: "carl@example.com" });
  assert.match(text(html), /^kebbie hub ✓ Signed in with your passkey Add your email Every kebbie account has an email\./);
  assert.match(html, /value="carl@example.com"/);
  assert.deepEqual(primaries(html), ["add-email"]);
  assert.doesNotMatch(text(html), /Not now|Skip/);
});

test("connect without an account starts with email or GitHub and names the machine", () => {
  const html = connectEmailScreen({ name: "studio-mac", code: "Z78K-XF53", github: true });
  assert.match(text(html), /^kebbie hub Connect studio-mac to kebbie hub The hub shows studio-mac's board .* Start with your email or GitHub\. Email Send code or Continue with GitHub$/);
  assert.match(html, /href="\/auth\/github\?connect=Z78K-XF53&#38;next=%2Fconnect%3Fcode%3DZ78K-XF53"/);
});

test("every auth screen reads intro, art, then a form whose actions hold the one main action", () => {
  for (const html of [emailSignInScreen({ github: true }), passkeySignInScreen({}), codeScreen({ email: "a@b.c" }), createPasskeyScreen({}), addEmailScreen({}), connectEmailScreen({ name: "mac" }), keylessUnlockScreen({})]) {
    const order = ["auth-intro", "auth-art", "auth-form", "auth-actions"].map((name) => html.indexOf(`class="${name}"`));
    assert.ok(order.every((at, index) => at >= 0 && (index === 0 || at > order[index - 1])), html);
    assert.match(html, /<div class="auth-art" aria-hidden="true">/);
    assert.equal(html.match(/class="art-row"/g).length, 3);
    assert.doesNotMatch(html, /autofocus|style="/);
  }
  assert.match(emailSignInScreen({ github: true }), /<div class="auth-alt"><div class="or">.*Continue with GitHub.*kebbie setup/s);
  assert.equal(address(" Ada@Example.COM "), "ada@example.com");
  for (const wrong of ["", "ada", "@example.com", "ada@example", "ada@.com", "a da@example.com", "Ada <ada@example.com>"]) assert.equal(address(wrong), null, wrong);
});

test("every screen keeps its main action and the way back together in one action group", () => {
  for (const html of [emailSignInScreen({ github: true }), passkeySignInScreen({}), codeScreen({ email: "a@b.c" }), createPasskeyScreen({}), addEmailScreen({}), connectEmailScreen({ name: "mac" }), keylessUnlockScreen({})]) {
    assert.equal(primaries(actionsOf(html)).length, 1, html);
    assert.equal(primaries(html).length, 1);
  }
});

test("codes and refusals read as people say them", () => {
  assert.equal(typedCode(" 48 21-09x7"), "482109");
  assert.match(emailText(new HubError(400, "wrong-code")), /doesn't match, or it has expired/);
  assert.match(emailText(new HubError(429, "rate-limited")), /Too many tries/);
  assert.equal(emailText(new HubError(500, "boom")), "The hub answered 500 (boom).");
});
