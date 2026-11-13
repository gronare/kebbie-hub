import assert from "node:assert/strict";
import { test } from "node:test";

import { accountIdentity, fromB64, newAccountSecrets } from "../js/protocol.js";
import { notifyEnvironment, startPage } from "./pages.js";

const kept = new Map();
globalThis.indexedDB = {
  open() {
    const request = {};
    request.result = {
      transaction() {
        const transaction = {};
        const store = { get: (key) => ({ result: kept.get(key) }), put: (value, key) => (kept.set(key, value), { result: key }), delete: (key) => (kept.delete(key), { result: undefined }) };
        transaction.objectStore = () => store;
        queueMicrotask(() => transaction.oncomplete?.());
        return transaction;
      },
    };
    queueMicrotask(() => request.onsuccess?.());
    return request;
  },
};

const VAPID = "BEl62iUYgUivxIkv69yViEuiBIa-Ib9-SkvMeAtA3LFgDzkrxZJjSgSnfckjBJuBkr3qBUYIHBQFLXYp5Nksh8U";
const IPHONE = "Mozilla/5.0 (iPhone; CPU iPhone OS 18_2 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.2 Mobile/15E148 Safari/604.1";

function browserWith({ standalone = true, permission = "default", answer = "granted", extra = () => null } = {}) {
  const calls = [];
  const push = { asked: 0, subscribedWith: null, current: null, unsubscribed: 0 };
  const subscription = {
    endpoint: "https://web.push.apple.com/QGuQyavXutnMH",
    options: { applicationServerKey: fromB64(VAPID).buffer },
    toJSON: () => ({ endpoint: subscription.endpoint, expirationTime: null, keys: { p256dh: "BNcRdreALRFXTkOOUHK1EtK2wtaz5Ry4YfYCA_0QTpQtUbVlUls0VJXg7A8u-Ts1XbjhazAkj7I99e8QcYP7DkM", auth: "tBHItJI5svbpez7KI4CCXg" } }),
    unsubscribe: async () => {
      push.unsubscribed += 1;
      push.current = null;
      return true;
    },
  };
  const registration = {
    pushManager: {
      getSubscription: async () => push.current,
      subscribe: async (options) => {
        push.subscribedWith = options;
        push.current = subscription;
        return subscription;
      },
    },
  };
  const scope = {
    Notification: {
      permission,
      requestPermission() {
        push.asked += 1;
        this.permission = answer;
        return Promise.resolve(answer);
      },
    },
    navigator: { userAgent: IPHONE, standalone, serviceWorker: { getRegistration: async () => registration, register: async () => registration, ready: Promise.resolve(registration) } },
  };
  Object.defineProperty(globalThis, "navigator", { value: scope.navigator, configurable: true, writable: true });
  globalThis.Notification = scope.Notification;
  globalThis.PushManager = function PushManager() {};
  globalThis.location = new URL("http://hub.test/account");
  globalThis.history = { replaceState() {}, pushState() {} };
  globalThis.document = { activeElement: null, title: "", getElementById: () => null, addEventListener() {}, visibilityState: "visible" };
  globalThis.scrollTo = () => {};
  globalThis.fetch = async (url, options = {}) => {
    const method = options.method ?? "GET";
    calls.push({ method, url: String(url), body: options.body ? JSON.parse(options.body) : null });
    const special = await extra(String(url), method);
    if (special) return special;
    const answers = { "/v1/hub": { commands: true, vapid: VAPID }, "/v1/a/devices": method === "GET" ? { devices: [] } : null, "/v1/a/boards": { boards: [], list: null } };
    const body = Object.hasOwn(answers, String(url)) ? answers[String(url)] : null;
    return body === null ? new Response(null, { status: 204 }) : new Response(JSON.stringify(body), { status: 200, headers: { "Content-Type": "application/json" } });
  };
  const listeners = [];
  const root = {
    innerHTML: "",
    contains: () => false,
    querySelector: () => null,
    addEventListener(type, listener, options) {
      listeners.push({ type, listener, signal: options?.signal });
    },
    change(target) {
      for (const { type, listener, signal } of listeners) if (type === "change" && !signal?.aborted) listener({ target });
    },
    click(dataset) {
      const target = { dataset };
      for (const { type, listener, signal } of listeners) if (type === "click" && !signal?.aborted) listener({ target: { closest: (selector) => (selector === "[data-action]" ? target : null) }, preventDefault() {} });
    },
  };
  return { root, calls, push };
}

let accounts = 0;

async function unlocked() {
  accounts += 1;
  const fresh = await newAccountSecrets(`acc_0123456789abcdefghij${String(accounts).padStart(2, "0")}`);
  const identity = await accountIdentity(fresh.key, fresh.secrets, fresh.key.account);
  const me = { account: fresh.key.account, key_access: true };
  return { info: { commands: true, vapid: VAPID }, me, session: { state: "unlocked", me, identity, until: Date.now() + 60_000 }, address: null };
}

const until = async (check) => {
  for (let tries = 0; tries < 200 && !check(); tries++) await new Promise((resolve) => setTimeout(resolve, 5));
  assert.ok(check(), "the page never got there");
};

const switchText = (html) => /id="notify-state">([^<]*)</.exec(html)?.[1];

test("the switch asks for permission from the tap itself, subscribes with the hub's key and hands the hub the default categories", async () => {
  const { root, calls, push } = browserWith();
  const page = startPage(root, { given: await unlocked() });
  await page.started;
  assert.equal(switchText(root.innerHTML), "Off");
  root.change({ name: "notify", checked: true });
  assert.equal(push.asked, 1, "the permission is asked before anything is awaited");
  assert.equal(switchText(root.innerHTML), "Asking iOS for permission…");
  await until(() => switchText(root.innerHTML)?.startsWith("On for this iPhone."));
  assert.equal(push.subscribedWith.userVisibleOnly, true);
  assert.deepEqual(new Uint8Array(push.subscribedWith.applicationServerKey), fromB64(VAPID));
  const added = calls.find((call) => call.method === "POST" && call.url === "/v1/a/devices");
  const device = added.body.cert.device;
  assert.ok(added.body.cert.encryption, "the device registers its encryption key");
  const subscribed = calls.find((call) => call.method === "PUT" && call.url === `/v1/a/devices/${device}/push`);
  assert.deepEqual(subscribed.body, { endpoint: "https://web.push.apple.com/QGuQyavXutnMH", keys: { p256dh: "BNcRdreALRFXTkOOUHK1EtK2wtaz5Ry4YfYCA_0QTpQtUbVlUls0VJXg7A8u-Ts1XbjhazAkj7I99e8QcYP7DkM", auth: "tBHItJI5svbpez7KI4CCXg" }, categories: ["questions", "new-device", "idea-questions", "ready", "ci-failed"] });
  assert.match(root.innerHTML, /Notify me when it needs you/);

  root.change({ name: "notice-category", value: "started", checked: true });
  await until(() => calls.some((call) => call.url === `/v1/a/devices/${device}/categories`));
  assert.deepEqual(calls.find((call) => call.url === `/v1/a/devices/${device}/categories`).body, { categories: ["questions", "new-device", "idea-questions", "ready", "ci-failed", "started"] });

  root.change({ name: "notify", checked: false });
  await until(() => calls.some((call) => call.method === "DELETE"));
  await until(() => switchText(root.innerHTML) === "Off");
  assert.equal(push.unsubscribed, 1);
  assert.equal(calls.find((call) => call.method === "DELETE").url, `/v1/a/devices/${device}/push`);
  assert.doesNotMatch(root.innerHTML, /Notify me when/);
  assert.equal(kept.has("notify"), false, "off forgets which device the worker opens notices with");
  page.leave();
});

test("Account moves the browser's subscription to this device when the worker's record names an earlier one, and says why the last notice didn't open", async () => {
  const { deviceId } = await import("../js/protocol.js");
  const given = await unlocked();
  const earlier = deviceId();
  const { root, calls } = browserWith({ permission: "granted" });
  await (await navigator.serviceWorker.getRegistration("/")).pushManager.subscribe({});
  kept.set("notify", { account: given.session.identity.key, device: earlier });
  kept.set(`notify-categories:${earlier}`, ["questions", "ready"]);
  const at = Math.floor(Date.now() / 1000) - 60;
  kept.set("notice-failure", { at, reason: "device-changed", detail: earlier, machine: "mch_01", recipient: earlier, device: "dev_other", name: "MacBook" });
  const page = startPage(root, { given });
  await page.started;
  const added = calls.find((call) => call.method === "POST" && call.url === "/v1/a/devices");
  const device = added.body.cert.device;
  const moved = calls.find((call) => call.method === "PUT" && call.url === `/v1/a/devices/${device}/push`);
  assert.deepEqual(moved.body.categories, ["questions", "ready"]);
  assert.equal(moved.body.endpoint, "https://web.push.apple.com/QGuQyavXutnMH");
  assert.equal(kept.get("notify").device, device);
  assert.equal(kept.get(`device-key:${given.me.account}`).id, device);
  assert.match(root.innerHTML, /<div class="meta warn" id="notify-failure">The last notification, at \d\d:\d\d from MacBook, couldn&#39;t be opened: it was sealed to an earlier key of this device\.<\/div>/);
  page.leave();
  kept.delete("notice-failure");
});

test("a refusal in iOS greys the switch and says where to turn them on", async () => {
  const { root, calls } = browserWith({ answer: "denied" });
  const page = startPage(root, { given: await unlocked() });
  await page.started;
  root.change({ name: "notify", checked: true });
  await until(() => switchText(root.innerHTML)?.startsWith("Turned off"));
  assert.equal(switchText(root.innerHTML), "Turned off for kebbie in iOS. Turn them on in Settings › Notifications › kebbie.");
  assert.match(root.innerHTML, /name="notify"[^>]*disabled/);
  assert.ok(!calls.some((call) => call.url.endsWith("/push")), "nothing is subscribed");
  page.leave();
});

test("in Safari outside the Home Screen the row asks you to add kebbie there, and an earlier refusal shows at once", async () => {
  const tab = browserWith({ standalone: false });
  const page = startPage(tab.root, { given: await unlocked() });
  await page.started;
  assert.equal(switchText(tab.root.innerHTML), "Add kebbie to your Home Screen to get notifications. Share, then Add to Home Screen.");
  assert.match(tab.root.innerHTML, /name="notify"[^>]*disabled/);
  page.leave();
  const refused = browserWith({ permission: "denied" });
  const again = startPage(refused.root, { given: await unlocked() });
  await again.started;
  assert.match(switchText(refused.root.innerHTML), /^Turned off for kebbie in iOS/);
  again.leave();
});

test("the environment reads iOS, the Home Screen and push support from the browser", () => {
  const scope = (agent, extra = {}) => ({ navigator: { userAgent: agent, serviceWorker: {}, ...extra.navigator }, PushManager() {}, Notification: { permission: "default" }, ...extra.scope });
  assert.deepEqual(notifyEnvironment(scope(IPHONE, { navigator: { standalone: true } })), { ios: true, standalone: true, supported: true, permission: "default", word: "iPhone" });
  assert.equal(notifyEnvironment(scope(IPHONE)).standalone, false);
  assert.equal(notifyEnvironment({ navigator: { userAgent: IPHONE } }).supported, false);
  assert.equal(notifyEnvironment(scope("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)", { navigator: { platform: "MacIntel", maxTouchPoints: 5 } })).word, "iPad");
});

test("Approve on a waiting device's page sends a decision signed by this device to the machine that approved it", async () => {
  const { readFileSync } = await import("node:fs");
  const { parties } = JSON.parse(readFileSync(new URL("../vectors/keys.json", import.meta.url), "utf8"));
  const { deviceFingerprint, deviceId, emptyMachineList, importIdentity, machineListWith, newDeviceKey, openCommand, sealEnvelope, signDeviceCert, signMachineList } = await import("../js/protocol.js");
  const given = await unlocked();
  const identity = given.session.identity;
  const machine = await importIdentity(parties.machine);
  const list = machineListWith(emptyMachineList(), machine.id, parties.machine.public);
  const now = Math.floor(Date.now() / 1000);
  const certified = async (name) => {
    const key = await newDeviceKey();
    const id = deviceId();
    const cert = await signDeviceCert(identity, { device: id, name, signing: key.public, encryption: key.encryptionPublic, created_at: now - 60 });
    return { id, cert, signing: key.signing, encryption: key.encryption, fingerprint: await deviceFingerprint(key.public) };
  };
  const self = await certified("iPhone 15 Pro");
  const waiting = await certified("iPhone 16");
  const board = { type: "board", rev: 4, kebbie: "0.32.0", checkouts: [], routines: [], items: [], sessions_outside_tasks: 0, accepts: ["answer", "device-decision"], devices: { [self.id]: { state: "approved", fingerprint: self.fingerprint, at: now - 30 }, [waiting.id]: { state: "waiting", fingerprint: waiting.fingerprint, until: now + 3600 } } };
  const header = { v: 1, id: "brd_01K85Y2D8G0000000000000000", kind: "board", from: machine.id, machine: machine.id, at: now, exp: now + 3600, schema: 1 };
  const envelope = await sealEnvelope(header, new TextEncoder().encode(JSON.stringify(board)), machine, [{ id: identity.id, encryption: identity.key.keys.encryption }]);
  const listing = { boards: [{ machine: machine.id, name: "MacBook", machine_keys: parties.machine.public, rev: 4, envelope, last_seen: now, stored_at: now }], list: { list, signature: await signMachineList(identity, list) } };
  const row = (device) => ({ device: device.id, name: device.cert.name, kind: "web", fingerprint: device.fingerprint, cert: device.cert, created_at: now - 60, last_seen_at: now });
  const extra = (path, method) => {
    if (path === "/v1/a/boards") return Response.json(listing);
    if (path === "/v1/a/devices" && method === "GET") return Response.json({ devices: [row(self), row(waiting)] });
    if (path.endsWith("/commands") && method === "GET") return Response.json({ commands: [], cursor: null });
    if (path.startsWith("/v1/a/results")) return Response.json({ error: "unavailable" }, { status: 503 });
    return null;
  };
  const { root, calls } = browserWith({ extra });
  kept.set(`allow-changes:${given.me.account}`, true);
  const page = startPage(root, { given: { ...given, device: self } });
  await page.started;
  assert.match(root.innerHTML, /Devices · 2/);
  root.click({ action: "device", id: waiting.id });
  await until(() => root.innerHTML.includes('data-action="decide-device"'));
  assert.match(root.innerHTML, /data-approve="false">Deny<\/button><button[^>]*data-approve="true">Approve</);
  root.click({ action: "decide-device", machine: machine.id, approve: "true" });
  await until(() => root.innerHTML.includes("Approval sent to MacBook"));
  const sent = calls.find((call) => call.method === "POST" && call.url === "/v1/a/commands").body.envelope;
  const opened = await openCommand(sent, machine, identity.key, now);
  assert.equal(opened.device.device, self.id);
  assert.deepEqual(opened.payload, { type: "device-decision", device: waiting.id, fingerprint: waiting.fingerprint, approve: true });
  assert.doesNotMatch(root.innerHTML, /data-action="decide-device"/, "no second decision while the first is on its way");
  page.leave();
});
