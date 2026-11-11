// The app never caches Spotify credentials, API responses, or Spotify artwork.
const revision = "764525d68bba4963";
const root = new URL("./", self.location.href);
const prefix = "spotife-static:" + root.pathname + ":";
const version = prefix + revision;
const files = ["art/hero.jpg","art/mix-1.jpg","art/mix-2.jpg","art/mix-3.jpg","art/mix-4.jpg","audio/sample-1.wav","audio/sample-2.wav","audio/sample-3.wav","audio/sample-4.wav","audio/sample-5.wav","audio/sample-6.wav","demo-library.json","demo.js","ferroui-loader.js","ferroui.js","host.css","icon-192.png","icon-512.png","icon.svg","index.html","main.js","manifest.webmanifest","snippets/spotify-ferro-495f3a2a301764bd/inline0.js","spotify-config.js","spotify-ferro.js","spotify.js","spotify_ferro.wasm","storage.js","update.js"];
const asset = name => {
  const url = new URL(name, root);
  url.searchParams.set("v", revision);
  return url.href;
};
const shell = asset("index.html");

self.addEventListener("install", event => {
  event.waitUntil((async () => {
    const cache = await caches.open(version);
    await cache.addAll(files.map(name => new Request(asset(name), { cache: "reload" })));
    const html = await (await cache.match(shell)).text();
    if (!html.includes('<meta name="app-revision" content="' + revision + '">')) {
      await caches.delete(version);
      throw new Error("The new app shell is still being published.");
    }
    await self.skipWaiting();
  })());
});
self.addEventListener("activate", event => {
  event.waitUntil((async () => {
    const oldShell = new URL("index.html", root).href;
    for (const key of await caches.keys()) {
      if (key.startsWith(prefix) && key !== version) {
        await caches.delete(key);
      } else if (key.startsWith("spotifast-static-") && await caches.match(oldShell, { cacheName: key })) {
        await caches.delete(key);
      }
    }
    await self.clients.claim();
  })());
});
self.addEventListener("fetch", event => {
  const request = event.request;
  const url = new URL(request.url);
  if (request.method !== "GET" || !url.href.startsWith(root.href)) return;
  if (request.mode === "navigate" && (url.pathname === root.pathname || url.pathname === new URL("index.html", root).pathname)) {
    event.respondWith(caches.open(version).then(cache => cache.match(shell))
      .then(cached => cached ?? fetch(request, { cache: "reload" })));
    return;
  }
  const path = url.pathname.slice(root.pathname.length);
  if (url.search || !files.includes(path)) return;
  event.respondWith((async () => {
    const cache = await caches.open(version);
    const cached = await cache.match(asset(path));
    const range = request.headers.get("range");
    if (cached && range && path.startsWith("audio/")) {
      const match = /^bytes=(\d*)-(\d*)$/.exec(range);
      if (!match || (!match[1] && !match[2])) return fetch(request);
      const bytes = await cached.arrayBuffer();
      const start = match[1] ? Number(match[1]) : Math.max(0, bytes.byteLength - Number(match[2]));
      const end = Math.min(match[1] && match[2] ? Number(match[2]) : bytes.byteLength - 1, bytes.byteLength - 1);
      if (start > end) return new Response(null, { status: 416, headers: { "Content-Range": "bytes */" + bytes.byteLength } });
      return new Response(bytes.slice(start, end + 1), { status: 206, headers: {
        "Content-Type": "audio/wav", "Content-Range": `bytes ${start}-${end}/${bytes.byteLength}`,
        "Content-Length": String(end - start + 1), "Accept-Ranges": "bytes",
      } });
    }
    if (cached) return cached;
    return fetch(request, { cache: "reload" });
  })());
});
