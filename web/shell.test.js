import assert from "node:assert/strict";
import { test } from "node:test";

import { fromNetwork, watchShell } from "./shell.js";

const settle = () => new Promise((resolve) => setImmediate(resolve));

function shell() {
  const workers = new EventTarget();
  workers.controller = { scriptURL: "/sw.js" };
  const registration = { updates: 0, update: async () => registration.updates++ };
  workers.register = async () => registration;
  const page = new EventTarget();
  page.visibilityState = "visible";
  const registered = watchShell(workers, { page });
  const show = (state) => {
    page.visibilityState = state;
    page.dispatchEvent(new Event("visibilitychange"));
  };
  return { registered, registration, show, takeOver: () => workers.dispatchEvent(new Event("controllerchange")) };
}

test("a new worker taking over never reloads the page, at start or after it comes back", async (t) => {
  const reloads = [];
  const location = globalThis.location;
  globalThis.location = { reload: () => reloads.push("reload") };
  t.after(() => (globalThis.location = location));
  const app = shell();
  await app.registered;
  app.takeOver();
  app.show("hidden");
  app.show("visible");
  await settle();
  app.takeOver();
  app.show("hidden");
  app.show("visible");
  assert.deepEqual(reloads, []);
});

test("the app looks for a new worker each time it comes back into view", async () => {
  const app = shell();
  await app.registered;
  app.show("hidden");
  await settle();
  assert.equal(app.registration.updates, 0);
  app.show("visible");
  await settle();
  assert.equal(app.registration.updates, 1);
});

test("the worker keeps a copy of a file the page has already read", async () => {
  const kept = [];
  const cache = { open: async () => settle().then(() => ({ put: async (request, response) => kept.push([request, await response.text()]) })) };
  const network = async () => new Response("app code", { status: 200 });
  const response = await fromNetwork("/app.js", { shell: "kebbie-shell-test", network, cache });
  assert.equal(await response.text(), "app code");
  await settle();
  assert.deepEqual(kept, [["/app.js", "app code"]]);
});

test("the worker keeps no copy of a failed response", async () => {
  const kept = [];
  const cache = { open: async () => ({ put: async (request) => kept.push(request) }) };
  const response = await fromNetwork("/gone.js", { shell: "kebbie-shell-test", network: async () => new Response("", { status: 404 }), cache });
  assert.equal(response.status, 404);
  await settle();
  assert.deepEqual(kept, []);
});
