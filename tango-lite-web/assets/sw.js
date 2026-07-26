// Offline, and a first paint that doesn't wait on the network.
//
// The app is a static drop whose bulk is one ~14MB wasm module, and
// everything it plays is already on the device — so once the shell is
// cached there is nothing a running session needs the network for. A
// link battle does, obviously, and so does the patch repo; both are
// cross-origin and this deliberately doesn't touch them.
//
// The policy is stale-while-revalidate: serve the cached copy at once
// and refresh it in the background, so a rebuild lands on the next
// load. Cache-first would never pick the rebuild up (nothing here has
// a hashed filename), and network-first would give up the fast start
// that is most of the point.

// Both of these are filled in by build.sh from what the build actually
// produced. The shell is generated rather than listed because
// wasm-bindgen emits a `snippets/` tree whose paths nobody chooses --
// hand-maintaining the list means booting offline works until the day
// it silently doesn't. The version is a digest of those same files, so
// a rebuild lands in a new cache and the old one is dropped.
const CACHE = "tango-lite-__BUILD_ID__";
const SHELL = __SHELL__;

self.addEventListener("install", (event) => {
    event.waitUntil(
        (async () => {
            const cache = await caches.open(CACHE);
            // `reload` so installing a new worker can't populate itself
            // from the HTTP cache it is meant to be replacing.
            await cache.addAll(SHELL.map((url) => new Request(url, { cache: "reload" })));
            // Take over without waiting for every tab to close: the
            // shell is versioned by cache name, so there is no
            // half-updated state to protect.
            await self.skipWaiting();
        })(),
    );
});

self.addEventListener("activate", (event) => {
    event.waitUntil(
        (async () => {
            for (const key of await caches.keys()) {
                if (key !== CACHE) await caches.delete(key);
            }
            await self.clients.claim();
        })(),
    );
});

self.addEventListener("fetch", (event) => {
    const request = event.request;
    if (request.method !== "GET") return;

    const url = new URL(request.url);
    // Same-origin only. The matchmaking socket and the patch repo are
    // someone else's, and a cached patch index would be a stale one.
    if (url.origin !== self.location.origin) return;

    event.respondWith(
        (async () => {
            const cache = await caches.open(CACHE);
            const cached = await cache.match(request);
            const network = fetch(request)
                .then((response) => {
                    if (response.ok) cache.put(request, response.clone());
                    return response;
                })
                // Offline with nothing cached: let it fail as it would
                // have without us.
                .catch(() => cached);
            return cached || network;
        })(),
    );
});
