// Sortable grids: tiles put in order by pointer, touch or keys, and taken away. The
// page changes at once; a moment after the last change the whole order is posted
// through the grid's own form, which the core runtime sends (one at a time, the
// latest kept when changes come quickly) and whose answer's ops apply.
//
// Pointer: a press that moves 4 px with a mouse or a pen starts a drag. On touch the
// handle starts one at once, and anywhere else on the tile a press held still for
// 300 ms does, so a swipe still scrolls the page. Keys, on the handle: Space or Enter
// picks the tile up, the arrows move it (up and down by a row, left and right as the
// page reads), Home and End go to the ends, Space or Enter drops it, Escape and Tab
// put it back. Every step is said in the grid's own live region.
//
// Every read of a form goes through the prototypes: the grid's form holds fields
// named `order` and `remove`, which shadow the form's own properties.
import { all, attr, body, closest, holds, make, on, set, unset } from "./dom.js";

const ROOT = "[data-ui-grid]";
const ITEM = "[data-slot=sortable-grid-item]";
const HANDLE = "[data-slot=sortable-grid-handle]";
/** What a press on starts no drag: the tile's own controls, its handle aside. */
const CONTROL = "a,button,input,select,textarea,summary,label,[contenteditable],[data-ui-nodrag]";
const SLOP = 4, TOUCH_SLOP = 8, HOLD = 300, EASE = 180, EDGE = 48;

const E = Element.prototype, NP = Node.prototype;
const still = () => matchMedia("(prefers-reduced-motion: reduce)").matches;
/** @param {Node} parent @param {Node} node @param {Node | null} before */
const insert = (parent, node, before) => {
  const own = /** @type {any} */ (E).moveBefore;
  if (own && parent.isConnected && node.isConnected) {
    try {
      return own.call(parent, node, before);
    } catch {}
  }
  NP.insertBefore.call(parent, node, before);
};

/** @typedef {{root: Element, item: HTMLElement, keys: boolean, next: Node | null, ghost?: HTMLElement, id?: number, dx: number, dy: number, x: number, y: number, frame: number}} Drag */
/** @type {Drag | null} */
let drag = null;
/** @type {{item: HTMLElement, root: Element, id: number, x: number, y: number, touch: boolean, handle: boolean, timer: number} | null} */
let press = null;
let swallow = false, moving = false;
/** A pending save for each grid. @type {Map<Element, number>} */
const timers = new Map();

/** @param {Element} root @returns {HTMLElement[]} */
const itemsOf = (root) => /** @type {HTMLElement[]} */ ([...all(root, ITEM)].filter((i) => closest(i, ROOT) === root));
/** @param {Element} root @param {string} selector */
const own = (root, selector) => [...all(root, selector)].find((el) => closest(el, ROOT) === root);
/** @param {Element} root */
const formOf = (root) => /** @type {HTMLFormElement | undefined} */ (own(root, "form"));
/** @param {Element} root @param {string} which @param {Record<string, string | number>} values */
function say(root, which, values) {
  let text = "";
  try {
    text = String(JSON.parse(attr(root, "data-l") || "{}")[which] || "");
  } catch {}
  text = text.replace(/\{(\w+)\}/g, (all, name) => (name in values ? String(values[name]) : all));
  const status = own(root, "[data-slot=sortable-grid-status]");
  if (status && text) status.textContent = status.textContent === text ? text + " " : text;
}
/** What to say about `item` where it is now. @param {Element} root @param {Element} item */
const about = (root, item) => {
  const items = itemsOf(root);
  return { item: attr(item, "data-name") || attr(item, "data-key") || "", position: items.indexOf(/** @type {HTMLElement} */ (item)) + 1, total: items.length };
};
/** @param {Element} item */
const grip = (item) => /** @type {HTMLElement | undefined} */ (all(item, HANDLE)[0])?.focus({ preventScroll: false });

/** Where every tile of `root` is drawn now. @param {Element} root */
const measure = (root) => new Map(itemsOf(root).map((item) => [item, item.getBoundingClientRect()]));
/** Slides each tile from where it was to where it is, unless the reader asks for less motion. @param {Map<Element, DOMRect>} before @param {Element | null} [skip] */
function slide(before, skip = null) {
  if (still()) return;
  for (const [item, was] of before) {
    if (item === skip || !item.isConnected) continue;
    const is = item.getBoundingClientRect();
    const dx = was.left - is.left, dy = was.top - is.top;
    if (Math.abs(dx) >= 1 || Math.abs(dy) >= 1) item.animate([{ translate: `${dx}px ${dy}px` }, { translate: "0 0" }], { duration: EASE, easing: "cubic-bezier(.2,.7,.2,1)" });
  }
}
/** Puts `item` before `before` among `root`'s tiles, the move drawn as a slide. @param {Element} root @param {HTMLElement} item @param {Node | null} before */
function put(root, item, before) {
  const box = item.parentNode;
  if (!box || before === item || item.nextSibling === before) return false;
  const rects = measure(root);
  moving = true;
  insert(box, item, before);
  moving = false;
  slide(rects, drag?.keys ? null : item);
  return true;
}

/** The order the page shows, written into the grid's form; then the post, after the grid's pause. @param {Element} root @param {string} [removed] */
function changed(root, removed) {
  const form = formOf(root);
  if (!form) return;
  for (const input of all(form, "input[name=order]")) E.remove.call(input);
  const keys = itemsOf(root).map((item) => attr(item, "data-key") || "");
  const field = (name, value) => {
    const input = make("input");
    set(input, "type", "hidden");
    set(input, "name", name);
    set(input, "value", value);
    NP.appendChild.call(form, input);
  };
  for (const key of keys) field("order", key);
  if (removed !== undefined) field("remove", removed);
  // The buttons of a page with no script, kept true for one that loses it.
  const items = itemsOf(root);
  items.forEach((item, at) => {
    for (const step of all(item, "[data-slot=sortable-grid-step]")) {
      const end = attr(step, "name") === "up" ? at === 0 : at === items.length - 1;
      if (end) set(step, "disabled", "");
      else unset(step, "disabled");
    }
  });
  root.dispatchEvent(new CustomEvent("ui:grid-change", { bubbles: true, detail: { order: keys, removed } }));
  clearTimeout(timers.get(root));
  const wait = Number(attr(root, "data-ui-autosave"));
  timers.set(root, setTimeout(() => save(root), Number.isFinite(wait) ? Math.max(0, wait) : 500));
}
/** @param {Element} root */
function save(root) {
  timers.delete(root);
  const form = formOf(root);
  if (form?.isConnected) HTMLFormElement.prototype.requestSubmit.call(form);
}

/** Starts a pointer drag from the press. */
function begin() {
  if (!press || drag) return;
  const { item, root, id, x, y } = press;
  clearTimeout(press.timer);
  press = null;
  if (!item.isConnected) return;
  const rect = item.getBoundingClientRect();
  const ghost = /** @type {HTMLElement} */ (NP.cloneNode.call(item, true));
  for (const el of [ghost, ...all(ghost, "[id],[name],[form]")]) for (const name of ["id", "name", "form"]) unset(el, name);
  set(ghost, "data-ghost", "");
  set(ghost, "aria-hidden", "true");
  ghost.inert = true;
  ghost.style.width = rect.width + "px";
  ghost.style.height = rect.height + "px";
  body().append(ghost);
  drag = { root, item, keys: false, next: item.nextSibling, ghost, id, dx: x - rect.left, dy: y - rect.top, x, y, frame: 0 };
  try {
    item.setPointerCapture(id);
  } catch {}
  set(item, "data-dragging", "");
  set(root, "data-dragging", "");
  say(root, "picked", about(root, item));
  tick();
}

/** One frame of a pointer drag: the copy follows, the page scrolls near an edge, the tile finds its place. */
function tick() {
  if (!drag?.ghost) return;
  const { root, item, ghost, x, y } = drag;
  if (!item.isConnected) return end(false);
  ghost.style.translate = `${x - drag.dx}px ${y - drag.dy}px`;
  const by = y < EDGE ? -12 : y > innerHeight - EDGE ? 12 : 0;
  if (by) scrollBy(0, by);
  let target = null, best = Infinity;
  for (const other of itemsOf(root)) {
    const r = other.getBoundingClientRect();
    if (x >= r.left && x <= r.right && y >= r.top && y <= r.bottom) {
      target = other;
      break;
    }
    const d = Math.hypot(x - (r.left + r.right) / 2, y - (r.top + r.bottom) / 2);
    if (d < best && d < Math.max(r.width, r.height)) (best = d), (target = other);
  }
  if (target && target !== item) {
    const items = itemsOf(root);
    put(root, item, items.indexOf(item) < items.indexOf(target) ? target.nextSibling : target);
  }
  drag.frame = requestAnimationFrame(tick);
}

/** Ends the drag under way: dropped where it is, or put back; the keys stay on the handle unless they left. @param {boolean} keep @param {boolean} [left] */
function end(keep, left = false) {
  const was = drag;
  if (!was) return;
  drag = null;
  cancelAnimationFrame(was.frame);
  was.ghost?.remove();
  const { root, item } = was;
  unset(item, "data-dragging");
  unset(item, "data-lifted");
  unset(root, "data-dragging");
  if (!item.isConnected) return;
  const from = was.next;
  const moved = item.nextSibling !== from;
  if (!keep && moved && (!from || from.parentNode === item.parentNode)) put(root, item, from);
  if (was.keys && !left) grip(item);
  say(root, keep ? "dropped" : "cancelled", about(root, item));
  if (keep && moved) changed(root);
}

/** A press that may become a drag ends. */
function unpress() {
  if (press) clearTimeout(press.timer);
  press = null;
}

on(document, "pointerdown", (e) => {
  if (drag) {
    // A press elsewhere ends a move made with the keys.
    if (drag.keys && !holds(drag.item, /** @type {Node} */ (e.target))) end(false, true);
    return;
  }
  unpress();
  if (e.button !== 0 || !e.isPrimary) return;
  const item = /** @type {HTMLElement | null} */ (closest(e.target, ITEM));
  const root = item && closest(item, ROOT);
  if (!item || !root) return;
  const handle = !!closest(e.target, HANDLE);
  if (!handle && closest(e.target, CONTROL)) return;
  const touch = e.pointerType === "touch";
  press = { item, root, id: e.pointerId, x: e.clientX, y: e.clientY, touch, handle, timer: 0 };
  if (touch && !handle) press.timer = setTimeout(begin, HOLD);
});

on(document, "pointermove", (e) => {
  if (drag && !drag.keys && e.pointerId === drag.id) {
    drag.x = e.clientX;
    drag.y = e.clientY;
  } else if (press && e.pointerId === press.id) {
    const far = Math.hypot(e.clientX - press.x, e.clientY - press.y);
    // A finger that moves before the hold ends is a scroll.
    if (press.touch && !press.handle) far > TOUCH_SLOP && unpress();
    else if (far > SLOP) begin();
  }
});

for (const type of ["pointerup", "pointercancel"]) {
  on(document, type, (e) => {
    if (drag && !drag.keys && e.pointerId === drag.id) {
      swallow = true;
      setTimeout(() => (swallow = false));
      end(type === "pointerup");
    }
    unpress();
  });
}

// A drag on touch holds the page still, a held press opens no menu and selects no text,
// and the click that ends a drag is no click.
on(document, "touchmove", (e) => {
  if ((drag && !drag.keys) || (press?.touch && press.handle)) e.preventDefault();
}, { passive: false });
on(document, "contextmenu", (e) => {
  if (press?.touch || (drag && !drag.keys)) e.preventDefault();
});
on(document, "selectstart", (e) => press && e.preventDefault());
on(window, "blur", () => {
  unpress();
  if (drag && !drag.keys) end(false);
});
on(document, "click", (e) => {
  if (swallow) {
    swallow = false;
    e.preventDefault();
    e.stopPropagation();
    return;
  }
  const button = closest(e.target, "[data-slot=sortable-grid-remove]");
  const item = button && closest(button, ITEM);
  const root = item && closest(item, ROOT);
  if (!item || !root) return;
  // Taken away in the page at once; the post says so with the order.
  e.preventDefault();
  if (drag?.item === item) end(false);
  const items = itemsOf(root);
  const at = items.indexOf(/** @type {HTMLElement} */ (item));
  const next = items[at + 1] || items[at - 1];
  const rects = measure(root);
  const name = attr(item, "data-name") || "";
  E.remove.call(item);
  slide(rects);
  if (next) grip(next);
  else /** @type {HTMLElement} */ (root).focus();
  say(root, "removed", { item: name, total: items.length - 1 });
  changed(root, attr(item, "data-key") || "");
}, true);

on(document, "keydown", (e) => {
  if (drag && !drag.keys) {
    if (e.key === "Escape") end(false);
    return;
  }
  const handle = closest(e.target, HANDLE);
  const item = /** @type {HTMLElement | null} */ (handle && closest(handle, ITEM));
  const root = item && closest(item, ROOT);
  if (!item || !root || e.altKey || e.ctrlKey || e.metaKey) return;
  const key = e.key;
  if (!drag) {
    if (key !== " " && key !== "Enter") return;
    e.preventDefault();
    drag = { root, item, keys: true, next: item.nextSibling, dx: 0, dy: 0, x: 0, y: 0, frame: 0 };
    set(item, "data-lifted", "");
    say(root, "picked", about(root, item));
    return;
  }
  if (drag.item !== item) return;
  if (key === " " || key === "Enter" || key === "Escape") {
    e.preventDefault();
    return end(key !== "Escape");
  }
  const items = itemsOf(root);
  const at = items.indexOf(item);
  const top = items[0].getBoundingClientRect().top;
  const cols = Math.max(1, items.filter((i) => Math.abs(i.getBoundingClientRect().top - top) < 2).length);
  const back = getComputedStyle(root).direction === "rtl" ? 1 : -1;
  const step = { ArrowLeft: back, ArrowRight: -back, ArrowUp: -cols, ArrowDown: cols, Home: -at, End: items.length - 1 - at }[key];
  if (step === undefined) return;
  e.preventDefault();
  const to = Math.max(0, Math.min(items.length - 1, at + step));
  if (to === at) return say(root, "moved", about(root, item));
  put(root, item, to > at ? items[to].nextSibling : items[to]);
  grip(item);
  say(root, "moved", about(root, item));
});

// Leaving the handle while a tile is held puts it back, unless the page moved the handle itself.
on(document, "focusout", (e) => {
  if (drag?.keys && !moving && closest(e.target, HANDLE) && closest(e.target, ITEM) === drag.item) {
    end(false, true);
  }
});

// A page put away sends what it was about to.
on(document, "visibilitychange", () => {
  if (document.visibilityState === "hidden") for (const [root, timer] of timers) clearTimeout(timer), save(root);
});
