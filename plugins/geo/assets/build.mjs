// Builds the map's browser files next to this script:
//
//   map.js   the glue in src/map.js bundled with Leaflet 1.9.4 into one module that imports nothing
//   map.css  Leaflet's stylesheet without its three rules that name pictures, then src/own.css
//
// The output is committed, so `cargo build` never needs Node: the crate reads both files when it
// compiles, scopes the stylesheet to the plugin's components and checks both against their ceilings.
// `--check` fails when the committed output differs from a fresh build.
//
//   npm install && node build.mjs [--check]

import { build } from "esbuild";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { gzipSync } from "node:zlib";

const here = dirname(fileURLToPath(import.meta.url));
const check = process.argv.includes("--check");
const TARGET = ["chrome135", "firefox144", "safari26"];
const LEAFLET = "1.9.4";
/** The rules of Leaflet's stylesheet that name a picture file: the layers control's and the default marker's. */
const PICTURE_RULES = 3;

/** @param {string} message @returns {never} */
function fail(message) {
  console.error(message);
  process.exit(1);
}

const leafletDir = join(here, "node_modules", "leaflet");
if (!existsSync(join(leafletDir, "package.json"))) fail("leaflet is not installed: run `npm install` in this folder first");
const installed = JSON.parse(readFileSync(join(leafletDir, "package.json"), "utf8"));
if (installed.version !== LEAFLET) fail(`leaflet ${installed.version} is installed, and this map is written for ${LEAFLET}: run \`npm install\``);
if (typeof installed.module !== "string") console.warn("leaflet's package.json names no ES module: the bundle takes its script build instead, which also writes a global");

async function js() {
  const out = await build({
    entryPoints: [join(here, "src", "map.js")],
    bundle: true,
    minify: true,
    format: "esm",
    target: TARGET,
    mainFields: ["module", "main"],
    write: false,
    legalComments: "eof",
  });
  return Buffer.from(out.outputFiles[0].contents);
}

/** The index just after the string that starts at `from`. @param {string} css @param {number} from */
function afterString(css, from) {
  const quote = css[from];
  let at = from + 1;
  while (at < css.length && css[at] !== quote) at += css[at] === "\\" ? 2 : 1;
  return at + 1;
}

/**
 * The top-level rules and statements of `css`, each from where the one before it ended to its own closing
 * brace or semicolon, so the comments in front of a rule go with it. Braces and semicolons inside comments
 * and strings count for nothing.
 * @param {string} css
 */
function rulesOf(css) {
  /** @type {string[]} */
  const rules = [];
  let start = 0, depth = 0, at = 0;
  while (at < css.length) {
    const c = css[at];
    if (c === "/" && css[at + 1] === "*") {
      const end = css.indexOf("*/", at + 2);
      at = end < 0 ? css.length : end + 2;
    } else if (c === '"' || c === "'") {
      at = afterString(css, at);
    } else if (c === "\\") {
      at += 2;
    } else {
      if (c === "{") depth++;
      if (c === "}") depth--;
      if (depth === 0 && (c === "}" || c === ";")) {
        rules.push(css.slice(start, at + 1));
        start = at + 1;
      }
      at++;
    }
  }
  if (css.slice(start).trim()) rules.push(css.slice(start));
  return rules;
}

/**
 * `css` without each rule whose block names a picture of `images/`, counted in `found`. A group of
 * rules (`@media`) is looked into, so a rule inside it goes and the group stays.
 * @param {string} css @param {{count: number}} found @returns {string}
 */
function withoutPictureRules(css, found) {
  return rulesOf(css)
    .map((rule) => {
      if (!rule.includes("url(images/")) return rule;
      const lead = /** @type {RegExpMatchArray} */ (rule.match(/^(?:\s|\/\*[\s\S]*?\*\/)*/))[0];
      const text = rule.slice(lead.length);
      if (/^@(media|supports|container|layer)\b/.test(text) && text.includes("{")) {
        const open = lead.length + text.indexOf("{"), close = rule.lastIndexOf("}");
        return rule.slice(0, open + 1) + withoutPictureRules(rule.slice(open + 1, close), found) + rule.slice(close);
      }
      found.count++;
      return "";
    })
    .join("");
}

function css() {
  const found = { count: 0 };
  const leaflet = withoutPictureRules(readFileSync(join(leafletDir, "dist", "leaflet.css"), "utf8").replace(/\r\n/g, "\n"), found);
  if (found.count !== PICTURE_RULES) fail(`expected ${PICTURE_RULES} rules that name a picture in leaflet.css and found ${found.count}: this map is written for leaflet ${LEAFLET}`);
  if (leaflet.includes("url(images/")) fail("a picture of leaflet.css is still named after its rules were taken out");
  const own = readFileSync(join(here, "src", "own.css"), "utf8").replace(/\r\n/g, "\n");
  console.log(`removed ${found.count} rules that name a picture`);
  return Buffer.from(`${leaflet.trim()}\n\n${own.trim()}\n`);
}

const outputs = new Map([
  ["map.js", await js()],
  ["map.css", css()],
]);

if (check) {
  const stale = [...outputs].filter(([file, bytes]) => !existsSync(join(here, file)) || !readFileSync(join(here, file)).equals(bytes)).map(([file]) => file);
  if (stale.length) {
    console.error(`stale: ${stale.join(", ")}\nrun: node build.mjs`);
    process.exit(1);
  }
  console.log("up to date");
} else {
  for (const [file, bytes] of outputs) writeFileSync(join(here, file), bytes);
}
for (const [file, bytes] of outputs) {
  console.log(`${file.padEnd(8)} ${String(bytes.length).padStart(7)} raw ${String(gzipSync(bytes, { level: 9 }).length).padStart(7)} gzip`);
}
