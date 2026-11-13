import assert from "node:assert/strict";
import { test } from "node:test";

import { fromNetwork, SHELL, watchShell, watchUpdate } from "./shell.js";

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

function updating({ controller = { scriptURL: "/sw.js", state: "activated" }, cached = [] } = {}) {
  const workers = new EventTarget();
  workers.controller = controller;
  const names = new Set(cached);
  const updates = [];
  watchUpdate(workers, () => updates.push("update"), { cache: { has: async (name) => names.has(name) } });
  const takeOver = async (name) => {
    names.clear();
    names.add(name);
    workers.controller = Object.assign(new EventTarget(), { state: "activated" });
    workers.dispatchEvent(new Event("controllerchange"));
    await settle();
  };
  const takeOverActivating = async (name) => {
    const worker = Object.assign(new EventTarget(), { state: "activating" });
    workers.controller = worker;
    names.add(name);
    workers.dispatchEvent(new Event("controllerchange"));
    await settle();
    const before = [...updates];
    names.clear();
    names.add(name);
    worker.state = "activated";
    worker.dispatchEvent(new Event("statechange"));
    await settle();
    return before;
  };
  return { updates, takeOver, takeOverActivating };
}

test("a worker of a newer shell taking over offers the update, and the page's own shell or a first install never does", async () => {
  const newer = updating({ cached: [SHELL] });
  await newer.takeOver("kebbie-shell-next");
  assert.deepEqual(newer.updates, ["update"]);
  const same = updating({ cached: [SHELL] });
  await same.takeOver(SHELL);
  assert.deepEqual(same.updates, [], "the page loaded past the cache, so a worker of its own shell brings nothing new");
  const first = updating({ controller: null });
  await first.takeOver("kebbie-shell-next");
  assert.deepEqual(first.updates, [], "the first worker claiming a page is no update");
});

test("a worker that takes control while its activate step still clears the old caches is judged once it has finished", async () => {
  const newer = updating({ cached: [SHELL] });
  assert.deepEqual(await newer.takeOverActivating("kebbie-shell-next"), [], "nothing is said while the old cache still stands");
  assert.deepEqual(newer.updates, ["update"]);
});

test("the worker names its cache in its own bytes, the same name the page compares against", async () => {
  const { readFileSync } = await import("node:fs");
  const worker = readFileSync(new URL("./sw.js", import.meta.url), "utf8");
  assert.equal(/const SHELL = "([^"]+)";/.exec(worker)?.[1], SHELL, "a new shell changes sw.js itself, since a browser may not compare the modules it imports");
});
