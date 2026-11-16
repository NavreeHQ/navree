// Builds the map's browser files next to this script:
//
//   map.js   the glue in src/map.js bundled with Leaflet 1.9.4 into one module that imports nothing
//   map.css  Leaflet's stylesheet without its three rules that name pictures and its one rule for
//            VML, then src/own.css: a plugin's stylesheet names no address but a data: one
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
/** Its rule for VML, a drawing format of browsers long gone. */
const VML_RULES = 1;

/** @param {string} message @returns {never} */
function fail(message) {
  console.error(message);
  process.exit(1);
}

const leafletDir = join(here, "node_modules", "leaflet");
if (!existsSync(join(leafletDir, "package.json"))) fail("leaflet is not installed: run `npm install` in this folder first");
const installed = JSON.parse(readFileSync(join(leafletDir, "package.json"), "utf8"));
if (installed.version !== LEAFLET) fail(`leaflet ${installed.version} is installed, and this map is written for ${LEAFLET}: run \`npm install\``);
// src/map.js imports this file by name: Leaflet's package entry is its script build, which writes a global.
if (!existsSync(join(leafletDir, "dist", "leaflet-src.esm.js"))) fail("leaflet's module build, dist/leaflet-src.esm.js, is not in the installed package");

async function js() {
  const out = await build({
    entryPoints: [join(here, "src", "map.js")],
    bundle: true,
    minify: true,
    format: "esm",
    target: TARGET,
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
 * `css` without each rule whose block holds `needle`, counted in `found`. A group of rules (`@media`)
 * is looked into, so a rule inside it goes and the group stays.
 * @param {string} css @param {string} needle @param {{count: number}} found @returns {string}
 */
function withoutRules(css, needle, found) {
  return rulesOf(css)
    .map((rule) => {
      if (!rule.includes(needle)) return rule;
      const lead = /** @type {RegExpMatchArray} */ (rule.match(/^(?:\s|\/\*[\s\S]*?\*\/)*/))[0];
      const text = rule.slice(lead.length);
      if (/^@(media|supports|container|layer)\b/.test(text) && text.includes("{")) {
        const open = lead.length + text.indexOf("{"), close = rule.lastIndexOf("}");
        return rule.slice(0, open + 1) + withoutRules(rule.slice(open + 1, close), needle, found) + rule.slice(close);
      }
      found.count++;
      return "";
    })
    .join("");
}

function css() {
  const pictures = { count: 0 }, old = { count: 0 };
  let leaflet = readFileSync(join(leafletDir, "dist", "leaflet.css"), "utf8").replace(/\r\n/g, "\n");
  leaflet = withoutRules(leaflet, "url(images/", pictures);
  if (pictures.count !== PICTURE_RULES) fail(`expected ${PICTURE_RULES} rules that name a picture in leaflet.css and found ${pictures.count}: this map is written for leaflet ${LEAFLET}`);
  // The rule for a drawing format no browser of today has: its `behavior: url(#default#VML)` is the sheet's
  // one other address, and a plugin's stylesheet may name none.
  leaflet = withoutRules(leaflet, "url(#default#VML)", old);
  if (old.count !== VML_RULES) fail(`expected ${VML_RULES} rule with behavior: url(#default#VML) in leaflet.css and found ${old.count}: this map is written for leaflet ${LEAFLET}`);
  if (leaflet.includes("url(")) fail("leaflet.css still names an address after those rules were taken out: a plugin's stylesheet may name none");
  const own = readFileSync(join(here, "src", "own.css"), "utf8").replace(/\r\n/g, "\n");
  console.log(`removed ${pictures.count} rules that name a picture and ${old.count} that names a behaviour file`);
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
