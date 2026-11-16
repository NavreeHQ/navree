// A plugin's island whose body no one seals (the extensions module is not on the page: an old page, or a module
// that failed to load) does not import its script: the fallback stays, and the island is marked failed. An app's
// own island mounts as it always has. When the extensions module comes later (a navigation can bring it after the
// islands module), the plugin's island is sealed then, and mounted.
import { test } from "node:test";
import assert from "node:assert/strict";
import { register } from "node:module";
import { install, settle } from "./fake-dom.mjs";

// `https://app.test/_navree/ui/island/NAME.js` is `fixtures/island/NAME.mjs`: the script would load here, so what
// keeps it out is the islands module, not the loader.
register("./ext-loader.mjs", import.meta.url);

const ISLAND = "/_navree/ui/island";
install({
  url: "https://app.test/orders/new",
  observe: true,
  html: `<!doctype html><html data-build="b1"><head></head><body>
    <main id="main">
      <div id="notice" data-slot="island" data-ui-island="${ISLAND}/proof.notice-0123456789ab.js" data-ui-keep-attrs="data-ui-mounted data-ui-failed" data-ui-props='{"city":"Ludhiana"}'>
        <div id="notice-body" data-slot="island-body" data-ui-keep><p id="notice-drawn" data-ui-click='[["toggle","menu"]]'>Deliveries in Ludhiana</p></div>
      </div>
      <div id="clock" data-slot="island" data-ui-island="${ISLAND}/clock-0123456789ab.js" data-ui-keep-attrs="data-ui-mounted data-ui-failed" data-ui-props='{"zone":"IST"}'>
        <div id="clock-body" data-slot="island-body" data-ui-keep><p>10:30 IST</p></div>
      </div>
      <div id="menu" hidden>Zones</div>
    </main>
  </body></html>`,
});

await import("../src/islands.js");

const g = /** @type {any} */ (globalThis);
const $ = (/** @type {string} */ id) => /** @type {any} */ (document.getElementById(id));
/** Waits for `done`: a script comes through the module loader in its own time. @param {() => boolean} done @param {string} what */
async function until(done, what) {
  for (let i = 0; i < 400; i++) {
    if (done()) return;
    await new Promise((go) => setTimeout(go, 5));
  }
  assert.fail(`still waiting for ${what}`);
}

await until(() => $("clock").hasAttribute("data-ui-mounted"), "the app's island to mount");
// Long enough for a script the islands module asked for to come through the loader.
await new Promise((go) => setTimeout(go, 100));
await settle();

test("a plugin's island is not imported with no extensions module, and is marked failed", async () => {
  assert.equal(g.noticeLoaded, undefined, "a plugin's island script was imported with its body unsealed");
  assert.equal($("notice").hasAttribute("data-ui-failed"), true);
  assert.equal($("notice").hasAttribute("data-ui-mounted"), false);
  // The fallback stays as the server drew it.
  assert.equal($("notice-body").hasAttribute("data-ui-stored"), false);
  assert.equal($("notice-drawn").textContent, "Deliveries in Ludhiana");
  assert.equal($("notice-drawn").hasAttribute("data-ui-click"), true);
  // A swap asks again, and still nothing seals it.
  document.dispatchEvent(new Event("ui:swap"));
  await new Promise((go) => setTimeout(go, 50));
  await settle();
  assert.equal(g.noticeLoaded, undefined);
  assert.equal($("notice").hasAttribute("data-ui-failed"), true);
});

test("an app's own island mounts with no extensions module", () => {
  assert.equal(g.clockMount.body, $("clock-body"));
  assert.equal($("clock-body").hasAttribute("data-ui-stored"), false);
  assert.equal($("clock").hasAttribute("data-ui-failed"), false);
});

test("a plugin's island is sealed and mounted once the extensions module comes", async () => {
  await import("../src/extensions.js");
  await until(() => $("notice").hasAttribute("data-ui-mounted"), "the plugin's island to mount");
  await settle();
  assert.equal(g.noticeSealedAtLoad, true, "the body was not sealed when the script was imported");
  assert.equal($("notice").hasAttribute("data-ui-failed"), false);
  assert.equal($("notice-drawn").hasAttribute("data-ui-click"), false);
  assert.equal($("notice-planted").hasAttribute("data-ui-click"), false);
  assert.equal(g.noticeMount.body, $("notice-body"));
  // The app's island was mounted once, and stays as it was.
  assert.equal(g.clockMounts, 1);
  assert.equal($("clock-body").hasAttribute("data-ui-stored"), false);
});
