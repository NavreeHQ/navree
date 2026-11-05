// Sortable lists and boards: items move within a list and between lists, by
// pointer, touch or keys.
//
// A drop moves the item in the page at once, fills in the item's own move form and
// posts it through the core runtime, one move at a time for each sortable, so the
// server sees them in the order they were made. A refusal puts the item back where
// the server last had it. A failure, or no answer in time, may still have landed:
// the lists are drawn again from the server before anything is said, and only a
// move that didn't land is. The server has the last word: here a list's limit and
// its refusal only show while dragging.
//
// Pointer: a press that moves 4 px with a mouse or a pen starts a drag. On touch the
// handle starts one at once, and anywhere else on the item a press held still for
// 300 ms does, so a swipe still scrolls the page. Keys, on the handle: Space or Enter
// picks the item up, the arrows move it (left and right to the next list that takes
// it), Home and End go to the ends, Space or Enter drops it, Escape puts it back.
// Every step is said in the sortable's own live region.
//
// Every read of the page goes through the prototypes, so no element named like a
// method (`<img name="elementFromPoint">`, `<input name="requestSubmit">`) changes
// what this does.
import { active, all, attr, body, closest, every, field, has, holds, on, root as docRoot, set, unset, D as DP, T as TP } from "./dom.js";

const ROOT = "[data-ui-sortable]";
const LIST = "[data-slot=sortable-list]";
const BOX = "[data-slot=sortable-items]";
const ITEM = "[data-slot=sortable-item]";
const HANDLE = "[data-slot=sortable-handle]";
const FORM = "form[data-ui-move]";
/** What a press on starts no drag: the item's own controls, its handle aside. */
const CONTROL = "button,input,select,textarea,summary,label,option,[contenteditable],[data-ui-nodrag],[data-slot=sortable-move]";
const SLOP = 4, TOUCH_SLOP = 8, HOLD = 300, EDGE = 48, STEP = 18, EASE = 180;

const NP = Node.prototype;
/** @param {Node} parent @param {Node} node @param {Node | null} before */
const insert = (parent, node, before) => {
  const own = /** @type {any} */ (Element.prototype).moveBefore;
  if (own && parent.isConnected && node.isConnected) {
    try {
      own.call(parent, node, before);
      return;
    } catch {}
  }
  NP.insertBefore.call(parent, node, before);
};
/** @param {EventTarget} target @param {Event} event */
const fire = (target, event) => TP.dispatchEvent.call(target, event);
const still = () => matchMedia("(prefers-reduced-motion: reduce)").matches;

/** @typedef {{list: string, next: string, prev?: string}} Place where an item is: its list's key, and the keys of the items after and before it ("" for none) */
/** @typedef {{root: Element, item: HTMLElement, mode: "pointer" | "keys", from: Place, over: Element | null, refused: string, ghost?: HTMLElement, id?: number, dx: number, dy: number, x: number, y: number, frame: number, dirty: boolean, scrollers: Element[], said: Element | null}} Drag */

/** The drag under way: one at a time in a page. @type {Drag | null} */
let drag = null;
/** A press that may become a drag. @type {{item: HTMLElement, root: Element, id: number, x: number, y: number, touch: boolean, handle: boolean, timer: number} | null} */
let press = null;
/** The click that ends a pointer drag is not a click on the item. */
let swallow = false;
/** Set while this module moves the focused handle, which blurs it for a moment in an engine with no `moveBefore`. */
let moving = false;

/** @param {Element} el */
const keyOf = (el) => attr(el, "data-key") || "";
/** @param {Element} list */
const listKey = (list) => attr(list, "data-list") || "";
/** @param {Element} el */
const shown = (el) => el.getClientRects().length > 0;
/** The items of `root`'s own lists. @param {Element} root */
const itemsOfRoot = (root) => listsOf(root).flatMap((list) => itemsOf(list));
/** The lists of `root`, not those of a sortable inside one of its items. @param {Element} root */
const listsOf = (root) => [...all(root, LIST)].filter((list) => closest(list, ROOT) === root);
/** @param {Element} list @returns {HTMLElement[]} */
const itemsOf = (list) => /** @type {HTMLElement[]} */ ([...all(list, ITEM)].filter((item) => closest(item, LIST) === list));
/** @param {Element} list */
const boxOf = (list) => [...all(list, BOX)].find((box) => closest(box, LIST) === list) || null;
/** @param {Element} item */
const listOf = (item) => closest(item, LIST);
/** The number a list says it holds, drawn or not, or null. @param {Element} list */
const totalOf = (list) => {
  const total = attr(list, "data-total");
  return total !== null && /^\d+$/.test(total) ? Number(total) : null;
};
/** How many items `list` holds, `leaving` left out: its total, which every move here keeps true. @param {Element} list @param {Element | null} [leaving] */
const sizeOf = (list, leaving = null) => {
  const total = totalOf(list);
  const here = !!leaving && listOf(leaving) === list;
  return total === null ? itemsOf(list).filter((i) => i !== leaving).length : Math.max(0, total - (here ? 1 : 0));
};
/** Adds `by` to the total of `list`. @param {Element | null} list @param {number} by */
const shiftTotal = (list, by) => {
  const total = list && totalOf(list);
  if (list && total !== null) set(list, "data-total", String(Math.max(0, total + by)));
};
/** @param {Element} list */
const titleOf = (list) => ([...all(list, "[data-slot=sortable-list-title]")].find((t) => closest(t, LIST) === list)?.textContent || listKey(list)).trim();
/** @param {Element} item */
const nextOf = (item) => item.nextElementSibling;
/** @param {Element} item @returns {Place} */
const placeOf = (item) => {
  const list = listOf(item), next = nextOf(item), prev = item.previousElementSibling;
  return { list: list ? listKey(list) : "", next: next ? keyOf(next) : "", prev: prev ? keyOf(prev) : "" };
};

/** The words of `root`, read from its `data-l` each time: a redraw may bring new ones. @param {Element} root @returns {Record<string, string>} */
function words(root) {
  try {
    return Object(JSON.parse(attr(root, "data-l") || "{}"));
  } catch {
    return {};
  }
}
/** @param {unknown} text @param {Record<string, string | number>} values */
const fill = (text, values) => String(text ?? "").replace(/\{(\w+)\}/g, (all, name) => (name in values ? String(values[name]) : all));

/** What to say about `item` where it is now. @param {Element} item */
function about(item) {
  const list = listOf(item);
  const items = list ? itemsOf(list).filter((i) => i === item || shown(i)) : [];
  return { item: attr(item, "data-name") || keyOf(item), list: list ? titleOf(list) : "", position: items.indexOf(/** @type {HTMLElement} */ (item)) + 1, total: list ? Math.max(items.length, sizeOf(list)) : items.length };
}

/** Says `text` in `root`'s live region. The same words twice are still said: a mark is added in turn. @param {Element} root @param {string} text */
function say(root, text) {
  const status = [...all(root, "[data-slot=sortable-status]")].find((s) => closest(s, ROOT) === root);
  if (!status || !text) return;
  status.textContent = status.textContent === text ? text + " " : text;
}

/** Why `list` won't take `item` brought from `origin`, or "" when it will. @param {Element} root @param {Element} list @param {Element} item @param {string} origin */
function refusal(root, list, item, origin) {
  if (listKey(list) === origin) return "";
  const refuse = attr(list, "data-refuse");
  if (refuse) return refuse;
  const limit = attr(list, "data-limit");
  if (limit !== null && /^\d+$/.test(limit) && sizeOf(list, item) >= Number(limit)) {
    return fill(words(root).full, { list: titleOf(list), limit });
  }
  return "";
}

/** Where every item of `scope` is drawn now. @param {Element} scope */
function measure(scope) {
  /** @type {Map<Element, DOMRect>} */
  const rects = new Map();
  for (const item of all(scope, ITEM)) if (!has(item, "data-ghost")) rects.set(item, item.getBoundingClientRect());
  return rects;
}

/**
 * Slides each item from where it was to where it is: the change is made at once and
 * drawn as a move. Items with no place before fade in.
 * @param {Map<Element, DOMRect>} before @param {Iterable<Element>} [now] @param {Element | null} [skip]
 */
function slide(before, now = before.keys(), skip = null) {
  if (still()) return;
  for (const item of now) {
    if (item === skip || !item.isConnected || !shown(item)) continue;
    const was = before.get(item);
    if (!was) {
      item.animate([{ opacity: 0 }, { opacity: 1 }], { duration: EASE });
      continue;
    }
    const is = item.getBoundingClientRect();
    const dx = was.left - is.left, dy = was.top - is.top;
    if (Math.abs(dx) < 1 && Math.abs(dy) < 1) continue;
    item.animate([{ translate: `${dx}px ${dy}px` }, { translate: "0 0" }], { duration: EASE, easing: "cubic-bezier(.2,.7,.2,1)" });
  }
}

/** Gives the keys to `item`'s handle. @param {Element} item */
const grip = (item) => /** @type {HTMLElement | undefined} */ (all(item, HANDLE)[0])?.focus({ preventScroll: true });

/** Takes away the copy that followed the pointer. @param {HTMLElement} ghost */
function unghost(ghost) {
  try {
    ghost.hidePopover();
  } catch {}
  ghost.remove();
}

/** Puts `item` in `list` before `before` (null: last). @param {Element} list @param {Element} item @param {Element | null} before */
function put(list, item, before) {
  const box = boxOf(list);
  if (!box) return;
  const at = before && before.parentNode === box ? before : null;
  if (item.parentNode === box && (at ? item.nextElementSibling === at : !item.nextElementSibling)) return;
  const focused = holds(item, active());
  const was = listOf(item);
  moving = true;
  try {
    insert(box, item, at);
  } finally {
    moving = false;
  }
  if (was !== list) {
    shiftTotal(was, -1);
    shiftTotal(list, 1);
  }
  if (focused && !holds(item, active())) grip(item);
}

/** Puts `item` back at `place`, sliding there, and says the counts again. @param {Element} root @param {HTMLElement} item @param {Place} place */
function back(root, item, place) {
  const before = measure(root);
  restore(root, item, place);
  slide(before);
  count(root);
}

/** Puts `item` back at `place`, if its list is still there. @param {Element} root @param {HTMLElement} item @param {Place} place */
function restore(root, item, place) {
  const list = listsOf(root).find((l) => listKey(l) === place.list);
  if (!list) return;
  const items = itemsOf(list).filter((i) => i !== item);
  // No item has the key "": with no neighbour named, none is found.
  const prev = items.find((i) => keyOf(i) === place.prev);
  const next = items.find((i) => keyOf(i) === place.next);
  // It was first: it goes first. After the item it followed, if that is still here; else before the one it preceded.
  const at = place.prev === "" ? items[0] || null : prev ? prev.nextElementSibling : next || null;
  put(list, item, at && at !== item ? at : null);
}

/** Writes each list's count as the page shows it now. @param {Element} root */
function count(root) {
  const w = words(root);
  for (const list of listsOf(root)) {
    const n = sizeOf(list);
    const limit = attr(list, "data-limit");
    const has = limit !== null && /^\d+$/.test(limit);
    const box = [...all(list, "[data-slot=sortable-count]")].find((c) => closest(c, LIST) === list);
    if (!box) continue;
    const values = { count: n, limit: limit || "" };
    const shownCount = all(box, "[data-count]")[0], text = all(box, "[data-count-text]")[0];
    if (shownCount) shownCount.textContent = fill(has ? w.countLimit : w.count, values);
    if (text) text.textContent = fill(w.countText, values);
    box.toggleAttribute("data-full", has && n >= Number(limit));
    box.toggleAttribute("data-over", has && n > Number(limit));
  }
}

/** Marks the list under the pointer: taking the item, or refusing it with its reason. @param {Element} root @param {Element | null} list @param {string} why */
function mark(root, list, why) {
  for (const l of listsOf(root)) {
    l.toggleAttribute("data-over", l === list && !why);
    if (l === list && why) set(l, "data-refusing", why);
    else unset(l, "data-refusing");
  }
}

/* The queue: one move on its way for each sortable, in the order they were made. */

/** @typedef {{item: HTMLElement, form: HTMLFormElement, fields: Record<string, string>, to: Place}} Sent */
/** @type {WeakMap<Element, {sending: Sent | null, waiting: Sent[]}>} */
const queues = new WeakMap();
/** @param {Element} root */
const queueOf = (root) => {
  let q = queues.get(root);
  if (!q) queues.set(root, (q = { sending: null, waiting: [] }));
  return q;
};
/** Which sortable each move form posts for, even once a redraw has taken it out. @type {WeakMap<Element, Element>} */
const formRoot = new WeakMap();
/**
 * What the server last agreed for each item with moves on their way: where it goes
 * back to if one is refused, and where the moves sent go.
 * @type {WeakMap<Element, {back: Place, to: Place[]}>}
 */
const records = new WeakMap();

/** Posts the move of `item` from `from` to where it is now. @param {Element} root @param {HTMLElement} item @param {Place} from */
function commit(root, item, from) {
  const form = /** @type {HTMLFormElement | undefined} */ ([...all(item, FORM)].find((f) => closest(f, ITEM) === item));
  const to = placeOf(item);
  if (!form || (from.list === to.list && from.next === to.next)) return;
  const record = records.get(item) || { back: from, to: [] };
  record.to.push(to);
  records.set(item, record);
  set(item, "data-pending", "");
  formRoot.set(form, root);
  queueOf(root).waiting.push({ item, form, to, fields: { item: keyOf(item), from_list: from.list, to_list: to.list, before: to.next, after: to.prev || "", position: "" } });
  pump(root);
}

/** Sends the next move of `root` when none is on its way. @param {Element} root */
function pump(root) {
  const q = queueOf(root);
  while (!q.sending) {
    const next = q.waiting.shift();
    if (!next) {
      idle();
      return;
    }
    if (!holds(docRoot(), next.form)) {
      settle(root, next.item, false);
      continue;
    }
    q.sending = next;
    for (const [name, value] of Object.entries(next.fields)) {
      const control = /** @type {HTMLInputElement | HTMLSelectElement | null} */ (field(next.form, name));
      if (!control) continue;
      // A list that was full when the page was drawn has its choice switched off, and a
      // choice switched off is not sent.
      if (control instanceof HTMLSelectElement) for (const option of control.options) if (option.value === value) option.disabled = false;
      control.value = value;
    }
    try {
      HTMLFormElement.prototype.requestSubmit.call(next.form);
    } catch {
      q.sending = null;
      settle(root, next.item, false);
    }
  }
}

/**
 * The server answered a move of `item`, or gave none. Taken: it stays. Refused or
 * failed: it goes back to where the server last had it, and its other moves are
 * dropped: they were made from a place that never was.
 * @param {Element} root @param {HTMLElement} item @param {boolean} ok
 */
function settle(root, item, ok) {
  const record = records.get(item);
  if (!record) return;
  if (ok) {
    const done = record.to.shift();
    if (done) record.back = done;
    if (!record.to.length) {
      records.delete(item);
      unset(item, "data-pending");
    }
    return;
  }
  const q = queueOf(root);
  q.waiting = q.waiting.filter((m) => m.item !== item);
  records.delete(item);
  unset(item, "data-pending");
  if (!item.isConnected) return;
  if (drag?.item === item) end();
  const region = closest(root, "[data-ui-region]");
  back(root, item, record.back);
  // With a region, the lists are drawn again from the server, and that says where it is.
  if (region) {
    waiting.add(region);
    recheck.set(region, { root, key: keyOf(item), name: attr(item, "data-name"), at: record.back });
  } else say(root, fill(words(root).returned, about(item)));
}

/**
 * A move that got no answer, or a failure from the server, may have landed all the same.
 * Nothing is said yet: the item stays where it was dropped, and once nothing else in its
 * region is moving the region is drawn again from the server. Then the item is where the
 * server has it: where it was dropped means the move landed, and nothing went wrong. A
 * move that didn't land, or a region that can't be fetched, is said, and the latter puts
 * the item back.
 * @param {Element} root @param {HTMLElement} item
 */
function ask(root, item, status) {
  const record = records.get(item);
  const region = closest(root, "[data-ui-region]");
  if (!record) return;
  if (!region) {
    settle(root, item, false);
    tell(root, status ? "failed" : "offline");
    return;
  }
  const q = queueOf(root);
  // Its later moves were made from where this one put it: they go, and are said to.
  const more = q.waiting.some((m) => m.item === item);
  q.waiting = q.waiting.filter((m) => m.item !== item);
  records.delete(item);
  if (drag?.item === item) end();
  asks.set(region, [...(asks.get(region) || []), { root, key: keyOf(item), to: record.to[0], from: record.back, more }]);
  waiting.add(region);
}

/** @typedef {{root: Element, key: string, to: Place, from: Place, more: boolean}} Ask a move to learn the fate of: where it went, and where it came from */

/** Judges `batch`, the moves `region` was drawn again for: drawn, unless it couldn't be fetched. @param {Element} region @param {Ask[]} batch */
function judge(region, batch) {
  const drawn = asking.get(region) && !has(region, "data-ui-failed");
  asking.delete(region);
  for (const { root, key, to, from, more } of batch) {
    // A drawing keeps the lists' elements: the item is found in them, where the server has it.
    const item = root.isConnected && itemsOfRoot(root).find((i) => keyOf(i) === key);
    if (!item) continue;
    unset(item, "data-pending");
    const at = placeOf(item);
    if (drawn && !more && at.list === to.list && (at.next === to.next || at.prev === to.prev)) {
      say(root, fill(words(root).settled, about(item)));
      continue;
    }
    if (!drawn) {
      // Picked up again meanwhile: the drag ends, as the item goes back.
      if (drag?.item === item) end();
      back(root, item, from);
    }
    say(root, fill(words(root).returned, about(item)));
    tell(root, drawn ? "failed" : "offline");
  }
  idle();
}

/** Shows the toast of `root`'s words `which`, through the core runtime. @param {Element} root @param {string} which */
const tell = (root, which) => fire(document, new CustomEvent("ui:toast", { detail: { tone: "danger", text: words(root)[which] } }));

/* Redraws: a region that follows a stream waits while a drag or a move is under way, and
   what a redraw moves slides into place. */

/** Regions redrawn after a refusal, and the item to say the place of once they are: its name, and where the page put it back. @type {Map<Element, {root: Element, key: string, name: string | null, at: Place}>} */
const recheck = new Map();
/** Regions whose redraw waits for the sortables in them. @type {Set<Element>} */
const waiting = new Set();
/** The moves each region is to be drawn again for, to learn whether they landed. @type {Map<Element, Ask[]>} */
const asks = new Map();
/** The regions being drawn again for `asks`, and whether the server's drawing has come. @type {Map<Element, boolean>} */
const asking = new Map();
/** @param {Element} region */
const busyIn = (region) => {
  if (asking.has(region) || (drag && holds(region, drag.root))) return true;
  for (const root of all(region, ROOT)) {
    const q = queues.get(root);
    if (q && (q.sending || q.waiting.length)) return true;
  }
  return false;
};
/** Redraws the regions that waited, once nothing in them is moving. */
function idle() {
  if (drag) return;
  for (const region of [...waiting]) {
    if (busyIn(region)) continue;
    waiting.delete(region);
    if (!region.isConnected) continue;
    /** @type {{done?: Promise<void>}} */
    const detail = {};
    fire(region, new CustomEvent("ui:refresh", { bubbles: true, detail }));
    const batch = asks.get(region);
    if (!batch) continue;
    // Asked about once drawn: a move asked about meanwhile waits for the next drawing.
    asks.delete(region);
    asking.set(region, false);
    Promise.resolve(detail.done).finally(() => judge(region, batch));
  }
}
on(document, "ui:refresh", (e) => {
  const region = closest(e.target, "[data-ui-region]");
  if (!region || !busyIn(region)) return;
  e.stopImmediatePropagation();
  waiting.add(region);
}, true);

/** Where the items were before a redraw. @type {{rects: Map<Element, DOMRect>, roots: Element[]} | null} */
let redraw = null;
on(document, "ui:before-swap", (e) => {
  const target = /** @type {CustomEvent} */ (e).detail;
  if (!(target instanceof Element)) return;
  if (asking.has(target)) asking.set(target, true);
  const roots = [...all(target, ROOT)];
  const around = closest(target, ROOT);
  if (around) roots.push(around);
  if (!roots.length) return;
  const rects = new Map();
  for (const root of roots) for (const [item, rect] of measure(root)) rects.set(item, rect);
  redraw = { rects, roots };
});
/**
 * After a page of items joins a list: an item the page has already (it moved down
 * past the cursor) is not kept twice, and the region the lists stand in asks for as
 * many items as the page now holds, so the next redraw doesn't take them away.
 */
function tidy() {
  for (const root of every(ROOT)) {
    /** @type {Map<string, Element>} */
    const seen = new Map();
    for (const item of all(root, ITEM)) {
      if (has(item, "data-ghost") || closest(item, ROOT) !== root) continue;
      const id = attr(item, "id");
      if (!id) continue;
      const older = seen.get(id);
      // The copy a later page brought is where the server has it now.
      if (older && older !== drag?.item) older.remove();
      else if (older) {
        item.remove();
        continue;
      }
      seen.set(id, item);
    }
    const region = closest(root, "[data-ui-region]");
    const src = region && attr(region, "data-ui-src");
    if (!region || !src) continue;
    const url = new URL(src, location.href);
    for (const list of listsOf(root)) {
      if (has(list, "data-paged")) url.searchParams.set("show." + listKey(list), String(itemsOf(list).length));
    }
    const next = url.pathname + url.search;
    if (next !== src) set(region, "data-ui-src", next);
  }
  watch();
}

/* Long lists: the "Load more" button of a list is pressed as it comes into sight, one page at a time. */

/** @type {IntersectionObserver | null} */
let seer = null;
function watch() {
  seer ||= new IntersectionObserver((seen) => {
    for (const entry of seen) {
      const form = /** @type {HTMLFormElement} */ (entry.target);
      if (!entry.isIntersecting || !holds(docRoot(), form) || has(form, "data-pending")) continue;
      try {
        HTMLFormElement.prototype.requestSubmit.call(form);
      } catch {}
    }
  }, { rootMargin: "320px" });
  for (const form of every("form[data-ui-autoload]")) seer.observe(form);
}
watch();

on(document, "ui:swap", () => {
  tidy();
  for (const [region, { root, key, name, at }] of recheck) {
    if (waiting.has(region)) continue;
    recheck.delete(region);
    if (!root.isConnected) continue;
    const item = itemsOfRoot(root).find((i) => keyOf(i) === key);
    if (item) {
      say(root, fill(words(root).settled, about(item)));
      continue;
    }
    // The server has it where this page shows none of: said so, and the keys go back to
    // the list it was in, on the card that took its place, if they went with it.
    say(root, fill(words(root).gone, { item: name }));
    const list = listsOf(root).find((l) => listKey(l) === at.list);
    const items = list ? itemsOf(list) : [];
    const near = items.find((i) => keyOf(i) === at.next) || items.at(-1);
    if (near && active() === body()) grip(near);
  }
  if (drag) {
    if (!drag.item.isConnected || !drag.root.isConnected) end();
    else set(drag.item, drag.mode === "keys" ? "data-lifted" : "data-dragging", "");
  }
  const was = redraw;
  redraw = null;
  if (!was) return;
  const now = was.roots.filter((r) => r.isConnected).flatMap((r) => [...all(r, ITEM)]);
  slide(was.rects, now, drag?.item);
});

/* Pointer and touch */

/** The elements that scroll while a drag nears their edge: the lists, the board, its ancestors, the page. @param {Element} root */
function scrollersOf(root) {
  /** @param {Element} el */
  const scrolls = (el) => {
    const style = getComputedStyle(el);
    return (/auto|scroll/.test(style.overflowX) && el.scrollWidth > el.clientWidth) || (/auto|scroll/.test(style.overflowY) && el.scrollHeight > el.clientHeight);
  };
  const found = [...all(root, `${LIST},${BOX},[data-slot=sortable-lists]`)].filter(scrolls);
  for (let el = /** @type {Element | null} */ (root); el && el !== docRoot(); el = el.parentElement) if (scrolls(el)) found.push(el);
  found.push(docRoot());
  return found;
}

/** How far to scroll this frame, `d` px from an edge. @param {number} d */
const speed = (d) => (d < EDGE ? Math.ceil((1 - Math.max(d, 0) / EDGE) * STEP) : 0);

/** Scrolls what the pointer is near the edge of. True when something moved. @param {number} x @param {number} y */
function scroll(x, y) {
  if (!drag) return false;
  let moved = false;
  for (const el of drag.scrollers) {
    const page = el === docRoot();
    const r = page ? { left: 0, top: 0, right: innerWidth, bottom: innerHeight } : el.getBoundingClientRect();
    // Sideways while the pointer is level with it, up and down while it is in line: past an edge
    // (the board's edge sits inside the page's margin on a phone) is as near as it gets.
    const level = y >= r.top && y <= r.bottom, inline = x >= r.left && x <= r.right;
    const dx = level ? speed(r.right - x) - speed(x - r.left) : 0, dy = inline ? speed(r.bottom - y) - speed(y - r.top) : 0;
    if (!dx && !dy) continue;
    const left = el.scrollLeft, top = el.scrollTop;
    el.scrollBy(dx, dy);
    moved ||= el.scrollLeft !== left || el.scrollTop !== top;
  }
  return moved;
}

/** A copy of the item that follows the pointer, in the top layer, with nothing in it that acts. @param {HTMLElement} item */
function ghostOf(item) {
  const ghost = /** @type {HTMLElement} */ (NP.cloneNode.call(item, true));
  for (const el of all(ghost, "form,details,[data-slot=sortable-status]")) el.remove();
  for (const el of [ghost, ...all(ghost, "[id],[name]")]) {
    unset(el, "id");
    unset(el, "name");
  }
  set(ghost, "data-ghost", "");
  set(ghost, "aria-hidden", "true");
  set(ghost, "popover", "manual");
  ghost.inert = true;
  ghost.style.width = item.getBoundingClientRect().width + "px";
  return ghost;
}

/** Starts a pointer drag from the press, the pointer at `x`, `y`. @param {number} x @param {number} y */
function begin(x, y) {
  if (!press || drag) return;
  const { item, root, id } = press;
  clearTimeout(press.timer);
  const from = press;
  press = null;
  if (!item.isConnected || has(item, "data-locked")) return;
  const rect = item.getBoundingClientRect();
  const ghost = ghostOf(item);
  // In the body, so no redraw of the lists takes it, and in the top layer, so it shows over a dialog.
  set(ghost, "dir", getComputedStyle(root).direction);
  body().append(ghost);
  try {
    ghost.showPopover();
  } catch {}
  drag = {
    root, item, mode: "pointer", from: placeOf(item), refused: "", ghost, id,
    dx: from.x - rect.left, dy: from.y - rect.top, x, y, frame: 0, dirty: true, scrollers: scrollersOf(root), said: null,
  };
  try {
    item.setPointerCapture(id);
  } catch {}
  set(item, "data-dragging", "");
  set(root, "data-dragging", "");
  say(root, fill(words(root).picked, about(item)));
  tick();
}

/** One frame of a pointer drag: the copy follows, the page scrolls near an edge, the item finds its place. */
function tick() {
  if (!drag || drag.mode !== "pointer" || !drag.ghost) return;
  drag.ghost.style.translate = `${drag.x - drag.dx}px ${drag.y - drag.dy}px`;
  if (scroll(drag.x, drag.y) || drag.dirty) {
    drag.dirty = false;
    place(drag.x, drag.y);
  }
  if (drag) drag.frame = requestAnimationFrame(tick);
}

/** Moves the dragged item to where the pointer is over a list that takes it. @param {number} x @param {number} y */
function place(x, y) {
  if (!drag) return;
  const { root, item } = drag;
  const at = /** @type {Element | null} */ (DP.elementFromPoint.call(document, x, y));
  let list = at && closest(at, LIST);
  if (list && closest(list, ROOT) !== root) list = null;
  if (!list) {
    mark(root, null, "");
    drag.refused = "";
    return;
  }
  const why = refusal(root, list, item, drag.from.list);
  mark(root, list, why);
  drag.refused = why;
  if (why) {
    if (drag.said !== list) say(root, why);
    drag.said = list;
    return;
  }
  drag.said = null;
  let before = null;
  for (const other of itemsOf(list)) {
    if (other === item || !shown(other)) continue;
    const r = other.getBoundingClientRect();
    if (y < r.top + r.height / 2) {
      before = other;
      break;
    }
  }
  if (listOf(item) === list && nextOf(item) === before) return;
  const rects = measure(root);
  put(list, item, before);
  slide(rects, rects.keys(), item);
  count(root);
}

/** Cancels a press that never became a drag. */
function unpress() {
  if (press) clearTimeout(press.timer);
  press = null;
}

/** Ends a drag of either kind: the copy goes, the marks go. */
function end() {
  if (!drag) return;
  const { root, item, ghost, frame } = drag;
  cancelAnimationFrame(frame);
  drag = null;
  unset(item, "data-dragging");
  unset(item, "data-lifted");
  unset(root, "data-dragging");
  mark(root, null, "");
  if (ghost) unghost(ghost);
}

/** Drops the item where it is, or puts it back if it was let go over a list that refuses it. */
function drop() {
  if (!drag) return;
  const { root, item, from, refused, ghost, mode } = drag;
  if (refused) {
    cancel(refused);
    return;
  }
  const fly = ghost && !still() ? ghost : null;
  if (fly) {
    // The copy settles into the item's place, then goes.
    const to = item.getBoundingClientRect();
    cancelAnimationFrame(drag.frame);
    drag.ghost = undefined;
    const run = fly.animate([{ translate: fly.style.translate }, { translate: `${to.left}px ${to.top}px` }], { duration: 140, easing: "ease-out" });
    run.onfinish = run.oncancel = () => unghost(fly);
  }
  end();
  if (mode === "keys") grip(item);
  say(root, fill(words(root).dropped, about(item)));
  commit(root, item, from);
  count(root);
  idle();
}

/** Puts the item back where the drag began. @param {string} [why] what to say first */
function cancel(why = "") {
  if (!drag) return;
  const { root, item, from, mode } = drag;
  end();
  back(root, item, from);
  if (mode === "keys") grip(item);
  const where = fill(words(root).cancelled, about(item));
  say(root, why ? `${why} ${where}` : where);
  idle();
}

on(document, "pointerdown", (e) => {
  if (drag) {
    // A press elsewhere ends a move made with the keys.
    if (drag.mode === "keys" && !holds(drag.item, /** @type {Node} */ (e.target))) cancel();
    return;
  }
  unpress();
  if (e.button !== 0 || !e.isPrimary) return;
  const item = /** @type {HTMLElement | null} */ (closest(e.target, ITEM));
  const root = item && closest(item, ROOT);
  if (!item || !root || has(item, "data-ghost")) return;
  const handle = closest(e.target, HANDLE);
  // A control marked `data-ui-drag` (a card's title, a button) still starts a drag: a press that moves.
  const control = closest(e.target, CONTROL), free = closest(e.target, "[data-ui-drag]");
  if (control && control !== handle && holds(item, control) && !(free && holds(item, free))) return;
  if (handle && closest(handle, ITEM) !== item) return;
  if (has(item, "data-locked")) return;
  const touch = e.pointerType === "touch";
  press = { item, root, id: e.pointerId, x: e.clientX, y: e.clientY, touch, handle: !!handle, timer: 0 };
  if (touch && !handle) press.timer = /** @type {any} */ (setTimeout(() => press && begin(press.x, press.y), HOLD));
});

on(document, "pointermove", (e) => {
  if (press && e.pointerId === press.id && !drag) {
    const moved = Math.hypot(e.clientX - press.x, e.clientY - press.y);
    // A touch that moves before it is held is a scroll.
    if (press.touch && !press.handle) {
      if (moved > TOUCH_SLOP) unpress();
      return;
    }
    if (moved > SLOP) begin(e.clientX, e.clientY);
  }
  if (drag?.mode === "pointer" && e.pointerId === drag.id) {
    drag.x = e.clientX;
    drag.y = e.clientY;
    drag.dirty = true;
  }
});

on(document, "pointerup", (e) => {
  if (press && e.pointerId === press.id) unpress();
  if (drag?.mode === "pointer" && e.pointerId === drag.id) {
    drag.x = e.clientX;
    drag.y = e.clientY;
    place(drag.x, drag.y);
    drop();
    swallow = true;
    setTimeout(() => (swallow = false));
  }
});

on(document, "pointercancel", (e) => {
  if (press && e.pointerId === press.id) unpress();
  if (drag?.mode === "pointer" && e.pointerId === drag.id) cancel();
});

on(window, "blur", () => {
  unpress();
  if (drag?.mode === "pointer") cancel();
});

// The click that ends a drag, on the link the drag began on, goes nowhere.
on(document, "click", (e) => {
  if (!swallow) return;
  swallow = false;
  e.preventDefault();
  e.stopPropagation();
}, true);

// Links and pictures in an item are the browser's own drags: this one is the item's.
on(document, "dragstart", (e) => {
  const item = closest(e.target, ITEM);
  if (item && closest(item, ROOT)) e.preventDefault();
});

// A press on an item is a drag in the making, not the start of a text selection. WebKit keeps its
// selection going under the held pointer and, at the board's side, nudges the board along with it;
// the phone's snap points pull each nudge back, and with them the drag's own steps.
on(document, "selectstart", (e) => press && e.preventDefault());

// A drag on touch holds the page still; a long press opens no menu.
on(document, "touchmove", (e) => {
  if (drag?.mode === "pointer" || (press?.touch && press.handle)) e.preventDefault();
}, { passive: false });
on(document, "contextmenu", (e) => {
  if (drag || press?.touch) e.preventDefault();
});

/* Keys */

/** Picks `item` up with the keys, or says why it can't move. @param {Element} root @param {HTMLElement} item */
function lift(root, item) {
  const locked = attr(item, "data-locked");
  if (locked !== null) return say(root, `${fill(words(root).locked, about(item))} ${locked}`.trim());
  drag = /** @type {Drag} */ ({ root, item, mode: "keys", from: placeOf(item), refused: "" });
  set(item, "data-lifted", "");
  // The keys go to the handle, however it was pressed: WebKit doesn't focus a button it clicks.
  grip(item);
  say(root, fill(words(root).picked, about(item)));
}

/** Moves the lifted item to `before` in `list`, in sight, the focus on its handle. @param {Element} list @param {Element | null} before */
function shift(list, before) {
  if (!drag) return;
  const { root, item } = drag;
  const rects = measure(root);
  put(list, item, before);
  slide(rects);
  count(root);
  item.scrollIntoView({ block: "nearest", inline: "nearest" });
}

/** One step with the keys: `by` places up or down in the list, as far as its ends (Home and End step past them). @param {number} by */
function step(by) {
  if (!drag) return;
  const { root, item } = drag;
  const list = listOf(item);
  if (!list) return;
  const items = itemsOf(list).filter((i) => i === item || shown(i));
  const at = items.indexOf(item);
  const goal = Math.max(0, Math.min(items.length - 1, at + by));
  if (goal !== at) {
    const others = items.filter((i) => i !== item);
    shift(list, others[goal] || null);
  }
  say(root, fill(words(root).moved, about(item)));
}

/** Moves the lifted item to the next list that takes it, `by` 1 or -1, at the same place. @param {number} by */
function hop(by) {
  if (!drag) return;
  const { root, item } = drag;
  const lists = listsOf(root);
  const here = listOf(item);
  if (!here) return;
  const items = itemsOf(here).filter((i) => i === item || shown(i));
  const at = items.indexOf(item);
  const passed = [];
  for (let i = lists.indexOf(here) + by; i >= 0 && i < lists.length; i += by) {
    const why = refusal(root, lists[i], item, drag.from.list);
    if (why) {
      passed.push(why);
      continue;
    }
    const others = itemsOf(lists[i]).filter((o) => o !== item && shown(o));
    shift(lists[i], others[Math.min(at, others.length)] || null);
    say(root, [...passed, fill(words(root).moved, about(item))].join(" "));
    return;
  }
  say(root, passed[0] || fill(words(root).moved, about(item)));
}

on(document, "keydown", (e) => {
  if (drag?.mode === "pointer") {
    if (e.key === "Escape") {
      e.preventDefault();
      cancel();
    }
    return;
  }
  const handle = closest(e.target, HANDLE);
  if (drag?.mode === "keys") {
    if (!handle || closest(handle, ITEM) !== drag.item) return;
    const rtl = getComputedStyle(drag.root).direction === "rtl";
    switch (e.key) {
      case " ":
      case "Enter": drop(); break;
      case "Escape": cancel(); break;
      case "ArrowUp": step(-1); break;
      case "ArrowDown": step(1); break;
      case "ArrowLeft": hop(rtl ? 1 : -1); break;
      case "ArrowRight": hop(rtl ? -1 : 1); break;
      case "Home": step(-Infinity); break;
      case "End": step(Infinity); break;
      case "Tab": cancel(); return;
      default: return;
    }
    e.preventDefault();
    return;
  }
  if (!handle || (e.key !== " " && e.key !== "Enter") || e.repeat) return;
  const item = /** @type {HTMLElement | null} */ (closest(handle, ITEM));
  const root = item && closest(item, ROOT);
  if (!root) return;
  e.preventDefault();
  lift(root, /** @type {HTMLElement} */ (item));
});

// The handle's own click does nothing: Space and Enter are handled as they go down.
on(document, "click", (e) => {
  const handle = closest(e.target, HANDLE);
  const item = /** @type {HTMLElement | null} */ (handle && closest(handle, ITEM));
  const root = item && closest(item, ROOT);
  if (!root) return;
  e.preventDefault();
  if (drag?.mode === "keys" && drag.item === item) drop();
  else if (!drag) lift(root, /** @type {HTMLElement} */ (item));
});

// Leaving the handle while a move with the keys is under way puts the item back.
on(document, "focusout", (e) => {
  if (drag?.mode !== "keys" || moving || !holds(drag.item, /** @type {Node} */ (e.target))) return;
  const lifted = drag.item;
  setTimeout(() => {
    if (drag?.item === lifted && !holds(lifted, active())) cancel();
  });
});

/* The server's answer */

on(document, "ui:sent", (e) => {
  const { form, ok, status } = /** @type {CustomEvent} */ (e).detail || {};
  const root = formRoot.get(form);
  if (!root) return;
  const q = queueOf(root);
  const sent = q.sending;
  if (!sent || sent.form !== form) return;
  q.sending = null;
  // A refusal (4xx) is the server's answer; no answer, or its failure, is a question.
  if (ok || (status >= 400 && status < 500)) settle(root, sent.item, !!ok);
  else {
    e.preventDefault();
    ask(root, sent.item, status);
  }
  pump(root);
});
