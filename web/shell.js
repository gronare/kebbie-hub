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
