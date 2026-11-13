const DB = "kebbie-hub";
const STORE = "kv";
let opening = null;

function database() {
  opening ??= new Promise((resolve, reject) => {
    const request = indexedDB.open(DB, 1);
    request.onupgradeneeded = () => request.result.createObjectStore(STORE);
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
  return opening;
}

async function run(mode, act) {
  const db = await database();
  return new Promise((resolve, reject) => {
    const transaction = db.transaction(STORE, mode);
    const request = act(transaction.objectStore(STORE));
    transaction.oncomplete = () => resolve(request?.result);
    transaction.onerror = () => reject(transaction.error);
    transaction.onabort = () => reject(transaction.error);
  });
}

export const get = (key) => run("readonly", (store) => store.get(key));
export const put = (key, value) => run("readwrite", (store) => store.put(value, key));
export const del = (key) => run("readwrite", (store) => store.delete(key));

export const forgetDeviceKeys = () => run("readwrite", (store) => (store.delete("device"), store.delete("machines"), store.delete("attempts"))).catch(() => undefined);

// WebKit reads an X25519 CryptoKey back from IndexedDB as null (bugs.webkit.org/328001), so such a key is kept as pkcs8 sealed under its own non-extractable AES key.
export async function sealKey(pkcs8) {
  const key = await crypto.subtle.generateKey({ name: "AES-GCM", length: 256 }, false, ["encrypt", "decrypt"]);
  const iv = crypto.getRandomValues(new Uint8Array(12));
  return { key, iv, data: new Uint8Array(await crypto.subtle.encrypt({ name: "AES-GCM", iv }, key, pkcs8)) };
}

export async function openKey(sealed) {
  const pkcs8 = new Uint8Array(await crypto.subtle.decrypt({ name: "AES-GCM", iv: sealed.iv }, sealed.key, sealed.data));
  try {
    return await crypto.subtle.importKey("pkcs8", pkcs8, { name: "X25519" }, false, ["deriveBits"]);
  } finally {
    pkcs8.fill(0);
  }
}

export async function openEncryption(kept, publicKey) {
  if (kept?.privateKey) return kept;
  return { privateKey: await openKey(kept), publicKey: await crypto.subtle.importKey("raw", Uint8Array.fromBase64(publicKey, { alphabet: "base64url" }), { name: "X25519" }, true, []) };
}
