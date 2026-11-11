import { createReadStream } from "node:fs";
import { realpath, stat } from "node:fs/promises";
import { createServer } from "node:http";
import { extname, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

const dist = fileURLToPath(new URL("../dist/", import.meta.url));
const mime = {
  ".html": "text/html; charset=utf-8", ".js": "text/javascript; charset=utf-8", ".wasm": "application/wasm",
  ".css": "text/css; charset=utf-8", ".svg": "image/svg+xml", ".png": "image/png", ".jpg": "image/jpeg",
  ".webmanifest": "application/manifest+json", ".wav": "audio/wav", ".json": "application/json",
};
export async function serve(port = 4173) {
  const root = await realpath(dist).catch(() => { throw new Error("Build the app first: npm run build"); });
  const server = createServer(async (req, res) => {
    if (req.headers.host !== `127.0.0.1:${server.address().port}`) return res.writeHead(403).end();
    if (req.method !== "GET" && req.method !== "HEAD") return res.writeHead(405, { Allow: "GET, HEAD" }).end();
    try {
      const url = new URL(req.url, "http://127.0.0.1");
      const path = decodeURIComponent(url.pathname);
      if (path.split("/").some(part => part.startsWith("."))) return res.writeHead(404).end();
      const file = await realpath(resolve(root, "." + (path === "/" ? "/index.html" : path)));
      if (!file.startsWith(root + sep)) return res.writeHead(404).end();
      const info = await stat(file);
      if (!info.isFile()) return res.writeHead(404).end();
      const headers = {
        "Content-Type": mime[extname(file)] || "application/octet-stream",
        "X-Content-Type-Options": "nosniff", "Cache-Control": "no-cache",
        "Accept-Ranges": "bytes", "Content-Length": info.size,
      };
      let code = 200, start = 0, end = info.size - 1;
      const range = /^bytes=(\d*)-(\d*)$/.exec(req.headers.range || "");
      if (range && (range[1] || range[2])) {
        start = range[1] ? Number(range[1]) : Math.max(0, info.size - Number(range[2]));
        if (range[1] && range[2]) end = Math.min(Number(range[2]), end);
        if (start > end) return res.writeHead(416, { "Content-Range": "bytes */" + info.size }).end();
        headers["Content-Length"] = end - start + 1;
        headers["Content-Range"] = `bytes ${start}-${end}/${info.size}`;
        code = 206;
      }
      res.writeHead(code, headers);
      if (req.method === "HEAD" || !info.size) return res.end();
      const stream = createReadStream(file, { start, end });
      res.on("close", () => stream.destroy());
      stream.on("error", () => res.destroy());
      stream.pipe(res);
    } catch { if (!res.headersSent) res.writeHead(404).end("Not found"); else res.destroy(); }
  });
  await new Promise((ok, error) => {
    server.once("error", error);
    server.listen(port, "127.0.0.1", ok);
  });
  return server;
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const server = await serve(Number(process.env.PORT || 4173));
    console.log(`SpotiFe: http://127.0.0.1:${server.address().port}/`);
  } catch (error) { console.error(error.message); process.exitCode = 1; }
}
