/**
 * Runs after `next build`. Works around a Next.js 16.4 static-export bug on
 * Windows: the router's prefetch files should be written flat, as
 * `out/docs/x/__next.docs.$c$slug.__PAGE__.txt`, but on Windows the segment
 * path keeps its backslashes and they become nested directories
 * (`__next.docs/$c$slug/__PAGE__.txt`). The client then gets 404s for every
 * prefetch. This moves such files to the flat name the client requests. On
 * Linux and macOS there is nothing to move.
 */
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const OUT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "out");

function files(dir: string): string[] {
  return fs.readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const full = path.join(dir, entry.name);
    return entry.isDirectory() ? files(full) : [full];
  });
}

let moved = 0;
function walk(dir: string): void {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    if (!entry.isDirectory()) continue;
    const full = path.join(dir, entry.name);
    if (entry.name.startsWith("__next.")) {
      for (const file of files(full)) {
        const rest = path.relative(full, file).split(path.sep).join(".");
        fs.renameSync(file, path.join(dir, `${entry.name}.${rest}`));
        moved++;
      }
      fs.rmSync(full, { recursive: true, force: true });
    } else {
      walk(full);
    }
  }
}

if (!fs.existsSync(OUT)) {
  console.error("postbuild: out/ does not exist");
  process.exit(1);
}
walk(OUT);
console.log(moved ? `postbuild: flattened ${moved} prefetch files` : "postbuild: nothing to flatten");
