// The app never caches Spotify credentials, API responses, or Spotify artwork.
const prefix = "spotifast-static-";
const revision = "72daba75986f58a3";
const version = prefix + revision;
const snippets = ["snippets/spotify-ferro-495f3a2a301764bd/inline0.js"];
const root = new URL("./", self.location.href);
const shell = new URL("index.html", root).href;
const files = [
  "index.html", "host.css", "main.js", "demo.js", "demo-library.json", "spotify.js", "spotify-config.js",
  "icon.svg", "icon-192.png", "icon-512.png", "manifest.webmanifest",
  "ferroui.js", "ferroui-loader.js", "storage.js", "spotify-ferro.js", "spotify_ferro.wasm",
  ...snippets,
  ...["hero", "mix-1", "mix-2", "mix-3", "mix-4"].map(x => "art/" + x + ".jpg"),
  ...Array.from({ length: 6 }, (_, i) => "audio/sample-" + (i + 1) + ".wav"),
];
self.addEventListener("install", event => {
  event.waitUntil(caches.open(version).then(cache => cache.addAll(files.map(name => new URL(name, root)))));
});
self.addEventListener("activate", event => {
  event.waitUntil(caches.keys().then(keys => Promise.all(keys
    .filter(key => key.startsWith(prefix) && key !== version).map(key => caches.delete(key))))
    .then(() => self.clients.claim()));
});
self.addEventListener("fetch", event => {
  const request = event.request;
  const url = new URL(request.url);
  if (request.method !== "GET" || !url.href.startsWith(root.href)) return;
  if (request.mode === "navigate") {
    event.respondWith(fetch(request).catch(() => caches.open(version).then(cache => cache.match(shell))));
    return;
  }
  const path = url.pathname.slice(root.pathname.length);
  if (url.search || !files.includes(path)) return;
  event.respondWith((async () => {
    const cache = await caches.open(version);
    const cached = await cache.match(request.url);
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
    const response = await fetch(request);
    if (response.ok && response.status === 200) await cache.put(request, response.clone());
    return response;
  })());
});
