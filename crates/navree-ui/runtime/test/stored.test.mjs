// Stored content (rich text, Markdown) sits in an element the server marks `data-ui-stored`:
// no hook inside it drives the runtime, and no element inside it stands in for one of the page's own.
import { test } from "node:test";
import assert from "node:assert/strict";
import { answer, fire, install, settle } from "./fake-dom.mjs";

const page = install({
  url: "https://app.test/deals",
  cookie: "XSRF-TOKEN=tok",
  html: `<!doctype html><html data-build="b1"><head></head><body data-ui-boost>
    <main id="main">
      <p id="note">Note</p>
      <button id="own-copy" data-ui-click='[["toggle","note"]]'>Toggle</button>
      <div id="post" data-ui-stored>
        <p id="target">A paragraph of a post</p>
        <button id="planted" data-ui-click='[["copy","planted text"]]'>Copy</button>
        <a id="planted-key" data-ui-hotkey="k" href="#x">k</a>
        <div id="planted-poll" data-ui-src="/poll" data-ui-poll="1"></div>
        <div id="planted-lazy" data-ui-src="/lazy" data-ui-lazy></div>
        <div data-slot="code-block-bar" data-ui-own><button id="copy-code" data-ui-click='[["copy","cargo build"]]'>Copy code</button></div>
        <p id="later">A twin of an id the page has further down</p>
        <div data-ui-own><div data-ui-stored><p id="deep">Stored again, inside a part of the toolkit's own</p></div></div>
      </div>
      <p id="later">The page's own</p>
      <div data-ui-own><div id="outer-stored" data-ui-stored><button id="outer-planted" data-ui-click='[["copy","outer planted"]]'>Copy</button></div></div>
      <button id="opens-target" data-ui-click='[["hide","target"]]'>Hide</button>
      <a id="key" data-ui-hotkey="j" href="#j">j</a>
    </main>
  </body></html>`,
});

const dom = await import("../src/dom.js");
await import("../src/core.js");
await import("../src/keys.js");

const $ = (/** @type {string} */ id) => /** @type {any} */ (document.getElementById(id));

test("closest, every and byId find nothing in stored content", () => {
  const planted = $("planted");
  assert.equal(dom.closest(planted, "[data-ui-click]"), null);
  assert.equal(dom.closest($("own-copy"), "[data-ui-click]"), $("own-copy"));
  assert.deepEqual(dom.every("[data-ui-hotkey]").map((el) => el.id), ["key"]);
  assert.equal(dom.byId("target"), null);
  assert.equal(dom.byId("note"), $("note"));
  // The wrapper itself is the server's.
  assert.equal(dom.byId("post"), $("post"));
});

test("a part the server draws for itself inside stored content keeps its hooks", async () => {
  page.copied.length = 0;
  $("copy-code").click();
  await settle();
  assert.deepEqual(page.copied, ["cargo build"]);
});

test("a click command inside stored content does nothing", async () => {
  page.copied.length = 0;
  $("planted").click();
  await settle();
  assert.deepEqual(page.copied, []);
  assert.equal($("planted").hasAttribute("data-copied"), false);
});

test("a command that names an element by id doesn't reach one in stored content", () => {
  $("opens-target").click();
  assert.equal($("target").hasAttribute("hidden"), false);
  $("own-copy").click();
  assert.equal($("note").hasAttribute("hidden"), true);
});

test("a hotkey inside stored content is not taken", () => {
  const stolen = fire(document, "keydown", { key: "k" });
  assert.equal(stolen.defaultPrevented, false);
  const own = fire(document, "keydown", { key: "j" });
  assert.equal(own.defaultPrevented, true);
});

test("a region inside stored content is neither polled nor fetched", async () => {
  page.answers.push(answer(""), answer(""));
  await settle();
  assert.deepEqual(page.fetched.map((f) => new URL(f.url).pathname), []);
});

test("byId skips what stored content holds under the id of an element the page has", () => {
  const twins = [...document.querySelectorAll("[id=later]")];
  assert.equal(twins.length, 2);
  assert.equal(dom.stored(twins[0]), true);
  assert.equal(dom.stored(twins[1]), false);
  assert.equal(dom.byId("later"), twins[1]);
  assert.equal(dom.byId("later", document), twins[1]);
  // An id that only stored content has is still found by nothing.
  assert.equal(dom.byId("target"), null);
});

test("an element in a part the server draws inside stored content is found by its id", () => {
  assert.equal(dom.byId("copy-code"), $("copy-code"));
  assert.equal(dom.stored($("copy-code")), false);
});

test("a part of the toolkit's own that holds the stored wrapper doesn't open what is stored to hooks", async () => {
  // The wrapper is the server's, and so is what holds it.
  assert.equal(dom.stored($("outer-stored")), false);
  // What is in the wrapper is stored, whatever lies outside it.
  assert.equal(dom.stored($("outer-planted")), true);
  assert.equal(dom.closest($("outer-planted"), "[data-ui-click]"), null);
  page.copied.length = 0;
  $("outer-planted").click();
  await settle();
  assert.deepEqual(page.copied, []);
});

test("stored content inside a part of the toolkit's own inside stored content is stored", () => {
  // The part is nested in a wrapper, and the nearest wrapper of `deep` is the one inside the part.
  assert.equal(dom.stored($("deep")), true);
  assert.equal(dom.byId("deep"), null);
  assert.equal(dom.stored($("copy-code")), false);
});
