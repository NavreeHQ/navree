// Link boosting and regions: fetch a page, morph what changed, keep the shell
// alive. A failed fetch falls back to a real navigation.
import { morph } from "./morph.js";
import { announce } from "./ui.js";

const HEADER = { "x-navree-ui": "1" };
const PREFETCH_MS = 65;
const PREFETCH_TTL = 10000;

/** @type {Map<string, {at: number, page: Promise<Response>}>} */
const prefetched = new Map();
let generation = 0;

/** @param {string} url */
function fetchPage(url) {
  const hit = prefetched.get(url);
  prefetched.delete(url);
  if (hit && performance.now() - hit.at < PREFETCH_TTL) return hit.page;
  return fetch(url, { headers: HEADER });
}

/** @param {string} url */
function prefetch(url) {
  if (prefetched.has(url) || /** @type {any} */ (navigator).connection?.saveData) return;
  if (prefetched.size > 8) prefetched.clear();
  const page = fetch(url, { headers: HEADER, priority: "low" });
  page.catch(() => prefetched.delete(url));
  prefetched.set(url, { at: performance.now(), page });
}

/** Adds the stylesheets and module scripts of `doc` that this page lacks. @param {Document} doc */
function mergeHead(doc) {
  document.title = doc.title;
  for (const el of doc.head.querySelectorAll("link[rel=stylesheet],script[src]")) {
    const attr = el.nodeName === "LINK" ? "href" : "src";
    const url = el.getAttribute(attr) || "";
    if (!document.head.querySelector(`[${attr}="${CSS.escape(url)}"]`)) document.head.append(document.importNode(el, true));
  }
}

/**
 * Shows a fetched HTML response in this page.
 * @param {Response} response
 * @param {{region?: string, push?: boolean, url?: string}} [how]
 */
export async function showPage(response, how = {}) {
  const url = how.url || response.url;
  const doc = new DOMParser().parseFromString(await response.text(), "text/html");
  const build = doc.documentElement.getAttribute("data-build");
  // A page from another build, or one without the runtime, loads for real.
  if (build !== document.documentElement.getAttribute("data-build")) {
    location.assign(url);
    return;
  }
  if (how.push !== false && url !== location.href) history.pushState(null, "", url);
  if (how.region) {
    const from = document.getElementById(how.region), to = doc.getElementById(how.region);
    if (from && to) {
      morph(from, to);
      document.title = doc.title;
      return;
    }
  }
  mergeHead(doc);
  morph(document.body, doc.body);
  if (how.push !== false) scrollTo(0, 0);
  /** @type {HTMLElement | null} */ (document.querySelector("main"))?.focus({ preventScroll: true });
  announce(doc.title);
}

/** Goes to `url` like a boosted link. @param {string} url @param {{region?: string, push?: boolean}} [how] */
export async function go(url, how = {}) {
  const mine = ++generation;
  const root = document.documentElement;
  root.setAttribute("data-ui-loading", "");
  try {
    const response = await fetchPage(url);
    if (mine !== generation) return;
    if (!(response.headers.get("content-type") || "").startsWith("text/html")) {
      location.assign(url);
      return;
    }
    await showPage(response, { ...how, url: response.url || url });
  } catch {
    location.assign(url);
  } finally {
    if (mine === generation) root.removeAttribute("data-ui-loading");
  }
}

/** Changes the address bar. @param {string} href @param {string} mode */
export function setUrl(href, mode) {
  if (mode === "replace") history.replaceState(null, "", href);
  else history.pushState(null, "", href);
}

/** The newest fetch of each region: an older answer is dropped. @type {Map<string, number>} */
const regions = new Map();

/** Refetches a region. @param {string} id */
export async function refresh(id) {
  const el = document.getElementById(id);
  const src = el?.getAttribute("data-ui-src");
  if (!el || !src) return;
  const mine = (regions.get(id) || 0) + 1;
  regions.set(id, mine);
  try {
    const response = await fetch(src, { headers: HEADER });
    if (regions.get(id) !== mine || !response.ok) return;
    const doc = new DOMParser().parseFromString(await response.text(), "text/html");
    let to = doc.getElementById(id);
    if (!to) {
      to = /** @type {HTMLElement} */ (el.cloneNode(false));
      to.append(...doc.body.childNodes);
    }
    to.removeAttribute("data-ui-lazy");
    to.removeAttribute("aria-busy");
    morph(el, to);
  } catch {}
}

/** Starts lazy and polling regions, on load and after each swap. */
export function startRegions() {
  for (const el of document.querySelectorAll("[data-ui-lazy]")) refresh(el.id);
  for (const el of document.querySelectorAll("[data-ui-poll]")) {
    if (/** @type {any} */ (el).uiPoll) continue;
    const every = Number(el.getAttribute("data-ui-poll")) || 5000;
    /** @type {any} */ (el).uiPoll = setInterval(() => {
      if (!el.isConnected) clearInterval(/** @type {any} */ (el).uiPoll);
      else if (!document.hidden) refresh(el.id);
    }, every);
  }
}

/** @param {EventTarget | null} target */
function linkOf(target) {
  const a = /** @type {HTMLAnchorElement | null} */ (/** @type {Element} */ (target)?.closest?.("a[href]"));
  if (!a || a.origin !== location.origin || a.target || a.hasAttribute("download") || a.hasAttribute("data-ui-noboost")) return null;
  if (!document.body.hasAttribute("data-ui-boost")) return null;
  // A link to a place on this page is the browser's.
  if (a.pathname === location.pathname && a.search === location.search && a.hash) return null;
  return a;
}

let intent = 0;
/** @param {Event} e */
function onIntent(e) {
  const a = linkOf(e.target);
  if (!a) return;
  clearTimeout(intent);
  intent = setTimeout(() => prefetch(a.href), PREFETCH_MS);
}

/** Wires up links. */
export function startNav() {
  document.addEventListener("click", (e) => {
    if (e.defaultPrevented || e.button || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return;
    const a = linkOf(e.target);
    if (!a) return;
    e.preventDefault();
    const region = a.closest("[data-ui-region]");
    go(a.href, region && a.hasAttribute("data-ui-in-region") ? { region: region.id } : {}).then(startRegions);
  });
  document.addEventListener("mouseover", onIntent, { passive: true });
  document.addEventListener("focusin", onIntent, { passive: true });
  document.addEventListener("touchstart", (e) => {
    const a = linkOf(e.target);
    if (a) prefetch(a.href);
  }, { passive: true });
  addEventListener("popstate", () => {
    go(location.href, { push: false }).then(startRegions);
  });
}
