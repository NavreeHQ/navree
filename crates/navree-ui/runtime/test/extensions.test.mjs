// The extensions module: each element's script is mounted once, with its props, and only from this
// site's extension route. Its bodies are sealed before the script is imported, so nothing a script or a
// library it bundles writes there drives the runtime, mounts anything or joins the form around it. What
// it calls reaches only the routes the server wrote on its element, through core's write path. The
// stylesheets the page's elements name are linked once, and dropped when no element names them.
import { test } from "node:test";
import assert from "node:assert/strict";
import { register } from "node:module";
import { answer, fire, install, settle } from "./fake-dom.mjs";

// `https://app.test/_navree/ui/ext/NAME.js` is `fixtures/ext/NAME.mjs`; any other address fails to load.
register("./ext-loader.mjs", import.meta.url);

const EXT = "/_navree/ui/ext";
const page = install({
  url: "https://app.test/deliveries",
  cookie: "XSRF-TOKEN=tok",
  observe: true,
  html: `<!doctype html><html data-build="b1"><head><link rel="stylesheet" href="${EXT}/proof.pad-0123456789ab.css" data-ui-ext-link></head><body data-ui-boost>
    <main id="main">
      <form id="new-delivery" data-ui-form method="post" action="/deliveries">
        <div id="vendor" data-slot="proof:vendor" data-ui-ext="${EXT}/proof.vendor-0123456789ab.js" data-ui-ext-routes="/proof/" data-ui-props="{}">
          <div id="vendor-body" data-slot="proof:vendor-body" data-ui-keep></div>
        </div>
        <input id="charge" name="charge" value="40">
      </form>
      <div id="pad" data-slot="proof:pad" data-ui-ext="${EXT}/proof.pad-0123456789ab.js" data-ui-ext-sheet="${EXT}/proof.pad-0123456789ab.css"
           data-ui-ext-routes="/proof/ =/deliveries/42/drop" data-ui-props='{"step":1,"next":"/account/email"}'>
        <div data-ui-stored><p id="pad-stored" data-slot="proof:line">A note Meera Sharma saved</p></div>
        <div id="pad-body" data-slot="proof:pad-body" data-ui-keep>
          <div id="nested" data-slot="proof:pad" data-ui-ext="${EXT}/proof.pad-0123456789ab.js" data-ui-props="{}"></div>
        </div>
      </div>
      <div id="gone" data-slot="proof:pad" data-ui-ext="${EXT}/proof.pad-0123456789ab.js" data-ui-props='{"step":1}'><div data-slot="proof:pad-body" data-ui-keep></div></div>
      <div id="timers" data-slot="proof:pad" data-ui-ext="${EXT}/proof.pad-0123456789ab.js" data-ui-props='{"step":1}'><div data-slot="proof:pad-body" data-ui-keep></div></div>
      <div id="late" data-slot="proof:late" data-ui-ext="${EXT}/proof.late-0123456789ab.js" data-ui-props="{}">
        <div id="late-body" data-slot="proof:late-body" data-ui-keep><button id="late-button" data-ui-click='[["toggle","menu"]]'>Show the zones</button></div>
      </div>
      <div id="two" data-slot="proof:two" data-ui-ext="${EXT}/proof.two-0123456789ab.js" data-ui-props="{}"></div>
      <div id="grow" data-slot="proof:pad" data-ui-ext="${EXT}/proof.pad-0123456789ab.js" data-ui-props='{"step":1}'></div>
      <div id="slow" data-slot="proof:slow" data-ui-ext="${EXT}/proof.slow-0123456789ab.js" data-ui-props="{}">
        <div id="slow-body" data-slot="proof:slow-body" data-ui-keep></div>
      </div>
      <div id="off-site" data-ui-ext="https://cdn.example${EXT}/proof.pad-0123456789ab.js" data-ui-props="{}"></div>
      <div id="island-route" data-ui-ext="/_navree/ui/island/proof.pad-0123456789ab.js" data-ui-props="{}"></div>
      <div id="with-query" data-ui-ext="${EXT}/proof.pad-0123456789ab.js?v=2" data-ui-props="{}"></div>
      <div data-ui-stored>
        <div id="stored-copy" data-slot="proof:pad" data-ui-ext="${EXT}/proof.pad-0123456789ab.js" data-ui-ext-sheet="${EXT}/proof.copy-0123456789ab.css" data-ui-props="{}"></div>
      </div>
      <div id="badges">
        <span id="badge-1" data-slot="proof:badge" data-ui-ext-sheet="${EXT}/proof.badge-0123456789ab.css">Gurpreet Singh</span>
        <span id="badge-2" data-slot="proof:badge" data-ui-ext-sheet="${EXT}/proof.badge-0123456789ab.css">Kuldeep Gill</span>
        <span id="badge-off" data-slot="proof:badge" data-ui-ext-sheet="https://cdn.example${EXT}/proof.badge-0123456789ab.css">Simran Kaur</span>
      </div>
      <div id="menu" hidden>Zones</div>
      <div id="zones" data-ui-region data-ui-src="/zones"><p id="zones-text">Model Town</p></div>
    </main>
  </body></html>`,
});

/** What the console was told: the module's diagnostics. @type {string[]} */
const told = [];
console.error = (/** @type {any[]} */ ...args) => void told.push(args.join(" "));

const g = /** @type {any} */ (globalThis);
// The slow component's script stays on its way until a test lets it come.
/** @type {() => void} */
let letSlowCome = () => {};
g.slowGate = new Promise((/** @type {any} */ go) => (letSlowCome = go));

await import("../src/core.js");
await import("../src/extensions.js");

const $ = (/** @type {string} */ id) => /** @type {any} */ (document.getElementById(id));
/** Waits for `done`: a script comes through the module loader in its own time. @param {() => boolean} done @param {string} what */
async function until(done, what) {
  for (let i = 0; i < 400; i++) {
    if (done()) return;
    await new Promise((go) => setTimeout(go, 5));
  }
  assert.fail(`still waiting for ${what}`);
}
/** The calls the script of the element `id` got, in order. @param {string} id */
const calls = (id) => (g.seen || []).filter((/** @type {any} */ s) => s.id === id).map((/** @type {any} */ s) => s.call);
/** The props of the last call that brought some. @param {string} id */
const lastProps = (id) => (g.seen || []).filter((/** @type {any} */ s) => s.id === id && s.props).at(-1)?.props;
const swap = () => document.dispatchEvent(new Event("ui:swap"));
/** A morph is about to change `from`: the page's body for a page swap. @param {any} from */
const beforeSwap = (from) => document.dispatchEvent(new CustomEvent("ui:before-swap", { detail: from }));
/** The names of the runtime's hooks on what `node` holds. @param {any} node */
const hooks = (node) => [...node.querySelectorAll("*")].flatMap((el) => el.attributes.map((/** @type {any} */ a) => a.name)).filter((name) => name.startsWith("data-ui-"));
const toasts = () => [...document.querySelectorAll("[data-slot=toast-text]")].map((p) => p.textContent);
const sheets = () => [...document.head.querySelectorAll("link[data-ui-ext-link]")].map((l) => l.getAttribute("href"));
const OPS = { "content-type": "application/vnd.navree.ops+json" };
const PAGE = { "content-type": "text/html; charset=utf-8", "x-navree-page": "1" };
const NOTHING = { status: 204, headers: {} };
const REFUSED = { ok: false, status: 0, data: null };

await until(() => ["pad", "gone", "timers", "late", "vendor", "grow"].every((id) => $(id).hasAttribute("data-ui-mounted")) && $("two").hasAttribute("data-ui-failed"), "the scripts to mount");
// The library in the vendor's component works on a timer.
await settle();

test("mounts each element once and gives new props to update", () => {
  const mounts = g.seen.filter((/** @type {any} */ s) => s.id === "pad" && s.call === "mount");
  assert.equal(mounts.length, 1);
  assert.deepEqual(mounts[0].props, { step: 1, next: "/account/email" });
  assert.equal($("pad").hasAttribute("data-ui-mounted"), true);

  $("pad").setAttribute("data-ui-props", '{"step":2}');
  swap();
  assert.deepEqual(calls("pad"), ["mount", "update"]);
  assert.deepEqual(lastProps("pad"), { step: 2 });
  // The same props, and no morph that reaches it: nothing more.
  swap();
  assert.deepEqual(calls("pad"), ["mount", "update"]);
  assert.equal($("pad").hasAttribute("data-ui-mounted"), true);
});

test("calls update after a morph that reaches the element", () => {
  // A morph of a region that doesn't hold it gives it nothing.
  beforeSwap($("zones"));
  swap();
  assert.deepEqual(calls("pad"), ["mount", "update"]);
  // A morph of the page holds it: its props come again, the same as they were.
  beforeSwap(document.body);
  swap();
  assert.deepEqual(calls("pad"), ["mount", "update", "update"]);
  assert.deepEqual(lastProps("pad"), { step: 2 });
});

test("leaves an element that is gone or names another script", async () => {
  const first = g.uis.gone;
  // The page swap of the test before gave it an update: what counts is what comes from here.
  const before = calls("gone").length;
  const since = () => calls("gone").slice(before);
  // The same component, built again: another hash.
  $("gone").setAttribute("data-ui-ext", `${EXT}/proof.pad-fedcba987654.js`);
  swap();
  assert.equal(first.signal.aborted, true);
  await until(() => since().length === 2 && $("gone").hasAttribute("data-ui-mounted"), "the new script to mount");
  assert.deepEqual(since(), ["leave", "mount"]);
  const second = g.uis.gone;
  assert.notEqual(second, first);

  $("gone").remove();
  swap();
  assert.deepEqual(since(), ["leave", "mount", "leave"]);
  assert.equal(second.signal.aborted, true);
});

test("does not mount an element inside stored content or a sealed body", () => {
  assert.deepEqual(calls("stored-copy"), []);
  assert.deepEqual(calls("nested"), []);
  for (const id of ["stored-copy", "nested"]) {
    assert.equal($(id).hasAttribute("data-ui-mounted"), false, id);
    assert.equal($(id).hasAttribute("data-ui-failed"), false, id);
  }
  // The body it lies in was sealed before its own element's script came: the hook went with the rest.
  assert.equal($("nested").hasAttribute("data-ui-ext"), false);
});

test("refuses an address off this site, outside /_navree/ui/ext/, or with a query", () => {
  // None of these can load here: one that was tried would be marked failed.
  for (const id of ["off-site", "island-route", "with-query"]) {
    assert.equal($(id).hasAttribute("data-ui-failed"), false, `${id} was imported`);
    assert.equal($(id).hasAttribute("data-ui-mounted"), false, id);
  }
  assert.equal(told.some((line) => /cdn\.example|island|\?v=2/.test(line)), false, told.join("\n"));
  assert.deepEqual(page.fetched, []);
});

test("does not mount a script of an unknown contract and marks it failed", () => {
  assert.equal($("two").hasAttribute("data-ui-failed"), true);
  assert.equal($("two").hasAttribute("data-ui-mounted"), false);
  assert.equal(g.mountedTwo, undefined);
  assert.ok(told.includes(`navree-ui: ${EXT}/proof.two-0123456789ab.js is written for contract 2; this page runs contract 1`), told.join("\n"));
});

test("seals an owned body before the script loads and scrubs what is there", () => {
  assert.equal(g.sealedAtLoad, true, "the body was not sealed when its script was loaded");
  assert.equal($("late-body").hasAttribute("data-ui-stored"), true);
  // What the page drew in it beforehand is no hook, and no field of a form.
  assert.equal($("late-button").hasAttribute("data-ui-click"), false);
  assert.equal($("late-button").getAttribute("form"), "ui-sealed");
  // Nor is what the script drew once what it waited for came.
  assert.ok($("late-planted"));
  assert.deepEqual(hooks($("late-body")), []);
  $("late-button").click();
  $("late-planted").click();
  assert.equal($("menu").hasAttribute("hidden"), true, "a command in a sealed body ran");
});

test("markup written into an owned body drives no hook", async () => {
  await settle();
  // The library's form, its own part (`data-ui-own`) and the command in it: no hook is left.
  assert.deepEqual(hooks($("vendor-body")), []);
  page.fetched.length = 0;
  $("planted").click();
  assert.equal($("menu").hasAttribute("hidden"), true, "a command written into a body ran");
  // The form it wrote is the browser's to send, not the runtime's: no fetch with the session's token,
  // and, its fields joining no form, nothing in it.
  const form = $("vendor-body").querySelector("form");
  HTMLFormElement.prototype.requestSubmit.call(form);
  $("vendor-send").click();
  await settle();
  assert.deepEqual(page.fetched, [], "a form written into a body was posted by the runtime");
  assert.deepEqual([...new FormData(form)], []);
  // A field it moved into the form around the component, on its timer, was taken out again.
  assert.equal($("vendor-charge").getAttribute("form"), "ui-sealed");
});

test("a field written into an owned body does not post with the surrounding form", () => {
  assert.ok($("vendor-late"), "the library's timer did not run");
  for (const field of $("vendor-body").querySelectorAll("input,button")) assert.equal(field.getAttribute("form"), "ui-sealed", field.outerHTML);
  assert.equal($("vendor-charge").getAttribute("form"), "ui-sealed");
  assert.deepEqual([...new FormData($("new-delivery"))], [["charge", "40"]]);
});

test("a form written into a body with the id the sealed fields name owns none of them", () => {
  const written = $("vendor-body").querySelector("form[id=ui-sealed]");
  assert.ok(written, "the library did not write its form");
  // The page's first element with that id is the runtime's own, and it is no form.
  assert.notEqual(document.getElementById("ui-sealed"), written);
  assert.equal(document.getElementById("ui-sealed").nodeName === "FORM", false);
  for (const id of ["vendor-search", "vendor-charge", "late-button"]) assert.equal($(id).form, null, id);
  assert.deepEqual([...new FormData(written)], []);
  page.submitted.length = 0;
  $("vendor-search").click();
  assert.deepEqual(page.submitted, [], "a written form sent the sealed fields");
});

test("a sealed body gets its mark back", async () => {
  // The library took it away on its timer.
  assert.ok($("vendor-late"), "the library's timer did not run");
  assert.equal($("vendor-body").hasAttribute("data-ui-stored"), true);
  $("vendor-body").removeAttribute("data-ui-stored");
  await settle();
  assert.equal($("vendor-body").hasAttribute("data-ui-stored"), true);
  // A hook written on the body itself goes too; the server's mark that keeps it through a morph stays.
  $("vendor-body").setAttribute("data-ui-click", '[["toggle","menu"]]');
  await settle();
  assert.equal($("vendor-body").hasAttribute("data-ui-click"), false);
  assert.equal($("vendor-body").hasAttribute("data-ui-keep"), true);
});

test("a body a morph adds to a mounted element, or puts in place of its body, is sealed and is the script's", async () => {
  const ui = g.uis.grow;
  // The element had no body; the server's morph gives it one, with a hook drawn in it.
  beforeSwap($("grow"));
  const added = document.createElement("div");
  added.innerHTML = `<div id="grow-body" data-slot="proof:pad-body" data-ui-keep><button id="grow-button" data-ui-click='[["toggle","menu"]]'>Show the zones</button></div>`;
  $("grow").append(...added.childNodes);
  swap();
  assert.equal($("grow-body").hasAttribute("data-ui-stored"), true, "a body added to a mounted element was not sealed");
  assert.deepEqual(hooks($("grow-body")), []);
  assert.equal($("grow-button").getAttribute("form"), "ui-sealed");
  assert.equal(ui.find("#grow-button"), $("grow-button"));
  assert.equal(ui.stored($("grow-button")), false);
  // What the script writes there from then on is scrubbed too.
  const later = document.createElement("button");
  later.setAttribute("id", "grow-later");
  later.setAttribute("data-ui-click", '[["toggle","menu"]]');
  $("grow-body").append(later);
  await settle();
  assert.deepEqual(hooks($("grow-body")), []);

  // Another morph puts another element in its place: that one is sealed, and is the script's.
  beforeSwap($("grow"));
  const other = document.createElement("div");
  other.innerHTML = `<section id="grow-section" data-slot="proof:pad-body" data-ui-keep><button id="grow-other" data-ui-click='[["toggle","menu"]]'>Model Town</button></section>`;
  $("grow-body").replaceWith(...other.childNodes);
  swap();
  assert.equal($("grow-section").hasAttribute("data-ui-stored"), true, "a body put in place of another was not sealed");
  assert.deepEqual(hooks($("grow-section")), []);
  assert.equal(ui.find("#grow-other"), $("grow-other"));
  assert.deepEqual(ui.findAll("button"), [$("grow-other")]);
  $("grow-other").click();
  assert.equal($("menu").hasAttribute("hidden"), true, "a command in a body added by a morph ran");
});

test("a body put in place of another while the script is on its way is sealed before the script draws in it", async () => {
  assert.equal($("slow-body").hasAttribute("data-ui-stored"), true);
  beforeSwap(document.body);
  const other = document.createElement("div");
  other.innerHTML = `<section id="slow-section" data-slot="proof:slow-body" data-ui-keep><button id="slow-drawn" data-ui-click='[["toggle","menu"]]'>Sarabha Nagar</button></section>`;
  $("slow-body").replaceWith(...other.childNodes);
  swap();
  // Sealed before the script comes: what the server drew there is no hook.
  assert.equal($("slow-section").hasAttribute("data-ui-stored"), true, "the body put in place of another was not sealed");
  assert.deepEqual(hooks($("slow-section")), []);
  $("slow-drawn").click();
  letSlowCome();
  await until(() => $("slow").hasAttribute("data-ui-mounted"), "the slow script to mount");
  await settle();
  assert.ok($("slow-planted"), "the slow script did not draw into its body");
  assert.equal($("slow-section").hasAttribute("data-ui-stored"), true, "the body the script drew in was not sealed");
  assert.deepEqual(hooks($("slow-section")), []);
  $("slow-planted").click();
  assert.equal($("menu").hasAttribute("hidden"), true, "a command in the slow script's body ran");
});

test("ui.stored answers for a text node and for something that is no node", () => {
  const ui = g.uis.pad;
  const line = ui.find('[data-slot="proof:line"]');
  assert.equal(ui.stored(line.firstChild), false);
  assert.equal(ui.stored($("pad-stored").firstChild), true);
  assert.equal(ui.stored(/** @type {any} */ (globalThis)), false);
  assert.equal(ui.stored(document.createTextNode("Ludhiana")), false);
});

test("ui.find skips stored content but searches the owned body", () => {
  const ui = g.uis.pad;
  const line = ui.find('[data-slot="proof:line"]');
  assert.ok(line, "the script's own line was not found");
  assert.equal(line.parentNode, $("pad-body"));
  assert.deepEqual(ui.findAll('[data-slot="proof:line"]'), [line]);
  assert.equal(ui.find("#pad-stored"), null);
  assert.equal(ui.stored($("pad-stored")), true);
  assert.equal(ui.stored(line), false);
  assert.equal(ui.closest(line, '[data-slot="proof:pad"]'), $("pad"));
  assert.equal(ui.closest($("pad-stored"), "p"), null);
  assert.equal(ui.closest($("menu"), "div"), null);
});

test("ui.call refuses another origin before sending", async () => {
  const ui = g.uis.pad;
  page.fetched.length = 0;
  const before = toasts().length;
  assert.deepEqual(await ui.call("https://cdn.example/proof/x"), REFUSED);
  assert.deepEqual(toasts().slice(before), ["That didn't work. Try again."]);
  for (const url of ["//cdn.example/proof/x", "https://user:pass@app.test/proof/x", "https://app.test:8443/proof/x"]) {
    assert.deepEqual(await ui.call(url, {}, { quiet: true }), REFUSED, url);
  }
  assert.equal(toasts().length, before + 1, "a quiet call was toasted");
  // A read is held to the same.
  await assert.rejects(ui.get("https://cdn.example/proof/tiles.json"));
  assert.deepEqual(page.fetched, []);
});

test("ui.call compares the parsed path, not the text", async () => {
  const ui = g.uis.pad;
  page.fetched.length = 0;
  for (const url of ["/proof/../account/email", "/proof/%2e%2e/account/email", "/proof\\..\\account/email", "/proof"]) {
    assert.deepEqual(await ui.call(url, {}, { quiet: true }), REFUSED, url);
  }
  assert.deepEqual(page.fetched, []);

  page.answers.push(answer("", NOTHING));
  assert.deepEqual(await ui.call("/proof/save", { at: "30.8890,75.8340" }), { ok: true, status: 204, data: null });
  assert.equal(page.fetched.length, 1);
  const [{ url, init }] = page.fetched;
  assert.equal(url, "https://app.test/proof/save");
  assert.equal(init.method, "POST");
  assert.equal(init.headers["x-xsrf-token"], "tok");
  assert.equal(init.headers["x-navree-ui"], "1");
  assert.equal(init.headers["x-navree-from"], "/deliveries");
  assert.equal(String(init.body), "at=30.8890%2C75.8340");
});

test("ui.call refuses an address outside its routes even when the props hold it", async () => {
  const ui = g.uis.pad;
  const { next } = g.seen.find((/** @type {any} */ s) => s.id === "pad" && s.call === "mount").props;
  assert.equal(next, "/account/email");
  page.fetched.length = 0;
  assert.deepEqual(await ui.call(next, {}, { quiet: true }), REFUSED);
  // Nor what a library wrote into the body.
  assert.deepEqual(await ui.call($("vendor-body").querySelector("form").getAttribute("action"), {}, { quiet: true }), REFUSED);
  await assert.rejects(ui.get(next));
  assert.deepEqual(page.fetched, []);
});

test("ui.call reaches an exact address the server wrote", async () => {
  const ui = g.uis.pad;
  page.fetched.length = 0;
  page.answers.push(answer("", NOTHING), answer("", NOTHING));
  assert.equal((await ui.call("/deliveries/42/drop", { at: "30.8890,75.8340" })).ok, true);
  // A query on it is sent as parsed.
  assert.equal((await ui.call("/deliveries/42/drop?from=map", {})).ok, true);
  for (const url of ["/deliveries/43/drop", "/deliveries/42/drop/x", "/deliveries/42"]) {
    assert.deepEqual(await ui.call(url, {}, { quiet: true }), REFUSED, url);
  }
  assert.deepEqual(
    page.fetched.map((f) => f.url),
    ["https://app.test/deliveries/42/drop", "https://app.test/deliveries/42/drop?from=map"],
  );
});

test("ui.call treats a redirect off the site, or a fresh answer, as a failure and applies no ops", async () => {
  const ui = g.uis.pad;
  const remove = `{"ops":[{"op":"remove","target":"zones"}]}`;
  page.answers.push(answer(remove, { headers: OPS, redirected: true, url: "https://elsewhere.example/done" }));
  assert.deepEqual(await ui.call("/proof/save", {}, { quiet: true }), REFUSED);
  page.answers.push(answer(remove, { headers: { ...OPS, "x-navree-fresh": "1" } }));
  const fresh = await ui.call("/proof/save", {}, { quiet: true });
  assert.equal(fresh.ok, false);
  assert.equal(fresh.data, null);
  assert.ok($("zones"), "the ops of an answer that is no answer to this page were applied");
});

test("a region fetched before a ui.call is not shown after it", async () => {
  const ui = g.uis.pad;
  /** The region as the server draws it. @param {string} text */
  const zones = (text) =>
    answer(`<!doctype html><html data-build="b1"><head></head><body><div id="zones" data-ui-region data-ui-src="/zones"><p id="zones-text">${text}</p></div></body></html>`, { headers: PAGE });
  /** @type {() => void} */
  let release = () => {};
  const held = new Promise((/** @type {any} */ go) => (release = go));
  page.fetched.length = 0;
  // The region's answer is on its way, from before the write.
  page.answers.push(async () => {
    await held;
    return zones("Sarabha Nagar, before the save");
  });
  fire($("zones"), "ui:refresh");
  page.answers.push(answer("", NOTHING), zones("Sarabha Nagar, after the save"));
  assert.equal((await ui.call("/proof/save", { zone: "Sarabha Nagar" })).ok, true);
  release();
  await settle();
  assert.equal($("zones-text").textContent, "Sarabha Nagar, after the save", "the call did not take core's write path");
  assert.deepEqual(
    page.fetched.map((f) => new URL(f.url).pathname),
    ["/zones", "/proof/save", "/zones"],
  );
});

test("ui.call gives a json answer to the script and shows no html page unless asked", async () => {
  const ui = g.uis.pad;
  page.answers.push(answer(`{"total":"₹1,00,000"}`, { headers: { "content-type": "application/json" } }));
  assert.deepEqual(await ui.call("/proof/total", { city: "Pune" }), { ok: true, status: 200, data: { total: "₹1,00,000" } });

  const receipt = `<!doctype html><html data-build="b1"><head><title>Receipt</title></head><body><main>Paid ₹40</main></body></html>`;
  page.assigned.length = 0;
  page.answers.push(answer(receipt, { headers: PAGE, url: "https://app.test/proof/receipt" }));
  const html = await ui.call("/proof/pay", {}, { quiet: true });
  assert.equal(html.ok, false);
  assert.equal(html.data, null);
  assert.deepEqual(page.assigned, []);
  assert.equal($("main").textContent.includes("Paid ₹40"), false);

  page.answers.push(answer(receipt, { headers: PAGE, url: "https://app.test/proof/receipt", redirected: true }));
  assert.equal((await ui.call("/proof/pay", {}, { page: true })).ok, true);
  assert.deepEqual(page.assigned, ["https://app.test/proof/receipt"]);

  page.answers.push(answer(`{"ops":[{"op":"toast","tone":"success","text":"Drop point saved for Simran Kaur"}]}`, { headers: OPS }));
  assert.deepEqual(await ui.call("/proof/save", {}), { ok: true, status: 200, data: null });
  assert.ok(toasts().includes("Drop point saved for Simran Kaur"), toasts().join("\n"));
});

test("ui.on listeners and ui.later timers end when the element leaves", async () => {
  const ui = g.uis.timers;
  let clicks = 0, kept = 0, ticks = 0;
  ui.on(document, "click", () => clicks++);
  const off = ui.on(document, "click", () => kept++);
  ui.later(30, () => ticks++);
  fire(document, "click");
  assert.equal(clicks, 1);
  // What it gave back takes its own listener away.
  off();
  fire(document, "click");
  assert.equal(kept, 1);

  $("timers").remove();
  swap();
  assert.equal(ui.signal.aborted, true);
  fire(document, "click");
  await new Promise((go) => setTimeout(go, 60));
  assert.equal(clicks, 2);
  assert.equal(ticks, 0);
});

test("ui.emit names the event under the script's namespace", () => {
  const ui = g.uis.pad;
  assert.equal(ui.namespace, "proof");
  assert.equal(ui.contract, 1);
  /** @type {any} */
  let heard = null;
  document.addEventListener("proof:moved", (e) => (heard = e));
  assert.equal(ui.emit("moved", { at: "30.8890,75.8340" }), true);
  assert.ok(heard, "the event did not reach the document");
  assert.equal(heard.target, $("pad"));
  assert.equal(heard.bubbles, true);
  assert.deepEqual(heard.detail, { at: "30.8890,75.8340" });
  // Its toasts and its words to assistive technology go where the page's own do.
  ui.toast("info", "Harpreet Sandhu is 2 km away");
  assert.ok(toasts().includes("Harpreet Sandhu is 2 km away"));
  ui.announce("Drop point moved");
  assert.equal(document.querySelector("[data-slot=announcer]")?.textContent, "Drop point moved");
});

test("links a sheet named by an element that arrives, once, and when it starts", () => {
  // At the start: the server's link for the pad stays one, the two badges' sheet is linked once, and
  // neither the sheet of stored content nor one off the site is.
  assert.deepEqual(sheets(), [`${EXT}/proof.pad-0123456789ab.css`, `${EXT}/proof.badge-0123456789ab.css`]);

  const more = document.createElement("div");
  more.innerHTML = `<span id="badge-3" data-slot="proof:badge" data-ui-ext-sheet="${EXT}/proof.badge-0123456789ab.css">Meera Sharma</span><span id="tag-1" data-slot="proof:tag" data-ui-ext-sheet="${EXT}/proof.tag-0123456789ab.css">Amritsar</span>`;
  $("badges").append(...more.childNodes);
  swap();
  assert.deepEqual(sheets(), [`${EXT}/proof.pad-0123456789ab.css`, `${EXT}/proof.badge-0123456789ab.css`, `${EXT}/proof.tag-0123456789ab.css`]);
});

test("removes sheet links no element names after a page swap", () => {
  beforeSwap(document.body);
  for (const id of ["badge-1", "badge-2", "badge-3", "tag-1"]) $(id).remove();
  swap();
  assert.deepEqual(sheets(), [`${EXT}/proof.pad-0123456789ab.css`]);
});
