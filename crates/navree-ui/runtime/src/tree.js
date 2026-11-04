// The tree module. The server writes nested lists, links and inputs that work with
// no script; this makes them WAI-ARIA's tree view: the roles, one tab stop, the keys,
// type-ahead, opening and closing, tri-state parents, and children fetched as a branch
// opens. Everything is delegated from `document` and read through dom.js, so a morph
// needs no wiring and a name in stored markup can't shadow a method.
import { active, all, attr, byId, clean, closest, every, has, make, on, sameOrigin, set, unset } from "./dom.js";

const ROOT = "[data-ui-tree]";
const ITEM = "li[data-slot=tree-item],li[data-slot=tree-more]";
const BRANCH = "li[data-slot=tree-item]";
/** What this module adds to an element, which a morph must leave: the element lists it itself. */
const KEEP = "data-ui-keep-attrs role aria-expanded aria-checked aria-selected aria-level aria-setsize aria-posinset aria-labelledby aria-label aria-busy aria-multiselectable tabindex data-open data-checked data-enhanced";
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
/** @param {Element} item @returns {HTMLInputElement | null} */
const inputOf = (item) => /** @type {any} */ (first(item, ":scope > [data-slot=tree-row] input"));
/** The words an item starts with, for type-ahead. @param {Element} item */
const textOf = (item) => (first(item, ":scope > [data-slot=tree-row] [data-slot=tree-text]") || first(item, "a"))?.textContent?.trim().toLowerCase() || "";

let counter = 0;
/** @param {Element} item */
function enhanceItem(item) {
  const more = has(item, "data-slot") && attr(item, "data-slot") === "tree-more";
  set(item, "role", "treeitem");
  set(item, "data-ui-keep-attrs", KEEP);
  for (const [from, to] of [["data-level", "aria-level"], ["data-setsize", "aria-setsize"], ["data-posinset", "aria-posinset"]]) {
    const value = attr(item, from);
    if (value) set(item, to, value);
  }
  if (!has(item, "tabindex")) set(item, "tabindex", "-1");
  const row = first(item, ":scope > [data-slot=tree-row]");
  const text = more ? first(item, "a") : row && first(row, "[data-slot=tree-text]");
  if (text) {
    if (!text.id) {
      do text.id = "ut" + counter++;
      while (byId(text.id) !== text);
    }
    set(item, "aria-labelledby", text.id);
  }
  if (more) {
    for (const link of list(item, "a")) set(link, "tabindex", "-1");
    return;
  }
  if (isBranch(item)) set(item, "aria-expanded", String(has(item, "data-open")));
  const group = first(item, ":scope > [data-slot=tree-group]");
  if (group) set(group, "role", "group");
  // What the keys of the tree do, its controls need not be reached by Tab.
  if (row) for (const control of list(row, "a,input,button")) set(control, "tabindex", "-1");
  const toggle = row && first(row, "[data-slot=tree-toggle]");
  if (toggle) set(toggle, "aria-hidden", "true");
  checks(item);
}

/** Brings an item's checked state, as the page and assistive technology read it, in line with its input. @param {HTMLElement} item */
function checks(item) {
  const root = rootOf(item);
  const input = inputOf(item);
  if (!root || !input) return;
  const single = attr(root, "data-select") === "single";
  const mixed = has(root, "data-cascade") && attr(item, "data-checked") === "mixed" && !input.checked;
  input.indeterminate = mixed;
  const state = mixed ? "mixed" : input.checked ? "true" : "false";
  set(item, "data-checked", state);
  set(item, single ? "aria-selected" : "aria-checked", state);
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
  for (const item of items) enhanceItem(item);
  // One tab stop: the item that has it, or the first.
  const stops = items.filter((item) => attr(item, "tabindex") === "0");
  for (const extra of stops.slice(1)) set(extra, "tabindex", "-1");
  if (!stops.length && items[0]) set(items[0], "tabindex", "0");
  // A branch the server drew open, whose children are still to come.
  for (const item of items) if (has(item, "data-open") && has(item, "data-lazy")) load(item);
}

/** Makes `item` the one tab stop and focuses it. @param {HTMLElement} item @param {boolean} [focus] */
function go(item, focus = true) {
  const root = rootOf(item);
  if (!root) return;
  for (const other of list(root, ITEM + "[tabindex='0']")) if (other !== item) set(other, "tabindex", "-1");
  set(item, "tabindex", "0");
  if (focus) {
    item.focus({ preventScroll: true });
    item.scrollIntoView({ block: "nearest", inline: "nearest" });
  }
}

// ---- Opening and closing -------------------------------------------------

/** @param {HTMLElement} item */
function open(item) {
  if (!isBranch(item) || has(item, "data-open")) return;
  set(item, "data-open", "");
  set(item, "aria-expanded", "true");
  if (has(item, "data-lazy")) load(item);
}

/** @param {HTMLElement} item */
function close(item) {
  if (!isBranch(item) || !has(item, "data-open")) return;
  const focused = active();
  unset(item, "data-open");
  set(item, "aria-expanded", "false");
  // Focus, or the tab stop, on a node that is no longer shown goes to the one that was closed.
  const lost = list(item, ITEM + "[tabindex='0']").length > 0;
  if (lost || (focused && focused !== item && item.contains(focused))) go(item, !!focused && item.contains(focused));
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
  clean(doc.body);
  const wrapped = first(doc.body, "ul[data-slot=tree-group]");
  const from = wrapped || doc.body;
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

/** A row that says something about the group: loading, or that it failed. @param {string} text @param {string} [retry] */
function note(text, retry) {
  const row = make("li");
  set(row, "data-slot", "tree-note");
  set(row, "role", "none");
  const span = make("span");
  span.textContent = text;
  row.append(span);
  if (retry) {
    const button = make("button");
    set(button, "type", "button");
    set(button, "data-slot", "tree-retry");
    set(button, "data-variant", "link");
    button.textContent = retry;
    row.append(button);
  }
  return row;
}

/** Fetches the children of a lazy branch and puts them in its group. @param {HTMLElement} item */
async function load(item) {
  const root = rootOf(item);
  const group = first(item, ":scope > [data-slot=tree-group]");
  const src = group && attr(group, "data-src");
  if (!root || !group || !src || pending.has(group)) return;
  const l = words(root);
  const mine = (pending.get(group) || 0) + 1;
  pending.set(group, mine);
  set(item, "aria-busy", "true");
  group.replaceChildren(note(l.loading));
  say(root, l.loading);
  let items = null;
  try {
    items = await fetchItems(src);
  } catch {}
  pending.delete(group);
  // The branch went away, or was pointed somewhere else, while this was on its way.
  if (!item.isConnected || attr(group, "data-src") !== src) return;
  unset(item, "aria-busy");
  if (!items) {
    group.replaceChildren(note(l.failed, l.retry));
    say(root, l.failed);
    return;
  }
  unset(group, "data-src");
  unset(item, "data-lazy");
  set(item, "data-loaded", "");
  group.replaceChildren(...items);
  settle(root, item, items);
  say(root, l.loaded.replace("{n}", String(items.length)));
}

/** Fetches the next page of a list and puts it where the row stood. @param {HTMLElement} row */
async function more(row) {
  const root = rootOf(row);
  const link = first(row, "a");
  const src = link && attr(link, "href");
  if (!root || !src || pending.has(row)) return;
  const l = words(root);
  pending.set(row, 1);
  set(row, "aria-busy", "true");
  say(root, l.loading);
  let items = null;
  try {
    items = await fetchItems(src);
  } catch {}
  pending.delete(row);
  if (!row.isConnected) return;
  unset(row, "aria-busy");
  if (!items) {
    say(root, l.failed);
    return;
  }
  const holder = row.parentNode;
  const parent = parentOf(row);
  const focusedHere = active() === row;
  row.replaceWith(...items);
  settle(root, parent, items);
  say(root, l.loaded.replace("{n}", String(items.filter((item) => attr(item, "data-slot") === "tree-item").length)));
  const next = items.find((item) => attr(item, "data-slot") === "tree-item") || items[0];
  if (focusedHere && next && holder) go(next);
}

/** Gives new items their roles, and the choice their parent already made. @param {HTMLElement} root @param {HTMLElement | null} parent @param {HTMLElement[]} items */
function settle(root, parent, items) {
  const input = parent && inputOf(parent);
  for (const item of items) {
    for (const child of [item, ...list(item, ITEM)]) enhanceItem(child);
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
    const kids = list(up, ":scope > [data-slot=tree-group] > li[data-slot=tree-item]");
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
      if (radio) set(other, "aria-selected", String(radio.checked));
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
  const retry = closest(node, "[data-slot=tree-retry]");
  const owner = retry && retry.parentNode && closest(/** @type {Element} */ (retry.parentNode), BRANCH);
  if (retry && owner) {
    const group = first(owner, ":scope > [data-slot=tree-group]");
    if (group) pending.delete(group);
    load(owner);
    return;
  }
  // A click on the words of a branch with nothing to choose opens it.
  const label = closest(node, "span[data-slot=tree-label]");
  if (label && item && isBranch(item)) toggle(item);
  if (item) go(item, false);
});

// Focus that lands on a control inside an item goes to the item, which is what the keys act on.
on(document, "focusin", (e) => {
  const node = /** @type {Element} */ (e.target);
  const item = itemOf(node);
  if (!item || !rootOf(item)) return;
  if (item !== node && closest(node, "[data-slot=tree-row]")) {
    go(item);
    return;
  }
  go(item, false);
});

let buffer = "";
let bufferTimer = 0;
/** @param {HTMLElement} root @param {HTMLElement} from @param {string} key */
function typeahead(root, from, key) {
  clearTimeout(bufferTimer);
  bufferTimer = window.setTimeout(() => { buffer = ""; }, 500);
  buffer += key.toLowerCase();
  const items = shown(root).filter((item) => attr(item, "data-slot") === "tree-item");
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
  if (e.altKey || e.defaultPrevented) return;
  const item = itemOf(e.target);
  const root = item && rootOf(item);
  if (!item || !root || !has(root, "data-enhanced")) return;
  // Keys that mean something to the page: leave them to it.
  if (e.metaKey) return;
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
  } else if (key === "Enter") {
    const link = first(item, ":scope > [data-slot=tree-row] a[href]:not([data-slot=tree-toggle])") || (attr(item, "data-slot") === "tree-more" ? first(item, "a") : null);
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
    } else if (attr(item, "data-slot") === "tree-more") first(item, "a")?.click();
    else if (multiple) choose(item);
    else if (single) choose(item, true);
  } else if (key === "*") {
    done();
    const group = item.parentNode;
    for (const sibling of list(/** @type {Element} */ (group), ":scope > " + BRANCH)) open(sibling);
  } else if (key.length === 1 && !e.ctrlKey) {
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
