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
hash.update(worker);
for (const name of files) {
  hash.update(name);
  hash.update(await readFile(resolve(dist, name)));
}
const output = worker.replace('const revision = "dev";', 'const revision = "' + hash.digest("hex").slice(0, 16) + '";')
  .replace("const snippets = [];", "const snippets = " + JSON.stringify(files.filter(name => name.startsWith("snippets/"))) + ";");
await writeFile(resolve(dist, "sw.js"), output);
