// A page that starts clean (it carries `data-ui-fresh-page`) imports the app's own island scripts and
// no plugin's: a script named under a namespace is left alone there, and its island keeps what the
// server drew. A script cannot load here, so an address that is tried marks its island `data-ui-failed`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { install, settle } from "./fake-dom.mjs";

install({
  url: "https://app.test/sign-in",
  html: `<!doctype html><html data-build="b1" data-ui-fresh-page><head></head><body>
    <div id="own" data-ui-island="/_navree/ui/island/clock-0123456789ab.js" data-ui-props="{}"></div>
    <div id="plugins" data-ui-island="/_navree/ui/island/announcements.clock-0123456789ab.js" data-ui-props="{}">
      <div data-slot="island-body" data-ui-keep><p id="drawn">Ludhiana, 9:30 IST</p></div>
    </div>
  </body></html>`,
});

await import("../src/islands.js");
await settle();

const $ = (/** @type {string} */ id) => /** @type {any} */ (document.getElementById(id));

test("the app's own island script is imported on a page that starts clean", () => {
  assert.equal($("own").hasAttribute("data-ui-failed"), true);
});

test("a plugin's island script is not imported on a page that starts clean", () => {
  assert.equal($("plugins").hasAttribute("data-ui-failed"), false);
  assert.equal($("plugins").hasAttribute("data-ui-mounted"), false);
  assert.equal($("drawn").textContent, "Ludhiana, 9:30 IST");
});

test("a plugin's island that arrives on a page that starts clean is left alone too", async () => {
  const late = document.createElement("div");
  late.id = "late";
  late.setAttribute("data-ui-island", "/_navree/ui/island/notice_board.depot-map-0123456789ab.js");
  late.setAttribute("data-ui-props", "{}");
  document.body.append(late);
  document.dispatchEvent(new Event("ui:swap"));
  await settle();
  assert.equal(late.hasAttribute("data-ui-failed"), false);
  assert.equal(late.hasAttribute("data-ui-mounted"), false);
});
