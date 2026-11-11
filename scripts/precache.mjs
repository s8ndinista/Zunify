import { createHash } from "node:crypto";
import { readdir, readFile, writeFile } from "node:fs/promises";
import { resolve } from "node:path";

const dist = resolve("dist");
const files = (await readdir(dist, { recursive: true, withFileTypes: true }))
  .filter(entry => entry.isFile() && !entry.name.endsWith(".map") && !entry.name.startsWith("."))
  .map(entry => resolve(entry.parentPath, entry.name).slice(dist.length + 1).replaceAll("\\", "/"))
  .filter(name => name !== "sw.js" && name !== "ferroui-sw.js").sort();
const hash = createHash("sha256");
const worker = await readFile("wwwroot/sw.js", "utf8");
const html = await readFile("wwwroot/index.html", "utf8");
hash.update(worker);
for (const name of files) {
  hash.update(name);
  hash.update(name === "index.html" ? html : await readFile(resolve(dist, name)));
}
const revision = hash.digest("hex").slice(0, 16);
const output = worker.replace('const revision = "dev";', 'const revision = "' + revision + '";')
  .replace("const files = [];", "const files = " + JSON.stringify(files) + ";");
await writeFile(resolve(dist, "index.html"), html.replace('name="app-revision" content="dev"', 'name="app-revision" content="' + revision + '"'));
await writeFile(resolve(dist, "sw.js"), output);
