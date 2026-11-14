const STORE = "kv";
const BLOBS = "blobs";
const opening = new Map();

function database(name, store) {
  if (!opening.has(name)) {
    opening.set(
      name,
      new Promise((resolve, reject) => {
        const request = indexedDB.open(name, 1);
        request.onupgradeneeded = () => request.result.createObjectStore(store);
        request.onsuccess = () => resolve(request.result);
        request.onerror = () => reject(request.error);
      }),
    );
  }
  return opening.get(name);
}

async function run(mode, act, name = "kebbie-hub", store = STORE) {
  const db = await database(name, store);
  return new Promise((resolve, reject) => {
    const transaction = db.transaction(store, mode);
    const request = act(transaction.objectStore(store));
    transaction.oncomplete = () => resolve(request?.result);
    transaction.onerror = () => reject(transaction.error);
    transaction.onabort = () => reject(transaction.error);
  });
}

export const get = (key) => run("readonly", (store) => store.get(key));
export const put = (key, value) => run("readwrite", (store) => store.put(value, key));
export const del = (key) => run("readwrite", (store) => store.delete(key));

const blobRun = (mode, act) => run(mode, act, "kebbie-hub-blobs", BLOBS);

export const keptBlobs = {
  get: (id) => blobRun("readonly", (store) => store.get(id)).then((kept) => kept?.bytes ?? null),
  put: (id, bytes, at) => blobRun("readwrite", (store) => store.put({ bytes, at }, id)),
  prune: (before) =>
    blobRun("readwrite", (store) => {
      const walk = store.openCursor();
      walk.onsuccess = () => {
        const cursor = walk.result;
        if (!cursor) return;
        if (!(cursor.value?.at >= before)) cursor.delete();
        cursor.continue();
      };
      return null;
    }),
  clear: () => blobRun("readwrite", (store) => store.clear()),
};

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
