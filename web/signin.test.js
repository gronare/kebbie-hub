import assert from "node:assert/strict";
import { test } from "node:test";

import { HubError } from "./hub.js";
import { autofillAvailable, NoPasskeyOffered, signInText, signInWith, signUpText } from "./signin.js";

const bytes = () => new Uint8Array([1, 2, 3]).buffer;
const assertion = { id: "AQID", rawId: bytes(), type: "public-key", response: { authenticatorData: bytes(), clientDataJSON: bytes(), signature: bytes(), userHandle: bytes() } };

function fakes({ refuse = null, unknown = null } = {}) {
  const begun = [];
  const asked = [];
  const account = {
    loginBegin: async (request) => {
      begun.push(request);
      if (unknown && request.credential === unknown) throw new HubError(401, "unauthorized");
      return { ceremony: "c1", options: { publicKey: { challenge: "AAAA", rpId: "localhost", userVerification: "required" } }, prf_salts: request.credential ? { [request.credential]: "AQID" } : {} };
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

test("a passkey signs in naming no account, and the authenticator is asked without a list or mediation", async () => {
  const { begun, asked, signingIn } = fakes();
  const { signedIn, prf } = await signingIn.withPasskey();
  assert.deepEqual(begun, [{}]);
  assert.deepEqual(asked[0].publicKey.allowCredentials, []);
  assert.equal(asked[0].mediation, undefined);
  assert.equal(asked[0].publicKey.extensions, undefined);
  assert.equal(signedIn.user, "AQID", "the user handle goes to the hub");
  assert.equal(prf, null);
});

test("a remembered passkey is asked for by its id with its PRF salt, and one the hub forgot falls back to any passkey", async () => {
  const { begun, asked, signingIn } = fakes();
  await signingIn.withPasskey("Cg");
  assert.deepEqual(begun, [{ credential: "Cg" }]);
  assert.deepEqual([...asked[0].publicKey.extensions.prf.evalByCredential.Cg.first], [1, 2, 3]);
  const forgotten = fakes({ unknown: "Cg" });
  await forgotten.signingIn.withPasskey("Cg");
  assert.deepEqual(forgotten.begun, [{ credential: "Cg" }, {}]);
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
