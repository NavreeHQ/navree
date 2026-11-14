// Live regions: a server-sent event stream tells a region, or a table, to fetch
// itself again. An event is a nudge. Its payload is never read, so nothing a
// stream says can become markup, a URL or a selector; the region fetches its own
// source, which is what the server meant to show.
//
// One connection serves every region that names the same stream. A hidden page
// holds none. A connection that fails or ends comes back after 1 s, 2 s, 4 s ... 30 s,
// each with up to a quarter more at random; only that timer reopens it, so nothing
// else on the page (a poll, a keystroke, a swap) can bring it back early. The
// stream's own `retry:` line is not read: EventSource keeps it to itself, and this
// schedule is the same in every browser. A region that may have missed something
// fetches again once when its stream is back, and never has more than one fetch
// of its own in flight: events that come while it waits cost one more fetch after it lands.
// A fetch is given up on after 15 s (in nav.js); one that somehow is still out after 20 s
// is left behind and the next event fetches anew.
//
// A region may listen for several names (`data-ui-live-event="a b"`). The app's live hub
// starts every connection with `ui:live`, and gives each event an id. A connection to it
// that comes back sends the last id it heard (`last_event_id`, as EventSource can't set
// headers); the hub answers `ui:resumed` and says again only what was missed, so no region
// fetches unless it missed something. When it answers `ui:live`, or nothing within 2.5 s,
// every region catches up as with any other stream. Only an event's name and id are read.
//
// A region the server drew while what it shows was still changing (`data-ui-live-catch-up`)
// can't hear what was said before its stream opened: it fetches once when the stream opens,
// or at once when it joins one that is open. Any other region drawn with the page doesn't.
import { cookies, every, fetchHere, hidden, on, sameOrigin } from "./dom.js";

/**
 * Asks the core runtime to fetch a region again, as a tab shown for the first time does. The
 * answer is the fetch: it ends when the region has been drawn, or when it could not be.
 * @param {Element} el
 * @returns {Promise<unknown>}
 */
function refresh(el) {
  const detail = /** @type {{done?: Promise<unknown>}} */ ({});
  el.dispatchEvent(new CustomEvent("ui:refresh", { bubbles: true, detail }));
  return Promise.resolve(detail.done);
}

/** Quiet time before a refresh, so a burst of events costs one fetch. */
const QUIET = 300;
const FIRST = 1000;
const LONGEST = 30000;
/** A connection that held this long was a good one: its failures are forgiven. */
const STEADY = 5000;
/** How long a tab shown again waits for its stream before it catches up anyway. */
const CATCH_WAIT = 2500;
/** A region's fetch still out after this long is not waited for any more. More than the fetch's own limit. */
const STALL = 20000;
/** How long the question to a stream that failed to open may take. */
const PROBE_LIMIT = 10000;
/** A stream is reopened for a changed session no more often than this: a server that sets a new token on every answer is no loop. */
const SESSION_GAP = 5000;
/** Event names a region may listen for. `open` and `error` are the connection's own. */
const NAME = /^[\w.:-]{1,64}$/;
const OWN = new Set(["open", "error"]);
/** What the live hub starts a connection with: it could not resume (catch up), or it did. */
const FRESH = "ui:live";
const RESUMED = "ui:resumed";
/** What a stream's refusal means for good: nothing to send, or not allowed. */
const FINAL = new Set([204, 401, 403]);

/**
 * @typedef {{names: string[], key: string, timer: number, busy: boolean, dirty: boolean, since: number, run: number, owed: boolean}} Sub
 * @typedef {{url: string, es: EventSource | null, subs: Map<Element, Sub>, failed: number, retry: number, steady: number, wait: number, listening: Set<string>, state: string, missed: boolean, opened: boolean, probed: boolean, stopped: boolean, token: string, changed: number, last: string, hub: boolean, resuming: boolean}} Stream
 */

/** One stream per address. @type {Map<string, Stream>} */
const streams = new Map();

/** The session's token as the page can read it: it changes with the user. */
function token() {
  for (const part of cookies().split("; ")) if (part.startsWith("XSRF-TOKEN=")) return part.slice(11);
  return "";
}

/** @param {Stream} s @param {string} state */
function paint(s, state) {
  s.state = state;
  for (const el of s.subs.keys()) el.setAttribute("data-live", state);
}

/** Whether a region's fetch has been out so long that it is not waited for. @param {Sub} sub */
function stalled(sub) {
  return sub.busy && performance.now() - sub.since > STALL;
}

/** Fetches `el` again now, or, if its last fetch is still out, once more when it lands. @param {Stream} s @param {Element} el */
function fire(s, el) {
  const sub = s.subs.get(el);
  if (!sub) return;
  sub.timer = 0;
  if (!el.isConnected) {
    leave(s, el);
    return;
  }
  if (hidden() || !el.id) return;
  if (sub.busy && !stalled(sub)) {
    sub.dirty = true;
    return;
  }
  // A fetch left behind (stalled) no longer owns the flags: its answer would be dropped as an older one.
  const mine = ++sub.run;
  sub.busy = true;
  sub.dirty = false;
  const started = (sub.since = performance.now());
  refresh(el)
    .catch(() => {})
    .finally(() => {
      if (sub.run !== mine) return;
      sub.busy = false;
      if (sub.dirty && s.subs.get(el) === sub) {
        // Exactly one more, and no sooner than a quiet time after the last began: a fast answer
        // and a busy stream make a few fetches a second, not as many as the answers can take.
        sub.dirty = false;
        sub.timer = /** @type {any} */ (setTimeout(() => fire(s, el), Math.max(0, QUIET - (performance.now() - started))));
      }
    });
}

/** @param {Stream} s @param {string} name */
function heard(s, name) {
  for (const [el, sub] of s.subs) {
    if (!sub.names.includes(name)) continue;
    if (sub.busy && !stalled(sub)) sub.dirty = true;
    else if (!sub.timer) sub.timer = /** @type {any} */ (setTimeout(() => fire(s, el), QUIET));
  }
}

/** Every region on this stream fetches once, now. @param {Stream} s */
function catchUp(s) {
  s.missed = false;
  clearTimeout(s.wait);
  for (const [el, sub] of s.subs) {
    clearTimeout(sub.timer);
    sub.owed = false;
    fire(s, el);
  }
}

/** Keeps the id of what was heard, to come back from. @param {Stream} s @param {Event} e */
function note(s, e) {
  const id = /** @type {MessageEvent} */ (e).lastEventId;
  if (id) s.last = id;
}

/** @param {Stream} s */
function listen(s) {
  const es = s.es;
  if (!es) return;
  for (const sub of s.subs.values()) {
    for (const name of sub.names) {
      if (s.listening.has(name)) continue;
      s.listening.add(name);
      es.addEventListener(name, (e) => {
        note(s, e);
        heard(s, name);
      });
    }
  }
}

/** The stream's address with the last id heard, so the hub says only what was missed. @param {Stream} s */
function resumeUrl(s) {
  const url = new URL(s.url);
  url.searchParams.set("last_event_id", s.last);
  return url.href;
}

/** Asks once what a stream that never opened answers: the browser says only that it failed. @param {Stream} s */
async function probe(s) {
  s.probed = true;
  const stop = new AbortController();
  // A stream that answers nothing at all holds the question open: it is let go of.
  const giveUp = setTimeout(() => stop.abort(), PROBE_LIMIT);
  try {
    const response = await fetchHere(s.url, { headers: { accept: "text/event-stream", "x-ui-probe": "1" }, signal: stop.signal, cache: "no-store" });
    stop.abort();
    // A 204 is the stream's way of saying "stop"; a 401 or 403 says the reader may not have it.
    if (FINAL.has(response.status) && !s.es && streams.get(s.url) === s) {
      close(s);
      s.stopped = true;
      paint(s, "closed");
    }
  } catch {} finally {
    clearTimeout(giveUp);
  }
}

/** @param {Stream} s */
function open(s) {
  // A retry that is pending is the only thing that reopens a failed stream.
  if (s.es || s.retry || s.stopped || hidden() || !s.subs.size) return;
  // A hub that heard us before is asked for what we missed; any other stream is caught up with.
  s.resuming = s.hub && !!s.last;
  const es = new EventSource(s.resuming ? resumeUrl(s) : s.url);
  s.es = es;
  s.listening = new Set();
  s.opened = false;
  s.token = token();
  es.addEventListener(RESUMED, (e) => {
    s.hub = true;
    note(s, e);
    if (!s.resuming) return;
    // Nothing to catch up with: what was missed follows as events.
    s.resuming = false;
    s.missed = false;
    clearTimeout(s.wait);
  });
  es.addEventListener(FRESH, (e) => {
    s.hub = true;
    note(s, e);
    if (!s.resuming) return;
    s.resuming = false;
    catchUp(s);
  });
  paint(s, s.failed ? "retry" : "connecting");
  // An event a stream names `open` or `error` reaches these handlers too, as a MessageEvent.
  es.onopen = (e) => {
    if (e instanceof MessageEvent) return;
    s.opened = true;
    s.probed = false;
    paint(s, "on");
    // Whatever was said while it was down was lost, unless the hub says it again.
    const owed = s.failed > 0 || s.missed;
    clearTimeout(s.steady);
    s.steady = /** @type {any} */ (setTimeout(() => (s.failed = 0), STEADY));
    if (!owed) {
      // A region drawn while it was changing catches up even so: it wasn't listening.
      for (const [el, sub] of s.subs) if (sub.owed) {
        sub.owed = false;
        fire(s, el);
      }
      return;
    }
    if (!s.resuming) {
      catchUp(s);
      return;
    }
    clearTimeout(s.wait);
    s.wait = /** @type {any} */ (setTimeout(() => {
      s.resuming = false;
      catchUp(s);
    }, CATCH_WAIT));
  };
  es.onerror = (e) => {
    if (e instanceof MessageEvent) return;
    // The browser would retry on its own schedule; ours is the same in every browser.
    es.close();
    s.es = null;
    clearTimeout(s.steady);
    // A tab just shown keeps its catch-up (s.wait) though the stream is down: the region need not wait for the stream.
    const reached = s.opened;
    s.opened = false;
    s.failed++;
    if (hidden()) return;
    paint(s, "retry");
    const wait = Math.min(LONGEST, FIRST * 2 ** (s.failed - 1));
    s.retry = /** @type {any} */ (setTimeout(() => {
      s.retry = 0;
      open(s);
    }, wait * (1 + Math.random() / 4)));
    if (!reached && !s.probed) probe(s);
  };
  listen(s);
}

/** @param {Stream} s */
function close(s) {
  clearTimeout(s.retry);
  clearTimeout(s.steady);
  clearTimeout(s.wait);
  s.retry = 0;
  s.es?.close();
  s.es = null;
}

/** Leaves the stream of a region that is gone or has moved. @param {Stream} s @param {Element} el */
function leave(s, el) {
  clearTimeout(s.subs.get(el)?.timer);
  s.subs.delete(el);
  if (s.subs.size) return;
  close(s);
  streams.delete(s.url);
}

/** Lets go of every region that is no longer in the page. */
function prune() {
  for (const s of [...streams.values()]) {
    for (const el of [...s.subs.keys()]) if (!el.isConnected) leave(s, el);
  }
  if (!streams.size) {
    watching?.disconnect();
    watching = undefined;
  }
}

/** @type {MutationObserver | undefined} */
let watching;
let pruning = 0;

/** Notices a region taken out of the page by anything at all, so its stream isn't left open. */
function watch() {
  if (watching || !streams.size) return;
  watching = new MutationObserver(() => {
    if (!pruning) pruning = /** @type {any} */ (setTimeout(() => {
      pruning = 0;
      prune();
    }, 100));
  });
  watching.observe(document, { childList: true, subtree: true });
}

/** Joins every region on the page to its stream, and lets go of those that left. */
function scanLive() {
  /** @type {Map<Element, {url: string, name: string}>} */
  const wanted = new Map();
  for (const el of every("[data-ui-live-src]")) {
    const raw = el.getAttribute("data-ui-live-src") || "";
    // A name must be a name an event can have; a region with none left listens for nothing.
    const names = (el.getAttribute("data-ui-live-event") || "message").split(/\s+/).filter((n) => NAME.test(n) && !OWN.has(n));
    // A stream on another site is no stream of ours.
    if (!el.id || !sameOrigin(raw) || !names.length) continue;
    wanted.set(el, { url: new URL(raw, location.href).href, names, key: names.join(" ") });
  }
  for (const s of [...streams.values()]) {
    for (const el of [...s.subs.keys()]) {
      const now = wanted.get(el);
      if (!el.isConnected || !now || now.url !== s.url || now.key !== s.subs.get(el)?.key) leave(s, el);
    }
  }
  for (const [el, { url, names, key }] of wanted) {
    let s = streams.get(url);
    if (!s) {
      s = { url, es: null, subs: new Map(), failed: 0, retry: 0, steady: 0, wait: 0, listening: new Set(), state: "connecting", missed: false, opened: false, probed: false, stopped: false, token: token(), changed: -Infinity, last: "", hub: false, resuming: false };
      streams.set(url, s);
    }
    if (!s.subs.has(el)) {
      const owed = el.hasAttribute("data-ui-live-catch-up");
      s.subs.set(el, { names, key, timer: 0, busy: false, dirty: false, since: 0, run: 0, owed: owed && !s.opened });
      if (owed && s.opened) fire(s, el);
    }
    el.setAttribute("data-live", hidden() ? "paused" : s.state);
    open(s);
    listen(s);
  }
  session();
  watch();
}

let rechecking = 0;

/**
 * A session that changed: an open stream is the old reader's, and a stopped one (refused with a 401 or
 * a 403) may be allowed now. Each is started again as the new reader, but no more often than every
 * SESSION_GAP, and a change that comes sooner is looked at again when the gap is over.
 */
function session() {
  clearTimeout(rechecking);
  rechecking = 0;
  const now = performance.now();
  let later = 0;
  for (const s of streams.values()) {
    if (!(s.es || s.stopped) || s.token === token()) continue;
    const wait = SESSION_GAP - (now - s.changed);
    if (wait > 0) {
      later = later ? Math.min(later, wait) : wait;
      continue;
    }
    s.changed = now;
    close(s);
    s.stopped = false;
    s.failed = 0;
    s.missed = true;
    // Another reader: what the page shows was drawn for the last one, so it all comes again.
    s.last = "";
    if (hidden()) paint(s, "paused");
    open(s);
  }
  if (later) rechecking = /** @type {any} */ (setTimeout(session, later + 50));
}

/** The page is out of sight: no connection is held for it. */
function pause() {
  for (const s of streams.values()) {
    close(s);
    s.missed = true;
    if (!s.stopped) paint(s, "paused");
  }
}

/** The page is back: connect, and catch up when the connection is there, so nothing falls between. */
function resume() {
  for (const s of streams.values()) {
    if (s.stopped) continue;
    s.failed = 0;
    s.missed = true;
    paint(s, "connecting");
    open(s);
    // A stream that says nothing at all (a browser reports it open only with the first bytes) must not keep a region stale.
    s.wait = /** @type {any} */ (setTimeout(() => s.missed && catchUp(s), CATCH_WAIT));
  }
}

/** Starts watching the page for live regions. */
function startLive() {
  on(document, "ui:swap", scanLive);
  on(document, "visibilitychange", () => (hidden() ? pause() : resume()));
  // A page kept for the back button holds no connection either.
  on(window, "pagehide", pause);
  on(window, "pageshow", (/** @type {PageTransitionEvent} */ e) => {
    if (e.persisted) resume();
  });
  scanLive();
}

startLive();
