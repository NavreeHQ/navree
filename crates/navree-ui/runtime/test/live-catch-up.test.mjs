// A live list's catch-up fetch: the page loads the live module and the module that draws a list as
// separate scripts and waits for neither, so the stream can open before anything answers the live
// module's `ui:refresh`. A refresh nobody answered is not a fetch: the list is asked again after a quiet
// time, for a few seconds, and then left. Timers here run when the test says (`t.mock.timers`).
import { test } from "node:test";
import assert from "node:assert/strict";
import { install } from "./fake-dom.mjs";

const page = install({
  url: "https://app.test/assistant/42",
  cookie: "XSRF-TOKEN=tok",
  html: `<!doctype html><html data-build="b1"><head></head><body></body></html>`,
});

/** Every stream the page opened. */
const streams = /** @type {any[]} */ ([]);
Object.assign(globalThis, {
  EventSource: class {
    /** @param {string} url */
    constructor(url) {
      this.url = new URL(url, location.href).pathname;
      this.closed = false;
      this.listeners = /** @type {Record<string, any[]>} */ ({});
      streams.push(this);
    }
    /** @param {string} name @param {any} fn */
    addEventListener(name, fn) {
      (this.listeners[name] ||= []).push(fn);
    }
    close() {
      this.closed = true;
    }
  },
});

// Every `ui:refresh` aimed at a region, answered or not, by the region's id.
const asked = /** @type {string[]} */ ([]);
document.addEventListener("ui:refresh", (/** @type {any} */ e) => asked.push(e.target.id));

await import("../src/live.js");

/** The list's module, which answers a `ui:refresh` aimed at a list with a fetch of its own. It is not there until `start`. */
const drawing = {
  fetched: 0,
  /** @param {any} e */
  answer(e) {
    drawing.fetched++;
    e.detail.done = Promise.resolve();
  },
  start() {
    document.addEventListener("ui:refresh", drawing.answer, true);
  },
  stop() {
    document.removeEventListener("ui:refresh", drawing.answer);
  },
};

/** Lets every promise that is ready settle: timers are the test's own to run. */
const flush = () => new Promise((done) => setImmediate(done));

/** Runs `n` quiet times, one after the other: a timer that is set while another runs waits for the next. @param {any} t @param {number} n */
const quiet = (t, n) => {
  for (let i = 0; i < n; i++) t.mock.timers.tick(300);
};

/**
 * The page draws a live list, and the stream it names is made.
 * @param {string} id @param {string} [more]
 */
function draw(id, more = "") {
  asked.length = 0;
  drawing.fetched = 0;
  document.body.innerHTML = `<div id="${id}" data-ui-lazy-list data-ui-src="/assistant/42/items" data-ui-live-src="/events/${id}" ${more}></div>`;
  document.dispatchEvent(new Event("ui:swap"));
  const es = streams.at(-1);
  assert.equal(es.url, `/events/${id}`);
  return es;
}

/** The browser says the stream is open. @param {any} es */
const opens = (es) => es.onopen({ type: "open" });

test("a list whose script arrives after the stream opened is asked again, and fetched once, when it has", async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const es = draw("chat", "data-ui-live-catch-up");
  opens(es);
  // The stream opened and nobody answered: the list owes a fetch.
  assert.deepEqual([...asked], ["chat"]);
  assert.equal(drawing.fetched, 0);
  t.mock.timers.tick(300);
  assert.deepEqual([...asked], ["chat", "chat"]);
  assert.equal(drawing.fetched, 0);
  // The module that draws the list arrives.
  drawing.start();
  t.after(() => drawing.stop());
  t.mock.timers.tick(300);
  await flush();
  assert.deepEqual([...asked], ["chat", "chat", "chat"]);
  assert.equal(drawing.fetched, 1);
  // The fetch was served: it is not asked for again.
  quiet(t, 40);
  await flush();
  assert.equal(asked.length, 3);
  assert.equal(drawing.fetched, 1);
  assert.deepEqual(page.errors, []);
});

test("a refresh that was answered is not asked again", async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  drawing.start();
  t.after(() => drawing.stop());
  const es = draw("answered", "data-ui-live-catch-up");
  opens(es);
  await flush();
  assert.equal(drawing.fetched, 1);
  quiet(t, 40);
  await flush();
  assert.deepEqual([...asked], ["answered"]);
  assert.equal(drawing.fetched, 1);
});

test("a region that was answered is given its twenty asks again", async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const es = draw("again", `data-ui-live-catch-up data-ui-live-event="changed"`);
  opens(es);
  quiet(t, 15);
  assert.equal(asked.length, 16);
  drawing.start();
  t.after(() => drawing.stop());
  quiet(t, 1);
  await flush();
  assert.equal(asked.length, 17);
  assert.equal(drawing.fetched, 1);
  // The script is gone again (the page was drawn anew): an event finds nobody, and is asked for as before.
  drawing.stop();
  es.listeners.changed[0]({ lastEventId: "" });
  quiet(t, 100);
  assert.equal(asked.length, 17 + 21);
});

test("a region nobody ever answers for is asked again twenty times, about six seconds, and then left", (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const es = draw("never", "data-ui-live-catch-up");
  opens(es);
  quiet(t, 19);
  assert.equal(asked.length, 20);
  quiet(t, 1);
  assert.equal(asked.length, 21);
  quiet(t, 200);
  assert.equal(asked.length, 21);
  assert.equal(es.closed, false, "the stream stays");
  assert.deepEqual(page.errors, []);
});

test("a region taken out of the page while it is asked again is not asked once more", (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const es = draw("gone", "data-ui-live-catch-up");
  opens(es);
  t.mock.timers.tick(300);
  assert.equal(asked.length, 2);
  // Taken out by something that never says so (a swap that no one hears): the next ask finds it gone.
  document.getElementById("gone")?.remove();
  quiet(t, 40);
  assert.equal(asked.length, 2);
  assert.equal(es.closed, true, "its stream is let go of");
  assert.deepEqual(page.errors, []);
});

test("a region the page lets go of while it is asked again has no ask waiting", (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const es = draw("swapped", "data-ui-live-catch-up");
  opens(es);
  t.mock.timers.tick(300);
  assert.equal(asked.length, 2);
  // The page is drawn again without it.
  document.body.innerHTML = "";
  document.dispatchEvent(new Event("ui:swap"));
  assert.equal(es.closed, true);
  quiet(t, 40);
  assert.equal(asked.length, 2);
  assert.deepEqual(page.errors, []);
});

test("a region that is not owed a catch-up is not fetched when its stream opens, and an event fetches it once", async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  drawing.start();
  t.after(() => drawing.stop());
  const es = draw("plain", `data-ui-live-event="changed"`);
  opens(es);
  await flush();
  quiet(t, 40);
  assert.deepEqual([...asked], []);
  assert.equal(drawing.fetched, 0);
  // An event waits a quiet time, so a burst of them costs one fetch.
  const [hear] = es.listeners.changed;
  hear({ lastEventId: "" });
  hear({ lastEventId: "" });
  t.mock.timers.tick(299);
  assert.deepEqual([...asked], []);
  t.mock.timers.tick(1);
  await flush();
  assert.deepEqual([...asked], ["plain"]);
  assert.equal(drawing.fetched, 1);
  quiet(t, 40);
  await flush();
  assert.equal(drawing.fetched, 1);
});
