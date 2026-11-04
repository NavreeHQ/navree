// SplitPane: separators a person drags, or moves with the keys of the WAI-ARIA
// window splitter pattern (the arrows, Shift for a bigger step, Home, End, Enter to
// collapse and restore), panes that collapse, and the sizes posted to the server when
// the page names where. The server drew every size, so this moves only what a person
// moves, and a page with no script keeps its layout.
//
// A size is a custom property set through the CSSOM, which the content security
// policy allows; the server's whole percents stay in `data-split-basis`. Saves wait
// for a pause, one is out at a time, and changes made meanwhile go in one more save
// when it lands, so the last word is always the last thing done.
import { attr, byId, closest, cookie, every, has, on, sameOrigin, set, toggle, unset } from "./dom.js";

/** Percent per arrow key, and with Shift. */
const STEP = 2;
const BIG = 10;
/** Quiet time before a save, and how long one may take. */
const QUIET = 400;
const LIMIT = 10000;
const KEYS = new Set(["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown", "Home", "End", "Enter"]);

const E = Element.prototype;
/** The element's children, read through the prototype. @param {Element} el */
const kids = (el) => /** @type {Element[]} */ ([...(/** @type {any} */ (Object.getOwnPropertyDescriptor(E, "children")?.get?.call(el)) || [])]);
/** @param {Element} el @param {string} slot */
const isSlot = (el, slot) => attr(el, "data-slot") === slot;
/** @param {Element} root */
const panesOf = (root) => kids(root).filter((el) => isSlot(el, "split-pane"));
/** @param {Element} root */
const handlesOf = (root) => kids(root).filter((el) => isSlot(el, "split-handle"));
/** @param {Element} el */
const style = (el) => getComputedStyle(el);
/** Tenths in an attribute, as a percent. @param {Element} el @param {string} name @param {number} fallback */
const percentAttr = (el, name, fallback) => {
  const n = Number(attr(el, name));
  return attr(el, name) !== null && Number.isFinite(n) ? Math.min(100, Math.max(0, n / 10)) : fallback;
};

/** Whether the split shows one pane at a time. @param {Element} root */
const stacked = (root) => {
  const first = panesOf(root)[0];
  return !!first && style(first).getPropertyValue("--split-stacked").trim() === "1";
};
/** Whether a pane is in view. @param {Element} pane */
const shown = (pane) => style(pane).display !== "none";
/** @param {Element} pane */
const isFill = (pane) => has(pane, "data-split-fill");
/** @param {Element} pane */
const collapsible = (pane) => has(pane, "data-split-collapsible") || has(pane, "data-collapse-below");
/** @param {Element} pane */
const minOf = (pane) => percentAttr(pane, "data-split-min", 10);
/** @param {Element} pane */
const maxOf = (pane) => percentAttr(pane, "data-split-max", 90);

/** A pane's size in percent: what this page set, or what the server drew. @param {Element} pane */
function sizeOf(pane) {
  const own = parseFloat(/** @type {HTMLElement} */ (pane).style.getPropertyValue("--split-basis"));
  if (Number.isFinite(own)) return own;
  const basis = Number(attr(pane, "data-split-basis"));
  return percentAttr(pane, "data-split-size", Number.isFinite(basis) && attr(pane, "data-split-basis") !== null ? basis : 30);
}
/** @param {Element} pane @param {number} size */
function setSize(pane, size) {
  /** @type {HTMLElement} */ (pane).style.setProperty("--split-basis", `${Math.round(size * 10) / 10}%`);
}

/** The most `pane` may take now: its own most, and what the panes in view leave the fill pane. @param {Element} root @param {Element} pane */
function room(root, pane) {
  let fillMin = 0, others = 0;
  for (const p of panesOf(root)) {
    if (isFill(p)) fillMin = minOf(p);
    else if (p !== pane && shown(p)) others += sizeOf(p);
  }
  return Math.max(minOf(pane), Math.min(maxOf(pane), 100 - fillMin - others));
}

/** Sizes before a pane collapsed, to come back at. @type {WeakMap<Element, number>} */
const before = new WeakMap();

/** @param {Element} pane */
function collapse(pane) {
  if (!collapsible(pane) || isFill(pane)) return;
  before.set(pane, sizeOf(pane));
  unset(pane, "data-split-open");
  set(pane, "data-collapsed", "");
}
/** @param {Element} root @param {Element} pane @param {number} [size] */
function expand(root, pane, size) {
  unset(pane, "data-collapsed");
  if (!shown(pane)) set(pane, "data-split-open", "");
  setSize(pane, Math.min(room(root, pane), Math.max(minOf(pane), size ?? before.get(pane) ?? sizeOf(pane))));
}

/** The handle's primary pane, among its split's own. @param {Element} handle */
function primaryOf(handle) {
  const root = handle.parentElement;
  if (!root || !has(root, "data-ui-split")) return null;
  const id = attr(handle, "aria-controls");
  const pane = panesOf(root).find((p) => p.id === id);
  return pane ? { root, pane } : null;
}

/** Which way a pane grows as its separator moves right (or down): +1 or -1. @param {Element} root @param {Element} handle */
function sign(root, handle) {
  const after = has(handle, "data-split-after") ? -1 : 1;
  const vertical = attr(root, "data-orientation") === "vertical";
  return after * (!vertical && style(root).direction === "rtl" ? -1 : 1);
}

/** Says where each separator is, and whether each toggle's pane is open. @param {Element} root */
function paint(root) {
  for (const handle of handlesOf(root)) {
    const found = primaryOf(handle);
    if (!found) continue;
    const { pane } = found;
    const open = shown(pane);
    if (!has(handle, "tabindex")) set(handle, "tabindex", "0");
    set(handle, "aria-valuemin", String(collapsible(pane) ? 0 : minOf(pane)));
    set(handle, "aria-valuemax", String(Math.round(room(root, pane) * 10) / 10));
    set(handle, "aria-valuenow", String(open ? Math.round(sizeOf(pane) * 10) / 10 : 0));
    if (open) unset(handle, "aria-valuetext");
    else set(handle, "aria-valuetext", "Collapsed");
    toggle(handle, "data-collapsed-at", !open);
  }
  for (const button of every("[data-ui-split-toggle]")) {
    const pane = byId(attr(button, "data-ui-split-toggle") || "");
    if (pane && closest(pane, "[data-ui-split]") === root) set(button, "aria-expanded", String(shown(pane) && !stacked(root)));
  }
}

/** Every pane's size in percent with one decimal, the fill pane taking what is left, and the collapsed ones. @param {Element} root */
function layoutOf(root) {
  const panes = panesOf(root);
  const sizes = panes.map((p) => (isFill(p) ? 0 : Math.round(sizeOf(p) * 10) / 10));
  const used = panes.reduce((sum, p, i) => (isFill(p) || !shown(p) ? sum : sum + sizes[i]), 0);
  const fill = panes.findIndex(isFill);
  if (fill >= 0) sizes[fill] = Math.max(0, Math.round((100 - used) * 10) / 10);
  const collapsed = panes.flatMap((p, i) => (has(p, "data-collapsed") ? [i] : []));
  return { sizes, collapsed };
}

/** @typedef {{timer: number, busy: boolean, dirty: boolean}} Save */
/** @type {WeakMap<Element, Save>} */
const saves = new WeakMap();
/** Splits with a save waiting, to send when the page goes. @type {Set<Element>} */
const waiting = new Set();

/** Posts the split's sizes, now. @param {Element} root @param {boolean} [leaving] */
async function save(root, leaving = false) {
  const s = saves.get(root);
  const url = attr(root, "data-ui-split-save");
  if (!s || !url || !sameOrigin(url)) return;
  clearTimeout(s.timer);
  s.timer = 0;
  waiting.delete(root);
  if (s.busy) {
    s.dirty = true;
    return;
  }
  s.busy = true;
  s.dirty = false;
  const { sizes, collapsed } = layoutOf(root);
  const body = new URLSearchParams({ split: root.id, sizes: sizes.join(","), collapsed: collapsed.join(",") });
  /** @type {Record<string, string>} */
  const headers = { "content-type": "application/x-www-form-urlencoded", "x-navree-ui": "1" };
  const token = cookie("XSRF-TOKEN");
  if (token) headers["x-xsrf-token"] = token;
  const stop = new AbortController();
  const limit = setTimeout(() => stop.abort(), LIMIT);
  try {
    // A save the page is leaving with still goes.
    await fetch(new URL(url, location.href), { method: "POST", headers, body, signal: stop.signal, credentials: "same-origin", keepalive: leaving });
  } catch {} finally {
    clearTimeout(limit);
    s.busy = false;
    if (s.dirty && root.isConnected) save(root);
  }
}

/** A person changed the split: say so, and save after a pause. @param {Element} root */
function commit(root) {
  paint(root);
  root.dispatchEvent(new CustomEvent("split:change", { bubbles: true, detail: layoutOf(root) }));
  if (!attr(root, "data-ui-split-save")) return;
  let s = saves.get(root);
  if (!s) saves.set(root, (s = { timer: 0, busy: false, dirty: false }));
  clearTimeout(s.timer);
  waiting.add(root);
  s.timer = /** @type {any} */ (setTimeout(() => save(root), QUIET));
}

/**
 * Moves the primary pane of `handle` to `size`, or collapses it when it may and a drag
 * takes it under half its least.
 * @param {Element} root @param {Element} pane @param {number} size @param {boolean} [snap]
 */
function moveTo(root, pane, size, snap = false) {
  const min = minOf(pane);
  if (snap && collapsible(pane) && size < min / 2) {
    if (shown(pane)) collapse(pane);
    return;
  }
  const next = Math.min(room(root, pane), Math.max(min, size));
  if (!shown(pane)) expand(root, pane, next);
  else setSize(pane, next);
}

/** @type {{handle: Element, root: Element, pane: Element, id: number, start: number, from: number, length: number, sign: number, vertical: boolean, moved: boolean} | null} */
let drag = null;

on(document, "pointerdown", (/** @type {PointerEvent} */ e) => {
  const handle = closest(e.target, "[data-slot=split-handle]");
  if (!handle || e.button !== 0 || !e.isPrimary || drag) return;
  const found = primaryOf(handle);
  if (!found || stacked(found.root)) return;
  const { root, pane } = found;
  const vertical = attr(root, "data-orientation") === "vertical";
  const box = root.getBoundingClientRect();
  e.preventDefault();
  /** @type {HTMLElement} */ (handle).focus({ preventScroll: true });
  try {
    handle.setPointerCapture(e.pointerId);
  } catch {}
  drag = { handle, root, pane, id: e.pointerId, start: vertical ? e.clientY : e.clientX, from: shown(pane) ? sizeOf(pane) : 0, length: (vertical ? box.height : box.width) || 1, sign: sign(root, handle), vertical, moved: false };
  set(handle, "data-dragging", "");
  set(root, "data-split-dragging", "");
});

on(document, "pointermove", (/** @type {PointerEvent} */ e) => {
  if (!drag || e.pointerId !== drag.id) return;
  const at = drag.vertical ? e.clientY : e.clientX;
  const delta = ((at - drag.start) / drag.length) * 100 * drag.sign;
  if (!drag.moved && Math.abs(at - drag.start) < 2) return;
  drag.moved = true;
  moveTo(drag.root, drag.pane, drag.from + delta, true);
  paint(drag.root);
});

/** @param {PointerEvent} e */
function drop(e) {
  if (!drag || e.pointerId !== drag.id) return;
  const { handle, root, moved } = drag;
  drag = null;
  unset(handle, "data-dragging");
  unset(root, "data-split-dragging");
  if (moved) commit(root);
}
on(document, "pointerup", drop);
on(document, "pointercancel", drop);
on(document, "lostpointercapture", drop);

on(document, "keydown", (/** @type {KeyboardEvent} */ e) => {
  const handle = closest(e.target, "[data-slot=split-handle]");
  if (!handle || e.target !== handle || !KEYS.has(e.key) || e.altKey || e.ctrlKey || e.metaKey) return;
  const found = primaryOf(handle);
  if (!found) return;
  const { root, pane } = found;
  const vertical = attr(root, "data-orientation") === "vertical";
  const open = shown(pane);
  const size = open ? sizeOf(pane) : 0;
  const step = e.shiftKey ? BIG : STEP;
  // Right and Down move the separator toward the end; Left and Up toward the start.
  const toward = { ArrowRight: vertical ? 0 : 1, ArrowLeft: vertical ? 0 : -1, ArrowDown: vertical ? 1 : 0, ArrowUp: vertical ? -1 : 0 }[e.key];
  if (toward === 0) return;
  e.preventDefault();
  if (e.key === "Enter") {
    if (!collapsible(pane)) return;
    if (open) collapse(pane);
    else expand(root, pane);
  } else if (e.key === "Home") {
    if (collapsible(pane)) {
      if (open) collapse(pane);
    } else moveTo(root, pane, minOf(pane));
  } else if (e.key === "End") moveTo(root, pane, room(root, pane));
  else if (toward !== undefined) {
    const grow = toward * sign(root, handle);
    if (!open && grow < 0) return;
    moveTo(root, pane, open ? size + grow * step : minOf(pane));
  }
  commit(root);
});

on(document, "click", (/** @type {MouseEvent} */ e) => {
  const button = closest(e.target, "[data-ui-split-toggle]");
  if (!button || e.defaultPrevented || e.button !== 0 || e.ctrlKey || e.metaKey || e.shiftKey || e.altKey) return;
  const pane = byId(attr(button, "data-ui-split-toggle") || "");
  const root = pane && closest(pane, "[data-ui-split]");
  // With the panes stacked, the toggle is a link to the page that shows the pane.
  if (!pane || !root || stacked(root) || isFill(pane)) return;
  e.preventDefault();
  if (shown(pane)) collapse(pane);
  else expand(root, pane);
  commit(root);
});

/** @type {WeakSet<Element>} */
const watched = new WeakSet();
const resized = typeof ResizeObserver === "function" ? new ResizeObserver((entries) => entries.forEach((entry) => paint(entry.target))) : null;

/** Gives every separator its value and its tab stop, and watches each split's width. */
function scan() {
  for (const root of every("[data-ui-split]")) {
    paint(root);
    if (!watched.has(root)) {
      watched.add(root);
      resized?.observe(root);
    }
  }
}

on(document, "ui:swap", scan);
// A save that is waiting goes before the page does.
on(window, "pagehide", () => {
  for (const root of [...waiting]) save(root, true);
});
scan();
