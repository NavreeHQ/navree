// Named elements shadow properties: `<img name="cookie">` makes `document.cookie`
// that image, `<input name="method">` makes `form.method` that input, and an element
// with the id `addEventListener` is what `addEventListener` finds. Stored markup
// can hold any of these. Everything the runtime reads from a form or a document
// goes through the prototype, so no name in a page can change what it does.

const D = Document.prototype, E = Element.prototype, T = EventTarget.prototype;
/** @param {object} proto @param {string} name */
const getter = (proto, name) => /** @type {(this: any) => any} */ (Object.getOwnPropertyDescriptor(proto, name)?.get);
const gBody = getter(D, "body"), gHead = getter(D, "head"), gRoot = getter(D, "documentElement"), gActive = getter(D, "activeElement");
const gCookie = getter(D, "cookie"), gTitle = getter(D, "title"), gHidden = getter(D, "hidden");
const sTitle = Object.getOwnPropertyDescriptor(D, "title")?.set, sCookie = Object.getOwnPropertyDescriptor(D, "cookie")?.set;

/** @param {Document} [doc] @returns {HTMLElement} */
export const body = (doc = document) => gBody.call(doc);
/** @param {Document} [doc] @returns {HTMLHeadElement} */
export const head = (doc = document) => gHead.call(doc);
/** @param {Document} [doc] @returns {HTMLElement} */
export const root = (doc = document) => gRoot.call(doc);
/** @returns {Element | null} */
export const active = () => gActive.call(document);
/** @returns {string} */
export const cookies = () => gCookie.call(document);
/** @param {string} cookie */
export const setCookie = (cookie) => sCookie?.call(document, cookie);
/** @returns {boolean} */
export const hidden = () => gHidden.call(document);
/** @param {Document} [doc] @returns {string} */
export const titleOf = (doc = document) => gTitle.call(doc);
/** @param {string} title */
export const setTitle = (title) => sTitle?.call(document, title);
/** @param {string} id @param {Document} [doc] @returns {HTMLElement | null} */
export const byId = (id, doc = document) => D.getElementById.call(doc, id);
/** @param {string} selector @returns {Element | null} */
export const one = (selector) => D.querySelector.call(document, selector);
/** @param {string} selector @param {Document} [doc] @returns {NodeListOf<Element>} */
export const every = (selector, doc = document) => D.querySelectorAll.call(doc, selector);
/** @param {string} tag @returns {HTMLElement} */
export const make = (tag) => D.createElement.call(document, tag);
/** @param {EventTarget} target @param {string} type @param {(e: any) => void} listener @param {boolean | AddEventListenerOptions} [options] */
export const on = (target, type, listener, options) => T.addEventListener.call(target, type, listener, options);

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
const elements = /** @type {(this: HTMLFormElement) => HTMLFormControlsCollection} */ (getter(HTMLFormElement.prototype, "elements"));
/** The field called `name` in `form`: one element, the first of a radio group. @param {HTMLFormElement} form @param {string} name */
export const field = (form, name) => {
  const found = elements.call(form).namedItem(name);
  return found instanceof RadioNodeList ? found[0] : found;
};
