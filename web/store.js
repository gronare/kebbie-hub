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

export const forgetDeviceKeys = () => run("readwrite", (store) => (store.delete("device"), store.delete("machines"), store.delete("attempts"))).catch(() => undefined);
