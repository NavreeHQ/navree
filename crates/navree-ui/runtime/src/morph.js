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

/**
 * Takes an old node out. A keyed node in it may still be wanted by a parent not
 * reached yet: it waits, detached, in `ids`.
 * @param {Node} node
 */
function removeOld(node) {
  if (node.nodeType === 1 && /** @type {Element} */ (node).hasAttribute("data-ui-persist")) return;
  node.parentNode?.removeChild(node);
}

/**
 * Adds a new node. If old keyed nodes belong inside it, they come along.
 * @param {Element} a @param {ChildNode} next @param {Node | null} before @param {Map<string, Element>} ids
 * @returns {Node} what was added
 */
function insertNew(a, next, before, ids) {
  if (next.nodeType !== 1 || !ids.size || !/** @type {Element} */ (next).querySelector("[id],[data-key]")) {
    a.insertBefore(next, before);
    return next;
  }
  const shell = /** @type {Element} */ (next.cloneNode(false));
  a.insertBefore(shell, before);
  morphChildren(shell, /** @type {Element} */ (next), ids);
  return shell;
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
    /** The node that now stands for `next`. @type {Node} */
    let placed;
    if (key) {
      wanted.delete(key);
      const old = ids.get(key);
      if (old && old.nodeName === next.nodeName && !old.contains(a)) {
        ids.delete(key);
        if (old !== cur) moveTo(a, old, cur);
        morphElement(old, /** @type {Element} */ (next), ids);
        placed = old;
      } else placed = insertNew(a, next, cur, ids);
    } else {
      /** @type {Node | null} */
      let match = cur;
      while (match && !same(match, next)) {
        const k = keyOf(match);
        if (k && wanted.has(k)) {
          match = null;
          break;
        }
        match = match.nextSibling;
      }
      if (match) {
        while (cur && cur !== match) {
          const gone = cur;
          cur = gone.nextSibling;
          removeOld(gone);
        }
        morphNode(match, next, ids);
        placed = match;
      } else placed = insertNew(a, next, cur, ids);
    }
    // A morph below may have taken nodes from here, so read this afresh.
    cur = placed.nextSibling;
  }
  while (cur) {
    const gone = cur;
    cur = gone.nextSibling;
    removeOld(gone);
  }
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
