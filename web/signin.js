import { HubError } from "./hub.js";
import { assertionJSON, creationOptions, registrationJSON, requestOptions } from "./passkey.js";

export class NoPasskeyOffered extends Error {
  constructor() {
    super("no-passkey-offered");
  }
}

// A conditional request left pending blocks the next get(), so a new sign-in aborts the one before it.
export function signInWith({ account, credentials }) {
  let pending = null;
  const run = async (name, mediation) => {
    pending?.abort();
    const controller = new AbortController();
    pending = controller;
    const begun = await account.loginBegin(name?.trim().toLowerCase() || null);
    const options = { ...requestOptions(begun.options), signal: controller.signal, ...(mediation ? { mediation } : {}) };
    let credential;
    try {
      credential = await credentials.get(options);
    } catch (error) {
      if (error?.name === "NotAllowedError" && !name && !mediation) throw new NoPasskeyOffered();
      throw error;
    }
    if (pending === controller) pending = null;
    return account.loginFinish(begun.ceremony, assertionJSON(credential));
  };
  return {
    withPasskey: () => run(null, null),
    byName: (name) => run(name, null),
    autofill: () => run(null, "conditional"),
    stop: () => pending?.abort(),
  };
}

export async function autofillAvailable(credential = globalThis.PublicKeyCredential) {
  try {
    return (await credential?.isConditionalMediationAvailable?.()) === true;
  } catch {
    return false;
  }
}

export async function addPasskey({ account, credentials, name }) {
  const begun = await account.passkeyBegin();
  const created = await credentials.create(creationOptions(begun.options));
  return account.passkeyFinish(begun.ceremony, registrationJSON(created), name);
}

const SIGN_IN = {
  unauthorized: "No passkey for this account on this device.",
  "unknown-passkey": "That passkey belongs to an account this hub does not have.",
  "rate-limited": "Too many tries. Wait a minute.",
};

const SIGN_UP = {
  unauthorized: "That invite is used, expired or mistyped. Ask for a new one.",
  "handle-taken": "That account name is taken.",
  "bad-handle": "An account name is lowercase letters, digits and dashes, and starts with a letter.",
  "passkey-taken": "This passkey already belongs to an account.",
  "rate-limited": "Too many tries. Wait a minute.",
};

export const OFFER_TEXT = "Add a passkey this device offers by itself, so next time you sign in without typing your account name.";

export function signInText(error) {
  if (error instanceof NoPasskeyOffered) return "This device offered no passkey for this hub. If you made yours before, sign in with your account name.";
  if (error instanceof HubError) return SIGN_IN[error.code] ?? `The hub answered ${error.status} (${error.code}).`;
  if (error?.name === "NotAllowedError") return "No passkey for this account on this device, or it was cancelled.";
  if (error?.name === "AbortError") return null;
  return error?.message ?? String(error);
}

export function signUpText(error) {
  if (error instanceof HubError) return SIGN_UP[error.code] ?? null;
  if (error?.name === "NotAllowedError") return "The passkey was cancelled.";
  if (error?.name === "InvalidStateError") return "This device already holds a passkey for this account.";
  return null;
}
