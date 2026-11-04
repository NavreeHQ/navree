// The toolbar module. The server writes a labelled group of tools that wraps and works
// with no script; this makes it WAI-ARIA's toolbar: one tab stop, the arrow keys,
// Home and End, and, when the tools do not fit, the ones at the end move into the
// "More" menu. Each tool that can move has a twin in that menu from the start: this
// shows the twins of the tools it hides. Everything is delegated from `document` and
// read through dom.js, so a morph needs no wiring and a name in stored markup can't
// shadow a method.
import { active, all, attr, closest, every, has, on, set, toggle, unset } from "./dom.js";

const ROOT = "[data-ui-toolbar]";
const KEEP = "data-ui-keep-attrs role tabindex data-enhanced data-overflow hidden";
const CONTROL = "button,a[href],input:not([type=hidden])";

/** @param {Element} node @param {string} selector @returns {HTMLElement | null} */
const first = (node, selector) => /** @type {any} */ (all(node, selector)[0] || null);
/** @param {Element} node @param {string} selector @returns {HTMLElement[]} */
const list = (node, selector) => /** @type {any} */ ([...all(node, selector)]);
/** @param {Element} node */
const rtl = (node) => getComputedStyle(node).direction === "rtl";
/** @param {Element} bar */
const tools = (bar) => list(bar, ":scope > [data-slot=toolbar-tool]");
/** @param {Element} bar */
const moreOf = (bar) => first(bar, ":scope > [data-slot=toolbar-more]");

/** The controls a key reaches: not hidden, not disabled, not inside the open menu. @param {HTMLElement} bar */
function stops(bar) {
  const found = [];
  for (const tool of tools(bar)) {
    if (has(tool, "data-overflow")) continue;
    for (const control of list(tool, CONTROL)) {
      const el = /** @type {HTMLInputElement} */ (control);
      if (el.disabled || closest(el, "[popover]") || has(el, "hidden")) continue;
      found.push(el);
    }
  }
  const more = moreOf(bar);
  const trigger = more && first(more, "[data-slot-more]");
  if (trigger && !has(trigger, "hidden")) found.push(trigger);
  return found;
}

/** Makes `control` the one tab stop of its bar. @param {HTMLElement} bar @param {HTMLElement} control */
function rove(bar, control) {
  for (const other of list(bar, "[tabindex='0']")) if (other !== control && !closest(other, "[popover]")) set(other, "tabindex", "-1");
  set(control, "tabindex", "0");
}

/** @param {HTMLElement} bar */
function settle(bar) {
  const all_ = stops(bar);
  for (const control of all_) if (!has(control, "tabindex")) set(control, "tabindex", "-1");
  const has0 = all_.find((control) => attr(control, "tabindex") === "0");
  for (const control of all_) if (control !== has0) set(control, "tabindex", "-1");
  if (!has0 && all_[0]) set(all_[0], "tabindex", "0");
}

// ---- Overflow ------------------------------------------------------------

/** @param {HTMLElement} bar */
const fits = (bar) => bar.scrollWidth <= bar.clientWidth + 1;

/** Shows the twins of the tool `index`, or hides them. @param {HTMLElement} bar @param {number} index @param {boolean} on */
function twins(bar, index, on) {
  const more = moreOf(bar);
  if (!more) return;
  for (const twin of list(more, `[data-twin="${index}"]`)) toggle(twin, "hidden", !on);
}

/** Hides the separators of the menu that would stand first, last or twice in a row. @param {HTMLElement} bar */
function tidy(bar) {
  const more = moreOf(bar);
  const menu = more && first(more, "[data-ui-menu]");
  if (!menu) return;
  let shown = false;
  /** @type {HTMLElement | null} */
  let waiting = null;
  for (const entry of list(menu, ":scope > *")) {
    // A submenu's own popover is not an entry.
    if (has(entry, "hidden") || has(entry, "popover")) continue;
    if (attr(entry, "role") === "separator") {
      if (!shown || waiting) set(entry, "hidden", "");
      else waiting = entry;
    } else {
      shown = true;
      waiting = null;
    }
  }
  if (waiting) set(waiting, "hidden", "");
}

/** The width each bar had when it was last laid out: a resize report for the same width needs no second pass. @type {WeakMap<Element, number>} */
const widths = new WeakMap();

/** Puts the real tools and the twins of the tools that do not fit where they belong. @param {HTMLElement} bar */
function layout(bar) {
  const more = moreOf(bar);
  const trigger = more && first(more, "[data-slot-more]");
  const items = tools(bar);
  const focused = active();
  // Start from everything showing.
  for (const tool of items) {
    unset(tool, "data-overflow");
    twins(bar, Number(attr(tool, "data-tool")), false);
  }
  if (trigger) set(trigger, "hidden", "");
  const vertical = attr(bar, "data-orientation") === "vertical";
  if (!vertical && !fits(bar) && trigger && more) {
    unset(trigger, "hidden");
    // From the last tool back, those that may move do, until the rest fit.
    for (let at = items.length - 1; at >= 0 && !fits(bar); at--) {
      const tool = items[at];
      if (has(tool, "data-keep") || attr(tool, "data-kind") === "separator") continue;
      set(tool, "data-overflow", "");
      twins(bar, Number(attr(tool, "data-tool")), true);
    }
    // A separator left at the end of the bar goes with what it divided.
    for (let at = items.length - 1; at >= 0; at--) {
      const tool = items[at];
      if (has(tool, "data-overflow")) continue;
      if (attr(tool, "data-kind") === "separator") set(tool, "data-overflow", "");
      else break;
    }
    // Nothing could move: the button has nothing to show.
    if (!items.some((tool) => has(tool, "data-overflow") && attr(tool, "data-kind") !== "separator")) set(trigger, "hidden", "");
  }
  tidy(bar);
  mirror(bar);
  // Focus on a tool that moved goes to the button that now holds it.
  if (focused && closest(focused, "[data-overflow]") && trigger && !has(trigger, "hidden")) {
    rove(bar, trigger);
    trigger.focus({ preventScroll: true });
  }
  widths.set(bar, bar.clientWidth);
  settle(bar);
}

/** The key a twin shares with its real input, if it is one this module wrote. @param {Element} input */
const keyOf = (input) => {
  const key = attr(input, "data-twin-key") || "";
  return /^\d+\.\d+$/.test(key) ? key : "";
};

/** The real input a twin follows. @param {HTMLElement} bar @param {string} key @returns {HTMLInputElement | null} */
const realOf = (bar, key) => {
  if (!key) return null;
  const found = first(bar, `:scope > [data-slot=toolbar-tool] input[data-twin-key="${key}"]`);
  return found instanceof HTMLInputElement ? found : null;
};

/** Makes each twin say what its real tool says. @param {HTMLElement} bar */
function mirror(bar) {
  const more = moreOf(bar);
  if (!more) return;
  for (const twin of list(more, "input[data-twin-key]")) {
    const real = realOf(bar, keyOf(twin));
    if (real && twin instanceof HTMLInputElement) twin.checked = real.checked;
  }
}

/** @type {WeakMap<Element, number>} */
const queued = new WeakMap();
/** @param {HTMLElement} bar */
function later(bar) {
  if (queued.has(bar)) return;
  queued.set(bar, requestAnimationFrame(() => {
    queued.delete(bar);
    if (bar.isConnected) layout(bar);
  }));
}

/** @type {WeakSet<Element>} */
const watched = new WeakSet();
const resizes =
  typeof ResizeObserver === "function"
    ? new ResizeObserver((entries) => {
        for (const entry of entries) {
          const bar = /** @type {HTMLElement} */ (entry.target);
          // Some engines report a resize again after the pass that already fitted the new width.
          if (widths.get(bar) !== bar.clientWidth) later(bar);
        }
      })
    : null;

// ---- Keys ----------------------------------------------------------------

on(document, "focusin", (e) => {
  const control = closest(e.target, CONTROL);
  const bar = control && /** @type {HTMLElement | null} */ (closest(control, ROOT));
  if (!control || !bar || closest(control, "[popover]") || !has(bar, "data-enhanced")) return;
  if (stops(bar).includes(/** @type {HTMLElement} */ (control))) rove(bar, /** @type {HTMLElement} */ (control));
});

on(document, "keydown", (e) => {
  if (e.defaultPrevented || e.ctrlKey || e.metaKey || e.altKey || e.shiftKey) return;
  const control = closest(e.target, CONTROL);
  const bar = /** @type {HTMLElement | null} */ (control && closest(control, ROOT));
  // A menu opened from the bar has its own keys.
  if (!control || !bar || closest(control, "[popover]") || !has(bar, "data-enhanced")) return;
  const vertical = attr(bar, "data-orientation") === "vertical";
  const flip = !vertical && rtl(bar);
  const next = vertical ? "ArrowDown" : flip ? "ArrowLeft" : "ArrowRight";
  const prev = vertical ? "ArrowUp" : flip ? "ArrowRight" : "ArrowLeft";
  const key = e.key;
  if (key !== next && key !== prev && key !== "Home" && key !== "End") return;
  const list_ = stops(bar);
  const at = list_.indexOf(/** @type {HTMLElement} */ (control));
  if (at < 0) return;
  const wrap = has(bar, "data-loop");
  let to = -1;
  if (key === "Home") to = 0;
  else if (key === "End") to = list_.length - 1;
  else if (key === next) to = at + 1 < list_.length ? at + 1 : wrap ? 0 : at;
  else to = at > 0 ? at - 1 : wrap ? list_.length - 1 : at;
  e.preventDefault();
  const target = list_[to];
  if (target && target !== control) {
    rove(bar, target);
    target.focus();
  }
});

// A twin that stands for a toggle or a choice hands its click to the real tool; the
// real tool, which can change by itself, is mirrored back.
on(document, "change", (e) => {
  const input = /** @type {HTMLInputElement | null} */ (closest(e.target, "input[data-twin-key]"));
  const bar = /** @type {HTMLElement | null} */ (input && closest(input, ROOT));
  if (!input || !bar) return;
  if (closest(input, "[data-slot=toolbar-more]")) {
    const real = realOf(bar, keyOf(input));
    if (real && real.checked !== input.checked) real.click();
  }
  mirror(bar);
});

function start() {
  for (const el of every(ROOT)) {
    const bar = /** @type {HTMLElement} */ (el);
    set(bar, "role", "toolbar");
    set(bar, "data-ui-keep-attrs", KEEP);
    if (attr(bar, "data-orientation") === "vertical") set(bar, "aria-orientation", "vertical");
    set(bar, "data-enhanced", "");
    if (resizes && !watched.has(bar)) {
      watched.add(bar);
      resizes.observe(bar);
    }
    layout(bar);
  }
}
start();
on(document, "ui:swap", start);
on(window, "load", start);
on(window, "pageshow", start);
// A named element in the page can stand in for `document.fonts`: it is read through the prototype.
const fonts = Object.getOwnPropertyDescriptor(Document.prototype, "fonts")?.get?.call(document);
if (fonts && fonts.ready) fonts.ready.then(start);
