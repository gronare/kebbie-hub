import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import { HubError } from "./hub.js";
import { autofillAvailable, NoPasskeyOffered, signInText, signInWith, signUpText } from "./signin.js";

const HUB_SALT = JSON.parse(readFileSync(new URL("../vectors/accounts.json", import.meta.url), "utf8")).hub_prf_salt.salt;
const fromB64 = (value) => Uint8Array.fromBase64(value, { alphabet: "base64url" });
const bytes = () => new Uint8Array([1, 2, 3]).buffer;
const assertion = { id: "AQID", rawId: bytes(), type: "public-key", response: { authenticatorData: bytes(), clientDataJSON: bytes(), signature: bytes(), userHandle: bytes() } };

function fakes({ refuse = null, unknown = null } = {}) {
  const begun = [];
  const asked = [];
  const account = {
    loginBegin: async (request) => {
      begun.push(request);
      if (unknown && request.credential === unknown) throw new HubError(401, "unauthorized");
      return { ceremony: "c1", options: { publicKey: { challenge: "AAAA", rpId: "localhost", userVerification: "required" } }, prf_salts: request.credential ? { [request.credential]: "AQID" } : {}, prf_salt: HUB_SALT };
    },
    loginFinish: async (ceremony, credential) => ({ ceremony, user: credential.response.userHandle }),
  };
  const credentials = {
    get: (options) => {
      asked.push(options);
      if (options.mediation === "conditional") return new Promise((_, reject) => options.signal.addEventListener("abort", () => reject(Object.assign(new Error("aborted"), { name: "AbortError" }))));
      if (refuse) return Promise.reject(Object.assign(new Error("no"), { name: refuse }));
      return Promise.resolve(assertion);
    },
  };
  return { begun, asked, signingIn: signInWith({ account, credentials }) };
}

test("a passkey signs in naming no account, and the authenticator is asked without a list or mediation, for the PRF of the hub's salt", async () => {
  const { begun, asked, signingIn } = fakes();
  const { signedIn, prf } = await signingIn.withPasskey();
  assert.deepEqual(begun, [{}]);
  assert.deepEqual(asked[0].publicKey.allowCredentials, []);
  assert.equal(asked[0].mediation, undefined);
  assert.deepEqual(asked[0].publicKey.extensions, { prf: { eval: { first: fromB64(HUB_SALT) } } });
  assert.equal(signedIn.user, "AQID", "the user handle goes to the hub");
  assert.equal(prf, null);
});

test("a remembered passkey is asked for by its id with the hub's salt, and one the hub forgot falls back to any passkey", async () => {
  const { begun, asked, signingIn } = fakes();
  await signingIn.withPasskey("Cg");
  assert.deepEqual(begun, [{ credential: "Cg" }]);
  assert.deepEqual(asked[0].publicKey.extensions, { prf: { eval: { first: fromB64(HUB_SALT) } } });
  const forgotten = fakes({ unknown: "Cg" });
  await forgotten.signingIn.withPasskey("Cg");
  assert.deepEqual(forgotten.begun, [{ credential: "Cg" }, {}]);
});

test("a passkey asked for a connect names the code to the hub, also when it falls back to any passkey", async () => {
  const { begun, signingIn } = fakes({ unknown: "Cg" });
  await signingIn.withPasskey("Cg", { connect: "ABCD-EFGH" });
  assert.deepEqual(begun, [{ credential: "Cg", connect: "ABCD-EFGH" }, { connect: "ABCD-EFGH" }]);
});

test("a device that offers no passkey says so without asking for an account name", async () => {
  await assert.rejects(fakes({ refuse: "NotAllowedError" }).signingIn.withPasskey(), NoPasskeyOffered);
  const said = signInText(new NoPasskeyOffered());
  assert.equal(said, "This device offered no passkey for this hub, or it was cancelled.");
  assert.doesNotMatch(said, /account name/);
});

test("autofill waits as a conditional request until a sign-in by button takes its place", async () => {
  const { asked, signingIn } = fakes();
  const waiting = signingIn.autofill();
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.equal(asked[0].mediation, "conditional");
  await signingIn.withPasskey();
  await assert.rejects(waiting, { name: "AbortError" });
  assert.equal(signInText(Object.assign(new Error(), { name: "AbortError" })), null, "an aborted autofill says nothing");
  assert.equal(await autofillAvailable({ isConditionalMediationAvailable: async () => true }), true);
  assert.equal(await autofillAvailable(undefined), false);
  assert.equal(await autofillAvailable({ isConditionalMediationAvailable: async () => { throw new Error(); } }), false);
});

test("sign-in and sign-up errors name no account, invite field or account name", () => {
  const unauthorized = new HubError(401, "unauthorized");
  assert.equal(signInText(unauthorized), "This passkey isn't one this hub knows.");
  assert.equal(signInText(new HubError(401, "unknown-passkey")), "That passkey belongs to an account this hub does not have.");
  assert.equal(signUpText(unauthorized), "That link is used or expired. Ask for a new one.");
  assert.equal(signUpText(new HubError(403, "signup-closed")), "This hub makes new accounts by invite only.");
  for (const said of [signInText(unauthorized), signUpText(unauthorized), signUpText(new HubError(409, "passkey-taken"))]) assert.doesNotMatch(said, /account name|handle/i);
});

test("a sign-in tells a browser with signalCurrentUserDetails the passkey's readable name, and one without it is left alone", async () => {
  const signalled = [];
  const platform = { signalCurrentUserDetails: (details) => (signalled.push(details), new Promise(() => {})) };
  const account = {
    loginBegin: async () => ({ ceremony: "c1", options: { publicKey: { challenge: "AAAA", rpId: "hub.example.com" } }, prf_salt: HUB_SALT }),
    loginFinish: async () => ({ account: "acc_1", handle: "kaca606abca3f", github: "octocat" }),
  };
  const credentials = { get: async () => assertion };
  const { signedIn } = await signInWith({ account, credentials, platform, store: memoryStore() }).withPasskey();
  assert.equal(signedIn.github, "octocat");
  assert.deepEqual(signalled, [{ rpId: "hub.example.com", userId: "AQID", name: "octocat", displayName: "octocat" }]);
  await assert.doesNotReject(signInWith({ account, credentials, platform: {}, store: memoryStore() }).withPasskey());
});

test("a sign-in names the passkey after the account's email when the hub keeps one, and only says so when the name changed", async () => {
  const signalled = [];
  const platform = { signalCurrentUserDetails: (details) => (signalled.push(details.name), new Promise(() => {})) };
  let kept = { email: "alice@example.com", verified: true, sent: false };
  const account = {
    loginBegin: async () => ({ ceremony: "c1", options: { publicKey: { challenge: "AAAA", rpId: "hub.example.com" } }, prf_salt: HUB_SALT }),
    loginFinish: async () => ({ account: "acc_1", handle: "kaca606abca3f", github: "octocat" }),
    email: async () => kept,
  };
  const credentials = { get: async () => assertion };
  const store = memoryStore();
  await signInWith({ account, credentials, platform, store }).withPasskey();
  await signInWith({ account, credentials, platform, store }).withPasskey();
  assert.deepEqual(signalled, ["alice@example.com"], "the second sign-in with the same name says nothing");
  kept = { email: "alice@work.example.com", verified: false, sent: true };
  await signInWith({ account, credentials, platform, store }).withPasskey();
  assert.deepEqual(signalled, ["alice@example.com", "alice@work.example.com"]);
  const noEmail = { ...account, email: async () => { throw new Error("404"); } };
  await signInWith({ account: noEmail, credentials, platform, store: memoryStore() }).withPasskey();
  assert.equal(signalled.at(-1), "octocat", "a hub without email falls back to the GitHub login");
});

function memoryStore() {
  const map = new Map();
  return { getItem: (key) => map.get(key) ?? null, setItem: (key, value) => map.set(key, String(value)) };
}
