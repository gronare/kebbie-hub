const SHELL = "kebbie-shell-v3";
const FILES = ["/", "/app.css", "/app.js", "/board.js", "/connect.js", "/device-name.js", "/pages.js", "/hub.js", "/store.js", "/passkey.js", "/signin.js", "/unlock.js", "/js/protocol.js", "/js/hpke.js", "/manifest.webmanifest", "/icon.svg", "/icon-192.png", "/apple-touch-icon.png"];
const FALLBACK = { title: "kebbie", body: "Something on the board changed.", tag: "kebbie", url: "/#/board" };

self.addEventListener("install", (event) => {
  event.waitUntil(caches.open(SHELL).then((cache) => cache.addAll(FILES)).then(() => self.skipWaiting()));
});

self.addEventListener("activate", (event) => {
  event.waitUntil(
    caches
      .keys()
      .then((names) => Promise.all(names.filter((name) => name !== SHELL).map((name) => caches.delete(name))))
      .then(() => self.clients.claim()),
  );
});

self.addEventListener("fetch", (event) => {
  const url = new URL(event.request.url);
  if (event.request.method !== "GET" || url.origin !== self.location.origin || url.pathname.startsWith("/v1/")) return;
  event.respondWith(
    fetch(event.request)
      .then((response) => {
        if (response.ok) caches.open(SHELL).then((cache) => cache.put(event.request, response.clone()));
        return response;
      })
      .catch(() => caches.match(event.request, { ignoreSearch: true }).then((cached) => cached ?? caches.match("/"))),
  );
});

self.addEventListener("push", (event) => {
  event.waitUntil(self.registration.showNotification(FALLBACK.title, { body: FALLBACK.body, tag: FALLBACK.tag, data: { url: FALLBACK.url }, icon: "/icon-192.png" }));
});

self.addEventListener("notificationclick", (event) => {
  event.notification.close();
  const url = event.notification.data?.url ?? "/";
  event.waitUntil(
    self.clients.matchAll({ type: "window", includeUncontrolled: true }).then((windows) => {
      const open = windows[0];
      return open ? open.navigate(url).then((client) => (client ?? open).focus()) : self.clients.openWindow(url);
    }),
  );
});
