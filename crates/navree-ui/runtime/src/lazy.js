// Lazy lists: a long list that loads as it is read. Each end of a list is a link the
// server drew (`a[data-slot=lazy-more]`); with no script it goes to a page that holds
// the next part. Here, when the reader nears an end, the page asks the list's source
// for what lies beyond the item at that end, by its cursor, never by an offset.
//
// - One fetch per end at a time, each given up on after 10 s; an answer for a list that
//   has since changed (new filters, a new page) is dropped.
// - Items added above what the reader looks at keep the reader's place: the first item
//   in view is measured before and after, and the scroll moves by the difference.
// - Past `data-ui-max` items, those far off screen at the other end are taken out and
//   that end is opened again, so a reader can scroll through thousands with a page that
//   stays the same size.
// - A live list (`data-ui-live-src`, followed by the live module) asks what changed since
//   its revision. An item it has is changed where it stands, node by node, so the focus,
//   a caret and the scroll stay; one that moved is moved, and keeps the focus. New ones
//   go where their cursor sorts, and gone ones go. An item is never in the list twice.
//   An answer that says more is waiting is followed by another ask, and so is one asked
//   for before something was written, which is not merged.
// - New items are announced once they stop coming (`data-l-new`), and a list that names
//   `data-ui-seen` tells the server, by a POST, when the reader has seen what it shows.
//
// Every read of the document goes through dom.js: a page element named like a document
// member (`<img name="importNode">`) can't change what this does.
import { active, adopt, attr, body, clean, closest, cookie, emit, every, has, hidden, holds, on, sameOrigin, scrolling, set, toggle, unset } from "./dom.js";

const LIMIT = 10000;
/** How near an end, in pixels, the reader is when its next page is asked for. */
const NEAR = 800;
/** Pages asked for in a row while an end stays near: a short page doesn't stop the list. */
const CHAIN = 6;
/** New items are announced this long after the last arrives, and never later than LATEST after the first. */
const QUIET = 1000;
const LATEST = 5000;
const HEADERS = { "x-navree-ui": "1" };

// An item's nodes are read through their prototypes: a form in an item whose fields
// are named `firstChild` or `attributes` can't change what the merge reads.
const NP = Node.prototype;
/** @param {object} proto @param {string} name */
const getter = (proto, name) => /** @type {(this: any) => any} */ (Object.getOwnPropertyDescriptor(proto, name)?.get);
const gFirst = getter(NP, "firstChild"), gNext = getter(NP, "nextSibling"), gType = getter(NP, "nodeType"), gName = getter(NP, "nodeName");
const gAttrs = getter(Element.prototype, "attributes");

/** @typedef {"start" | "end" | "live"} Dir */
/** @typedef {{gen: number, src: string, box: Element | null, busy: Record<Dir, boolean>, dirty: boolean, io?: IntersectionObserver, watched: Element[], chain: number, unseen: boolean, seeing: boolean, waited: number}} State */
/** @type {WeakMap<Element, State>} */
const states = new WeakMap();

/** @param {Element} list */
const box = (list) => /** @type {HTMLElement | null} */ (list.querySelector(":scope > [data-slot=lazy-items]"));
/** @param {Element} list @param {"start" | "end"} dir */
const link = (list, dir) => /** @type {HTMLAnchorElement | null} */ (list.querySelector(`:scope > [data-slot=lazy-more][data-dir=${dir}]`));
/** @param {Element} parent */
const items = (parent) => /** @type {HTMLElement[]} */ ([...parent.children].filter((el) => has(el, "data-key")));
/** @param {Element} parent @param {string} key */
const find = (parent, key) => /** @type {HTMLElement | null} */ (parent.querySelector(`:scope > [data-key="${CSS.escape(key)}"]`));
/** @param {Element} el */
const cursorOf = (el) => attr(el, "data-cursor") || "";

/** The element that scrolls the list: its nearest scrolling ancestor, or the page. @param {Element} list */
function scroller(list) {
  for (let el = list.parentElement; el; el = el.parentElement) {
    const overflow = getComputedStyle(el).overflowY;
    if ((overflow === "auto" || overflow === "scroll") && el.scrollHeight > el.clientHeight) return el;
  }
  return scrolling();
}

/** The top and bottom of what the reader sees, in the viewport's coordinates. @param {HTMLElement} scroll */
function view(scroll) {
  if (scroll === scrolling()) return { top: 0, bottom: innerHeight };
  const box = scroll.getBoundingClientRect();
  return { top: box.top, bottom: box.bottom };
}

/** Runs `change` and keeps the first item in view where it was. @param {Element} list @param {() => void} change */
function anchored(list, change) {
  const scroll = scroller(list);
  const { top } = view(scroll);
  const parent = box(list);
  const anchor = parent && items(parent).find((el) => el.getBoundingClientRect().bottom > top);
  const before = anchor?.getBoundingClientRect().top;
  change();
  if (anchor?.isConnected && before !== undefined) {
    const moved = anchor.getBoundingClientRect().top - before;
    if (Math.abs(moved) >= 1) scroll.scrollTop += moved;
  }
}

/** Whether an end of the list is reached. @param {Element} list @param {"start" | "end"} dir */
const done = (list, dir) => {
  const parent = box(list);
  return !parent || has(parent, `data-ui-${dir}-done`);
};

/** Whether `a` comes before `b` in the list. @param {Element} list @param {string} a @param {string} b */
const precedes = (list, a, b) => (attr(list, "data-ui-order") === "asc" ? a < b : a > b);

/** The list's source with a question added. @param {Element} list @param {Record<string, string>} ask */
function sourceUrl(list, ask) {
  const url = new URL(attr(list, "data-ui-src") || "", location.href);
  for (const [name, value] of Object.entries(ask)) url.searchParams.set(name, value);
  return url;
}

/** The items of an answer, out of the markup. @param {string} html */
function parse(html) {
  const doc = new DOMParser().parseFromString(`<ul>${html}</ul>`, "text/html");
  clean(body(doc));
  return /** @type {HTMLElement[]} */ ([...every("li[data-key]", doc)].filter((li) => !closest(li.parentElement, "li[data-key]")).map(adopt));
}

/** Says something to assistive technology, in the list's own polite status. @param {Element} list @param {string} text */
function say(list, text) {
  const status = list.querySelector(":scope > [data-slot=lazy-status]");
  // The same words again are a change too: a reader hears both.
  if (status) status.textContent = status.textContent === text ? text + " " : text;
}

/** What a list has to say about new items, and has not said yet. @type {WeakMap<Element, {n: number, one: string, soon: number, late: number}>} */
const saying = new WeakMap();

/** Says that `fresh` items came, once they have stopped coming, or five seconds after the first. @param {Element} list @param {Element[]} fresh */
function announce(list, fresh) {
  const many = attr(list, "data-l-new");
  if (!many || !fresh.length) return;
  const s = saying.get(list) || { n: 0, one: "", soon: 0, late: 0 };
  saying.set(list, s);
  s.n += fresh.length;
  s.one = attr(fresh[fresh.length - 1], "data-ui-say") || attr(list, "data-l-new-one") || "";
  const flush = () => {
    clearTimeout(s.soon);
    clearTimeout(s.late);
    s.soon = s.late = 0;
    const n = s.n;
    s.n = 0;
    if (n) say(list, n === 1 && s.one ? s.one : many.replace("{n}", String(n)));
  };
  clearTimeout(s.soon);
  s.soon = /** @type {any} */ (setTimeout(flush, QUIET));
  if (!s.late) s.late = /** @type {any} */ (setTimeout(flush, LATEST));
}

/**
 * Makes `a` what `b` is, keeping every node of `a` that `b` still has at the same place:
 * attributes are set and taken away, text is changed in place, and only nodes of
 * another kind are swapped. A focused link stays the focused link.
 * @param {Element} a @param {Element} b
 */
function patch(a, b) {
  for (const { name } of [...gAttrs.call(a)]) if (!has(b, name)) unset(a, name);
  for (const { name, value } of [...gAttrs.call(b)]) if (attr(a, name) !== value) set(a, name, value);
  let x = gFirst.call(a);
  for (let y = gFirst.call(b); y; ) {
    const after = gNext.call(y);
    if (x && gType.call(x) === gType.call(y) && gName.call(x) === gName.call(y)) {
      if (gType.call(x) === 1) patch(x, y);
      else if (x.nodeValue !== y.nodeValue) x.nodeValue = y.nodeValue;
      x = gNext.call(x);
    } else NP.insertBefore.call(a, y, x);
    y = after;
  }
  while (x) {
    const after = gNext.call(x);
    NP.removeChild.call(a, x);
    x = after;
  }
}

/**
 * Merges a live answer: gone items go, changed ones change where they stand or move,
 * new ones go where their cursor sorts. Returns the items that are new to the list.
 * @param {Element} list @param {HTMLElement} parent @param {HTMLElement[]} changed @param {string[]} gone
 */
function merge(list, parent, changed, gone) {
  const focused = /** @type {HTMLInputElement | null} */ (active());
  /** @type {[number | null, number | null] | null} */
  let caret = null;
  try {
    if (focused && "selectionStart" in focused) caret = [focused.selectionStart, focused.selectionEnd];
  } catch {}
  /** @type {HTMLElement[]} */
  const fresh = [];
  anchored(list, () => {
    for (const key of gone) find(parent, key)?.remove();
    for (const li of changed) {
      const cursor = cursorOf(li);
      const old = find(parent, attr(li, "data-key") || "");
      const others = items(parent).filter((el) => el !== old);
      const first = others[0], last = others[others.length - 1];
      // Beyond what is loaded at an end that isn't: it arrives when the reader gets there.
      if ((last && !done(list, "end") && precedes(list, cursorOf(last), cursor)) || (first && !done(list, "start") && precedes(list, cursor, cursorOf(first)))) {
        old?.remove();
        continue;
      }
      const before = others.find((el) => precedes(list, cursor, cursorOf(el))) || null;
      if (!old) {
        parent.insertBefore(li, before);
        if (!has(li, "data-ui-quiet")) fresh.push(li);
        continue;
      }
      patch(old, li);
      if (old.nextElementSibling === before) continue;
      // Moving a node takes the focus out of it: it goes back where it was.
      const kept = holds(old, focused);
      parent.insertBefore(old, before);
      if (kept && focused && active() !== focused) {
        try {
          HTMLElement.prototype.focus.call(focused, { preventScroll: true });
          if (caret) focused.setSelectionRange(caret[0], caret[1]);
        } catch {}
      }
    }
  });
  return fresh;
}

/** A key as `x-ui-gone` writes it: each one encoded, so a comma in a key is no separator. @param {string} text */
const unkey = (text) => {
  try {
    return decodeURIComponent(text);
  } catch {
    return text;
  }
};

/** Takes items far off screen out of the other end, past the list's most. @param {Element} list @param {"start" | "end"} grew */
function trim(list, grew) {
  const parent = box(list);
  if (!parent) return;
  const most = Number(attr(list, "data-ui-max")) || 300;
  const all = items(parent);
  let over = all.length - most;
  if (over <= 0) return;
  const { top, bottom } = view(scroller(list));
  const side = grew === "end" ? all : all.reverse();
  const gone = [];
  for (const el of side) {
    if (over <= 0) break;
    const box = el.getBoundingClientRect();
    // Only what is out of sight, beyond a screen's length.
    if (grew === "end" ? box.bottom > top - (bottom - top) : box.top < bottom + (bottom - top)) break;
    gone.push(el);
    over--;
  }
  if (!gone.length) return;
  anchored(list, () => gone.forEach((el) => el.remove()));
  const other = grew === "end" ? "start" : "end";
  const parent2 = box(list);
  if (parent2) unset(parent2, `data-ui-${other}-done`);
  const end = link(list, other);
  if (end) unset(end, "hidden");
}

/** Asks for the next page at one end. @param {Element} list @param {"start" | "end"} dir */
async function load(list, dir) {
  const state = stateOf(list);
  const parent = box(list);
  if (!parent || state.busy[dir] || done(list, dir)) return;
  const all = items(parent);
  const edge = dir === "end" ? all[all.length - 1] : all[0];
  if (!edge) return;
  const src = attr(list, "data-ui-src") || "";
  if (!sameOrigin(src)) return;
  const gen = state.gen;
  state.busy[dir] = true;
  const end = link(list, dir);
  if (end) {
    set(end, "aria-busy", "true");
    unset(end, "data-failed");
  }
  const stop = new AbortController();
  const limit = setTimeout(() => stop.abort(), LIMIT);
  try {
    const response = await fetch(sourceUrl(list, { dir, cursor: cursorOf(edge) }), { headers: HEADERS, signal: stop.signal });
    if (!response.ok) throw new Error(String(response.status));
    const html = await response.text();
    // The list changed while this was on its way: the answer is for a list that's gone.
    if (state.gen !== gen || !list.isConnected || !edge.isConnected) return;
    const found = parse(html).filter((li) => !find(parent, attr(li, "data-key") || ""));
    anchored(list, () => {
      if (dir === "end") parent.append(...found);
      else parent.prepend(...found);
    });
    const more = response.headers.get("x-ui-more") === "1";
    toggle(parent, `data-ui-${dir}-done`, !more);
    if (end) toggle(end, "hidden", !more);
    if (found.length) {
      say(list, (attr(list, "data-l-loaded") || "{n} more").replace("{n}", String(found.length)));
      trim(list, dir);
      emit(new Event("ui:swap"));
    }
  } catch {
    if (state.gen === gen && end) {
      set(end, "data-failed", "");
      say(list, attr(list, "data-l-failed") || "");
    }
    state.chain = CHAIN;
    return;
  } finally {
    clearTimeout(limit);
    state.busy[dir] = false;
    if (end) unset(end, "aria-busy");
  }
  // A short page leaves the end in sight: ask again, a few times at most.
  if (state.gen === gen && state.chain++ < CHAIN && near(list, dir)) load(list, dir);
}

/** Whether the reader is near an end. @param {Element} list @param {"start" | "end"} dir */
function near(list, dir) {
  const end = link(list, dir);
  if (!end || done(list, dir)) return false;
  const { top, bottom } = view(scroller(list));
  const parent = box(list);
  const all = parent ? items(parent) : [];
  const edge = dir === "end" ? all[all.length - 1] : all[0];
  const rect = (edge || end).getBoundingClientRect();
  return dir === "end" ? rect.top < bottom + NEAR : rect.bottom > top - NEAR;
}

/** Moves on whenever something was written, here or in another tab, or the page was shown again from the back-forward cache. */
let changes = 0;
on(document, "ui:stale", () => changes++);

/**
 * Merges what changed since the list's revision. An answer asked for before something was written may be from
 * before the write, and merged after the page was drawn again for it, it would change back what the write changed:
 * it is not merged, and what changed is asked for again.
 * @param {Element} list
 */
async function live(list) {
  const state = stateOf(list);
  const parent = box(list);
  const since = parent && attr(parent, "data-ui-since");
  if (!parent || since === null || !sameOrigin(attr(list, "data-ui-src") || "")) return;
  if (state.busy.live) {
    state.dirty = true;
    return;
  }
  const gen = state.gen;
  const asked = changes;
  state.busy.live = true;
  const stop = new AbortController();
  const limit = setTimeout(() => stop.abort(), LIMIT);
  try {
    // The window loaded: the server may draw again what changes in it with no write, a clock.
    const all = items(parent);
    const ask = { dir: "live", since };
    if (all.length) Object.assign(ask, { from: cursorOf(all[0]), to: cursorOf(all[all.length - 1]) });
    const response = await fetch(sourceUrl(list, ask), { headers: HEADERS, signal: stop.signal });
    if (!response.ok || state.gen !== gen || !list.isConnected) return;
    if (changes !== asked) {
      state.dirty = true;
      return;
    }
    const html = await response.text();
    if (state.gen !== gen || box(list) !== parent) return;
    if (changes !== asked) {
      state.dirty = true;
      return;
    }
    const changed = parse(html);
    const gone = (response.headers.get("x-ui-gone") || "").split(",").filter(Boolean).map(unkey);
    const fresh = merge(list, parent, changed, gone);
    const next = response.headers.get("x-ui-since");
    if (next) set(parent, "data-ui-since", next);
    // The answer was cut short: the rest comes with one more ask.
    if (response.headers.get("x-ui-more") === "1") state.dirty = true;
    const empty = list.querySelector(":scope > [data-slot=lazy-empty]");
    if (empty && items(parent).length) empty.remove();
    if (fresh.length) {
      announce(list, fresh);
      state.unseen = true;
      seen(list);
    }
    if (changed.length || gone.length) emit(new Event("ui:swap"));
  } catch {} finally {
    clearTimeout(limit);
    state.busy.live = false;
    if (state.dirty && state.gen === gen && list.isConnected) {
      state.dirty = false;
      live(list);
    }
  }
}

/** Tells the server the reader has seen what the list shows: one POST at a time, only while the page is shown. @param {Element} list */
async function seen(list) {
  const state = stateOf(list);
  const url = attr(list, "data-ui-seen");
  if (!url || !state.unseen || state.seeing || hidden() || !sameOrigin(url)) return;
  // A live list waits for its stream (ten seconds at most): the news of what it saw
  // then reaches this page too, and the rest of it shows the change.
  if (has(list, "data-ui-live-src") && attr(list, "data-live") !== "on" && state.waited++ < 50) {
    setTimeout(() => seen(list), 200);
    return;
  }
  state.unseen = false;
  state.seeing = true;
  // An empty form, as a form posts it: the server checks it as it checks any form.
  /** @type {Record<string, string>} */
  const headers = { ...HEADERS, "content-type": "application/x-www-form-urlencoded" };
  const token = cookie("XSRF-TOKEN");
  if (token) headers["x-xsrf-token"] = token;
  try {
    await fetch(url, { method: "POST", headers, body: "", credentials: "same-origin", signal: AbortSignal.timeout(LIMIT) });
  } catch {} finally {
    state.seeing = false;
    if (state.unseen && list.isConnected) seen(list);
  }
}

/** @param {Element} list @returns {State} */
function stateOf(list) {
  let state = states.get(list);
  if (!state) states.set(list, (state = { gen: 0, src: attr(list, "data-ui-src") || "", box: box(list), busy: { start: false, end: false, live: false }, dirty: false, watched: [], chain: 0, unseen: true, seeing: false, waited: 0 }));
  return state;
}

/** Marks the item the page is about: its first link says it is the current page. @param {Element} list */
function mark(list) {
  const parent = box(list);
  if (!parent) return;
  const current = attr(list, "data-ui-current");
  for (const li of items(parent)) {
    const a = li.querySelector("a[href]");
    if (!a) continue;
    const on = current !== null && attr(li, "data-key") === current;
    if (on) set(a, "aria-current", "page");
    else unset(a, "aria-current");
    toggle(a, "data-current", on);
  }
}

/** Watches each list's ends, and notices a list whose source changed under it. */
function scan() {
  for (const list of every("[data-ui-lazy-list]")) {
    const state = stateOf(list);
    const src = attr(list, "data-ui-src") || "";
    const parent = box(list);
    if (src !== state.src || parent !== state.box) {
      // Another view: answers on their way are for the list that was.
      state.gen++;
      state.src = src;
      state.box = parent;
      state.dirty = false;
      state.busy = { start: false, end: false, live: false };
      state.unseen = true;
    }
    seen(list);
    // The page drew the ends for its first page; the list the reader kept may differ.
    for (const dir of /** @type {const} */ (["start", "end"])) {
      const end = link(list, dir);
      if (end) toggle(end, "hidden", done(list, dir));
    }
    mark(list);
    const ends = [link(list, "start"), link(list, "end")].filter((el) => !!el);
    if (state.io && ends.length === state.watched.length && ends.every((el, i) => el === state.watched[i])) continue;
    state.io?.disconnect();
    state.watched = /** @type {Element[]} */ (ends);
    const scroll = scroller(list);
    state.io = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          if (!entry.isIntersecting) continue;
          state.chain = 0;
          load(list, /** @type {"start" | "end"} */ (attr(entry.target, "data-dir")));
        }
      },
      { root: scroll === scrolling() ? null : scroll, rootMargin: `${NEAR}px 0px` },
    );
    for (const dir of /** @type {const} */ (["start", "end"])) {
      const end = link(list, dir);
      if (end) state.io.observe(end);
    }
  }
}

on(document, "click", (/** @type {MouseEvent} */ e) => {
  const end = closest(e.target, "[data-slot=lazy-more]");
  const list = end && closest(end, "[data-ui-lazy-list]");
  if (!end || !list || e.button !== 0 || e.ctrlKey || e.metaKey || e.shiftKey || e.altKey) return;
  e.preventDefault();
  stateOf(list).chain = CHAIN;
  load(list, /** @type {"start" | "end"} */ (attr(end, "data-dir")));
});
// The live module asks a list to refresh: a list merges what changed, and no region
// around it fetches itself for it.
on(
  document,
  "ui:refresh",
  (/** @type {CustomEvent} */ e) => {
    const list = /** @type {Element} */ (e.target);
    if (!(list instanceof Element) || !has(list, "data-ui-lazy-list")) return;
    e.stopPropagation();
    if (e.detail && typeof e.detail === "object") e.detail.done = live(list);
    else live(list);
  },
  true,
);
// A page shown again tells the server what it showed while it was hidden.
on(document, "visibilitychange", () => {
  if (!hidden()) for (const list of every("[data-ui-lazy-list][data-ui-seen]")) seen(list);
});
on(document, "ui:swap", scan);
scan();
