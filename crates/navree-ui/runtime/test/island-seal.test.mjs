// A plugin's island (its script named under a namespace) has its body sealed before its script is imported, as a
// plugin component's owned body is: the islands module asks the extensions module, which the page loads ahead of
// it, and imports the script only once the body is sealed. What the script, or a library it bundles, writes there
// drives no hook, joins no form and mounts no component. An app's own island is not sealed, and works as it did.
import { test } from "node:test";
import assert from "node:assert/strict";
import { register } from "node:module";
import { install, settle } from "./fake-dom.mjs";

// `https://app.test/_navree/ui/island/NAME.js` is `fixtures/island/NAME.mjs`; any other address fails to load.
register("./ext-loader.mjs", import.meta.url);

const ISLAND = "/_navree/ui/island";
install({
  url: "https://app.test/orders/new",
  observe: true,
  html: `<!doctype html><html data-build="b1"><head></head><body data-ui-boost>
    <main id="main">
      <form id="order" data-ui-form method="post" action="/orders">
        <input id="charge" name="charge" value="40">
        <div id="notice" data-slot="island" data-ui-island="${ISLAND}/proof.notice-0123456789ab.js" data-ui-keep-attrs="data-ui-mounted data-ui-failed" data-ui-props='{"city":"Ludhiana"}'>
          <div id="notice-body" data-slot="island-body" data-ui-keep><button id="notice-drawn" type="button" data-ui-click='[["toggle","menu"]]'>Deliveries in Ludhiana</button></div>
        </div>
        <div id="clock" data-slot="island" data-ui-island="${ISLAND}/clock-0123456789ab.js" data-ui-keep-attrs="data-ui-mounted data-ui-failed" data-ui-props='{"zone":"IST"}'>
          <div id="clock-body" data-slot="island-body" data-ui-keep><button id="clock-drawn" type="button" data-ui-click='[["toggle","clock-menu"]]'>10:30 IST</button></div>
        </div>
      </form>
      <div id="bare" data-slot="island" data-ui-island="${ISLAND}/proof.bare-0123456789ab.js" data-ui-props="{}"></div>
      <div id="menu" hidden>Zones</div>
      <div id="clock-menu" hidden>Zones</div>
    </main>
  </body></html>`,
});

await import("../src/core.js");
// As the server links them on a page with a plugin's island: the extensions module ahead of the islands module.
await import("../src/extensions.js");
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
/** The names of the runtime's hooks on what `node` holds. @param {any} node */
const hooks = (node) => [...node.querySelectorAll("*")].flatMap((el) => el.attributes.map((/** @type {any} */ a) => a.name)).filter((name) => name.startsWith("data-ui-"));

await until(() => $("notice").hasAttribute("data-ui-mounted") && $("clock").hasAttribute("data-ui-mounted"), "both islands to mount");
// What the scripts wrote is in the page: the seal's observer has had its turn.
await settle();

test("a plugin's island body is sealed before its script is imported", () => {
  assert.equal(g.noticeSealedAtLoad, true, "the body was not sealed when the script was imported");
  assert.equal($("notice-body").hasAttribute("data-ui-stored"), true);
  // The script mounts in its body, with its props.
  assert.equal(g.noticeMount.body, $("notice-body"));
  assert.equal(g.noticeMount.island, $("notice"));
  assert.deepEqual(g.noticeMount.props, { city: "Ludhiana" });
  // What the server drew there before is no hook either.
  assert.equal($("notice-drawn").hasAttribute("data-ui-click"), false);
  $("notice-drawn").click();
  assert.equal($("menu").hasAttribute("hidden"), true, "a command the server drew in a sealed island body ran");
});

test("a hook a plugin's island script writes in its body does nothing", () => {
  assert.ok($("notice-planted"), "the script did not write into its body");
  assert.deepEqual(hooks($("notice-body")), []);
  $("notice-planted").click();
  assert.equal($("menu").hasAttribute("hidden"), true, "a command written into a sealed island body ran");
});

test("a field a plugin's island script writes in its body joins no form", () => {
  assert.equal($("notice-field").getAttribute("form"), "ui-sealed");
  assert.equal($("notice-field").form, null);
  assert.equal($("notice-drawn").form, null);
  // The app island's field is the form's, as the server's own is.
  assert.deepEqual([...new FormData($("order"))], [["charge", "40"], ["zone", "Asia/Kolkata"]]);
});

test("a component a plugin's island script writes in its body is not mounted", async () => {
  document.dispatchEvent(new Event("ui:swap"));
  // Long enough for a script the extensions module asked for to come through the loader.
  await new Promise((go) => setTimeout(go, 100));
  await settle();
  assert.equal($("notice-ext").hasAttribute("data-ui-ext"), false);
  assert.equal($("notice-ext").hasAttribute("data-ui-mounted"), false);
  assert.equal($("notice-ext").hasAttribute("data-ui-failed"), false);
  assert.equal(g.seen, undefined, "a component written into a sealed island body was mounted");
});

test("an app's own island is not sealed and works as before", () => {
  assert.equal($("clock-body").hasAttribute("data-ui-stored"), false);
  assert.equal(g.clockMount.body, $("clock-body"));
  assert.deepEqual(g.clockMount.props, { zone: "IST" });
  // What the server drew and what the script wrote keep their hooks and act.
  assert.equal($("clock-drawn").hasAttribute("data-ui-click"), true);
  assert.equal($("clock-zone").hasAttribute("form"), false);
  $("clock-planted").click();
  assert.equal($("clock-menu").hasAttribute("hidden"), false, "a command the app island's script wrote did not run");
  $("clock-drawn").click();
  assert.equal($("clock-menu").hasAttribute("hidden"), true, "a command the server drew in an app island did not run");
});

test("a plugin's island with no body to seal is not imported and is marked failed", () => {
  assert.equal(g.bareLoaded, undefined, "the script of a plugin's island with no body was imported");
  assert.equal($("bare").hasAttribute("data-ui-failed"), true);
  assert.equal($("bare").hasAttribute("data-ui-mounted"), false);
  assert.equal($("bare").hasAttribute("data-ui-stored"), false);
});

test("a plugin's island with new props and no update is mounted again in its sealed body", async () => {
  const first = g.noticeMount;
  $("notice").setAttribute("data-ui-props", '{"city":"Jalandhar"}');
  document.dispatchEvent(new Event("ui:swap"));
  await until(() => g.noticeMount !== first && $("notice").hasAttribute("data-ui-mounted"), "the island to mount again");
  await settle();
  assert.deepEqual(g.noticeMount.props, { city: "Jalandhar" });
  assert.equal(g.noticeMount.body, $("notice-body"));
  assert.equal($("notice-body").hasAttribute("data-ui-stored"), true);
  assert.equal($("notice").hasAttribute("data-ui-failed"), false);
  assert.deepEqual(hooks($("notice-body")), []);
});

test("the extensions module seals only an island's body when asked", () => {
  // An ask sent to any other element is not answered, and seals nothing.
  for (const id of ["main", "clock", "charge"]) {
    assert.equal($(id).dispatchEvent(new Event("ui:seal", { cancelable: true })), true, id);
    assert.equal($(id).hasAttribute("data-ui-stored"), false, id);
  }
  assert.equal($("clock-drawn").hasAttribute("data-ui-click"), true);
});
