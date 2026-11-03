// Overlays: placing popovers next to what opens them (one code path), and the
// hover and focus behaviour of tooltips and hover cards.
import { byId, closest, on, one } from "./dom.js";

const GAP = 4;
const MARGIN = 8;
const HOVER_DELAY = 300;

/** Popups showing now that are placed by script: they follow a scroll and a resize. @type {Set<HTMLElement>} */
const shown = new Set();

/** The side a list opened on, kept while it is open so it doesn't jump as its rows change. @type {WeakMap<HTMLElement, boolean>} */
const downs = new WeakMap();

/** @param {HTMLElement} pop @param {Element} anchor @param {boolean} [again] decide the side afresh */
function place(pop, anchor, again) {
  const a = anchor.getBoundingClientRect();
  // A list is at least as wide as its control; its own height is what the room allows.
  if (pop.hasAttribute("data-ui-match-width")) pop.style.minWidth = a.width + "px";
  const fit = pop.hasAttribute("data-ui-fit");
  if (fit) pop.style.maxHeight = "";
  // Layout sizes, not the box as drawn: a popup that is fading in is a little scaled.
  const p = { width: pop.offsetWidth, height: pop.offsetHeight };
  // `start` and `end` follow the text's direction (a submenu opens toward the end); the rest are screen sides.
  const rtl = getComputedStyle(anchor).direction === "rtl";
  const named = pop.getAttribute("data-side") || "bottom";
  const side = named === "end" ? (rtl ? "left" : "right") : named === "start" ? (rtl ? "right" : "left") : named;
  const vw = innerWidth, vh = Math.min(innerHeight, visualViewport ? visualViewport.height : innerHeight);
  let top, left;
  if (side === "left" || side === "right") {
    const room = side === "right" ? vw - a.right : a.left;
    const flip = room < p.width + GAP && (side === "right" ? a.left : vw - a.right) > room;
    left = (side === "right") !== flip ? a.right + GAP : a.left - p.width - GAP;
    // A submenu lines its first item up with the item that opened it (less the menu's padding).
    const align = pop.getAttribute("data-align");
    top = align === "start" ? a.top - GAP : align === "end" ? a.bottom - p.height + GAP : a.top + (a.height - p.height) / 2;
  } else {
    const below = vh - a.bottom, above = a.top;
    const room = side === "bottom" ? below : above;
    // A list that grows (a search is on its way) is placed for the height it may reach.
    const want = pop.hasAttribute("data-ui-grow") ? Math.max(p.height, Math.min(parseFloat(getComputedStyle(pop).maxHeight) || 0, 260)) : p.height;
    const flip = room < want + GAP && (side === "bottom" ? above : below) > room;
    const kept = !again && shown.has(pop) ? downs.get(pop) : undefined;
    const down = kept ?? (side === "bottom") !== flip;
    downs.set(pop, down);
    let height = p.height;
    if (fit) {
      const space = Math.max(96, (down ? below : above) - GAP - MARGIN);
      if (height > space) pop.style.maxHeight = space + "px";
      height = Math.min(height, space);
    }
    top = down ? a.bottom + GAP : a.top - height - GAP;
    // Start and end are logical: they swap in a right-to-left page.
    const align = pop.getAttribute("data-align");
    const edge = align === "start" ? (rtl ? "r" : "l") : align === "end" ? (rtl ? "l" : "r") : "c";
    left = edge === "l" ? a.left : edge === "r" ? a.right - p.width : a.left + (a.width - p.width) / 2;
  }
  pop.style.left = Math.max(MARGIN, Math.min(left, vw - p.width - MARGIN)) + "px";
  pop.style.top = Math.max(MARGIN, Math.min(top, vh - Math.min(p.height, vh - 2 * MARGIN) - MARGIN)) + "px";
}

/** @param {HTMLElement} pop */
function anchorOf(pop) {
  const id = CSS.escape(pop.id);
  return one(`[popovertarget="${id}"],[data-ui-hover="${id}"],[data-ui-anchor="${id}"]`);
}

on(document, "toggle", (e) => {
  const pop = /** @type {HTMLElement} */ (e.target);
  if (!pop.hasAttribute?.("data-ui-place")) return;
  if (/** @type {ToggleEvent} */ (e).newState !== "open") {
    shown.delete(pop);
    return;
  }
  const anchor = anchorOf(pop);
  if (anchor) place(pop, anchor);
  shown.add(pop);
}, true);

// A popup opened by script asks to be placed in the same frame it shows in, so it is
// never seen where the browser first puts it.
on(document, "ui:place", (e) => {
  const pop = /** @type {HTMLElement} */ (e.target);
  const anchor = anchorOf(pop);
  if (anchor) place(pop, anchor);
  shown.add(pop);
}, true);

let frame = 0;
/** @param {Event} e */
function follow(e) {
  if (!shown.size || frame) return;
  // Scrolling inside a popup moves nothing under it. A resize has the window for its target.
  const inside = e.target;
  if (inside instanceof Node) for (const pop of shown) if (pop.contains?.(inside)) return;
  frame = requestAnimationFrame(() => {
    frame = 0;
    for (const pop of shown) {
      const anchor = pop.isConnected && pop.matches(":popover-open") ? anchorOf(pop) : null;
      if (anchor) place(pop, anchor, true);
      else shown.delete(pop);
    }
  });
}
on(window, "scroll", follow, { capture: true, passive: true });
on(window, "resize", follow, { passive: true });

/** @type {Map<string, number>} */
const timers = new Map();

/** @param {Element} trigger @param {boolean} show */
function hover(trigger, show) {
  const id = trigger.getAttribute("data-ui-hover") || "";
  const pop = byId(id);
  if (!pop) return;
  clearTimeout(timers.get(id));
  timers.set(id, setTimeout(() => {
    try {
      if (show) pop.showPopover();
      else pop.hidePopover();
    } catch {}
  }, show ? HOVER_DELAY : 100));
}

for (const [event, show] of /** @type {[string, boolean][]} */ ([["mouseover", true], ["focusin", true], ["mouseout", false], ["focusout", false]])) {
  on(document, event, (e) => {
    const trigger = closest(e.target, "[data-ui-hover]");
    if (trigger) {
      hover(trigger, show);
      return;
    }
    // Moving onto an open hover card keeps it open.
    const card = closest(e.target, "[data-slot=hover-card-content]");
    const opener = card && one(`[data-ui-hover="${CSS.escape(card.id)}"]`);
    if (opener) hover(opener, show);
  }, { passive: true });
}
on(document, "keydown", (e) => {
  if (e.key !== "Escape") return;
  for (const id of timers.keys()) {
    try { byId(id)?.hidePopover(); } catch {}
  }
});
