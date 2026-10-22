import { toB64 } from "../js/protocol.js";

const DB = "kelpie-hub";
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
export const remove = (key) => run("readwrite", (store) => store.delete(key));

export async function newDeviceKeys() {
  const signing = await crypto.subtle.generateKey({ name: "Ed25519" }, false, ["sign", "verify"]);
  const encryption = await crypto.subtle.generateKey({ name: "X25519" }, false, ["deriveBits"]);
  const raw = async (key) => toB64(new Uint8Array(await crypto.subtle.exportKey("raw", key)));
  return {
    signing,
    encryption,
    public: { signing: await raw(signing.publicKey), encryption: await raw(encryption.publicKey) },
  };
}

export const identity = (device) => ({ id: device.id, signing: device.keys.signing.privateKey, encryption: device.keys.encryption });

export const loadDevice = () => get("device");
export const saveDevice = (device) => put("device", device);
export const loadMachines = async () => (await get("machines")) ?? [];
export const saveMachines = (machines) => put("machines", machines);
export const loadAttempts = async () => (await get("attempts")) ?? [];

export async function recordAttempt(attempt) {
  const attempts = [attempt, ...(await loadAttempts())].slice(0, 20);
  await put("attempts", attempts);
  return attempts;
}
