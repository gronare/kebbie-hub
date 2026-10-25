const decode = (value) => Uint8Array.fromBase64(value, { alphabet: "base64url", lastChunkHandling: "loose" });
const encode = (buffer) => new Uint8Array(buffer).toBase64({ alphabet: "base64url", omitPadding: true });
const credentialList = (list) => list?.map((credential) => ({ ...credential, id: decode(credential.id) }));

export function creationOptions(json, prfSalt = null) {
  const options = json.publicKey;
  const extensions = prfSalt ? { ...options.extensions, prf: { eval: { first: decode(prfSalt) } } } : options.extensions;
  return {
    publicKey: {
      ...options,
      challenge: decode(options.challenge),
      user: { ...options.user, id: decode(options.user.id) },
      excludeCredentials: credentialList(options.excludeCredentials) ?? [],
      ...(extensions ? { extensions } : {}),
    },
  };
}

export function requestOptions(json, prfSalts = {}) {
  const options = json.publicKey;
  const salts = Object.entries(prfSalts ?? {});
  const evalByCredential = Object.fromEntries(salts.map(([credential, salt]) => [credential, { first: decode(salt) }]));
  const extensions = salts.length ? { ...options.extensions, prf: { evalByCredential } } : options.extensions;
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

export const supported = () => typeof globalThis.PublicKeyCredential === "function";
