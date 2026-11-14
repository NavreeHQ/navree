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
      </div>
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
