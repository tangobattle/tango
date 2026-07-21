// tango-web's offline shell. Everything the app fetches is same-origin,
// and everything but the shell itself is content-hashed, so:
//
//  - navigations: network-first (a deploy lands as soon as it can
//    load), falling back to the cached shell offline. The shell names
//    the hashed asset set, so a changed shell means a new deploy and
//    the old asset cache resets with it.
//  - everything else: cache-first, filled on first fetch — hashed
//    files never change in place.
//
// This file must live at the site root: its URL sets the registration
// scope, and GitHub Pages can't send Service-Worker-Allowed headers.
const CACHE = "tango-web-v1";
const SHELL = "/";

self.addEventListener("install", (e) => {
  e.waitUntil(
    caches
      .open(CACHE)
      .then((c) => c.add(SHELL))
      .then(() => self.skipWaiting())
  );
});

self.addEventListener("activate", (e) => {
  e.waitUntil(
    caches
      .keys()
      .then((keys) => Promise.all(keys.filter((k) => k !== CACHE).map((k) => caches.delete(k))))
      .then(() => self.clients.claim())
  );
});

async function navigate(request) {
  const cache = await caches.open(CACHE);
  let fresh;
  try {
    fresh = await fetch(request);
  } catch {
    const cached = await cache.match(SHELL);
    return cached || Response.error();
  }
  if (fresh.ok) {
    const text = await fresh.clone().text();
    const cached = await cache.match(SHELL);
    if (cached && (await cached.text()) !== text) {
      // New deploy: drop the previous asset set.
      for (const key of await cache.keys()) {
        await cache.delete(key);
      }
    }
    await cache.put(SHELL, fresh.clone());
  }
  return fresh;
}

async function asset(request) {
  const cache = await caches.open(CACHE);
  const hit = await cache.match(request);
  if (hit) {
    return hit;
  }
  const fresh = await fetch(request);
  if (fresh.ok) {
    await cache.put(request, fresh.clone());
  }
  return fresh;
}

self.addEventListener("fetch", (e) => {
  const url = new URL(e.request.url);
  if (e.request.method !== "GET" || url.origin !== location.origin) {
    return;
  }
  e.respondWith(e.request.mode === "navigate" ? navigate(e.request) : asset(e.request));
});
