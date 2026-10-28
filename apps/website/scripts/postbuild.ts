/**
 * Runs after `next build` and finishes the static export in `out/`:
 *
 * 1. Flattens prefetch files. Works around a Next.js 16.4 static-export bug
 *    on Windows: the router's prefetch files should be written flat, as
 *    `out/docs/x/__next.docs.$c$slug.__PAGE__.txt`, but on Windows the
 *    segment path keeps its backslashes and they become nested directories
 *    (`__next.docs/$c$slug/__PAGE__.txt`). The client then gets 404s for
 *    every prefetch. This moves such files to the flat name the client
 *    requests. On Linux and macOS there is nothing to move.
 *
 * 2. Paints before it hydrates. Next writes its scripts as `<script async>`,
 *    which a fast connection delivers before the browser has painted the
 *    page, so the first frame waits for React to hydrate, and on a slow one
 *    they take bandwidth from the CSS and fonts the first frame needs. Every
 *    page here is complete HTML, so a small inline loader requests the
 *    scripts once the first contentful paint is on screen instead. Nothing
 *    changes for the page itself: it hydrates the same way, a little later.
 *
 * 3. Checks the fonts: every family a next/font CSS variable names must have
 *    an @font-face in the CSS (see src/app/fonts.ts), or text would silently
 *    fall back to a system font.
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

function fail(message: string): never {
  console.error(`\npostbuild: ${message}\n`);
  process.exit(1);
}

if (!fs.existsSync(OUT)) fail("out/ does not exist");

// ------------------------------------------------------------ 1. prefetch files

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
walk(OUT);
console.log(moved ? `postbuild: flattened ${moved} prefetch files` : "postbuild: nothing to flatten");

// ------------------------------------------------- 2. scripts after first paint

/**
 * Runs the scripts once the first contentful paint is reported (or after the
 * first frame where the browser has no paint timing, or after 4 s at most).
 * Called with the list of script URLs, in document order.
 */
const LOADER = `(function(S){var d=0;function go(){if(d)return;d=1;for(var i=0;i<S.length;i++){var e=document.createElement("script");e.src=S[i];e.async=true;document.head.appendChild(e)}}try{if(PerformanceObserver.supportedEntryTypes.indexOf("paint")<0)throw 0;new PerformanceObserver(function(l){if(l.getEntriesByName("first-contentful-paint").length)go()}).observe({type:"paint",buffered:true})}catch(_){requestAnimationFrame(function(){setTimeout(go,0)})}setTimeout(go,4000)})`;

const asyncScript = /<script src="([^"]+)" async=""><\/script>/g;
const scriptPreload = /<link rel="preload" as="script"[^>]*\/>/g;
let pages = 0;
for (const file of files(OUT).filter((f) => f.endsWith(".html"))) {
  let html = fs.readFileSync(file, "utf8");
  if (html.includes('id="__paint_first"')) continue;
  const sources = [...html.matchAll(asyncScript)].map((m) => m[1]);
  if (!sources.length) continue;
  html = html.replace(asyncScript, "").replace(scriptPreload, "");
  const loader = `<script id="__paint_first">${LOADER}(${JSON.stringify(sources)})</script>`;
  if (!html.includes("</body>")) fail(`${path.relative(OUT, file)} has no </body>`);
  html = html.replace("</body>", `${loader}</body>`);
  fs.writeFileSync(file, html);
  pages++;
}
console.log(`postbuild: ${pages} pages run their scripts after the first paint`);

// ------------------------------------------------------------------ 3. fonts

const css = files(path.join(OUT, "_next", "static"))
  .filter((f) => f.endsWith(".css"))
  .map((f) => fs.readFileSync(f, "utf8"))
  .join("\n");
const unquote = (s: string) => s.trim().replace(/^['"]|['"]$/g, "").toLowerCase();
const faces = new Set([...css.matchAll(/@font-face\s*\{[^}]*?font-family:\s*([^;}]+)/g)].map((m) => unquote(m[1])));
const variables = [...css.matchAll(/(--font-(?:mona|geist|monaspace)):\s*([^;}]+)/g)];
if (!variables.length) fail("found no next/font CSS variables (--font-mona, --font-geist, --font-monaspace)");
for (const [, name, value] of variables) {
  const family = unquote(value.split(",")[0]);
  if (!faces.has(family)) fail(`${name} names the font family "${family}", which has no @font-face`);
}
console.log(`postbuild: ${faces.size} font families, every next/font variable has a face`);
