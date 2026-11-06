// Writes css/themes.css and src/themes.ts from navree-ui's theme table (see themes-source.mjs).
//
//   node scripts/generate-themes.mjs          writes both files
//   node scripts/generate-themes.mjs --check  fails when they are stale
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { readThemes, themesCss, themesTs } from "./themes-source.mjs";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const ui = join(root, "..", "..", "..", "crates", "navree-ui");

const themes = readThemes({
  tokensCss: readFileSync(join(ui, "styles", "tokens.css"), "utf8"),
  themeRs: readFileSync(join(ui, "src", "theme", "mod.rs"), "utf8"),
});
const files = [
  [join(root, "css", "themes.css"), themesCss(themes)],
  [join(root, "src", "themes.ts"), themesTs(themes)],
];

const check = process.argv.includes("--check");
let stale = false;
for (const [path, text] of files) {
  if (!check) {
    writeFileSync(path, text);
    continue;
  }
  let committed = "";
  try {
    committed = readFileSync(path, "utf8").replace(/\r\n/g, "\n");
  } catch {
    // missing: stale
  }
  if (committed !== text) {
    console.error(`${path} is stale. Run: node scripts/generate-themes.mjs`);
    stale = true;
  }
}
if (stale) process.exit(1);
