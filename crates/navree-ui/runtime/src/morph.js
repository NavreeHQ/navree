// Morph: turn one element into another, keeping what the user is doing.
// Focus, caret, selection, an active IME composition and typed values survive;
// elements with an id keep their identity, even when they move. Native "dirty"
// semantics do the value work: a changed `value` attribute reaches an input
// only if the user has not typed in it.

/** The element being composed in, if any. @type {Element | null} */
let composing = null;
document.addEventListener("compositionstart", (e) => { composing = /** @type {Element} */ (e.target); }, true);
document.addEventListener("compositionend", () => { composing = null; }, true);

const OPEN_STATE = new Set(["DIALOG", "DETAILS"]);

/** @param {Node} node */
function keyOf(node) {
  return node.nodeType === 1 ? /** @type {Element} */ (node).id || /** @type {Element} */ (node).getAttribute("data-key") : null;
}

/** @param {Node} parent @param {Node} node @param {Node | null} before */
function moveTo(parent, node, before) {
  if (node.nextSibling === before && node.parentNode === parent) return;
  if (/** @type {any} */ (parent).moveBefore && parent.isConnected && node.isConnected) {
    try { /** @type {any} */ (parent).moveBefore(node, before); return; } catch {}
  }
  parent.insertBefore(node, before);
}

/** Whether two nodes can become one another without replacing. @param {Node} a @param {Node} b */
function same(a, b) {
  if (a.nodeType !== b.nodeType || a.nodeName !== b.nodeName) return false;
  if (a.nodeType !== 1) return true;
  if (a.nodeName === "INPUT" && /** @type {Element} */ (a).getAttribute("type") !== /** @type {Element} */ (b).getAttribute("type")) return false;
  const key = keyOf(a), other = keyOf(b);
  return (!key && !other) || key === other;
}

/** @param {Element} a @param {Element} b */
function morphAttrs(a, b) {
  const keep = (a.getAttribute("data-ui-keep-attrs") || "").split(" ");
  const focused = a === document.activeElement;
  for (const { name, value } of b.attributes) {
    if (keep.includes(name) || (focused && name === "value") || (name === "open" && OPEN_STATE.has(a.nodeName))) continue;
    if (a.getAttribute(name) !== value) a.setAttribute(name, value);
  }
  for (const { name } of [...a.attributes]) {
    if (b.hasAttribute(name) || keep.includes(name) || (name === "open" && OPEN_STATE.has(a.nodeName))) continue;
    a.removeAttribute(name);
  }
}

/** @param {Element} a @param {Element} b @param {Map<string, Element>} ids */
function morphElement(a, b, ids) {
  if (a === composing || a.hasAttribute("data-ui-keep")) return;
  morphAttrs(a, b);
  if (a.nodeName === "TEXTAREA" && a === document.activeElement) return;
  morphChildren(a, b, ids);
}

/** @param {Node} a @param {Node} b @param {Map<string, Element>} ids */
function morphNode(a, b, ids) {
  if (a.nodeType === 1) morphElement(/** @type {Element} */ (a), /** @type {Element} */ (b), ids);
  else if (a.nodeValue !== b.nodeValue) a.nodeValue = b.nodeValue;
}

/** @param {Node} node @param {Map<string, Element>} ids */
function removeOld(node, ids) {
  if (node.nodeType === 1 && /** @type {Element} */ (node).hasAttribute("data-ui-persist")) return;
  const key = keyOf(node);
  if (key) ids.delete(key);
  node.parentNode?.removeChild(node);
}

/** @param {Element} a @param {Element} b @param {Map<string, Element>} ids */
function morphChildren(a, b, ids) {
  /** Keys still to come among the new children: an old child with one waits for it. */
  const wanted = new Set();
  for (const child of b.childNodes) {
    const key = keyOf(child);
    if (key) wanted.add(key);
  }
  /** @type {Node | null} */
  let cur = a.firstChild;
  for (const next of [...b.childNodes]) {
    const key = keyOf(next);
    if (key) {
      wanted.delete(key);
      const old = ids.get(key);
      if (old && old.nodeName === next.nodeName && !old.contains(a)) {
        ids.delete(key);
        if (old === cur) cur = cur.nextSibling;
        else moveTo(a, old, cur);
        morphElement(old, /** @type {Element} */ (next), ids);
        continue;
      }
      a.insertBefore(next, cur);
      continue;
    }
    /** @type {Node | null} */
    let match = cur;
    while (match && !same(match, next)) {
      const k = keyOf(match);
      if (k && wanted.has(k)) { match = null; break; }
      match = match.nextSibling;
    }
    if (match) {
      while (cur !== match) {
        const gone = /** @type {Node} */ (cur);
        cur = gone.nextSibling;
        // A keyed node wanted elsewhere stays where it is until its turn;
        // anything else that was skipped is gone.
        const k = keyOf(gone);
        if (!(k && ids.has(k) && wantedLater(k, wanted))) removeOld(gone, ids);
      }
      cur = match.nextSibling;
      morphNode(match, next, ids);
    } else {
      a.insertBefore(next, cur);
    }
  }
  while (cur) {
    const gone = cur;
    cur = gone.nextSibling;
    removeLeftover(gone, ids);
  }
}

/** @param {string} key @param {Set<string>} wanted */
const wantedLater = (key, wanted) => wanted.has(key);

/**
 * What's left after the last new child. A keyed node may still be wanted by a
 * parent not reached yet: it waits, detached, in `ids`.
 * @param {Node} node @param {Map<string, Element>} ids
 */
function removeLeftover(node, ids) {
  if (node.nodeType === 1 && /** @type {Element} */ (node).hasAttribute("data-ui-persist")) return;
  node.parentNode?.removeChild(node);
}

/**
 * Morphs `from` into `to` in place.
 * @param {Element} from the element in the page
 * @param {Element} to a detached element with the new content (it is consumed)
 */
export function morph(from, to) {
  const active = /** @type {HTMLInputElement | null} */ (document.activeElement);
  const inField = active && "selectionStart" in active;
  let start = null, end = null, direction = null;
  if (inField) {
    try { start = active.selectionStart; end = active.selectionEnd; direction = active.selectionDirection; } catch {}
  }
  /** @type {Map<string, Element>} */
  const ids = new Map();
  for (const element of from.querySelectorAll("[id],[data-key]")) {
    const key = keyOf(element);
    if (key) ids.set(key, element);
  }
  morphElement(from, to, ids);
  if (active && active !== document.body && active !== document.activeElement && active.isConnected) {
    active.focus({ preventScroll: true });
    if (inField && start !== null) {
      try { active.setSelectionRange(start, end, direction || undefined); } catch {}
    }
  }
}

/** Parses `html` into its first element. @param {string} html */
export function parse(html) {
  const template = document.createElement("template");
  template.innerHTML = html;
  return template.content.firstElementChild;
}
