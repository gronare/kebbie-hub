import assert from "node:assert/strict";
import { test } from "node:test";

import { HubError } from "./hub.js";
import { autofillAvailable, NoPasskeyOffered, signInText, signInWith, signUpText } from "./signin.js";

const bytes = () => new Uint8Array([1, 2, 3]).buffer;
const assertion = { id: "AQID", rawId: bytes(), type: "public-key", response: { authenticatorData: bytes(), clientDataJSON: bytes(), signature: bytes(), userHandle: bytes() } };

function fakes({ refuse = null } = {}) {
  const begun = [];
  const asked = [];
  const account = {
    loginBegin: async (handle) => (begun.push(handle), { ceremony: "c1", options: { publicKey: { challenge: "AAAA", rpId: "localhost", userVerification: "required" } } }),
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
  const me = await signingIn.withPasskey();
  assert.deepEqual(begun, [null]);
  assert.deepEqual(asked[0].publicKey.allowCredentials, []);
  assert.equal(asked[0].mediation, undefined);
  assert.equal(me.user, "AQID", "the user handle goes to the hub");
});

test("a device that offers no passkey is told to use the account name, a refusal by name talks about that account", async () => {
  await assert.rejects(fakes({ refuse: "NotAllowedError" }).signingIn.withPasskey(), NoPasskeyOffered);
  const said = signInText(new NoPasskeyOffered());
  assert.match(said, /account name/);
  const byName = fakes({ refuse: "NotAllowedError" });
  const error = await byName.signingIn.byName("  Carl ").catch((error) => error);
  assert.deepEqual(byName.begun, ["carl"]);
  assert.equal(error.name, "NotAllowedError");
  assert.equal(signInText(error), "No passkey for this account on this device, or it was cancelled.");
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

test("sign-in errors talk about signing in and sign-up errors about the invite", () => {
  const unauthorized = new HubError(401, "unauthorized");
  assert.equal(signInText(unauthorized), "No passkey for this account on this device.");
  assert.equal(signInText(new HubError(401, "unknown-passkey")), "That passkey belongs to an account this hub does not have.");
  assert.doesNotMatch(signInText(unauthorized), /invite|handle/i);
  assert.match(signUpText(unauthorized), /invite/);
  assert.match(signUpText(new HubError(409, "handle-taken")), /account name/);
});
