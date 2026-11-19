// A page that starts clean imports only the toolkit's own script modules, whatever an ops envelope's
// `modules` list or the head of a fetched page or fragment names: never a plugin's component script, a
// plugin's or an app's island script, a stylesheet, a data file, an unknown name under `/_navree/ui/`, a
// query or fragment on a module's address, or another origin.
import { test } from "node:test";
import assert from "node:assert/strict";
import { register } from "node:module";
import { answer, fire, install, settle } from "./fake-dom.mjs";

register("./own-module-loader.mjs", import.meta.url);

const page = install({
  url: "https://app.test/sign-in",
  cookie: "XSRF-TOKEN=tok",
  html: `<!doctype html><html data-build="b1" data-ui-fresh-page><head></head><body data-ui-boost>
    <main id="main">
      <h1 id="heading">Sign in</h1>
      <form id="check" data-ui-form method="post" action="/sign-in/check"><button id="send" type="submit">Continue</button></form>
      <a id="to-code" href="/sign-in/code">Enter a code instead</a>
      <div id="help" data-ui-region data-ui-src="/sign-in/help">Help</div>
    </main>
  </body></html>`,
});

const { ownModule } = await import("../src/dom.js");
await import("../src/core.js");
await settle();

const g = /** @type {any} */ (globalThis);
const $ = (/** @type {string} */ id) => /** @type {any} */ (document.getElementById(id));
const OPS = { "content-type": "application/vnd.navree.ops+json" };
const PAGE = { "content-type": "text/html; charset=utf-8", "x-navree-page": "1", "x-navree-fresh": "1" };

const OWN = [
  "core",
  "sidebar",
  "overlay",
  "table",
  "live",
  "select",
  "dates",
  "date-text",
  "time",
  "menus",
  "slider",
  "tree",
  "toolbar",
  "split",
  "keys",
  "lazy",
  "sortable",
  "sortable-grid",
  "upload",
  "otp",
  "carousel",
  "menubar",
  "charts",
  "count-up",
  "islands",
  "parts",
  "genui",
  "extensions",
  "phone",
];

const REFUSED = [
  "/_navree/ui/ext/proof.pad-0123456789ab.js",
  "https://app.test/_navree/ui/ext/proof.vendor-0123456789ab.js",
  "/_navree/ui/island/proof.notice-0123456789ab.js",
  "/_navree/ui/island/clock-0123456789ab.js",
  "/_navree/ui/app/brand-0123456789ab.css",
  "/_navree/ui/theme/amber-0123456789ab.css",
  "/_navree/ui/stand-in.js",
  "/_navree/ui/stand-in-0123456789ab.js",
  "/_navree/ui/navree-ui-0123456789ab.css",
  "/_navree/ui/phone-data-0123456789ab.json",
  "/_navree/ui/keys-0123456789a.js",
  "/_navree/ui/keys-0123456789abc.js",
  "/_navree/ui/keys-0123456789AB.js",
  "/_navree/ui/keys-0123456789ab.js?v=2",
  "/_navree/ui/keys-0123456789ab.js#part",
  "../_navree/ui/keys-0123456789ab.js",
  "/_navree/ui/./keys-0123456789ab.js",
  "https://app.test/_navree/ui/keys-0123456789ab.js",
  "https://user:pass@app.test/_navree/ui/keys-0123456789ab.js",
  "https://cdn.example/_navree/ui/keys-0123456789ab.js",
];

test("a fresh page imports only the toolkit's own modules, whatever an envelope or a fetched head lists", async () => {
  for (const name of OWN) {
    assert.equal(ownModule(`/_navree/ui/${name}-0123456789ab.js`), true, name);
    assert.equal(ownModule(`/_navree/ui/${name}.js`), true, name);
  }
  for (const bad of REFUSED) assert.equal(ownModule(bad), false, bad);

  // An ops envelope that names plugin scripts, island scripts, unknown files and two of the toolkit's own.
  const listed = [...REFUSED, "/_navree/ui/otp-0123456789ab.js", "/_navree/ui/keys.js"];
  page.answers.push(answer(JSON.stringify({ modules: listed, ops: [] }), { headers: OPS }));
  $("send").click();
  await settle();
  await new Promise((go) => setTimeout(go, 50));
  assert.deepEqual(g.imported, ["/_navree/ui/otp-0123456789ab.js", "/_navree/ui/keys.js"]);

  // A boosted navigation to another fresh page, and a region's answer, whose heads list the same mix.
  const scripts = [...REFUSED, "/_navree/ui/overlay-0123456789ab.js"].map((src) => `<script type="module" src="${src}"></script>`).join("");
  page.answers.push(
    answer(
      `<!doctype html><html data-build="b1" data-ui-fresh-page><head><title>Enter code</title>${scripts}</head><body data-ui-boost><main id="main"><h1 id="heading">Enter code</h1><div id="help" data-ui-region data-ui-src="/sign-in/help">Help</div></main></body></html>`,
      { headers: PAGE },
    ),
  );
  $("to-code").click();
  await settle();
  assert.equal($("heading").textContent, "Enter code");

  const regionScripts = [
    "/_navree/ui/ext/proof.pad-0123456789ab.js",
    "/_navree/ui/island/proof.notice-0123456789ab.js",
    "/_navree/ui/menus-0123456789ab.js",
  ]
    .map((src) => `<script type="module" src="${src}"></script>`)
    .join("");
  page.answers.push(
    answer(
      `<!doctype html><html data-build="b1" data-ui-fresh-page><head>${regionScripts}</head><body><div id="help" data-ui-region data-ui-src="/sign-in/help">Code sent to Simran Kaur</div></body></html>`,
      { headers: PAGE },
    ),
  );
  fire($("help"), "ui:refresh");
  await settle();
  assert.equal($("help").textContent, "Code sent to Simran Kaur");
  assert.deepEqual(
    [...document.head.querySelectorAll("script")].map((s) => s.src || s.getAttribute("src")),
    ["/_navree/ui/overlay-0123456789ab.js", "/_navree/ui/menus-0123456789ab.js"],
  );
});
