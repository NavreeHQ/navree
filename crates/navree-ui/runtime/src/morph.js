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

// A form's named fields shadow its own properties and methods: in
// `<form><input name="id"></form>`, `form.id` is the input. The morph reads
// every property of a form through its prototype, so no field name can change
// what the page keeps. Other elements are read directly.
const NP = Node.prototype, EP = Element.prototype;
/** @param {object} proto @param {string} name */
const getter = (proto, name) => /** @type {(this: Node) => any} */ (Object.getOwnPropertyDescriptor(proto, name)?.get);
const gId = getter(EP, "id"), gAttributes = getter(EP, "attributes");
const gKids = getter(NP, "childNodes"), gFirst = getter(NP, "firstChild"), gNext = getter(NP, "nextSibling");
const gParent = getter(NP, "parentNode"), gName = getter(NP, "nodeName"), gType = getter(NP, "nodeType");
/** @param {any} n */
const form = (n) => n instanceof HTMLFormElement;
/** @param {Node} n */
const idOf = (n) => (form(n) ? gId.call(n) : /** @type {Element} */ (n).id);
/** @param {Node} n @returns {NamedNodeMap} */
const attrsOf = (n) => (form(n) ? gAttributes.call(n) : /** @type {Element} */ (n).attributes);
/** @param {Node} n @returns {NodeListOf<ChildNode>} */
const kidsOf = (n) => (form(n) ? gKids.call(n) : n.childNodes);
/** @param {Node} n @returns {ChildNode | null} */
const firstOf = (n) => (form(n) ? gFirst.call(n) : n.firstChild);
/** @param {Node} n @returns {ChildNode | null} */
const nextOf = (n) => (form(n) ? gNext.call(n) : n.nextSibling);
/** @param {Node} n @returns {ParentNode | null} */
const parentOf = (n) => (form(n) ? gParent.call(n) : n.parentNode);
/** @param {Node} n */
const nameOf = (n) => (form(n) ? gName.call(n) : n.nodeName);
/** @param {Node} n */
const typeOf = (n) => (form(n) ? gType.call(n) : n.nodeType);
/** @param {Element} n @param {string} name */
const attr = (n, name) => EP.getAttribute.call(n, name);
/** @param {Element} n @param {string} name */
const has = (n, name) => EP.hasAttribute.call(n, name);
/** @param {Element} n @param {string} name @param {string} value */
const set = (n, name, value) => EP.setAttribute.call(n, name, value);
/** @param {Element} n @param {string} name */
const unset = (n, name) => EP.removeAttribute.call(n, name);
/** @param {Node} n @param {Node} child @param {Node | null} before */
const insert = (n, child, before) => NP.insertBefore.call(n, child, before);
/** @param {Node} n @param {Node} other */
const holds = (n, other) => NP.contains.call(n, other);

/** @param {Node} node */
function keyOf(node) {
  return typeOf(node) === 1 ? idOf(node) || attr(/** @type {Element} */ (node), "data-key") : null;
}

/** @param {Node} parent @param {Node} node @param {Node | null} before */
function moveTo(parent, node, before) {
  if (nextOf(node) === before && parentOf(node) === parent) return;
  if (/** @type {any} */ (parent).moveBefore && parent.isConnected && node.isConnected) {
    try { /** @type {any} */ (parent).moveBefore(node, before); return; } catch {}
  }
  insert(parent, node, before);
}

/** Whether two nodes can become one another without replacing. @param {Node} a @param {Node} b */
function same(a, b) {
  if (typeOf(a) !== typeOf(b) || nameOf(a) !== nameOf(b)) return false;
  if (typeOf(a) !== 1) return true;
  if (nameOf(a) === "INPUT" && attr(/** @type {Element} */ (a), "type") !== attr(/** @type {Element} */ (b), "type")) return false;
  const key = keyOf(a), other = keyOf(b);
  return (!key && !other) || key === other;
}

/** @param {Element} a @param {Element} b */
function morphAttrs(a, b) {
  const keep = (attr(a, "data-ui-keep-attrs") || "").split(" ");
  const focused = a === document.activeElement;
  for (const { name, value } of attrsOf(b)) {
    if (keep.includes(name) || (focused && name === "value") || (name === "open" && OPEN_STATE.has(nameOf(a)))) continue;
    if (attr(a, name) !== value) set(a, name, value);
  }
  for (const { name } of [...attrsOf(a)]) {
    if (has(b, name) || keep.includes(name) || (name === "open" && OPEN_STATE.has(nameOf(a)))) continue;
    unset(a, name);
  }
}

/** @param {Element} a @param {Element} b @param {Map<string, Element>} ids */
function morphElement(a, b, ids) {
  if (a === composing || has(a, "data-ui-keep")) return;
  morphAttrs(a, b);
  if (nameOf(a) === "TEXTAREA" && a === document.activeElement) return;
  morphChildren(a, b, ids);
}

/** @param {Node} a @param {Node} b @param {Map<string, Element>} ids */
function morphNode(a, b, ids) {
  if (typeOf(a) === 1) morphElement(/** @type {Element} */ (a), /** @type {Element} */ (b), ids);
  else if (a.nodeValue !== b.nodeValue) a.nodeValue = b.nodeValue;
}

/**
 * Takes an old node out. A keyed node in it may still be wanted by a parent not
 * reached yet: it waits, detached, in `ids`.
 * @param {Node} node
 */
function removeOld(node) {
  if (typeOf(node) === 1 && has(/** @type {Element} */ (node), "data-ui-persist")) return;
  const parent = parentOf(node);
  if (parent) NP.removeChild.call(parent, node);
}

/**
 * Adds a new node. If old keyed nodes belong inside it, they come along.
 * @param {Element} a @param {ChildNode} next @param {Node | null} before @param {Map<string, Element>} ids
 * @returns {Node} what was added
 */
function insertNew(a, next, before, ids) {
  if (typeOf(next) !== 1 || !ids.size || !EP.querySelector.call(next, "[id],[data-key]")) {
    insert(a, next, before);
    return next;
  }
  const shell = /** @type {Element} */ (NP.cloneNode.call(next, false));
  insert(a, shell, before);
  morphChildren(shell, /** @type {Element} */ (next), ids);
  return shell;
}

/** @param {Element} a @param {Element} b @param {Map<string, Element>} ids */
function morphChildren(a, b, ids) {
  /** Keys still to come among the new children: an old child with one waits for it. */
  const wanted = new Set();
  for (const child of kidsOf(b)) {
    const key = keyOf(child);
    if (key) wanted.add(key);
  }
  /** @type {Node | null} */
  let cur = firstOf(a);
  for (const next of [...kidsOf(b)]) {
    const key = keyOf(next);
    /** The node that now stands for `next`. @type {Node} */
    let placed;
    if (key) {
      wanted.delete(key);
      const old = ids.get(key);
      if (old && nameOf(old) === nameOf(next) && !holds(old, a)) {
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
        match = nextOf(match);
      }
      if (match) {
        while (cur && cur !== match) {
          const gone = cur;
          cur = nextOf(gone);
          removeOld(gone);
        }
        morphNode(match, next, ids);
        placed = match;
      } else placed = insertNew(a, next, cur, ids);
    }
    // A morph below may have taken nodes from here, so read this afresh.
    cur = nextOf(placed);
  }
  while (cur) {
    const gone = cur;
    cur = nextOf(gone);
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
  for (const element of EP.querySelectorAll.call(from, "[id],[data-key]")) {
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
  document.dispatchEvent(new Event("ui:swap"));
}

/** Parses `html` into its first element. @param {string} html */
export function parse(html) {
  const template = document.createElement("template");
  template.innerHTML = html;
  return template.content.firstElementChild;
}
