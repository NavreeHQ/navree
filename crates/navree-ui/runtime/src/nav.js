// Link boosting and regions: fetch a page, morph what changed, keep the shell
// alive. A failed fetch falls back to a real navigation.
import { body, byId, clean, cookies, every, head, hidden, make, on, one, ownModule, root, sameOrigin, setTitle, titleOf } from "./dom.js";
import { morph, pieces } from "./morph.js";
import { announce } from "./ui.js";

const HEADER = { "x-navree-ui": "1" };
// `Sec-Purpose` can't be set from `fetch`, so a prefetch says so itself.
const PREFETCH = { ...HEADER, "x-navree-prefetch": "1" };
const PREFETCH_MS = 65;
// A page with `no-store` may stand in only as a head start on the click it was
// fetched for; one with `max-age` for that long, up to the most anything stands in.
const HEAD_START = 3000;
const MOST = 10000;

/** @type {Map<string, {at: number, epoch: number, cookies: string, page: Promise<Response>}>} */
const prefetched = new Map();
let generation = 0;
/** Moves on whenever what a page shows may have changed. */
let epoch = 0;
/** Writes on their way: what is fetched meanwhile may be from before them. */
let writing = 0;
const tabs = "BroadcastChannel" in window ? new BroadcastChannel("navree-ui") : null;

function drop() {
  epoch++;
  prefetched.clear();
}
if (tabs) tabs.onmessage = drop;
on(window, "pageshow", (/** @type {PageTransitionEvent} */ e) => { if (e.persisted) drop(); });

/** A write or a change of user: nothing fetched before it may be shown, in any tab. */
export function forget() {
  drop();
  tabs?.postMessage(0);
}

/** Runs `write`, with no prefetch standing or started until it's over. @template T @param {() => Promise<T>} write */
export async function whileWriting(write) {
  writing++;
  forget();
  try {
    return await write();
  } finally {
    writing--;
    forget();
  }
}

/** The address of a page, for the map: absolute, with no fragment. @param {string} url */
function keyOf(url) {
  const key = new URL(url, location.href);
  key.hash = "";
  return key.href;
}

/** How long this answer may stand in for a fresh one, from when it was asked for. @param {Response} response */
function standsInFor(response) {
  const control = (response.headers.get("cache-control") || "").toLowerCase();
  if (control.includes("no-cache")) return 0;
  const age = /max-age=(\d+)/.exec(control);
  if (age && !control.includes("no-store")) return Math.min(Number(age[1]) * 1000, MOST);
  return HEAD_START;
}

/** The prefetched answer for `key`, if it is still true. @param {string} key */
async function standing(key) {
  const hit = prefetched.get(key);
  prefetched.delete(key);
  // A session or token cookie that changed means another user, whoever changed it.
  if (!hit || hit.cookies !== cookies()) return null;
  const response = await hit.page.catch(() => null);
  if (!response?.ok || response.redirected || hit.epoch !== epoch) return null;
  if (!(response.headers.get("content-type") || "").startsWith("text/html")) return null;
  return performance.now() - hit.at < standsInFor(response) ? response : null;
}

/** @param {string} url */
async function fetchPage(url) {
  return (await standing(keyOf(url))) || fetch(url, { headers: HEADER });
}

/** @param {string} url */
function prefetch(url) {
  const key = keyOf(url);
  if (writing || prefetched.has(key) || /** @type {any} */ (navigator).connection?.saveData) return;
  if (prefetched.size > 8) prefetched.clear();
  const page = fetch(url, { headers: PREFETCH, priority: "low" });
  page.catch(() => prefetched.delete(key));
  prefetched.set(key, { at: performance.now(), epoch, cookies: cookies(), page });
}

/**
 * Adds the stylesheets and the toolkit's modules of `doc` that this page lacks. A
 * script from `DOMParser` never runs, so a module gets a new element; no other
 * script loads, whatever the page's head says.
 * @param {Document} doc
 */
function mergeHead(doc) {
  setTitle(titleOf(doc));
  for (const el of head(doc).querySelectorAll("link[rel=stylesheet],script[src]")) {
    const attr = el.nodeName === "LINK" ? "href" : "src";
    const url = el.getAttribute(attr) || "";
    if (head().querySelector(`[${attr}="${CSS.escape(url)}"]`)) continue;
    if (el.nodeName === "LINK") head().append(el);
    else if (el.getAttribute("type") === "module" && ownModule(url)) {
      const script = /** @type {HTMLScriptElement} */ (make("script"));
      script.type = "module";
      script.src = url;
      head().append(script);
    }
  }
}

/**
 * Shows a fetched HTML response in this page.
 * @param {Response} response
 * @param {{region?: string, push?: boolean, url?: string, keep?: boolean}} [how] `keep`: the same page
 * again, so what was typed stays.
 */
export async function showPage(response, how = {}) {
  const url = how.url || response.url;
  const doc = new DOMParser().parseFromString(await response.text(), "text/html");
  const build = root(doc).getAttribute("data-build");
  // A page from another build, or one without the runtime, loads for real.
  if (build !== root().getAttribute("data-build")) {
    location.assign(url);
    return;
  }
  if (how.push !== false && url !== location.href) history.pushState(null, "", url);
  clean(body(doc));
  if (how.region) {
    const from = byId(how.region), to = byId(how.region, doc);
    if (from && to) {
      morph(from, to);
      setTitle(titleOf(doc));
      return;
    }
  }
  mergeHead(doc);
  morph(body(), body(doc), { reset: !how.region && !how.keep });
  if (how.push !== false) scrollTo(0, 0);
  /** @type {HTMLElement | null} */ (one("main"))?.focus({ preventScroll: true });
  announce(titleOf(doc));
  whenQuiet();
}

/** A table whose fetch failed keeps its rows and says so. @param {Element} table @param {string} url @param {number} status */
function tableFailed(table, url, status) {
  // What was done here ahead of the answer is undone: the table shows what the URL says.
  table.dispatchEvent(new CustomEvent("ui:failed", { bubbles: true }));
  const alert = table.querySelector("[data-slot=table-alert]");
  if (!alert) return;
  let words = {};
  try {
    words = JSON.parse(table.getAttribute("data-l") || "{}");
  } catch {}
  const text = alert.querySelector("[data-slot=alert-text]");
  if (text) text.textContent = (status === 401 || status === 403 ? /** @type {any} */ (words).lost : /** @type {any} */ (words).failed) || "";
  alert.querySelector("[data-ui-retry]")?.setAttribute("href", url);
  alert.removeAttribute("hidden");
}

/**
 * Marks a table busy, within the frame of the request. A table that already shows the
 * answer, sorted here, isn't busy: the request only confirms it.
 * @param {Element | null} table @param {boolean} on
 */
function tablePending(table, on) {
  if (on && table?.hasAttribute("data-ui-confirm")) return;
  table?.toggleAttribute("data-pending", on);
  if (on) table?.setAttribute("aria-busy", "true");
  else table?.removeAttribute("aria-busy");
}

/** Goes to `url` like a boosted link. @param {string} url @param {{region?: string, push?: boolean}} [how] */
export async function go(url, how = {}) {
  // Leaving the site is said by name: `leave`.
  if (!sameOrigin(url)) return;
  const mine = ++generation;
  const html = root();
  const region = how.region ? document.getElementById(how.region) : null;
  const table = region?.hasAttribute("data-ui-table") ? region : null;
  // A table sorted here already shows the answer: the request only confirms it.
  if (!table?.hasAttribute("data-ui-confirm")) html.setAttribute("data-ui-loading", "");
  tablePending(table, true);
  try {
    const response = await fetchPage(url);
    if (mine !== generation) return;
    if (table && !response.ok) {
      tableFailed(table, url, response.status);
      return;
    }
    if (!(response.headers.get("content-type") || "").startsWith("text/html")) {
      location.assign(url);
      return;
    }
    await showPage(response, { ...how, url: response.url || url });
  } catch {
    if (mine !== generation) return;
    if (table) tableFailed(table, url, 0);
    else location.assign(url);
  } finally {
    if (mine === generation) {
      html.removeAttribute("data-ui-loading");
      tablePending(table, false);
    }
  }
}

/** Goes to another site, for real: only to a web address, and with nothing of the runtime's. @param {string} href */
export function leave(href) {
  const url = new URL(href, location.href);
  if (url.protocol === "https:" || url.protocol === "http:") location.assign(url.href);
}

/** Changes the address bar. @param {string} href @param {string} mode */
export function setUrl(href, mode) {
  if (!sameOrigin(href)) return;
  if (mode === "replace") history.replaceState(null, "", href);
  else history.pushState(null, "", href);
}

/** The newest fetch of each region: an older answer is dropped. @type {Map<string, number>} */
const regions = new Map();

/**
 * Whether a region can use this answer. A redirect means the place moved, as when
 * a session ends and the answer is the login page: the user goes there, and its
 * form doesn't land inside the region. Only HTML is markup.
 * @param {Response} response
 */
function takes(response) {
  if (response.redirected) {
    if (sameOrigin(response.url)) location.assign(response.url);
    return false;
  }
  return response.ok && (response.headers.get("content-type") || "").startsWith("text/html");
}

/**
 * A feed region asks for what is newer than its cursor and adds it at the start
 * of its container. The rows are the server's own markup, as in an `append` op.
 * @param {Element} el @param {string} src @param {() => boolean} current
 */
async function feed(el, src, current) {
  const into = byId(el.getAttribute("data-ui-feed") || "");
  if (!into || !el.contains(into)) return;
  const url = new URL(src, location.href);
  url.searchParams.set("after", el.getAttribute("data-ui-after") || "0");
  const response = await fetch(url, { headers: HEADER });
  if (!current() || !takes(response)) return;
  const html = await response.text();
  if (!current()) return;
  if (html.trim()) {
    into.prepend(pieces(html));
    byId(el.getAttribute("data-ui-feed-empty") || "")?.remove();
  }
  const cursor = response.headers.get("x-ui-cursor");
  if (cursor) el.setAttribute("data-ui-after", cursor);
}

/** Refetches a region. @param {string} id */
export async function refresh(id) {
  const el = byId(id);
  const src = el?.getAttribute("data-ui-src");
  if (!el || !src) return;
  const mine = (regions.get(id) || 0) + 1;
  regions.set(id, mine);
  const table = el.hasAttribute("data-ui-table") ? el : null;
  try {
    if (el.hasAttribute("data-ui-feed")) return await feed(el, src, () => regions.get(id) === mine);
    const response = await fetch(src, { headers: HEADER });
    if (regions.get(id) !== mine) return;
    // The user moved on while this was on its way: it answers a state that's gone.
    if (table && (el.getAttribute("data-ui-src") !== src || el.hasAttribute("data-pending"))) return;
    if (table && !response.ok) {
      tableFailed(table, src, response.status);
      return;
    }
    if (!takes(response)) return;
    const doc = new DOMParser().parseFromString(await response.text(), "text/html");
    const build = root(doc).getAttribute("data-build");
    if (build && build !== root().getAttribute("data-build")) {
      location.reload();
      return;
    }
    clean(body(doc));
    let to = byId(id, doc);
    if (!to) {
      // A page with the runtime that lacks the region is somewhere else, not a fragment.
      if (build) {
        if (sameOrigin(response.url)) location.assign(response.url);
        return;
      }
      to = /** @type {HTMLElement} */ (el.cloneNode(false));
      to.append(...body(doc).childNodes);
    }
    to.removeAttribute("data-ui-lazy");
    to.removeAttribute("aria-busy");
    morph(el, to);
  } catch {
    if (table && regions.get(id) === mine) tableFailed(table, src, 0);
  }
}

/** Starts lazy and polling regions, on load and after each swap. */
export function startRegions() {
  for (const el of every("[data-ui-lazy]")) refresh(el.id);
  for (const el of every("[data-ui-poll]")) {
    if (/** @type {any} */ (el).uiPoll) continue;
    const every = Number(el.getAttribute("data-ui-poll")) || 5000;
    /** @type {any} */ (el).uiPoll = setInterval(() => {
      if (!el.isConnected) clearInterval(/** @type {any} */ (el).uiPoll);
      else if (!hidden()) refresh(el.id);
    }, every);
  }
}

/** @param {EventTarget | null} target */
function linkOf(target) {
  const a = /** @type {HTMLAnchorElement | null} */ (/** @type {Element} */ (target)?.closest?.("a[href]"));
  if (!a || a.origin !== location.origin || a.target || a.hasAttribute("download") || a.hasAttribute("data-ui-noboost")) return null;
  if (!body().hasAttribute("data-ui-boost")) return null;
  // A link to a place on this page is the browser's.
  if (a.pathname === location.pathname && a.search === location.search && a.hash) return null;
  return a;
}

/**
 * The link to fetch ahead of a click. A GET can change state (a sign-out written
 * as a link, a delete), and a fetch ahead costs bytes, so nothing is fetched unless the
 * page asks: a link marked `data-ui-prefetch`, or a region's own link (a table's sort
 * and page links) in a region marked so.
 * @param {EventTarget | null} target
 */
function safeLinkOf(target) {
  const a = linkOf(target);
  return a && (a.hasAttribute("data-ui-prefetch") || (a.hasAttribute("data-ui-in-region") && a.closest("[data-ui-region][data-ui-prefetch]"))) ? a : null;
}

/** How many links a page may have fetched as soon as it is quiet. */
const EAGER_MOST = 4;

/**
 * Fetches ahead the links marked `data-ui-prefetch="eager"`, as soon as the page is
 * quiet, with no hover. Only a link the page marks this way, at most four per page, never
 * while a write is on its way or the connection is slow or saving data. It pays most for a
 * page that is the same for every user and says so with `max-age`: a `no-store` answer
 * stands in for only a few seconds.
 */
function eager() {
  const connection = /** @type {any} */ (navigator).connection;
  if (/(^|-)2g$/.test(connection?.effectiveType || "")) return;
  let left = EAGER_MOST;
  for (const a of every("a[data-ui-prefetch=eager]")) {
    const link = safeLinkOf(a);
    if (!link) continue;
    prefetch(link.href);
    if (--left === 0) break;
  }
}

/** Runs `eager` once the page has nothing better to do. */
function whenQuiet() {
  if (!one("a[data-ui-prefetch=eager]")) return;
  if ("requestIdleCallback" in window) requestIdleCallback(eager, { timeout: 2000 });
  else setTimeout(eager, 500);
}

let intent = 0;
/** @param {Event} e */
function onIntent(e) {
  const a = safeLinkOf(e.target);
  if (!a) return;
  clearTimeout(intent);
  intent = setTimeout(() => prefetch(a.href), PREFETCH_MS);
}

/** Wires up links. */
export function startNav() {
  whenQuiet();
  on(document, "click", (e) => {
    if (e.defaultPrevented || e.button || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return;
    const a = linkOf(e.target);
    if (!a) return;
    e.preventDefault();
    // The press focused the link: its intent is spent, not a reason to fetch again.
    clearTimeout(intent);
    const region = a.closest("[data-ui-region]");
    go(a.href, region && a.hasAttribute("data-ui-in-region") ? { region: region.id } : {}).then(startRegions);
  });
  on(document, "mouseover", onIntent, { passive: true });
  on(document, "focusin", onIntent, { passive: true });
  on(document, "touchstart", (e) => {
    const a = safeLinkOf(e.target);
    if (a) prefetch(a.href);
  }, { passive: true });
  on(document, "click", (e) => {
    const button = /** @type {Element} */ (e.target).closest?.("[data-ui-refresh]");
    const region = button?.closest("[data-ui-region]");
    if (region) refresh(region.id);
  });
  on(window, "popstate", () => {
    // Back within a table's own states changes the table, not the page.
    const table = document.querySelector("[data-ui-table]");
    const same = table && new URL(table.getAttribute("data-ui-src") || "", location.href).pathname === location.pathname;
    go(location.href, same ? { push: false, region: table.id } : { push: false }).then(startRegions);
  });
}
