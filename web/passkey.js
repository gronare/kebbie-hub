const decode = (value) => Uint8Array.fromBase64(value, { alphabet: "base64url", lastChunkHandling: "loose" });
const encode = (buffer) => new Uint8Array(buffer).toBase64({ alphabet: "base64url", omitPadding: true });
const credentialList = (list) => list?.map((credential) => ({ ...credential, id: decode(credential.id) }));

export const passkeyName = (me, email = null) => email || me?.github || "kebbie hub";

export function creationOptions(json, prfSalt = null, name = null) {
  const options = json.publicKey;
  const extensions = prfSalt ? { ...options.extensions, prf: { eval: { first: decode(prfSalt) } } } : options.extensions;
  return {
    publicKey: {
      ...options,
      challenge: decode(options.challenge),
      user: { ...options.user, id: decode(options.user.id), ...(name ? { name, displayName: name } : {}) },
      excludeCredentials: credentialList(options.excludeCredentials) ?? [],
      ...(extensions ? { extensions } : {}),
    },
  };
}

export function requestOptions(json, { first = null, byCredential = {} } = {}) {
  const options = json.publicKey;
  const salts = Object.entries(byCredential ?? {});
  const evalByCredential = () => Object.fromEntries(salts.map(([credential, salt]) => [credential, { first: decode(salt) }]));
  const prf = first ? { eval: { first: decode(first) } } : salts.length ? { evalByCredential: evalByCredential() } : null;
  const extensions = prf ? { ...options.extensions, prf } : options.extensions;
  return {
    publicKey: {
      ...options,
      challenge: decode(options.challenge),
      allowCredentials: credentialList(options.allowCredentials) ?? [],
      ...(extensions ? { extensions } : {}),
    },
  };
}

export function prfOutput(credential) {
  try {
    const first = credential?.getClientExtensionResults?.()?.prf?.results?.first;
    if (!first) return null;
    const bytes = new Uint8Array(ArrayBuffer.isView(first) ? first.buffer.slice(first.byteOffset, first.byteOffset + first.byteLength) : first.slice(0));
    return bytes.length === 32 ? bytes : null;
  } catch {
    return null;
  }
}

export function registrationJSON(credential) {
  const transports = credential.response.getTransports?.() ?? null;
  return {
    id: credential.id,
    rawId: encode(credential.rawId),
    type: credential.type,
    response: { attestationObject: encode(credential.response.attestationObject), clientDataJSON: encode(credential.response.clientDataJSON), transports },
    extensions: {},
  };
}

export function assertionJSON(credential) {
  const { response } = credential;
  return {
    id: credential.id,
    rawId: encode(credential.rawId),
    type: credential.type,
    response: {
      authenticatorData: encode(response.authenticatorData),
      clientDataJSON: encode(response.clientDataJSON),
      signature: encode(response.signature),
      userHandle: response.userHandle ? encode(response.userHandle) : null,
    },
    extensions: {},
  };
}

// Safari's promise may never settle, so it is not awaited.
// iOS shows "Passkey Updated" on every signal, so a name is only sent when it differs from the last one sent.
export function signalName(credential, me, rpId = globalThis.location?.hostname, platform = globalThis.PublicKeyCredential, email = null, store = globalThis.localStorage) {
  const userHandle = credential?.response?.userHandle;
  if (!userHandle || !rpId || typeof platform?.signalCurrentUserDetails !== "function") return;
  const userId = encode(userHandle);
  const name = passkeyName(me, email);
  const key = `kebbie-passkey-name:${rpId}:${userId}`;
  try {
    if (store?.getItem(key) === name) return;
  } catch {}
  try {
    Promise.resolve(platform.signalCurrentUserDetails({ rpId, userId, name, displayName: name })).catch(() => {});
    store?.setItem(key, name);
  } catch {}
}

export const supported = () => typeof globalThis.PublicKeyCredential === "function";
