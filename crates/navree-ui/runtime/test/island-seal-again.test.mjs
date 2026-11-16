// A plugin's island is sealed when it mounts, and stays mounted through a morph. A morph can still put another body
// in place of the island's body: a library gave the body it draws in an id, so the morph takes the server's body,
// which has none, for another element. The extensions module seals the body such an island has at each scan, as it
// does a component's bodies, so what is written in the new body drives no hook and joins no form. An app's own
// island is never sealed, and a plugin's island that is not mounted yet is left to the islands module's ask.
import { test } from "node:test";
import assert from "node:assert/strict";
import { register } from "node:module";
import { install, settle } from "./fake-dom.mjs";

// `https://app.test/_navree/ui/island/NAME.js` is `fixtures/island/NAME.mjs`; any other address fails to load.
register("./ext-loader.mjs", import.meta.url);

const ISLAND = "/_navree/ui/island";
/**
 * An island as the server draws it, with a command in its fallback: its body carries no id.
 * @param {string} id @param {string} script @param {string} props @param {string} menu @param {string} text
 */
const island = (id, script, props, menu, text) =>
  `<div id="${id}" data-slot="island" data-ui-island="${ISLAND}/${script}-0123456789ab.js" data-ui-keep-attrs="data-ui-mounted data-ui-failed" data-ui-props='${props}'>` +
  `<div data-slot="island-body" data-ui-keep><button type="button" data-ui-click='[["toggle","${menu}"]]'>${text}</button></div></div>`;
const NOTICE = island("notice", "proof.notice", `{"city":"Ludhiana"}`, "menu", "Deliveries in Ludhiana");
const CLOCK = island("clock", "clock", `{"zone":"IST"}`, "clock-menu", "10:30 IST");

install({
  url: "https://app.test/orders/new",
  observe: true,
  html: `<!doctype html><html data-build="b1"><head></head><body data-ui-boost>
    <main id="main">
      <form id="order" data-ui-form method="post" action="/orders">
        <input id="charge" name="charge" value="40">
        ${NOTICE}
        ${CLOCK}
      </form>
      <div id="menu" hidden>Zones</div>
      <div id="clock-menu" hidden>Zones</div>
    </main>
  </body></html>`,
});

const { morph } = await import("../src/morph.js");
await import("../src/core.js");
// As the server links them on a page with a plugin's island: the extensions module ahead of the islands module.
await import("../src/extensions.js");

const g = /** @type {any} */ (globalThis);
const $ = (/** @type {string} */ id) => /** @type {any} */ (document.getElementById(id));
/** The body the island `id` has now. @param {string} id */
const bodyOf = (id) => $(id).querySelector("[data-slot=island-body]");
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
/** A detached element of `html`, as a fetched page gives one. @param {string} html */
function server(html) {
  const box = document.createElement("div");
  box.innerHTML = html;
  return /** @type {any} */ (box.firstElementChild);
}

// The islands module has not come, so no island is mounted yet. The extensions module scans now, and what its scan
// leaves of the plugin's island is noted for the last test.
document.dispatchEvent(new Event("ui:swap"));
const unmounted = { stored: bodyOf("notice").hasAttribute("data-ui-stored"), hooks: hooks(bodyOf("notice")) };

await import("../src/islands.js");
await until(() => $("notice").hasAttribute("data-ui-mounted") && $("clock").hasAttribute("data-ui-mounted"), "both islands to mount");
// What the scripts wrote is in the page: the seal's observer has had its turn.
await settle();

test("a body a morph puts in place of a mounted plugin island's body is sealed at the next scan", async () => {
  const mount = g.noticeMount, old = mount.body;
  assert.equal(old.hasAttribute("data-ui-stored"), true);
  // A library gave the body it draws in an id of its own. The server draws the island again: its body has none,
  // so the morph puts the new body in and takes the old one out, and the island stays mounted.
  old.setAttribute("id", "notice-drawing");
  morph($("notice"), server(NOTICE));
  const body = bodyOf("notice");
  assert.notEqual(body, old);
  assert.equal(old.isConnected, false);
  assert.equal($("notice").hasAttribute("data-ui-mounted"), true);
  assert.equal(g.noticeMount, mount, "the island was mounted again");
  // The scan that follows the morph sealed the new body: what the server drew in it is no hook.
  assert.equal(body.hasAttribute("data-ui-stored"), true, "the body put in place of the island's body was not sealed");
  assert.deepEqual(hooks(body), []);
  body.querySelector("button").click();
  assert.equal($("menu").hasAttribute("hidden"), true, "a command the server drew in the new body ran");
  // The script draws again where it finds its body.
  const part = document.createElement("div");
  part.innerHTML = `<button id="notice-again" type="button" data-ui-click='[["toggle","menu"]]'>Open</button><input id="notice-again-field" name="charge" value="0">`;
  body.append(part);
  await settle();
  assert.deepEqual(hooks(body), []);
  $("notice-again").click();
  assert.equal($("menu").hasAttribute("hidden"), true, "a command written into the new body ran");
  assert.equal($("notice-again-field").getAttribute("form"), "ui-sealed");
  assert.equal($("notice-again-field").form, null);
  // The app island's field is the form's, as the server's own is.
  assert.deepEqual([...new FormData($("order"))], [["charge", "40"], ["zone", "Asia/Kolkata"]]);
});

test("a body a morph puts in place of an app island's body is not sealed", async () => {
  const mount = g.clockMount, old = mount.body;
  old.setAttribute("id", "clock-drawing");
  morph($("clock"), server(CLOCK));
  const body = bodyOf("clock");
  assert.notEqual(body, old);
  assert.equal(old.isConnected, false);
  assert.equal($("clock").hasAttribute("data-ui-mounted"), true);
  assert.equal(g.clockMount, mount, "the island was mounted again");
  assert.equal(body.hasAttribute("data-ui-stored"), false, "an app's own island body was sealed");
  // What the server drew there keeps its hook and acts, and so does what the script writes; its field posts.
  assert.deepEqual(hooks(body), ["data-ui-click"]);
  body.querySelector("button").click();
  assert.equal($("clock-menu").hasAttribute("hidden"), false, "a command the server drew in an app island did not run");
  const part = document.createElement("div");
  part.innerHTML = `<button id="clock-again" type="button" data-ui-click='[["toggle","clock-menu"]]'>Zones</button><input id="clock-again-zone" name="zone" value="Asia/Kolkata">`;
  body.append(part);
  await settle();
  assert.deepEqual(hooks(body), ["data-ui-click", "data-ui-click"]);
  $("clock-again").click();
  assert.equal($("clock-menu").hasAttribute("hidden"), true, "a command the app island's script wrote did not run");
  assert.equal($("clock-again-zone").hasAttribute("form"), false);
  assert.equal($("clock-again-zone").form, $("order"));
  assert.deepEqual([...new FormData($("order"))], [["charge", "40"], ["zone", "Asia/Kolkata"]]);
});

test("a copy a plugin's island script puts in place of its body is sealed at the next scan", async () => {
  const body = bodyOf("notice"), copy = body.cloneNode(true);
  body.replaceWith(copy);
  // The copy carries the mark, and nothing watches what is written in it until the next scan.
  assert.equal(copy.hasAttribute("data-ui-stored"), true);
  document.dispatchEvent(new Event("ui:swap"));
  const field = document.createElement("input");
  field.setAttribute("name", "charge");
  field.setAttribute("data-ui-change", '[["toggle","menu"]]');
  copy.append(field);
  await settle();
  assert.deepEqual(hooks(copy), []);
  assert.equal(field.getAttribute("form"), "ui-sealed");
  assert.equal(field.form, null);
});

test("a plugin's island that is not mounted yet is left as the server drew it by a scan", () => {
  // Noted before the islands module came: the scan sealed nothing, and the fallback kept its command.
  assert.deepEqual(unmounted, { stored: false, hooks: ["data-ui-click"] });
});
