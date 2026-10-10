// Release packaging replaces this marker with the exact immutable release ID.
const CACHE = "sentinal-__SENTINAL_RELEASE_ID__";
importScripts("/push-safety.js");

const SHELL = ["/", "/manifest.webmanifest", "/icons/app-icon.svg", "/push-safety.js"];
const { safeNotificationRoute, safePushPayload } = self.SentinalPushSafety;

self.addEventListener("install", (event) => {
  event.waitUntil(
    caches.open(CACHE).then((cache) => cache.addAll(SHELL)).then(async () => {
      const clients = await self.clients.matchAll({ type: "window", includeUncontrolled: true });
      for (const client of clients) client.postMessage({ type: "UPDATE_READY" });
    }),
  );
});

self.addEventListener("message", (event) => {
  if (event.data?.type === "SKIP_WAITING") void self.skipWaiting();
});

self.addEventListener("activate", (event) => {
  event.waitUntil(
    caches
      .keys()
      .then((keys) => Promise.all(keys.filter((key) => key !== CACHE).map((key) => caches.delete(key))))
      .then(() => self.clients.claim()),
  );
});

self.addEventListener("fetch", (event) => {
  if (event.request.method !== "GET") return;
  const url = new URL(event.request.url);
  if (
    url.origin !== self.location.origin ||
    url.pathname.startsWith("/api/") ||
    url.pathname.startsWith("/packages/") ||
    url.pathname === "/install.sh"
  ) return;
  if (event.request.mode === "navigate") {
    event.respondWith(fetch(event.request).catch(() => caches.match("/")));
    return;
  }
  if (SHELL.includes(url.pathname)) {
    event.respondWith(caches.match(event.request).then((cached) => cached ?? fetch(event.request)));
  }
});

self.addEventListener("push", (event) => {
  event.waitUntil(
    Promise.resolve().then(() => {
      const payload = safePushPayload(event.data?.text(), self.location.origin);
      if (!payload) return undefined;
      return self.registration.showNotification(payload.title, {
        body: payload.body,
        // Server-side policy already deduplicates logical Events. A unique tag
        // prevents Apple from silently replacing later Events in one category.
        tag: `sentinal-${payload.category.slice(0, 32)}-${payload.at}`,
        data: { route: payload.route },
      });
    }),
  );
});

self.addEventListener("notificationclick", (event) => {
  event.notification.close();
  const route = safeNotificationRoute(event.notification.data?.route, self.location.origin);
  if (!route) return;
  event.waitUntil(
    self.clients.matchAll({ type: "window", includeUncontrolled: true }).then(async (clients) => {
      for (const client of clients) {
        if (new URL(client.url).origin === self.location.origin) {
          await client.navigate(route);
          return client.focus();
        }
      }
      return self.clients.openWindow(route);
    }),
  );
});
