import { identity, loadDevice, loadMachines } from "./keystore.js";
import { FALLBACK, openNotice } from "./notice.js";

const SHELL = "kelpie-shell-v1";
const FILES = ["/", "/app.css", "/app.js", "/board.js", "/hub.js", "/keystore.js", "/pairing.js", "/passkey.js", "/notice.js", "/js/protocol.js", "/js/hpke.js", "/manifest.webmanifest", "/icon.svg", "/icon-192.png", "/apple-touch-icon.png"];

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

async function show(data) {
  let notice = FALLBACK;
  try {
    const device = await loadDevice();
    if (device) notice = await openNotice(data, identity(device), await loadMachines(), Math.floor(Date.now() / 1000));
  } catch {}
  if (notice.retract) {
    for (const shown of await self.registration.getNotifications({ tag: notice.retract })) shown.close();
  }
  await self.registration.showNotification(notice.title, { body: notice.body, tag: notice.tag, data: { url: notice.url }, icon: "/icon-192.png" });
}

self.addEventListener("push", (event) => {
  let data = null;
  try {
    data = event.data?.json() ?? null;
  } catch {}
  event.waitUntil(show(data));
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
