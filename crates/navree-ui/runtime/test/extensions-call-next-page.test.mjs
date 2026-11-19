// A component that stands on both pages of a boosted navigation (the same id and script, as a map control
// on two delivery pages has) keeps its element and its signal. Even so, the answer of a `ui.call` it began on
// the page before the navigation is not applied to the page that came: no ops run, no JSON or HTML answer is
// taken, and no toast is shown. Calls begun on the new page work as before.
import { test } from "node:test";
import assert from "node:assert/strict";
import { register } from "node:module";
import { answer, install, settle } from "./fake-dom.mjs";

register("./ext-loader.mjs", import.meta.url);

const EXT = "/_navree/ui/ext";
const PAD = `<div id="here" data-slot="proof:pad" data-ui-ext="${EXT}/proof.pad-0123456789ab.js" data-ui-ext-routes="/proof/" data-ui-keep-attrs="data-ui-mounted data-ui-failed" data-ui-props='{"step":1}'></div>`;
const page = install({
  url: "https://app.test/deliveries/1",
  cookie: "XSRF-TOKEN=tok",
  html: `<!doctype html><html data-build="b1"><head></head><body data-ui-boost>
    <main id="main"><h1 id="heading">Delivery 1</h1><p id="place">Ludhiana</p><a id="two" href="/deliveries/2">2</a>${PAD}</main>
  </body></html>`,
});

const written = /** @type {PropertyDescriptor} */ (Object.getOwnPropertyDescriptor(Element.prototype, "innerHTML"));
Object.defineProperty(HTMLTemplateElement.prototype, "innerHTML", {
  configurable: true,
  get: written.get,
  set(/** @type {string} */ text) {
    const holder = document.createElement("div");
    written.set?.call(holder, text);
    /** @type {any} */ (this).content.replaceChildren(...holder.childNodes);
  },
});

await import("../src/core.js");
await import("../src/extensions.js");

const g = /** @type {any} */ (globalThis);
const $ = (/** @type {string} */ id) => /** @type {any} */ (document.getElementById(id));
/** @param {() => boolean} done @param {string} what */
async function until(done, what) {
  for (let i = 0; i < 400; i++) {
    if (done()) return;
    await new Promise((go) => setTimeout(go, 5));
  }
  assert.fail(`still waiting for ${what}`);
}
const toasts = () => [...document.querySelectorAll("[data-slot=toast-text]")].map((p) => p.textContent);
const OPS = { "content-type": "application/vnd.navree.ops+json" };
const PAGE = { "content-type": "text/html; charset=utf-8", "x-navree-page": "1" };

/** An answer the test lets go of when it wants. */
function held() {
  /** @type {(value: any) => void} */
  let release = () => {};
  const promise = new Promise((done) => (release = done));
  return { promise, release };
}

await until(() => $("here").hasAttribute("data-ui-mounted"), "the script to mount");

test("the answer of a call whose element is still there after a navigation is not applied to the page that came", async () => {
  const before = $("here");
  const ui = g.uis.here;
  const opsSlow = held(), jsonSlow = held(), pageSlow = held(), failSlow = held();
  page.answers.push(() => opsSlow.promise, () => jsonSlow.promise, () => pageSlow.promise, () => failSlow.promise);

  // Four calls begin on Delivery 1 while their answers are on their way.
  const opsCall = ui.call("/proof/save", { at: "30.8890,75.8340" });
  const jsonCall = ui.call("/proof/total", {});
  const pageCall = ui.call("/proof/pay", {}, { page: true });
  const failCall = ui.call("/proof/fail", {});
  await settle(2);

  // The person navigates to Delivery 2, where the same element stands and stays mounted.
  page.answers.push(
    answer(
      `<!doctype html><html data-build="b1"><head><title>Delivery 2</title></head><body data-ui-boost><main id="main"><h1 id="heading">Delivery 2</h1><p id="place">Jalandhar</p>${PAD}</main></body></html>`,
      { headers: PAGE },
    ),
  );
  $("two").click();
  await settle();
  assert.equal($("heading").textContent, "Delivery 2");
  assert.equal($("place").textContent, "Jalandhar");
  assert.equal($("here"), before, "the morph kept the component's element");
  assert.equal(ui.signal.aborted, false, "the kept element's signal is still live");

  // Delivery 1's answers arrive now: none of them may touch Delivery 2 or raise a toast on it.
  opsSlow.release(
    answer(`{"ops":[{"op":"morph","target":"place","html":"<p id=\\"place\\">Ludhiana, after save</p>"},{"op":"toast","tone":"success","text":"Saved for Harpreet Sandhu"}]}`, {
      headers: OPS,
    }),
  );
  jsonSlow.release(answer(`{"total":"₹40"}`, { headers: { "content-type": "application/json" } }));
  pageSlow.release(answer(`<!doctype html><html data-build="b1"><head><title>Receipt</title></head><body>Paid</body></html>`, { headers: PAGE, url: "https://app.test/proof/receipt" }));
  failSlow.release(answer("broken", { status: 500, headers: { "content-type": "text/plain" } }));

  assert.deepEqual(await opsCall, { ok: false, status: 200, data: null });
  assert.deepEqual(await jsonCall, { ok: false, status: 200, data: null });
  assert.deepEqual(await pageCall, { ok: false, status: 200, data: null });
  assert.deepEqual(await failCall, { ok: false, status: 500, data: null });
  await settle();
  assert.equal($("place").textContent, "Jalandhar", "ops from the previous page morphed the new page");
  assert.deepEqual(page.assigned, [], "a page answer from the previous page left the new page");
  assert.deepEqual(toasts(), [], "a call from the previous page showed a toast on the new page");

  // A call begun on Delivery 2 itself applies as usual.
  page.answers.push(answer(`{"ops":[{"op":"morph","target":"place","html":"<p id=\\"place\\">Jalandhar, saved</p>"}]}`, { headers: OPS }));
  assert.deepEqual(await ui.call("/proof/save", { at: "31.3260,75.5762" }), { ok: true, status: 200, data: null });
  assert.equal($("place").textContent, "Jalandhar, saved");
});

test("a call begun while the next page waits on its stylesheets is not applied once that page shows", async () => {
  const before = $("here");
  const ui = g.uis.here;

  // A form post redirects to Delivery 3, whose head links a theme sheet not yet in the page.
  const formWrap = document.createElement("div");
  formWrap.innerHTML = `<form id="next-delivery" data-ui-form method="post" action="/deliveries/next"><button id="next-go" type="submit">Next</button></form>`;
  $("main").append(...formWrap.childNodes);

  const sheetUrl = "/_navree/ui/theme/amber-0123456789ab.css";
  page.answers.push(
    answer(
      `<!doctype html><html data-build="b1"><head><title>Delivery 3</title><link rel="stylesheet" href="${sheetUrl}"></head><body data-ui-boost><main id="main"><h1 id="heading">Delivery 3</h1><p id="place">Amritsar</p>${PAD}</main></body></html>`,
      { headers: PAGE, url: "https://app.test/deliveries/3" },
    ),
  );
  $("next-go").click();
  await until(() => !!document.head.querySelector(`link[href="${sheetUrl}"]`), "the new page's stylesheet link");
  // Delivery 2 is still on screen while the stylesheet loads.
  assert.equal($("heading").textContent, "Delivery 2");

  // While Delivery 3 waits on its stylesheet, the component on Delivery 2 starts a call.
  const slowSave = held();
  page.answers.push(() => slowSave.promise);
  const duringWait = ui.call("/proof/save", { at: "31.3260,75.5762" });
  await settle(2);

  // The stylesheet finishes loading and Delivery 3 is swapped in, keeping the component's element.
  const link = /** @type {any} */ (document.head.querySelector(`link[href="${sheetUrl}"]`));
  link.dispatchEvent(new Event("load"));
  await settle();
  assert.equal($("heading").textContent, "Delivery 3");
  assert.equal($("place").textContent, "Amritsar");
  assert.equal($("here"), before);
  assert.equal(ui.signal.aborted, false);

  // Delivery 2's call resolves after Delivery 3 has swapped in: nothing of it touches Delivery 3.
  slowSave.release(
    answer(`{"ops":[{"op":"morph","target":"place","html":"<p id=\\"place\\">Jalandhar, late</p>"},{"op":"toast","tone":"success","text":"Saved for Simran Kaur"}]}`, {
      headers: OPS,
    }),
  );
  assert.deepEqual(await duringWait, { ok: false, status: 200, data: null });
  await settle();
  assert.equal($("place").textContent, "Amritsar");
  assert.deepEqual(toasts(), []);
});
