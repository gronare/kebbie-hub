import { showPushed } from "./notify.js";
import { account } from "./hub.js";
import { fromNetwork } from "./shell.js";
import { del, get, put } from "./store.js";

const SHELL = "kebbie-shell-v24";
const FILES = ["/", "/app.css", "/app.js", "/backlog.js", "/blobs.js", "/board.js", "/card.js", "/commit.js", "/connect.js", "/decide.js", "/device.js", "/device-name.js", "/markdown.js", "/pages.js", "/screens.js", "/sections.js", "/pose.js", "/scroll.js", "/shell.js", "/hub.js", "/notify.js", "/store.js", "/passkey.js", "/review.js", "/signin.js", "/email.js", "/unlock.js", "/js/protocol.js", "/js/hpke.js", "/fonts/bricolage-grotesque-latin-opsz.woff2", "/fonts/ibm-plex-sans-latin-400.woff2", "/fonts/ibm-plex-sans-latin-500.woff2", "/fonts/ibm-plex-sans-latin-600.woff2", "/fonts/ibm-plex-mono-latin-500.woff2", "/manifest.webmanifest", "/icon.svg", "/icon-192.png", "/apple-touch-icon.png"];

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
    fromNetwork(event.request, { shell: SHELL }).catch(() => caches.match(event.request, { ignoreSearch: true }).then((cached) => cached ?? caches.match("/"))),
  );
});

const pushed = (event) => {
  try {
    return event.data?.json() ?? null;
  } catch {
    return null;
  }
};

self.addEventListener("push", (event) => {
  event.waitUntil(showPushed(self.registration, pushed(event), { get, put, del, boards: account.boards, now: Math.floor(Date.now() / 1000) }));
});

self.addEventListener("notificationclick", (event) => {
  event.notification.close();
  const url = new URL(event.notification.data?.url ?? "/", self.location.origin).href;
  event.waitUntil(
    self.clients.matchAll({ type: "window", includeUncontrolled: true }).then((windows) => {
      const open = windows[0];
      if (!open) return self.clients.openWindow(url);
      return open.navigate(url).then((client) => (client ?? open).focus()).catch(() => self.clients.openWindow(url));
    }),
  );
});
