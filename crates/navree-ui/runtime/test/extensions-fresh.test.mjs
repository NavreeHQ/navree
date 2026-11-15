// A page that starts clean (it carries `data-ui-fresh`) never holds plugin code: should the extensions
// module come to one (an op's reply can bring it), it imports nothing, seals nothing and links nothing.
import { test } from "node:test";
import assert from "node:assert/strict";
import { register } from "node:module";
import { install, settle } from "./fake-dom.mjs";

// The script the page names would load here: what keeps it out is the module, not the loader.
register("./ext-loader.mjs", import.meta.url);

const EXT = "/_navree/ui/ext";
install({
  url: "https://app.test/sign-in",
  observe: true,
  html: `<!doctype html><html data-build="b1" data-ui-fresh><head></head><body data-ui-boost>
    <main id="main">
      <h1>Sign in</h1>
      <div id="pad" data-slot="proof:pad" data-ui-ext="${EXT}/proof.pad-0123456789ab.js" data-ui-ext-sheet="${EXT}/proof.pad-0123456789ab.css"
           data-ui-ext-routes="/proof/" data-ui-props='{"step":1}'>
        <div id="pad-body" data-slot="proof:pad-body" data-ui-keep><button id="pad-button" data-ui-click='[["toggle","menu"]]'>Show</button></div>
      </div>
      <span id="badge" data-slot="proof:badge" data-ui-ext-sheet="${EXT}/proof.badge-0123456789ab.css">Harpreet Sandhu</span>
    </main>
  </body></html>`,
});

const before = /** @type {any} */ (document.getElementById("main")).outerHTML;
await import("../src/core.js");
await import("../src/extensions.js");
document.dispatchEvent(new Event("ui:swap"));
// Long enough for a script the module asked for to come through the loader.
await new Promise((go) => setTimeout(go, 200));
await settle();

test("does nothing on a fresh page", () => {
  const g = /** @type {any} */ (globalThis);
  assert.equal(g.seen, undefined, "a script was imported on a page that starts clean");
  assert.equal(/** @type {any} */ (document.getElementById("main")).outerHTML, before);
  assert.equal(document.head.querySelectorAll("link").length, 0);
});
