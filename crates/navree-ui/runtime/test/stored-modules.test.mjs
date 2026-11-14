// The modules start nothing they find in stored content: an island, a live source, a lazy list
// and its `seen` post, a form that sends itself into view, an upload area, a table's row link.
import { test } from "node:test";
import assert from "node:assert/strict";
import { fire, install, settle } from "./fake-dom.mjs";

const page = install({
  url: "https://app.test/inbox",
  cookie: "XSRF-TOKEN=tok",
  html: `<!doctype html><html data-build="b1"><head></head><body>
    <div data-ui-stored>
      <div id="planted-island" data-ui-island="/_navree/ui/island/x-0123456789ab.js" data-ui-props="{}"></div>
      <div id="planted-live" data-ui-live-src="/events"></div>
      <div id="planted-list" data-ui-lazy-list data-ui-seen="/planted/seen" data-ui-src="/planted/items"><ul data-slot="lazy-items"></ul></div>
      <form id="planted-form" data-ui-autoload method="post" action="/planted"></form>
      <div id="planted-upload" data-ui-upload></div>
    </div>
    <div id="t" data-ui-table><table><tbody><tr><td><div data-ui-stored>
      <table><tbody><tr id="planted-row" data-ui-row-href="/planted/row"><td id="planted-cell">x</td></tr></tbody></table>
    </div></td></tr></tbody></table></div>
    <div id="island" data-ui-island="/_navree/ui/island/y-0123456789ab.js" data-ui-props="{}"></div>
    <div id="live" data-ui-live-src="/own-events"></div>
    <div id="list" data-ui-lazy-list data-ui-seen="/own/seen" data-ui-src="/own/items"><ul data-slot="lazy-items"></ul></div>
    <form id="form" data-ui-autoload method="post" action="/own"></form>
    <div id="upload" data-ui-upload></div>
  </body></html>`,
});

/** What the page's observers and streams were asked to watch. */
const observed = /** @type {Element[]} */ ([]);
const streams = /** @type {string[]} */ ([]);
Object.assign(globalThis, {
  IntersectionObserver: class {
    /** @param {Element} el */
    observe(el) {
      observed.push(el);
    }
    disconnect() {}
  },
  EventSource: class {
    /** @param {string} url */
    constructor(url) {
      streams.push(new URL(url, location.href).pathname);
    }
    addEventListener() {}
    close() {}
  },
});

await import("../src/islands.js");
await import("../src/live.js");
await import("../src/lazy.js");
await import("../src/sortable.js");
await import("../src/upload.js");
await import("../src/table.js");
await settle();

const $ = (/** @type {string} */ id) => /** @type {any} */ (document.getElementById(id));

test("an island in stored content is not mounted", () => {
  // The page's own island tried its script (which can't load here) and says so.
  assert.equal($("island").hasAttribute("data-ui-failed"), true);
  assert.equal($("planted-island").hasAttribute("data-ui-failed"), false);
});

test("a live source in stored content opens no stream", () => {
  assert.deepEqual(streams, ["/own-events"]);
});

test("a lazy list in stored content posts no `seen`", () => {
  const posts = page.fetched.filter((f) => f.init?.method === "POST").map((f) => new URL(f.url).pathname);
  assert.deepEqual(posts, ["/own/seen"]);
});

test("a form in stored content is not sent as it comes into view", () => {
  assert.ok(observed.includes($("form")));
  assert.ok(!observed.includes($("planted-form")));
});

test("an upload area in stored content is not made ready", () => {
  assert.equal($("upload").hasAttribute("data-ready"), true);
  assert.equal($("planted-upload").hasAttribute("data-ready"), false);
});

test("a row link in stored content is not visited", () => {
  const before = page.assigned.length;
  fire($("planted-cell"), "click", { button: 0 });
  assert.equal(page.assigned.length, before);
  assert.equal(document.querySelectorAll("a[data-ui-in-region]").length, 0);
});
