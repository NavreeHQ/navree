// A region with no source (the server leaves `data-ui-src` out for an address it refuses, or writes `#`)
// is never fetched, polled or nudged: no answer to this page's own address stands in for it.
import { test } from "node:test";
import assert from "node:assert/strict";
import { fire, install, settle } from "./fake-dom.mjs";

const page = install({
  url: "https://app.test/deals",
  cookie: "XSRF-TOKEN=tok",
  html: `<!doctype html><html data-build="b1"><head></head><body data-ui-boost>
    <div id="ok" data-ui-region data-ui-src="/fragments/ok" data-ui-poll="1000" data-ui-live-src="/events">ok</div>
    <div id="none" data-ui-region data-ui-poll="1000" data-ui-lazy data-ui-live-src="/events">none</div>
    <div id="hash" data-ui-region data-ui-src="#" data-ui-poll="1000" data-ui-lazy data-ui-live-src="/events">
      hash <button id="again" type="button" data-ui-refresh>Retry</button>
    </div>
    <div id="board-none" data-ui-region data-ui-src="#">
      <div data-ui-sortable><div data-slot="sortable-list" data-list="a" data-paged><div data-slot="sortable-items"><div data-slot="sortable-item" data-key="1">One</div></div></div></div>
    </div>
    <div id="board-ok" data-ui-region data-ui-src="/boards/2">
      <div data-ui-sortable><div data-slot="sortable-list" data-list="a" data-paged><div data-slot="sortable-items"><div data-slot="sortable-item" data-key="1">One</div></div></div></div>
    </div>
  </body></html>`,
});

/** The delays of the polls the page set up. */
const intervals = /** @type {number[]} */ ([]);
const setIntervalOf = globalThis.setInterval;
globalThis.setInterval = /** @type {any} */ ((/** @type {any} */ fn, /** @type {number} */ ms) => (intervals.push(ms), setIntervalOf(fn, ms)));

/** Every stream the page opened, and what its listeners are. */
const streams = /** @type {any[]} */ ([]);
Object.assign(globalThis, {
  EventSource: class {
    /** @param {string} url */
    constructor(url) {
      this.url = new URL(url, location.href).pathname;
      this.listeners = /** @type {Record<string, any[]>} */ ({});
      streams.push(this);
    }
    /** @param {string} name @param {any} fn */
    addEventListener(name, fn) {
      (this.listeners[name] ||= []).push(fn);
    }
    close() {}
  },
});

await import("../src/core.js");
await import("../src/live.js");
await import("../src/sortable.js");
await settle();

const $ = (/** @type {string} */ id) => /** @type {any} */ (document.getElementById(id));
const paths = () => page.fetched.map((f) => new URL(f.url).pathname + new URL(f.url).search);

test("a region with no source, or `#`, is not fetched as it starts", () => {
  // The two lazy regions asked for their source as the page started: they have none.
  assert.deepEqual(paths(), []);
});

test("a region with no source, or `#`, is not polled", () => {
  assert.deepEqual(intervals, [1000]);
});

test("a refresh asked of a region with no source, or `#`, fetches nothing", async () => {
  fire($("none"), "ui:refresh");
  $("hash").dispatchEvent(new CustomEvent("ui:refresh", { bubbles: true }));
  $("again").click();
  await settle();
  assert.deepEqual(paths(), []);
  // The one with a source is fetched by the same request.
  $("ok").dispatchEvent(new CustomEvent("ui:refresh", { bubbles: true }));
  await settle();
  assert.deepEqual(paths(), ["/fragments/ok"]);
});

test("a stream's event nudges only the regions that have a source", async () => {
  page.fetched.length = 0;
  assert.deepEqual(streams.map((s) => s.url), ["/events"]);
  for (const listener of streams[0].listeners.message || []) listener({ lastEventId: "" });
  // The regions wait a quiet time before they fetch.
  await new Promise((done) => setTimeout(done, 450));
  assert.deepEqual(paths(), ["/fragments/ok"]);
});

test("a sortable's region with no source, or `#`, is not given this page's address as its source", () => {
  document.dispatchEvent(new Event("ui:swap"));
  assert.equal($("board-none").getAttribute("data-ui-src"), "#");
  // A region with a source is kept true to how many items its lists show.
  assert.equal($("board-ok").getAttribute("data-ui-src"), "/boards/2?show.a=1");
});