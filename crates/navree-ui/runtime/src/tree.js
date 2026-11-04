// The tree module. The server writes nested lists, links and inputs that work with no
// script; this makes them WAI-ARIA's tree view: the roles, one tab stop, the keys,
// type-ahead, opening and closing, tri-state parents, and children fetched as a branch
// opens. Everything is delegated from `document` and read through dom.js, so a morph
// needs no wiring and a name in stored markup can't shadow a method.
//
// The list items are `role=none` and each item's row is the `treeitem`: its name is its
// own label, not the text of everything under it, and its group is owned with
// `aria-owns`. A row is what has the tab stop and the focus.
import { active, all, attr, body, byId, clean, closest, every, has, make, on, sameOrigin, set, unset } from "./dom.js";

const ROOT = "[data-ui-tree]";
const ITEM = "li[data-slot=tree-item],li[data-slot=tree-more],li[data-slot=tree-note]";
const BRANCH = "li[data-slot=tree-item]";
const TREEITEM = "[role=treeitem]";
/** What this module adds, which a morph must leave: the element lists it itself. */
const KEEP = "data-ui-keep-attrs role aria-expanded aria-checked aria-selected aria-level aria-setsize aria-posinset aria-labelledby aria-label aria-owns aria-busy tabindex data-checked data-enhanced";
/** How long a fetch may take. */
const LIMIT = 15000;
/** The words, in English; the server sends its own in `data-l`. */
const EN = { loading: "Loading", failed: "Couldn't load this.", retry: "Try again", loaded: "{n} items loaded" };

/** @param {Element} node @param {string} selector @returns {HTMLElement | null} */
const first = (node, selector) => /** @type {any} */ (all(node, selector)[0] || null);
/** @param {Element} node @param {string} selector @returns {HTMLElement[]} */
const list = (node, selector) => /** @type {any} */ ([...all(node, selector)]);
/** @param {any} node @returns {HTMLElement | null} */
const itemOf = (node) => /** @type {any} */ (closest(node, ITEM));
/** @param {Element} node @returns {HTMLElement | null} */
const rootOf = (node) => /** @type {any} */ (closest(node, ROOT));
/** The row of an item: what is the treeitem. @param {Element} item @returns {HTMLElement | null} */
const rowOf = (item) => first(item, ":scope > [data-slot=tree-row]");
/** The item a nested item hangs from. @param {Element} item @returns {HTMLElement | null} */
const parentOf = (item) => /** @type {any} */ (closest(item.parentNode, BRANCH));
/** @param {Element} node */
const rtl = (node) => getComputedStyle(node).direction === "rtl";
/** @param {Element} root @returns {typeof EN} */
function words(root) {
  try {
    return { ...EN, ...JSON.parse(attr(root, "data-l") || "{}") };
  } catch {
    return EN;
  }
}
/** @param {Element} root @param {string} text */
function say(root, text) {
  const live = first(root, "[data-slot=tree-status]");
  if (live) live.textContent = text;
}

/** Whether an item is showing: every branch above it is open. @param {Element} item */
function visible(item) {
  for (let up = parentOf(item); up; up = parentOf(up)) if (!has(up, "data-open")) return false;
  return true;
}
/** The items a key reaches, in order. @param {Element} root */
const shown = (root) => list(root, ITEM).filter(visible);
/** @param {Element} item */
const isBranch = (item) => has(item, "data-branch");
/** @param {Element} item */
const isMore = (item) => attr(item, "data-slot") === "tree-more";
/** A row that says something about a group: it is loading, or failed. @param {Element} item */
const isNote = (item) => attr(item, "data-slot") === "tree-note";
/** @param {Element} item @returns {HTMLInputElement | null} */
const inputOf = (item) => /** @type {any} */ (first(item, ":scope > [data-slot=tree-row] input"));
/** The words an item starts with, for type-ahead. @param {Element} item */
const textOf = (item) => first(item, ":scope > [data-slot=tree-row] [data-slot=tree-text]")?.textContent?.trim().toLowerCase() || "";

let counter = 0;
/** An id that nothing on the page has. */
function fresh() {
  let id;
  do id = "ut" + counter++;
  while (byId(id));
  return id;
}

/** @param {HTMLElement} item */
function enhanceItem(item) {
  set(item, "role", "none");
  const row = rowOf(item);
  if (!row) return;
  set(row, "role", "treeitem");
  set(row, "data-ui-keep-attrs", KEEP);
  for (const [from, to] of [["data-setsize", "aria-setsize"], ["data-posinset", "aria-posinset"]]) {
    const value = attr(item, from);
    if (value) set(row, to, value);
  }
  // How deep it stands, from where it is: a branch that was fetched knows nothing of what it hangs from.
  let depth = 1;
  for (let up = parentOf(item); up; up = parentOf(up)) depth++;
  set(row, "aria-level", String(depth));
  if (!has(row, "tabindex")) set(row, "tabindex", "-1");
  const text = isMore(item) ? first(row, "a") : isNote(item) ? null : first(row, "[data-slot=tree-text]");
  if (text) {
    if (!text.id) text.id = fresh();
    set(row, "aria-labelledby", text.id);
  }
  // What the keys of the tree do, its controls need not be reached by Tab.
  for (const control of list(row, "a,input,button")) set(control, "tabindex", "-1");
  if (isMore(item) || isNote(item)) return;
  if (isBranch(item)) set(row, "aria-expanded", String(has(item, "data-open")));
  const group = first(item, ":scope > [data-slot=tree-group]");
  if (group) {
    set(group, "role", "group");
    if (!group.id) group.id = fresh();
    set(row, "aria-owns", group.id);
  }
  const toggle = first(row, "[data-slot=tree-toggle]");
  if (toggle) set(toggle, "aria-hidden", "true");
  checks(item);
}

/** Brings an item's checked state, as the page and assistive technology read it, in line with its input. @param {HTMLElement} item */
function checks(item) {
  const root = rootOf(item);
  const input = inputOf(item);
  const row = rowOf(item);
  if (!root || !input || !row) return;
  const single = attr(root, "data-select") === "single";
  const mixed = has(root, "data-cascade") && attr(item, "data-checked") === "mixed" && !input.checked;
  input.indeterminate = mixed;
  const state = mixed ? "mixed" : input.checked ? "true" : "false";
  set(item, "data-checked", state);
  set(row, single ? "aria-selected" : "aria-checked", state);
}

/** @param {HTMLElement} root */
function enhance(root) {
  set(root, "data-enhanced", "");
  set(root, "data-ui-keep-attrs", KEEP);
  const top = first(root, "[data-slot=tree-root]");
  if (!top) return;
  set(top, "role", "tree");
  set(top, "aria-label", attr(root, "data-label") || "");
  set(top, "data-ui-keep-attrs", KEEP);
  const items = list(root, ITEM);
  for (const item of items) {
    // The items can be marked again by a morph that brought them back as the server wrote them.
    enhanceItem(item);
    set(item, "data-ui-keep-attrs", "data-ui-keep-attrs role data-open data-checked");
  }
  // One tab stop: the row that has it, or the first.
  const rows = items.map(rowOf).filter((row) => !!row);
  const stops = rows.filter((row) => attr(row, "tabindex") === "0");
  for (const extra of stops.slice(1)) set(extra, "tabindex", "-1");
  if (!stops.length && rows[0]) set(rows[0], "tabindex", "0");
  // A branch the server drew open, whose children are still to come.
  for (const item of items) if (has(item, "data-open") && has(item, "data-lazy")) load(item);
}

/** Makes the row of `item` the one tab stop of its tree and focuses it. @param {HTMLElement} item @param {boolean} [focus] */
function go(item, focus = true) {
  const root = rootOf(item);
  const row = rowOf(item);
  if (!root || !row) return;
  for (const other of list(root, TREEITEM + "[tabindex='0']")) if (other !== row) set(other, "tabindex", "-1");
  set(row, "tabindex", "0");
  if (focus) {
    row.focus({ preventScroll: true });
    row.scrollIntoView({ block: "nearest", inline: "nearest" });
  }
}

// ---- Opening and closing -------------------------------------------------

/** @param {HTMLElement} item */
function open(item) {
  if (!isBranch(item) || has(item, "data-open")) return;
  set(item, "data-open", "");
  const row = rowOf(item);
  if (row) set(row, "aria-expanded", "true");
  if (has(item, "data-lazy")) load(item);
}

/** @param {HTMLElement} item */
function close(item) {
  if (!isBranch(item) || !has(item, "data-open")) return;
  const focused = active();
  const row = rowOf(item);
  unset(item, "data-open");
  if (row) set(row, "aria-expanded", "false");
  // Focus, or the tab stop, on a node that is no longer shown goes to the one that was closed.
  const lost = list(item, ":scope > [data-slot=tree-group] " + TREEITEM + "[tabindex='0']").length > 0;
  const inside = !!focused && focused !== row && item.contains(focused);
  if (lost || inside) go(item, inside);
}

/** @param {HTMLElement} item */
const toggle = (item) => (has(item, "data-open") ? close(item) : open(item));

// ---- Fetching children ---------------------------------------------------

/** What is on its way, by the element that asked: one request at a time for each. @type {WeakMap<Element, number>} */
const pending = new WeakMap();

/**
 * Asks for the markup at `src` and gives back the list items in it. A rejected answer is null.
 * @param {string} src
 * @returns {Promise<HTMLElement[] | null>}
 */
async function fetchItems(src) {
  if (!sameOrigin(src)) return null;
  const response = await fetch(src, {
    headers: { "X-Navree-UI": "1", Accept: "text/html" },
    credentials: "same-origin",
    redirect: "error",
    signal: typeof AbortSignal.timeout === "function" ? AbortSignal.timeout(LIMIT) : undefined,
  });
  if (!response.ok || !(response.headers.get("content-type") || "").startsWith("text/html")) return null;
  const doc = new DOMParser().parseFromString(await response.text(), "text/html");
  // The answer is read through the prototype: a named element in it can't stand in for `doc.body`.
  const holder = body(doc);
  clean(holder);
  // The items, or a list that holds them and nothing else.
  const only = holder.children.length === 1 ? holder.children[0] : null;
  const from = only && only.matches("ul[data-slot=tree-group]") ? only : holder;
  /** @type {HTMLElement[]} */
  const items = [];
  for (const child of from.children) {
    if (!(child instanceof HTMLElement) || !child.matches(ITEM)) continue;
    // An item may not bring an id the page already answers to, anywhere in it.
    if ([child, ...list(child, "[id]")].some((el) => el.id && byId(el.id))) continue;
    items.push(child);
  }
  return items;
}

/** A row that says something about the group: loading, or that it failed. Its row is a treeitem like the others, and the one to press to try again. @param {string} text @param {string} [retry] */
function note(text, retry) {
  const item = make("li");
  set(item, "data-slot", "tree-note");
  const row = make("div");
  set(row, "data-slot", "tree-row");
  if (retry) set(row, "data-retry", "");
  const span = make("span");
  span.textContent = text;
  row.append(span);
  if (retry) {
    const again = make("span");
    set(again, "data-slot", "tree-retry");
    again.textContent = retry;
    row.append(" ", again);
  }
  item.append(row);
  return item;
}

/** Puts a note in a group and gives it its role. @param {HTMLElement} group @param {string} text @param {string} [retry] */
function say_in(group, text, retry) {
  const item = note(text, retry);
  group.replaceChildren(item);
  enhanceItem(item);
  set(item, "data-ui-keep-attrs", "data-ui-keep-attrs role");
}

/** Fetches the children of a lazy branch and puts them in its group. @param {HTMLElement} item */
async function load(item) {
  const root = rootOf(item);
  const row = rowOf(item);
  const group = first(item, ":scope > [data-slot=tree-group]");
  const src = group && attr(group, "data-src");
  if (!root || !row || !group || !src || pending.has(group)) return;
  const l = words(root);
  pending.set(group, 1);
  set(row, "aria-busy", "true");
  say_in(group, l.loading);
  say(root, l.loading);
  let items = null;
  try {
    items = await fetchItems(src);
  } catch {}
  pending.delete(group);
  // The branch went away, or was pointed somewhere else, while this was on its way.
  if (!item.isConnected || attr(group, "data-src") !== src) return;
  unset(row, "aria-busy");
  if (!items) {
    say_in(group, l.failed, l.retry);
    say(root, l.failed);
    return;
  }
  unset(group, "data-src");
  unset(item, "data-lazy");
  group.replaceChildren(...items);
  settle(root, item, items);
  say(root, l.loaded.replace("{n}", String(items.length)));
}

/** Asks again for the children of the branch a failed note stands in. @param {HTMLElement} note */
function again(note) {
  const owner = parentOf(note);
  const group = owner && first(owner, ":scope > [data-slot=tree-group]");
  if (!owner || !group) return;
  pending.delete(group);
  load(owner);
}

/** Fetches the next page of a list and puts it where the row stood. @param {HTMLElement} item */
async function more(item) {
  const root = rootOf(item);
  const row = rowOf(item);
  const link = row && first(row, "a");
  const src = link && attr(link, "href");
  if (!root || !row || !src || pending.has(item)) return;
  const l = words(root);
  pending.set(item, 1);
  set(row, "aria-busy", "true");
  say(root, l.loading);
  let items = null;
  try {
    items = await fetchItems(src);
  } catch {}
  pending.delete(item);
  if (!item.isConnected) return;
  unset(row, "aria-busy");
  if (!items) {
    say(root, l.failed);
    return;
  }
  const parent = parentOf(item);
  const focusedHere = active() === row;
  item.replaceWith(...items);
  settle(root, parent, items);
  say(root, l.loaded.replace("{n}", String(items.filter((each) => !isMore(each)).length)));
  const next = items.find((each) => !isMore(each)) || items[0];
  if (focusedHere && next) go(next);
}

/** Gives new items their roles, and the choice their parent already made. @param {HTMLElement} root @param {HTMLElement | null} parent @param {HTMLElement[]} items */
function settle(root, parent, items) {
  const input = parent && inputOf(parent);
  for (const item of items) {
    for (const child of [item, ...list(item, ITEM)]) {
      enhanceItem(child);
      set(child, "data-ui-keep-attrs", "data-ui-keep-attrs role data-open data-checked");
    }
  }
  // A branch that was chosen before its children came stands for them.
  if (parent && input && input.checked && has(root, "data-cascade")) {
    for (const item of items) setBelow(item, true);
  }
  if (parent && has(root, "data-cascade")) refresh(parent);
}

// ---- Choosing ------------------------------------------------------------

/** Sets an item and everything loaded under it. @param {HTMLElement} item @param {boolean} on */
function setBelow(item, on) {
  const own = inputOf(item);
  if (own && !own.disabled) own.checked = on;
  for (const child of list(item, "li[data-slot=tree-item]")) {
    const input = inputOf(child);
    if (input && !input.disabled) input.checked = on;
  }
  for (const each of [item, ...list(item, BRANCH)]) {
    unset(each, "data-checked");
    const input = inputOf(each);
    if (input) input.indeterminate = false;
    checks(each);
  }
}

/** Works out a branch's state from its children, upward. @param {HTMLElement} item */
function refresh(item) {
  for (let up = item; up; up = parentOf(up)) {
    const input = inputOf(up);
    // A choice that is disabled is not the person's to make: it doesn't count.
    const kids = list(up, ":scope > [data-slot=tree-group] > li[data-slot=tree-item]").filter((kid) => !has(kid, "data-disabled"));
    if (input && kids.length) {
      const states = kids.map((kid) => attr(kid, "data-checked") || "false");
      const every = states.every((s) => s === "true");
      const none = states.every((s) => s === "false");
      input.checked = every;
      set(up, "data-checked", every ? "true" : none ? "false" : "mixed");
      input.indeterminate = !every && !none;
      checks(up);
    }
  }
}

on(document, "change", (e) => {
  const input = /** @type {HTMLInputElement | null} */ (closest(e.target, ROOT + " [data-slot=tree-row] input"));
  const item = input && itemOf(input);
  const root = item && rootOf(item);
  if (!input || !item || !root) return;
  if (attr(root, "data-select") === "single") {
    for (const other of list(root, BRANCH)) {
      const radio = inputOf(other);
      const row = rowOf(other);
      if (radio && row) set(row, "aria-selected", String(radio.checked));
    }
    return;
  }
  unset(item, "data-checked");
  input.indeterminate = false;
  if (has(root, "data-cascade")) {
    setBelow(item, input.checked);
    const up = parentOf(item);
    if (up) refresh(up);
  } else {
    checks(item);
  }
});

// ---- Keys, pointer -------------------------------------------------------

on(document, "click", (e) => {
  const node = /** @type {Element} */ (e.target);
  const tree = rootOf(node);
  if (!tree || !has(tree, "data-enhanced")) return;
  const caret = closest(node, "[data-slot=tree-toggle]");
  const item = itemOf(node);
  if (caret && item) {
    e.preventDefault();
    toggle(item);
    go(item);
    return;
  }
  const next = closest(node, "[data-slot=tree-more-link]");
  if (next && item) {
    e.preventDefault();
    more(item);
    return;
  }
  if (item && isNote(item) && has(/** @type {Element} */ (rowOf(item)), "data-retry")) {
    again(item);
    return;
  }
  // A click on the words of a branch with nothing to choose opens it.
  const label = closest(node, "span[data-slot=tree-label]");
  if (label && item && isBranch(item)) toggle(item);
  if (item) go(item, false);
});

// Focus that lands on a control inside a row goes to the row, which is what the keys act on.
on(document, "focusin", (e) => {
  const node = /** @type {Element} */ (e.target);
  const item = itemOf(node);
  const row = item && rowOf(item);
  if (!item || !row || !rootOf(item) || !has(/** @type {Element} */ (rootOf(item)), "data-enhanced")) return;
  if (node !== row && closest(node, "[data-slot=tree-row]") === row) go(item);
  else if (node === row) go(item, false);
});

let buffer = "";
let bufferTimer = 0;
/** @param {HTMLElement} root @param {HTMLElement} from @param {string} key */
function typeahead(root, from, key) {
  clearTimeout(bufferTimer);
  bufferTimer = window.setTimeout(() => { buffer = ""; }, 500);
  buffer += key.toLowerCase();
  const items = shown(root).filter((item) => !isMore(item) && !isNote(item));
  const repeated = [...buffer].every((c) => c === buffer[0]);
  const query = repeated ? buffer[0] : buffer;
  const start = items.indexOf(from) + (repeated ? 1 : 0);
  for (let k = 0; k < items.length; k++) {
    const item = items[(Math.max(start, 0) + k) % items.length];
    if (textOf(item).startsWith(query)) {
      go(item);
      return;
    }
  }
}

/** The anchor of a range of choices, by tree. @type {WeakMap<Element, Element>} */
const anchors = new WeakMap();

/** Chooses or unchooses with a click on the item's input. @param {HTMLElement} item @param {boolean} [force] */
function choose(item, force) {
  const input = inputOf(item);
  if (!input || input.disabled) return;
  if (force === undefined || input.checked !== force) input.click();
  const root = rootOf(item);
  if (root) anchors.set(root, item);
}

/** @param {HTMLElement} root @param {HTMLElement} from @param {HTMLElement} to */
function range(root, from, to) {
  const items = shown(root);
  let a = items.indexOf(from), b = items.indexOf(to);
  if (a < 0 || b < 0) return;
  if (a > b) [a, b] = [b, a];
  for (const item of items.slice(a, b + 1)) choose(item, true);
}

on(document, "keydown", (e) => {
  if (e.altKey || e.defaultPrevented || e.metaKey) return;
  const item = itemOf(e.target);
  const row = item && rowOf(item);
  const root = item && rootOf(item);
  // Only keys that land on a row act: a key inside the row's own control is left to it.
  if (!item || !row || !root || !has(root, "data-enhanced") || e.target !== row) return;
  const key = e.key;
  const multiple = attr(root, "data-select") === "multiple";
  const single = attr(root, "data-select") === "single";
  const items = shown(root);
  const at = items.indexOf(item);
  const flip = rtl(root);
  const done = () => e.preventDefault();
  if (e.ctrlKey && !e.shiftKey && key.toLowerCase() === "a" && multiple) {
    done();
    for (const each of items) choose(each, true);
    return;
  }
  if (e.ctrlKey && e.shiftKey && multiple && (key === "Home" || key === "End")) {
    done();
    range(root, item, key === "Home" ? items[0] : items[items.length - 1]);
    return;
  }
  if (e.ctrlKey) return;
  if (key === "ArrowDown" || key === "ArrowUp") {
    done();
    const to = items[key === "ArrowDown" ? at + 1 : at - 1];
    if (!to) return;
    go(to);
    if (e.shiftKey && multiple) choose(to);
  } else if (key === (flip ? "ArrowLeft" : "ArrowRight")) {
    done();
    if (!isBranch(item)) return;
    if (!has(item, "data-open")) open(item);
    else {
      const child = first(item, ":scope > [data-slot=tree-group] > " + ITEM);
      if (child) go(child);
    }
  } else if (key === (flip ? "ArrowRight" : "ArrowLeft")) {
    done();
    if (isBranch(item) && has(item, "data-open")) close(item);
    else {
      const up = parentOf(item);
      if (up) go(up);
    }
  } else if (key === "Home" || key === "End") {
    done();
    go(items[key === "Home" ? 0 : items.length - 1]);
  } else if ((key === "Enter" || key === " ") && isNote(item)) {
    done();
    if (has(row, "data-retry")) again(item);
  } else if (key === "Enter") {
    const link = isMore(item) ? first(row, "a") : first(row, "a[href]:not([data-slot=tree-toggle])");
    done();
    if (link) link.click();
    else if (single) choose(item, true);
    else if (isBranch(item)) toggle(item);
    else if (multiple) choose(item);
  } else if (key === " ") {
    done();
    if (e.shiftKey && multiple) {
      const anchor = /** @type {HTMLElement | undefined} */ (anchors.get(root));
      range(root, anchor && shown(root).includes(anchor) ? anchor : item, item);
    } else if (isMore(item)) first(row, "a")?.click();
    else if (multiple) choose(item);
    else if (single) choose(item, true);
  } else if (key === "*") {
    done();
    const group = item.parentNode;
    if (group) for (const sibling of list(/** @type {Element} */ (group), ":scope > " + BRANCH)) open(sibling);
  } else if (key.length === 1) {
    done();
    typeahead(root, item, key);
  }
});

function start() {
  for (const root of every(ROOT)) enhance(/** @type {HTMLElement} */ (root));
}
start();
on(document, "ui:swap", start);
on(window, "pageshow", start);
