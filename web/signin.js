import { HubError } from "./hub.js";
import { assertionJSON, creationOptions, prfOutput, registrationJSON, requestOptions } from "./passkey.js";

export class NoPasskeyOffered extends Error {
  constructor() {
    super("no-passkey-offered");
  }
}

// A conditional request left pending blocks the next get(), so a new sign-in aborts the one before it.
export function signInWith({ account, credentials }) {
  let pending = null;
  const run = async (request, mediation) => {
    pending?.abort();
    const controller = new AbortController();
    pending = controller;
    const begun = await account.loginBegin(request);
    const options = { ...requestOptions(begun.options, begun.prf_salts), signal: controller.signal, ...(mediation ? { mediation } : {}) };
    let credential;
    try {
      credential = await credentials.get(options);
    } catch (error) {
      if (error?.name === "NotAllowedError" && !request.handle && !request.credential && !mediation) throw new NoPasskeyOffered();
      throw error;
    }
    if (pending === controller) pending = null;
    return { signedIn: await account.loginFinish(begun.ceremony, assertionJSON(credential)), prf: prfOutput(credential) };
  };
  return {
    withPasskey: (remembered = null) => (remembered ? run({ credential: remembered }, null).catch((error) => (error instanceof HubError ? run({}, null) : Promise.reject(error))) : run({}, null)),
    byName: (name) => run({ handle: name?.trim().toLowerCase() }, null),
    autofill: () => run({}, "conditional"),
    stop: () => pending?.abort(),
  };
}

export async function signUpWith({ account, credentials }, ticket, handle, name) {
  const begun = await account.registerBegin({ ...ticket, handle: handle.trim().toLowerCase() });
  const created = await credentials.create(creationOptions(begun.options, begun.prf_salt));
  return { signedIn: await account.registerFinish(begun.ceremony, registrationJSON(created), name), prf: prfOutput(created) };
}

export async function autofillAvailable(credential = globalThis.PublicKeyCredential) {
  try {
    return (await credential?.isConditionalMediationAvailable?.()) === true;
  } catch {
    return false;
  }
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
  if (error?.code === "wrong-account-key" || error?.code === "key-mismatch") return "This passkey did not open your account's key. Nothing was read.";
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
