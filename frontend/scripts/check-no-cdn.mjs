// Fails if the built bundle references a CDN: the UI must work offline.
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const FORBIDDEN = ["cdn.jsdelivr.net", "unpkg.com"];
// fileURLToPath, not URL.pathname: on Windows pathname is "/D:/…", which breaks fs calls.
const dist = fileURLToPath(new URL("../dist/", import.meta.url));

function* files(dir) {
  for (const name of readdirSync(dir)) {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) yield* files(path);
    else yield path;
  }
}

let found = 0;
let checked = 0;
for (const path of files(dist)) {
  checked += 1;
  const text = readFileSync(path, "latin1");
  for (const host of FORBIDDEN) {
    if (text.includes(host)) {
      console.error(`${path}: references ${host}`);
      found += 1;
    }
  }
}
if (checked === 0) {
  console.error("dist/ is empty; run npm run build first");
  process.exit(1);
}
if (found > 0) process.exit(1);
console.log(`no CDN references in ${checked} files`);
