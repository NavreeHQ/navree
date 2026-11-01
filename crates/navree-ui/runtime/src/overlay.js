// Overlays: placing popovers next to what opens them (one code path), and the
// hover and focus behaviour of tooltips and hover cards.
const GAP = 4;
const MARGIN = 8;
const HOVER_DELAY = 300;

/** @param {HTMLElement} pop @param {Element} anchor */
function place(pop, anchor) {
  const a = anchor.getBoundingClientRect();
  const p = pop.getBoundingClientRect();
  const side = pop.getAttribute("data-side") || "bottom";
  const vw = innerWidth, vh = innerHeight;
  let top, left;
  if (side === "left" || side === "right") {
    const room = side === "right" ? vw - a.right : a.left;
    const flip = room < p.width + GAP && (side === "right" ? a.left : vw - a.right) > room;
    left = (side === "right") !== flip ? a.right + GAP : a.left - p.width - GAP;
    top = a.top + (a.height - p.height) / 2;
  } else {
    const room = side === "bottom" ? vh - a.bottom : a.top;
    const flip = room < p.height + GAP && (side === "bottom" ? a.top : vh - a.bottom) > room;
    top = (side === "bottom") !== flip ? a.bottom + GAP : a.top - p.height - GAP;
    const align = pop.getAttribute("data-align");
    left = align === "start" ? a.left : align === "end" ? a.right - p.width : a.left + (a.width - p.width) / 2;
  }
  pop.style.left = Math.max(MARGIN, Math.min(left, vw - p.width - MARGIN)) + "px";
  pop.style.top = Math.max(MARGIN, Math.min(top, vh - p.height - MARGIN)) + "px";
}

/** @param {HTMLElement} pop */
function anchorOf(pop) {
  const id = CSS.escape(pop.id);
  return document.querySelector(`[popovertarget="${id}"],[data-ui-hover="${id}"]`);
}

document.addEventListener("toggle", (e) => {
  const pop = /** @type {HTMLElement} */ (e.target);
  if (/** @type {ToggleEvent} */ (e).newState !== "open" || !pop.hasAttribute?.("data-ui-place")) return;
  const anchor = anchorOf(pop);
  if (anchor) place(pop, anchor);
}, true);

/** @type {Map<string, number>} */
const timers = new Map();

/** @param {Element} trigger @param {boolean} show */
function hover(trigger, show) {
  const id = trigger.getAttribute("data-ui-hover") || "";
  const pop = document.getElementById(id);
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
  document.addEventListener(event, (e) => {
    const target = /** @type {Element} */ (e.target);
    const trigger = target.closest?.("[data-ui-hover]");
    if (trigger) {
      hover(trigger, show);
      return;
    }
    // Moving onto an open hover card keeps it open.
    const card = target.closest?.("[data-slot=hover-card-content]");
    const opener = card && document.querySelector(`[data-ui-hover="${CSS.escape(card.id)}"]`);
    if (opener) hover(opener, show);
  }, { passive: true });
}
document.addEventListener("keydown", (e) => {
  if (e.key !== "Escape") return;
  for (const id of timers.keys()) {
    try { document.getElementById(id)?.hidePopover(); } catch {}
  }
});
