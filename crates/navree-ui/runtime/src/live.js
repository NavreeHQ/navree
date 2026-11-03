// Live regions: a server-sent event stream tells a region, or a table, to fetch
// itself again. An event is a nudge. Its payload is never read, so nothing a
// stream says can become markup, a URL or a selector; the region fetches its own
// source, which is what the server meant to show.
//
// One connection serves every region that names the same stream. A hidden page
// holds none. A connection that fails comes back after 1 s, 2 s, 4 s ... 30 s,
// each with up to a quarter more at random, and a region that may have missed
// something fetches again once when its stream is back.
import { every, hidden, on, sameOrigin } from "./dom.js";
import { refresh } from "./nav.js";

/** Quiet time before a refresh, so a burst of events costs one fetch. */
const QUIET = 300;
const FIRST = 1000;
const LONGEST = 30000;
/** A connection that lasted this long was a good one: its failures are forgiven. */
const STEADY = 5000;
/** Event names a region may listen for. */
const NAME = /^[\w.:-]{1,64}$/;

/**
 * @typedef {{name: string, timer: number}} Sub
 * @typedef {{url: string, es: EventSource | null, subs: Map<Element, Sub>, failed: number, since: number, retry: number, listening: Set<string>, state: string, missed: boolean}} Stream
 */

/** One stream per address. @type {Map<string, Stream>} */
const streams = new Map();

/** @param {Stream} s @param {string} state */
function paint(s, state) {
  s.state = state;
  for (const el of s.subs.keys()) el.setAttribute("data-live", state);
}

/** Fetches `el` again, if it is still there and anyone is looking. @param {Stream} s @param {Element} el */
function fire(s, el) {
  const sub = s.subs.get(el);
  if (sub) sub.timer = 0;
  if (!el.isConnected) {
    s.subs.delete(el);
    return;
  }
  if (!hidden() && el.id) refresh(el.id);
}

/** @param {Stream} s @param {string} name */
function heard(s, name) {
  for (const [el, sub] of s.subs) {
    if (sub.name !== name || sub.timer) continue;
    sub.timer = /** @type {any} */ (setTimeout(() => fire(s, el), QUIET));
  }
}

/** Every region on this stream fetches once, now. @param {Stream} s */
function catchUp(s) {
  s.missed = false;
  for (const [el, sub] of s.subs) {
    clearTimeout(sub.timer);
    fire(s, el);
  }
}

/** @param {Stream} s */
function listen(s) {
  const es = s.es;
  if (!es) return;
  for (const sub of s.subs.values()) {
    if (s.listening.has(sub.name)) continue;
    s.listening.add(sub.name);
    es.addEventListener(sub.name, () => heard(s, sub.name));
  }
}

/** @param {Stream} s */
function open(s) {
  if (s.es || hidden() || !s.subs.size) return;
  clearTimeout(s.retry);
  const es = new EventSource(s.url);
  s.es = es;
  s.listening = new Set();
  if (s.state !== "retry") paint(s, "connecting");
  es.onopen = () => {
    s.since = performance.now();
    paint(s, "on");
    // Whatever was said while it was down was lost.
    if (s.failed || s.missed) catchUp(s);
  };
  es.onerror = () => {
    // The browser would retry on its own schedule; ours is the same in every browser.
    es.close();
    s.es = null;
    if (s.since && performance.now() - s.since > STEADY) s.failed = 0;
    s.since = 0;
    s.failed++;
    if (hidden()) return;
    paint(s, "retry");
    const wait = Math.min(LONGEST, FIRST * 2 ** (s.failed - 1));
    s.retry = /** @type {any} */ (setTimeout(() => open(s), wait * (1 + Math.random() / 4)));
  };
  listen(s);
}

/** @param {Stream} s */
function close(s) {
  clearTimeout(s.retry);
  s.es?.close();
  s.es = null;
  s.since = 0;
}

/** Leaves the stream of a region that is gone or has moved. @param {Stream} s @param {Element} el */
function leave(s, el) {
  clearTimeout(s.subs.get(el)?.timer);
  s.subs.delete(el);
  if (s.subs.size) return;
  close(s);
  streams.delete(s.url);
}

/** Joins every region on the page to its stream, and lets go of those that left. */
export function scanLive() {
  /** @type {Map<Element, {url: string, name: string}>} */
  const wanted = new Map();
  for (const el of every("[data-ui-live-src]")) {
    const raw = el.getAttribute("data-ui-live-src") || "";
    const name = el.getAttribute("data-ui-live-event") || "message";
    // A stream on another site is no stream of ours.
    if (!el.id || !sameOrigin(raw) || !NAME.test(name)) continue;
    wanted.set(el, { url: new URL(raw, location.href).href, name });
  }
  for (const s of [...streams.values()]) {
    for (const el of [...s.subs.keys()]) {
      const now = wanted.get(el);
      if (!el.isConnected || !now || now.url !== s.url || now.name !== s.subs.get(el)?.name) leave(s, el);
    }
  }
  for (const [el, { url, name }] of wanted) {
    let s = streams.get(url);
    if (!s) {
      s = { url, es: null, subs: new Map(), failed: 0, since: 0, retry: 0, listening: new Set(), state: "connecting", missed: false };
      streams.set(url, s);
    }
    if (!s.subs.has(el)) s.subs.set(el, { name, timer: 0 });
    el.setAttribute("data-live", hidden() ? "paused" : s.state);
    open(s);
    listen(s);
  }
}

/** The page is out of sight: no connection is held for it. */
function pause() {
  for (const s of streams.values()) {
    close(s);
    s.missed = true;
    paint(s, "paused");
  }
}

/** The page is back: connect, and fetch what was missed. */
function resume() {
  for (const s of streams.values()) {
    s.failed = 0;
    paint(s, "connecting");
    open(s);
    catchUp(s);
  }
}

/** Starts watching the page for live regions. */
export function startLive() {
  on(document, "ui:swap", scanLive);
  on(document, "visibilitychange", () => (hidden() ? pause() : resume()));
  // A page kept for the back button holds no connection either.
  on(window, "pagehide", pause);
  on(window, "pageshow", (/** @type {PageTransitionEvent} */ e) => {
    if (e.persisted) resume();
  });
  scanLive();
}
