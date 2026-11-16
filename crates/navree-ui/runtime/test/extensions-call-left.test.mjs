// A call a component's script begins once its element has left the page is not sent: nothing goes out with the
// session's token, and it takes no turn of the page's, so a navigation on its way is shown. A call begun while the
// element is in the page is sent as before, and overtakes a navigation on its way.
import { test } from "node:test";
import assert from "node:assert/strict";
import { register } from "node:module";
import { answer, install, settle } from "./fake-dom.mjs";

// `https://app.test/_navree/ui/ext/NAME.js` is `fixtures/ext/NAME.mjs`; any other address fails to load.
register("./ext-loader.mjs", import.meta.url);

const EXT = "/_navree/ui/ext";
/** A component of a plugin's whose script may call the routes under `/proof/`. @param {string} id */
const pad = (id) =>
  `<div id="${id}" data-slot="proof:pad" data-ui-ext="${EXT}/proof.pad-0123456789ab.js" data-ui-ext-routes="/proof/" data-ui-keep-attrs="data-ui-mounted data-ui-failed" data-ui-props="{}"></div>`;
const page = install({
  url: "https://app.test/deliveries",
  cookie: "XSRF-TOKEN=tok",
  html: `<!doctype html><html data-build="b1"><head></head><body data-ui-boost>
    <main id="main">Deliveries
      <a id="one" href="/deliveries/1">1</a>
      ${pad("left")}
      ${pad("here")}
    </main>
  </body></html>`,
});

await import("../src/core.js");
await import("../src/extensions.js");

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
const toasts = () => [...document.querySelectorAll("[data-slot=toast-text]")].map((p) => p.textContent);
const PAGE = { "content-type": "text/html; charset=utf-8", "x-navree-page": "1" };
const NOTHING = { status: 204, headers: {} };
/** A page of the app with `main` in it, a link to the second delivery, and the component that stays. @param {string} main */
const html = (main) =>
  `<!doctype html><html data-build="b1"><head><title>${main}</title></head><body data-ui-boost><main id="main">${main} <a id="two" href="/deliveries/2">2</a>${pad("here")}</main></body></html>`;
/** An answer the test lets go of when it wants. */
function held() {
  /** @type {(value: any) => void} */
  let release = () => {};
  const promise = new Promise((done) => (release = done));
  return { promise, release };
}

await until(() => ["left", "here"].every((id) => $(id).hasAttribute("data-ui-mounted")), "the scripts to mount");

test("a ui.call begun once its element has left is not sent, and a navigation on its way is shown", async () => {
  const ui = g.uis.left;
  $("left").remove();
  document.dispatchEvent(new Event("ui:swap"));
  assert.equal(ui.signal.aborted, true);
  // The person follows a link, and its page is on its way.
  const slow = held();
  page.fetched.length = 0;
  page.pushed.length = 0;
  page.answers.push(() => slow.promise);
  $("one").click();
  await settle(2);
  // Something the script waited for comes now, and it calls.
  const before = toasts().length;
  assert.deepEqual(await ui.call("/proof/save", { at: "30.9010,75.8573" }), { ok: false, status: 0, data: null });
  slow.release(answer(html("Delivery 1"), { headers: PAGE }));
  await settle();
  assert.deepEqual(page.fetched.map((f) => new URL(f.url).pathname), ["/deliveries/1"], "a call begun after its element left was sent");
  assert.equal($("main").textContent.includes("Delivery 1"), true, "the navigation on its way was not shown");
  assert.deepEqual(page.pushed, ["https://app.test/deliveries/1"]);
  assert.deepEqual(page.assigned, []);
  assert.deepEqual(toasts().slice(before), [], "a call that was not sent showed a toast");
});

test("a ui.call begun while its element is in the page is sent, and overtakes a navigation on its way", async () => {
  // The component that stayed through the navigation of the test before this one.
  const ui = g.uis.here;
  assert.equal(ui.signal.aborted, false);
  const slow = held();
  page.fetched.length = 0;
  page.pushed.length = 0;
  page.answers.push(() => slow.promise, answer("", NOTHING));
  $("two").click();
  await settle(2);
  assert.deepEqual(await ui.call("/proof/save", { at: "30.9010,75.8573" }), { ok: true, status: 204, data: null });
  slow.release(answer(html("Delivery 2"), { headers: PAGE }));
  await settle();
  assert.deepEqual(page.fetched.map((f) => new URL(f.url).pathname), ["/deliveries/2", "/proof/save"]);
  assert.equal(page.fetched[1].init.headers["x-xsrf-token"], "tok");
  assert.equal($("main").textContent.includes("Delivery 2"), false, "a navigation the call overtook was shown");
  assert.deepEqual(page.pushed, []);
  assert.deepEqual(page.assigned, []);
});
