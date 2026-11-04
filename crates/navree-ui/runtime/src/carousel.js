// The carousel module. The track is the browser's own scroller with snap points, so it
// already scrolls by touch, wheel and keys, and each picker is a link to its slide. This
// makes the previous and next buttons work, keeps the buttons and the pickers in step
// with where the track is, gives the pickers the keys of the tabs pattern, and rotates
// the slides when asked, stopping for the pointer, for focus, for the pause button and
// for a reader who wants less motion. It is delegated from `document` and reads the
// page through dom.js, so a morph needs no wiring and a name in stored markup cannot
// shadow a method.
import { E, all, attr, closest, every, has, hidden, on, set, toggle } from "./dom.js";

const ROOT = "[data-ui-carousel]";
const SLIDE = "[data-slot=carousel-slide]";
const TAB = "[data-slot=carousel-picker]";

/** @param {Element} root @param {string} selector @returns {HTMLElement[]} */
const find = (root, selector) => /** @type {any} */ ([...all(root, selector)]);
/** @param {Element} root */
const track = (root) => find(root, "[data-slot=carousel-track]")[0];
/** @param {Element} root */
const column = (root) => attr(root, "data-orientation") === "vertical";
/** @param {Element} node */
const flipped = (node) => getComputedStyle(node).direction === "rtl";
const calm = () => matchMedia("(prefers-reduced-motion: reduce)").matches;

/** How far `slide` is from the start of the view, along the track: more when it is further on. @param {Element} root @param {Element} slide */
function away(root, slide) {
  const t = track(root);
  const a = t.getBoundingClientRect(), b = slide.getBoundingClientRect();
  return column(root) ? b.top - a.top : flipped(t) ? a.right - b.right : b.left - a.left;
}

/** The slide at the start of the view. @param {Element} root */
function here(root) {
  const slides = find(root, SLIDE);
  let at = 0;
  slides.forEach((slide, i) => {
    if (Math.abs(away(root, slide)) < Math.abs(away(root, slides[at]))) at = i;
  });
  return at;
}

/** Whether the track is at its start and at its end. @param {Element} root @returns {[boolean, boolean]} */
function ends(root) {
  const t = track(root), v = column(root);
  const at = Math.abs(v ? t.scrollTop : t.scrollLeft);
  const room = v ? t.scrollHeight - t.clientHeight : t.scrollWidth - t.clientWidth;
  return [at < 2, at > room - 2];
}

/** Scrolls slide `i` to the start of the view. @param {Element} root @param {number} i */
function go(root, i) {
  const t = track(root), slide = find(root, SLIDE)[i];
  if (!slide) return;
  const d = away(root, slide);
  /** @type {ScrollToOptions} */
  const to = { behavior: calm() ? "auto" : "smooth" };
  if (column(root)) to.top = d;
  else to.left = flipped(t) ? -d : d;
  t.scrollBy(to);
}

/** Moves by one slide; past the end it goes round when it may. @param {Element} root @param {number} by */
function step(root, by) {
  const [start, end] = ends(root);
  const last = find(root, SLIDE).length - 1;
  const round = has(root, "data-loop");
  let to = here(root) + by;
  if (by > 0 && end) to = round ? 0 : -1;
  else if (by < 0 && start) to = round ? last : -1;
  if (to >= 0) go(root, Math.min(to, last));
}

/** Brings the buttons and the pickers in line with the track. @param {Element} root */
function sync(root) {
  const [start, end] = ends(root);
  const at = here(root);
  for (const [name, off] of /** @type {[string, boolean][]} */ ([["prev", start], ["next", end]])) {
    for (const button of find(root, `[data-carousel=${name}]`)) set(button, "aria-disabled", String(off && !has(root, "data-loop")));
  }
  find(root, TAB).forEach((tab, i) => {
    set(tab, "aria-selected", String(i === at));
    tab.tabIndex = i === at ? 0 : -1;
  });
}

// ---- Rotation --------------------------------------------------------------

/** @type {WeakMap<Element, number>} */
const timers = new WeakMap();

/**
 * Starts or stops the rotation to match the reader: the pause button, a device that asks
 * for less motion, and, at each turn, the pointer or focus being on the carousel.
 * @param {Element} root
 */
function auto(root) {
  clearTimeout(timers.get(root));
  const ms = Number(attr(root, "data-autoplay"));
  if (!ms) return;
  const stopped = has(root, "data-stopped") || calm();
  const button = find(root, "[data-carousel=rotation]")[0];
  if (button) set(button, "aria-label", attr(button, has(root, "data-stopped") ? "data-play" : "data-pause") || "");
  set(track(root), "aria-live", stopped ? "polite" : "off");
  if (!stopped) {
    timers.set(root, window.setTimeout(() => {
      if (!root.isConnected) return;
      if (!hidden() && !E.matches.call(root, ":hover,:focus-within")) step(root, 1);
      auto(root);
    }, ms));
  }
}

// ---- Events ----------------------------------------------------------------

on(document, "click", (e) => {
  const button = closest(e.target, "[data-carousel]");
  const tab = closest(e.target, TAB);
  const root = closest(button || tab, ROOT);
  if (!root) return;
  if (tab) {
    e.preventDefault();
    go(root, find(root, TAB).indexOf(/** @type {HTMLElement} */ (tab)));
  } else if (attr(/** @type {Element} */ (button), "data-carousel") === "rotation") {
    toggle(root, "data-stopped");
    auto(root);
  } else if (attr(/** @type {Element} */ (button), "aria-disabled") !== "true") {
    step(root, attr(/** @type {Element} */ (button), "data-carousel") === "next" ? 1 : -1);
  }
});

// The pickers are tabs: the arrows move to a tab and open its slide, Home and End go to
// the first and last, and a space opens (a link answers Enter by itself).
on(document, "keydown", (e) => {
  const tab = closest(e.target, TAB);
  const root = closest(tab, ROOT);
  if (!tab || !root || e.altKey || e.ctrlKey || e.metaKey) return;
  const list = find(root, TAB);
  const i = list.indexOf(/** @type {HTMLElement} */ (tab));
  const k = e.key;
  const to = k === "Home" ? 0 : k === "End" ? list.length - 1 : k === " " ? i : k === "ArrowRight" ? i + (flipped(tab) ? -1 : 1) : k === "ArrowLeft" ? i + (flipped(tab) ? 1 : -1) : -2;
  if (to === -2) return;
  e.preventDefault();
  const next = list[(to + list.length) % list.length];
  next.focus();
  go(root, list.indexOf(next));
});

// A scroll event comes once a frame at most, so the track is read as it is scrolled.
on(document, "scroll", (e) => {
  const root = closest(e.target, ROOT);
  if (root) sync(root);
}, { capture: true, passive: true });
on(window, "resize", () => {
  for (const root of every(ROOT)) sync(root);
}, { passive: true });

// ---- Start -----------------------------------------------------------------

function enhance() {
  for (const root of every(ROOT)) {
    set(root, "data-ready", "");
    sync(root);
    auto(root);
  }
}
enhance();
on(document, "ui:swap", enhance);
