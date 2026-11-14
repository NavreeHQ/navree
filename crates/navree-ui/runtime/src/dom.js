// Named elements shadow properties: `<img name="cookie">` makes `document.cookie`
// that image, `<input name="method">` makes `form.method` that input, and an element
// with the id `addEventListener` is what `addEventListener` finds. Stored markup
// can hold any of these. Everything the runtime reads from a form or a document
// goes through the prototype, so no name in a page can change what it does.

export const D = Document.prototype, E = Element.prototype, T = EventTarget.prototype;
/** @param {object} proto @param {string} name @param {"get" | "set"} kind @returns {(this: any, ...args: any[]) => any} */
const accessor = (proto, name, kind) => /** @type {any} */ (Object.getOwnPropertyDescriptor(proto, name)?.[kind]);
// One statement for each, marked pure: a module that doesn't use one doesn't carry it.
const gBody = /* @__PURE__ */ accessor(D, "body", "get");
const gHead = /* @__PURE__ */ accessor(D, "head", "get");
const gRoot = /* @__PURE__ */ accessor(D, "documentElement", "get");
const gActive = /* @__PURE__ */ accessor(D, "activeElement", "get");
const gCookie = /* @__PURE__ */ accessor(D, "cookie", "get");
const gTitle = /* @__PURE__ */ accessor(D, "title", "get");
const gHidden = /* @__PURE__ */ accessor(D, "hidden", "get");
const sTitle = /* @__PURE__ */ accessor(D, "title", "set");
const sCookie = /* @__PURE__ */ accessor(D, "cookie", "set");

/** @param {Document} [doc] @returns {HTMLElement} */
export const body = (doc = document) => gBody.call(doc);
/** @param {Document} [doc] @returns {HTMLHeadElement} */
export const head = (doc = document) => gHead.call(doc);
/** @param {Document} [doc] @returns {HTMLElement} */
export const root = (doc = document) => gRoot.call(doc);
/** The element that scrolls the page. @returns {HTMLElement} */
export const scrolling = () => accessor(D, "scrollingElement", "get").call(document) || gRoot.call(document);
/** A deep copy of `node`, made a node of this document. @param {Node} node @returns {any} */
export const adopt = (node) => D.importNode.call(document, node, true);
/** @returns {Element | null} */
export const active = () => gActive.call(document);
/** @returns {string} */
export const cookies = () => gCookie.call(document);
/**
 * The cookie called `name`, decoded. Another site on the same domain can set one
 * that doesn't decode, or that no header can carry: those are passed over.
 * @param {string} name
 */
export const cookie = (name) => {
  for (const part of cookies().split("; ")) {
    if (!part.startsWith(name + "=")) continue;
    try {
      const value = decodeURIComponent(part.slice(name.length + 1));
      if (/^[\x21-\x7e]+$/.test(value)) return value;
    } catch {}
  }
  return "";
};
/** @param {string} cookie */
export const setCookie = (cookie) => sCookie?.call(document, cookie);
/** @returns {boolean} */
export const hidden = () => gHidden.call(document);
/** @param {Document} [doc] @returns {string} */
export const titleOf = (doc = document) => gTitle.call(doc);
/** @param {string} title */
export const setTitle = (title) => sTitle?.call(document, title);
/**
 * Whether `el` is stored content: inside what the server marks `data-ui-stored` (rich text, Markdown),
 * and not in a part it draws there for itself (`data-ui-own`, a code block's copy button). No hook in
 * stored content drives the runtime, and no element in it stands in for one of the page's own. Neither
 * mark can be written from inside: stored markup loses every `data-ui-*` when it is cleaned.
 * @param {Element} el
 */
export const stored = (el) => E.matches.call(el, "[data-ui-stored] :not([data-ui-own],[data-ui-own] *)");
/** @param {Element | null} el */
const own = (el) => (el && !stored(el) ? el : null);
/** The element with this id, unless it is stored content. @param {string} id @param {Document} [doc] @returns {HTMLElement | null} */
export const byId = (id, doc = document) => own(D.getElementById.call(doc, id));
/** @param {string} selector @returns {Element | null} */
export const one = (selector) => own(D.querySelector.call(document, selector));
/** The elements that match, stored content left out. @param {string} selector @param {Document} [doc] @returns {Element[]} */
export const every = (selector, doc = document) => [...D.querySelectorAll.call(doc, selector)].filter((el) => !stored(el));
/** @param {string} tag @returns {HTMLElement} */
export const make = (tag) => D.createElement.call(document, tag);
/** @param {string} text @returns {Text} */
export const textNode = (text) => D.createTextNode.call(document, text);
/** @param {EventTarget} target @param {string} type @param {(e: any) => void} listener @param {boolean | AddEventListenerOptions} [options] */
export const on = (target, type, listener, options) => T.addEventListener.call(target, type, listener, options);
/** Fires `event` at the document. @param {Event} event */
export const emit = (event) => T.dispatchEvent.call(document, event);

/**
 * Takes out what acts by being in a page and that no content security policy
 * stops: a refresh that leaves the site, a base that repoints every link.
 * @param {Element | DocumentFragment} tree
 */
export const clean = (tree) => {
  const found = tree.nodeType === 11 ? DocumentFragment.prototype.querySelectorAll.call(tree, "meta[http-equiv],base") : E.querySelectorAll.call(tree, "meta[http-equiv],base");
  for (const el of found) el.remove();
};

/** Whether `url` resolves to this origin. @param {string} url */
export const sameOrigin = (url) => {
  try {
    return new URL(url, location.href).origin === location.origin;
  } catch {
    return false;
  }
};

/** What the runtime made and keeps on every page. An attribute would let stored markup ask for the same. @type {WeakSet<Node>} */
export const persistent = new WeakSet();

/**
 * The persistent nodes that stand in the page now, which a removal must not take
 * along inside something else (the toaster stands in an open dialog).
 * @type {Set<HTMLElement>}
 */
export const keeping = new Set();

/**
 * Before `node` goes, takes out of it what the runtime keeps and puts it back in the page.
 * @param {Node} node
 */
export const rescue = (node) => {
  for (const kept of keeping) {
    if (!kept.isConnected) keeping.delete(kept);
    else if (kept !== node && holds(node, kept)) body().append(kept);
  }
};

/** Whether `url` is one of the toolkit's own modules. @param {string} url */
export const ownModule = (url) => {
  try {
    const parsed = new URL(url, location.href);
    return parsed.origin === location.origin && parsed.pathname.startsWith("/_navree/ui/");
  } catch {
    return false;
  }
};

/** @param {Element} el @param {string} name */
export const attr = (el, name) => E.getAttribute.call(el, name);
/**
 * The nearest ancestor of `node` (or itself) that matches, or null: safe on anything an event can name.
 * One in stored content is none: what a hook looks for there is not found.
 * @param {any} node @param {string} selector @returns {Element | null}
 */
export const closest = (node, selector) => own(node instanceof Element ? E.closest.call(node, selector) : null);
/** Whether `a` holds `b`. @param {Node | null | undefined} a @param {Node | null | undefined} b */
export const holds = (a, b) => !!(a && b) && Node.prototype.contains.call(a, b);
/** @param {Element} el @param {string} name */
export const has = (el, name) => E.hasAttribute.call(el, name);
/** @param {Element} el @param {string} name @param {string} value */
export const set = (el, name, value) => E.setAttribute.call(el, name, value);
/** @param {Element} el @param {string} name */
export const unset = (el, name) => E.removeAttribute.call(el, name);
/** @param {Element} el @param {string} name @param {boolean} [force] */
export const toggle = (el, name, force) => E.toggleAttribute.call(el, name, force);
/** @param {Element} el @param {string} selector */
export const all = (el, selector) => E.querySelectorAll.call(el, selector);
/** The field called `name` in `form`: one element, the first of a radio group. @param {HTMLFormElement} form @param {string} name */
export const field = (form, name) => {
  const found = accessor(HTMLFormElement.prototype, "elements", "get").call(form).namedItem(name);
  return found instanceof RadioNodeList ? found[0] : found;
};
