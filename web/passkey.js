const decode = (value) => Uint8Array.fromBase64(value, { alphabet: "base64url", lastChunkHandling: "loose" });
const encode = (buffer) => new Uint8Array(buffer).toBase64({ alphabet: "base64url", omitPadding: true });
const credentialList = (list) => list?.map((credential) => ({ ...credential, id: decode(credential.id) }));

export function creationOptions(json) {
  const options = json.publicKey;
  return {
    publicKey: {
      ...options,
      challenge: decode(options.challenge),
      user: { ...options.user, id: decode(options.user.id) },
      excludeCredentials: credentialList(options.excludeCredentials) ?? [],
    },
  };
}

export function requestOptions(json) {
  const options = json.publicKey;
  return {
    publicKey: {
      ...options,
      challenge: decode(options.challenge),
      allowCredentials: credentialList(options.allowCredentials) ?? [],
    },
  };
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
