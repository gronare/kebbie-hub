export const SHELL = "kebbie-shell-v31";

// A new worker never reloads the page: the page fetched its files past the cache, so it already runs the newest code.
export function watchShell(workers, { page = globalThis.document } = {}) {
  const registered = workers.register("/sw.js", { type: "module", scope: "/", updateViaCache: "none" }).catch(() => null);
  page.addEventListener("visibilitychange", () => {
    if (page.visibilityState === "visible") registered.then((registration) => registration?.update()).catch(() => {});
  });
  return registered;
}

export function fromNetwork(request, { shell, network = globalThis.fetch, cache = globalThis.caches } = {}) {
  return network(request, { cache: "no-store" }).then((response) => {
    if (response.ok) {
      const copy = response.clone();
      cache.open(shell).then((opened) => opened.put(request, copy)).catch(() => {});
    }
    return response;
  });
}

const settledWorker = (worker) =>
  new Promise((resolve) => {
    const done = () => !worker || worker.state === "activated" || worker.state === "redundant";
    if (done()) return resolve();
    worker.addEventListener("statechange", function waiting() {
      if (!done()) return;
      worker.removeEventListener("statechange", waiting);
      resolve();
    });
  });

// A worker takes control before its activate step has cleared the old caches, so the check waits for it to finish.
export function watchUpdate(workers, onUpdate, { cache = globalThis.caches, shell = SHELL } = {}) {
  if (!workers.controller) return;
  workers.addEventListener("controllerchange", () => {
    settledWorker(workers.controller)
      .then(() => cache.has(shell))
      .then((current) => current || onUpdate())
      .catch(() => {});
  });
}
