// A small document for the runtime's unit tests: enough of the DOM for the modules to load and for
// their listeners, lookups and fetches to run, with no browser and no dependency. What it doesn't
// model (layout, styles, the top layer) a test doesn't ask about. The browser suites under e2e/ui
// run the same code in real engines.
//
//   import { install } from "./fake-dom.mjs";
//   const page = install({ html: "<main id=x></main>" });
//   await import("../src/commands.js");

/* Events */

const listeners = new WeakMap();

class FakeEvent {
  /** @param {string} type @param {any} [init] */
  constructor(type, init = {}) {
    this.type = type;
    this.bubbles = !!init.bubbles;
    this.cancelable = !!init.cancelable;
    this.detail = init.detail ?? null;
    this.defaultPrevented = false;
    this.target = null;
    this.currentTarget = null;
    this.stopped = false;
    this.persisted = !!init.persisted;
    Object.assign(this, init.extra || {});
  }
  preventDefault() {
    if (this.cancelable) this.defaultPrevented = true;
  }
  stopPropagation() {
    this.stopped = true;
  }
  stopImmediatePropagation() {
    this.stopped = true;
  }
}
class FakeCustomEvent extends FakeEvent {}

class FakeEventTarget {
  /** @param {string} type @param {any} fn @param {any} [options] */
  addEventListener(type, fn, options) {
    if (!fn) return;
    let mine = listeners.get(this);
    if (!mine) listeners.set(this, (mine = []));
    const capture = typeof options === "boolean" ? options : !!options?.capture;
    mine.push({ type, fn, capture, once: !!options?.once });
  }
  /** @param {string} type @param {any} fn */
  removeEventListener(type, fn) {
    const mine = listeners.get(this);
    if (mine) listeners.set(this, mine.filter((l) => l.type !== type || l.fn !== fn));
  }
  /** @param {FakeEvent} event */
  dispatchEvent(event) {
    event.target = this;
    const path = [];
    for (let at = this.parentNode; at; at = at.parentNode) path.push(at);
    if (path.length && path[path.length - 1] === globalThis.document) path.push(globalThis);
    const run = (/** @type {any} */ target, /** @type {boolean} */ capture) => {
      event.currentTarget = target;
      for (const l of [...(listeners.get(target) || [])]) {
        if (l.type !== event.type || (capture !== null && l.capture !== capture)) continue;
        if (l.once) target.removeEventListener(l.type, l.fn);
        // A listener that throws is reported and the event goes on, as in a browser.
        try {
          typeof l.fn === "function" ? l.fn.call(target, event) : l.fn.handleEvent(event);
        } catch (error) {
          globalThis.page?.errors.push(error);
        }
      }
    };
    for (const target of [...path].reverse()) {
      if (event.stopped) break;
      run(target, true);
    }
    if (!event.stopped) run(this, null);
    if (event.bubbles) for (const target of path) {
      if (event.stopped) break;
      run(target, false);
    }
    return !event.defaultPrevented;
  }
}

/* Selectors: lists, compounds (tag, *, #id, .class, attributes, :not, :scope), descendant and child. */

/** @param {string} text */
function splitTop(text, sep) {
  const parts = [];
  let depth = 0, quote = "", start = 0;
  for (let i = 0; i < text.length; i++) {
    const c = text[i];
    if (quote) {
      if (c === quote) quote = "";
    } else if (c === '"' || c === "'") quote = c;
    else if (c === "(" || c === "[") depth++;
    else if (c === ")" || c === "]") depth--;
    else if (depth === 0 && c === sep) {
      parts.push(text.slice(start, i));
      start = i + 1;
    }
  }
  parts.push(text.slice(start));
  return parts.map((p) => p.trim()).filter(Boolean);
}

/** A complex selector as [compound, combinator][]: the last compound first. @param {string} text */
function complex(text) {
  const steps = [];
  let depth = 0, quote = "", buf = "", comb = " ";
  const flush = () => {
    if (buf.trim()) steps.push({ compound: buf.trim(), comb });
    buf = "";
  };
  for (let i = 0; i < text.length; i++) {
    const c = text[i];
    if (quote) {
      buf += c;
      if (c === quote) quote = "";
      continue;
    }
    if (c === '"' || c === "'") quote = c;
    if (c === "(" || c === "[") depth++;
    if (c === ")" || c === "]") depth--;
    if (depth === 0 && (c === " " || c === ">")) {
      if (buf.trim()) {
        flush();
        comb = " ";
      }
      if (c === ">") comb = ">";
      continue;
    }
    buf += c;
  }
  flush();
  return steps;
}

/** @param {any} el @param {string} compound @param {any} scope */
function matchCompound(el, compound, scope) {
  let rest = compound;
  const tag = /^([a-zA-Z][\w-]*|\*)/.exec(rest);
  if (tag) {
    if (tag[1] !== "*" && el.localName !== tag[1].toLowerCase()) return false;
    rest = rest.slice(tag[1].length);
  }
  while (rest) {
    let m;
    if ((m = /^#([\w-]+)/.exec(rest))) {
      if (el.getAttribute("id") !== m[1]) return false;
    } else if ((m = /^\.([\w-]+)/.exec(rest))) {
      if (!(el.getAttribute("class") || "").split(/\s+/).includes(m[1])) return false;
    } else if ((m = /^\[([\w:-]+)(?:([\^$*~|]?=)(?:"([^"]*)"|'([^']*)'|([^\]]*)))?\]/.exec(rest))) {
      const value = el.getAttribute(m[1]);
      if (value === null) return false;
      if (m[2]) {
        const want = m[3] ?? m[4] ?? m[5];
        if (m[2] === "=" && value !== want) return false;
        if (m[2] === "^=" && !value.startsWith(want)) return false;
        if (m[2] === "$=" && !value.endsWith(want)) return false;
        if (m[2] === "*=" && !value.includes(want)) return false;
        if (m[2] === "~=" && !value.split(/\s+/).includes(want)) return false;
      }
    } else if ((m = /^:not\(/.exec(rest))) {
      let depth = 1, i = 5;
      for (; i < rest.length && depth; i++) {
        if (rest[i] === "(") depth++;
        if (rest[i] === ")") depth--;
      }
      const inner = rest.slice(5, i - 1);
      if (matches(el, inner, scope)) return false;
      rest = rest.slice(i);
      continue;
    } else if ((m = /^:scope/.exec(rest))) {
      if (el !== scope) return false;
    } else if ((m = /^:[\w-]+(\([^)]*\))?/.exec(rest))) {
      // A state this document doesn't model (`:modal`, `:popover-open`) is never on.
      return false;
    } else throw new Error("selector not modelled: " + compound);
    rest = rest.slice(m[0].length);
  }
  return true;
}

/** @param {any} el @param {any[]} steps @param {number} i @param {any} scope */
function matchSteps(el, steps, i, scope) {
  if (!matchCompound(el, steps[i].compound, scope)) return false;
  if (i === 0) return true;
  const comb = steps[i].comb;
  if (comb === ">") {
    const parent = el.parentNode;
    return parent instanceof FakeElement && matchSteps(parent, steps, i - 1, scope);
  }
  for (let at = el.parentNode; at instanceof FakeElement; at = at.parentNode) if (matchSteps(at, steps, i - 1, scope)) return true;
  return false;
}

/** @param {any} el @param {string} selector @param {any} [scope] */
function matches(el, selector, scope) {
  return splitTop(selector, ",").some((one) => {
    const steps = complex(one);
    return matchSteps(el, steps, steps.length - 1, scope);
  });
}

/* Nodes */

class FakeNode extends FakeEventTarget {
  constructor() {
    super();
    /** @type {any} */
    this.parentNode = null;
    /** @type {any[]} */
    this.childNodes = [];
  }
  // What the morph reads through `Node.prototype` lives on the prototype here too.
  get parentNode() {
    return this._parent ?? null;
  }
  set parentNode(node) {
    this._parent = node;
  }
  get childNodes() {
    return this._kids ?? [];
  }
  set childNodes(nodes) {
    this._kids = nodes;
  }
  get nodeName() {
    return this._name;
  }
  set nodeName(name) {
    this._name = name;
  }
  get nodeType() {
    return this._type;
  }
  set nodeType(type) {
    this._type = type;
  }
  get parentElement() {
    return this.parentNode instanceof FakeElement ? this.parentNode : null;
  }
  get ownerDocument() {
    let at = this;
    while (at.parentNode) at = at.parentNode;
    return at instanceof FakeDocument ? at : globalThis.document;
  }
  get isConnected() {
    let at = this;
    while (at.parentNode) at = at.parentNode;
    return at === globalThis.document;
  }
  get firstChild() {
    return this.childNodes[0] || null;
  }
  get nextSibling() {
    const kids = this.parentNode?.childNodes || [];
    return kids[kids.indexOf(this) + 1] || null;
  }
  get textContent() {
    return this.childNodes.map((n) => n.textContent).join("");
  }
  set textContent(text) {
    for (const kid of this.childNodes) kid.parentNode = null;
    this.childNodes = [];
    if (text) this.append(new FakeText(String(text)));
  }
  /** @param {any} other */
  isEqualNode(other) {
    return !!other && html(this) === html(other);
  }
  /** @param {any} other */
  contains(other) {
    for (let at = other; at; at = at.parentNode) if (at === this) return true;
    return false;
  }
  /** @param {any[]} nodes */
  _take(nodes) {
    const out = [];
    for (const node of nodes) {
      if (node instanceof FakeFragment) {
        out.push(...node.childNodes);
        for (const kid of node.childNodes) kid.parentNode = null;
        node.childNodes = [];
      } else out.push(typeof node === "string" ? new FakeText(node) : node);
    }
    for (const node of out) node.parentNode?.removeChild(node);
    return out;
  }
  append(...nodes) {
    for (const node of this._take(nodes)) {
      node.parentNode = this;
      this.childNodes.push(node);
    }
  }
  prepend(...nodes) {
    const taken = this._take(nodes);
    for (const node of taken) node.parentNode = this;
    this.childNodes.unshift(...taken);
  }
  /** @param {any} node */
  appendChild(node) {
    this.append(node);
    return node;
  }
  /** @param {any} node @param {any} before */
  insertBefore(node, before) {
    const [taken] = this._take([node]);
    taken.parentNode = this;
    const at = before ? this.childNodes.indexOf(before) : -1;
    if (at < 0) this.childNodes.push(taken);
    else this.childNodes.splice(at, 0, taken);
    return node;
  }
  /** @param {any} node */
  removeChild(node) {
    this.childNodes = this.childNodes.filter((n) => n !== node);
    node.parentNode = null;
    return node;
  }
  remove() {
    this.parentNode?.removeChild(this);
  }
  /** @param {any[]} nodes */
  replaceWith(...nodes) {
    const parent = this.parentNode;
    if (!parent) return;
    const taken = this._take(nodes);
    const at = parent.childNodes.indexOf(this);
    for (const node of taken) node.parentNode = parent;
    parent.childNodes.splice(at, 1, ...taken);
    this.parentNode = null;
  }
  /** @param {any[]} nodes */
  replaceChildren(...nodes) {
    for (const kid of this.childNodes) kid.parentNode = null;
    this.childNodes = [];
    this.append(...nodes);
  }
  get children() {
    return this.childNodes.filter((n) => n instanceof FakeElement);
  }
  get childElementCount() {
    return this.children.length;
  }
  get firstElementChild() {
    return this.children[0] || null;
  }
  get lastElementChild() {
    const kids = this.children;
    return kids[kids.length - 1] || null;
  }
  get previousElementSibling() {
    const kids = this.parentNode?.children || [];
    return kids[kids.indexOf(this) - 1] || null;
  }
  get nextElementSibling() {
    const kids = this.parentNode?.children || [];
    return kids[kids.indexOf(this) + 1] || null;
  }
  /** @returns {any[]} */
  _descendants() {
    const out = [];
    const walk = (/** @type {any} */ node) => {
      for (const kid of node.childNodes) {
        if (kid instanceof FakeElement) {
          out.push(kid);
          walk(kid);
        }
      }
    };
    walk(this);
    return out;
  }
  /** @param {string} selector */
  querySelectorAll(selector) {
    return this._descendants().filter((el) => matches(el, selector, this));
  }
  /** @param {string} selector */
  querySelector(selector) {
    return FakeNode.prototype.querySelectorAll.call(this, selector)[0] || null;
  }
}

class FakeText extends FakeNode {
  /** @param {string} text */
  constructor(text) {
    super();
    this.data = text;
    this.nodeType = 3;
    this.nodeName = "#text";
  }
  get textContent() {
    return this.data;
  }
  set textContent(text) {
    this.data = text;
  }
  get nodeValue() {
    return this.data;
  }
  set nodeValue(text) {
    this.data = text;
  }
  cloneNode() {
    return new FakeText(this.data);
  }
}

class FakeFragment extends FakeNode {
  constructor() {
    super();
    this.nodeType = 11;
    this.nodeName = "#document-fragment";
  }
}

const VOID = new Set(["input", "img", "br", "hr", "meta", "link", "area", "base", "col", "embed", "source", "track", "wbr"]);

class FakeElement extends FakeNode {
  /** @param {string} tag */
  constructor(tag) {
    super();
    this.localName = tag.toLowerCase();
    this.nodeName = this.tagName = tag.toUpperCase();
    this.nodeType = 1;
    /** @type {Map<string, string>} */
    this._attrs = new Map();
    this.style = { setProperty() {} };
    this.dataset = {};
  }
  /** The attributes, as a browser lists them: read through the prototype, as the morph does. */
  get attributes() {
    return [...this._attrs].map(([name, value]) => ({ name, value }));
  }
  /** @param {string} name */
  getAttribute(name) {
    return this._attrs.has(name.toLowerCase()) ? /** @type {string} */ (this._attrs.get(name.toLowerCase())) : null;
  }
  /** @param {string} name @param {string} value */
  setAttribute(name, value) {
    this._attrs.set(name.toLowerCase(), String(value));
  }
  /** @param {string} name */
  hasAttribute(name) {
    return this._attrs.has(name.toLowerCase());
  }
  /** @param {string} name */
  removeAttribute(name) {
    this._attrs.delete(name.toLowerCase());
  }
  /** @param {string} name @param {boolean} [force] */
  toggleAttribute(name, force) {
    const on = force === undefined ? !this.hasAttribute(name) : force;
    if (on) this.setAttribute(name, this.getAttribute(name) ?? "");
    else this.removeAttribute(name);
    return on;
  }
  get id() {
    return this.getAttribute("id") || "";
  }
  set id(value) {
    this.setAttribute("id", value);
  }
  get hidden() {
    return this.hasAttribute("hidden");
  }
  set hidden(on) {
    this.toggleAttribute("hidden", !!on);
  }
  get isContentEditable() {
    return false;
  }
  /** @param {string} selector */
  matches(selector) {
    return matches(this, selector, this);
  }
  /** @param {string} selector */
  closest(selector) {
    for (let at = this; at instanceof FakeElement; at = at.parentNode) if (matches(at, selector, at)) return at;
    return null;
  }
  /** @param {boolean} [deep] */
  cloneNode(deep) {
    const copy = /** @type {any} */ (make(this.localName));
    for (const [name, value] of this._attrs) copy._attrs.set(name, value);
    if (deep) for (const kid of this.childNodes) copy.append(kid.cloneNode(true));
    return copy;
  }
  get innerHTML() {
    return this.childNodes.map(html).join("");
  }
  set innerHTML(text) {
    this.replaceChildren(...parseNodes(String(text)));
  }
  get outerHTML() {
    return html(this);
  }
  focus() {
    globalThis.document._active = this;
  }
  blur() {
    if (globalThis.document._active === this) globalThis.document._active = null;
  }
  click() {
    const event = new FakeEvent("click", { bubbles: true, cancelable: true, extra: { button: 0 } });
    const go = this.dispatchEvent(event);
    if (!go) return;
    // What a click does by default: a link is followed, a submit button sends its form.
    const link = this.closest("a[href]");
    if (link) globalThis.location.assign(new URL(link.getAttribute("href"), globalThis.location.href).href);
    else if (this instanceof FakeButton && (this.getAttribute("type") || "submit") === "submit") {
      const form = this.form;
      if (form) FakeForm.prototype.requestSubmit.call(form, this);
    }
  }
  getBoundingClientRect() {
    return { top: 0, bottom: 0, left: 0, right: 0, width: 0, height: 0 };
  }
  getAnimations() {
    return [];
  }
  scrollIntoView() {}
  setPointerCapture() {}
  showPopover() {}
  hidePopover() {}
}

class FakeHTMLElement extends FakeElement {}
class FakeAnchor extends FakeHTMLElement {
  get href() {
    return new URL(this.getAttribute("href") || "", globalThis.location.href).href;
  }
  set href(value) {
    this.setAttribute("href", value);
  }
  get origin() {
    return new URL(this.href).origin;
  }
  get pathname() {
    return new URL(this.href).pathname;
  }
  get search() {
    return new URL(this.href).search;
  }
  get hash() {
    return new URL(this.href).hash;
  }
  get target() {
    return this.getAttribute("target") || "";
  }
}
/** The form a control belongs to: its `form` attribute's, or the form around it. @param {any} el */
function formOf(el) {
  const named = el.getAttribute("form");
  if (named !== null) {
    const found = globalThis.document.getElementById(named);
    return found instanceof FakeForm ? found : null;
  }
  return el.closest("form");
}
class FakeControl extends FakeHTMLElement {
  get form() {
    return formOf(this);
  }
  get name() {
    return this.getAttribute("name") || "";
  }
  get disabled() {
    return this.hasAttribute("disabled");
  }
  set disabled(on) {
    this.toggleAttribute("disabled", !!on);
  }
}
class FakeInput extends FakeControl {
  constructor() {
    super("input");
    this._value = null;
    this.checked = false;
    this.files = [];
  }
  get value() {
    return this._value ?? this.getAttribute("value") ?? "";
  }
  set value(v) {
    this._value = String(v);
  }
  get type() {
    return (this.getAttribute("type") || "text").toLowerCase();
  }
  get defaultValue() {
    return this.getAttribute("value") ?? "";
  }
}
class FakeTextArea extends FakeControl {
  constructor() {
    super("textarea");
    this._value = null;
  }
  get value() {
    return this._value ?? this.textContent;
  }
  set value(v) {
    this._value = String(v);
  }
}
class FakeSelect extends FakeControl {
  constructor() {
    super("select");
  }
}
class FakeButton extends FakeControl {
  constructor() {
    super("button");
  }
}
class FakeDialog extends FakeHTMLElement {
  constructor() {
    super("dialog");
  }
  get open() {
    return this.hasAttribute("open");
  }
  showModal() {
    this.setAttribute("open", "");
  }
  close() {
    this.removeAttribute("open");
  }
}
class FakeForm extends FakeHTMLElement {
  constructor() {
    super("form");
  }
  get elements() {
    const id = this.getAttribute("id");
    const all = globalThis.document._descendants().filter((el) => el instanceof FakeControl && formOf(el) === this);
    // A list that can be indexed, iterated and asked for a name, as a form's own is.
    const list = /** @type {any} */ ([...all]);
    list.namedItem = (/** @type {string} */ name) => all.find((el) => el.getAttribute("name") === name || el.getAttribute("id") === name) || null;
    void id;
    return list;
  }
  /** @param {any} [submitter] */
  requestSubmit(submitter) {
    const event = new FakeEvent("submit", { bubbles: true, cancelable: true, extra: { submitter: submitter || null } });
    if (this.dispatchEvent(event)) globalThis.page.submitted.push({ form: this, submitter: submitter || null });
  }
  checkValidity() {
    return true;
  }
}
class FakeTemplate extends FakeHTMLElement {
  constructor() {
    super("template");
    this.content = new FakeFragment();
  }
}
class FakeTableRow extends FakeHTMLElement {
  /** The row's cells, as `row.cells` lists them. */
  get cells() {
    return this.children.filter((el) => el instanceof FakeTableCell);
  }
}
class FakeTableCell extends FakeHTMLElement {
  get cellIndex() {
    return this.parentNode ? this.parentNode.children.filter((el) => el instanceof FakeTableCell).indexOf(this) : -1;
  }
}

const KINDS = { a: FakeAnchor, input: FakeInput, textarea: FakeTextArea, select: FakeSelect, button: FakeButton, dialog: FakeDialog, form: FakeForm, template: FakeTemplate, tr: FakeTableRow, td: FakeTableCell, th: FakeTableCell };

/** @param {string} tag */
function make(tag) {
  const Kind = /** @type {any} */ (KINDS)[tag.toLowerCase()];
  return Kind ? (Kind === FakeAnchor || Kind === FakeTableRow || Kind === FakeTableCell ? new Kind(tag) : new Kind()) : new FakeHTMLElement(tag);
}

/* Markup, read and written: enough for test pages and answers. */

/** @param {string} text */
function decode(text) {
  return text.replace(/&(amp|lt|gt|quot|#39);/g, (_, e) => ({ amp: "&", lt: "<", gt: ">", quot: '"', "#39": "'" })[e]);
}

/** @param {string} source @returns {any[]} */
function parseNodes(source) {
  const root = new FakeFragment();
  /** @type {any[]} */
  const stack = [root];
  const top = () => stack[stack.length - 1];
  const re = /<!--[\s\S]*?-->|<!doctype[^>]*>|<\/([a-zA-Z][\w-]*)\s*>|<([a-zA-Z][\w-]*)((?:\s+[^\s=>/]+(?:\s*=\s*(?:"[^"]*"|'[^']*'|[^\s>]+))?)*)\s*\/?>|([^<]+)/gi;
  let m;
  while ((m = re.exec(source))) {
    if (m[1]) {
      const tag = m[1].toLowerCase();
      for (let i = stack.length - 1; i > 0; i--) {
        if (stack[i].localName === tag) {
          stack.length = i;
          break;
        }
      }
    } else if (m[2]) {
      const el = make(m[2]);
      const attrs = /([^\s=>/]+)(?:\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+)))?/g;
      let a;
      while ((a = attrs.exec(m[3] || ""))) el.setAttribute(a[1], decode(a[2] ?? a[3] ?? a[4] ?? ""));
      const into = top() instanceof FakeTemplate ? top().content : top();
      into.append(el);
      if (!VOID.has(el.localName) && !m[0].endsWith("/>")) stack.push(el);
    } else if (m[4] !== undefined && !m[0].startsWith("<")) {
      const into = top() instanceof FakeTemplate ? top().content : top();
      into.append(new FakeText(decode(m[4])));
    }
  }
  return [...root.childNodes];
}

/** @param {any} node @returns {string} */
function html(node) {
  if (node instanceof FakeText) return node.data.replace(/&/g, "&amp;").replace(/</g, "&lt;");
  if (!(node instanceof FakeElement)) return node.childNodes.map(html).join("");
  const attrs = [...node._attrs].map(([n, v]) => (v === "" ? ` ${n}` : ` ${n}="${v.replace(/"/g, "&quot;")}"`)).join("");
  if (VOID.has(node.localName)) return `<${node.localName}${attrs}>`;
  return `<${node.localName}${attrs}>${node.childNodes.map(html).join("")}</${node.localName}>`;
}

class FakeDocument extends FakeNode {
  constructor() {
    super();
    this.nodeType = 9;
    this.nodeName = "#document";
    this._cookie = "";
    this._title = "";
    this._active = null;
    this._hidden = false;
  }
  get documentElement() {
    return this.children[0] || null;
  }
  get head() {
    return this.documentElement?.children.find((el) => el.localName === "head") || null;
  }
  get body() {
    return this.documentElement?.children.find((el) => el.localName === "body") || null;
  }
  get activeElement() {
    return this._active?.isConnected ? this._active : this.body;
  }
  get cookie() {
    return this._cookie;
  }
  set cookie(value) {
    const [pair] = String(value).split(";");
    const name = pair.split("=")[0];
    const kept = this._cookie ? this._cookie.split("; ").filter((p) => p.split("=")[0] !== name) : [];
    if (pair.split("=")[1] !== "") kept.push(pair);
    this._cookie = kept.join("; ");
  }
  get title() {
    return this._title || this.querySelector("title")?.textContent || "";
  }
  set title(value) {
    this._title = value;
  }
  get hidden() {
    return this._hidden;
  }
  get scrollingElement() {
    return this.documentElement;
  }
  /** @param {string} id */
  getElementById(id) {
    return this._descendants().find((el) => el.getAttribute("id") === id) || null;
  }
  /** @param {string} tag */
  createElement(tag) {
    return make(tag);
  }
  /** @param {string} text */
  createTextNode(text) {
    return new FakeText(text);
  }
  /** @param {any} node @param {boolean} [deep] */
  importNode(node, deep) {
    return node.cloneNode(deep);
  }
}

/** A document of `markup`: a whole page, or what goes in a page's body. @param {string} markup */
function documentOf(markup) {
  const doc = new FakeDocument();
  const nodes = parseNodes(markup);
  let root = nodes.find((n) => n instanceof FakeElement && n.localName === "html");
  if (!root) {
    root = make("html");
    const head = make("head"), body = make("body");
    root.append(head, body);
    body.append(...nodes);
  } else {
    if (!root.children.some((el) => el.localName === "head")) root.prepend(make("head"));
    if (!root.children.some((el) => el.localName === "body")) root.append(make("body"));
  }
  doc.append(root);
  return doc;
}

/* The page */

/**
 * A fake answer, as `fetch` gives one.
 * @param {string} body @param {{status?: number, url?: string, redirected?: boolean, headers?: Record<string, string>}} [init]
 */
export function answer(body, init = {}) {
  const status = init.status ?? 200;
  return {
    ok: status >= 200 && status < 300,
    status,
    url: init.url ?? "",
    redirected: !!init.redirected,
    headers: new Headers(init.headers || { "content-type": "text/html; charset=utf-8" }),
    text: async () => body,
    json: async () => JSON.parse(body),
  };
}

/**
 * Installs a page at `url` holding `html`, and the window around it: location, history, storage,
 * a `fetch` that answers what the test says, and timers that run when the test says.
 * @param {{html?: string, url?: string, cookie?: string}} [options]
 */
export function install(options = {}) {
  const g = /** @type {any} */ (globalThis);
  const doc = documentOf(options.html ?? "");
  if (options.cookie) doc._cookie = options.cookie;
  const url = new URL(options.url || "https://app.test/");
  const page = {
    /** Where the page went for real: `location.assign` and `reload`. @type {string[]} */
    assigned: [],
    reloaded: 0,
    /** Submits nothing stopped: the browser would send them. @type {any[]} */
    submitted: [],
    /** Every fetch, in order. @type {{url: string, init: any}[]} */
    fetched: [],
    /** What the next fetches answer, in order; a function is called with the request. @type {any[]} */
    answers: [],
    /** History entries pushed. @type {string[]} */
    pushed: [],
    /** The channels to other tabs the page opened. @type {any[]} */
    channels: [],
    /** What listeners threw. @type {unknown[]} */
    errors: [],
    doc,
  };
  const location = {
    get href() {
      return url.href;
    },
    get origin() {
      return url.origin;
    },
    get pathname() {
      return url.pathname;
    },
    get search() {
      return url.search;
    },
    get hash() {
      return url.hash;
    },
    get host() {
      return url.host;
    },
    get protocol() {
      return url.protocol;
    },
    /** @param {string} to */
    assign(to) {
      page.assigned.push(new URL(to, url).href);
    },
    reload() {
      page.reloaded++;
    },
    toString() {
      return url.href;
    },
  };
  const history = {
    state: null,
    /** @param {any} _ @param {string} __ @param {string} to */
    pushState(_, __, to) {
      const next = new URL(to, url);
      url.href = next.href;
      page.pushed.push(next.href);
    },
    /** @param {any} _ @param {string} __ @param {string} to */
    replaceState(_, __, to) {
      url.href = new URL(to, url).href;
    },
  };
  /** @returns {Storage} */
  const storage = () => {
    const map = new Map();
    return /** @type {any} */ ({
      getItem: (/** @type {string} */ k) => (map.has(k) ? map.get(k) : null),
      setItem: (/** @type {string} */ k, /** @type {string} */ v) => map.set(k, String(v)),
      removeItem: (/** @type {string} */ k) => map.delete(k),
      clear: () => map.clear(),
      key: (/** @type {number} */ i) => [...map.keys()][i] ?? null,
      get length() {
        return map.size;
      },
    });
  };
  const globals = {
    page,
    document: doc,
    window: g,
    location,
    history,
    sessionStorage: storage(),
    localStorage: storage(),
    EventTarget: FakeEventTarget,
    Event: FakeEvent,
    CustomEvent: FakeCustomEvent,
    Node: FakeNode,
    Text: FakeText,
    Element: FakeElement,
    HTMLElement: FakeHTMLElement,
    HTMLAnchorElement: FakeAnchor,
    HTMLInputElement: FakeInput,
    HTMLTextAreaElement: FakeTextArea,
    HTMLSelectElement: FakeSelect,
    HTMLButtonElement: FakeButton,
    HTMLDialogElement: FakeDialog,
    HTMLFormElement: FakeForm,
    HTMLTemplateElement: FakeTemplate,
    HTMLTableRowElement: FakeTableRow,
    HTMLTableCellElement: FakeTableCell,
    Document: FakeDocument,
    DocumentFragment: FakeFragment,
    RadioNodeList: class {},
    // A form's fields, as a browser reads them: each named control, and the submitter's own.
    FormData: class extends FormData {
      /** @param {any} [form] @param {any} [submitter] */
      constructor(form, submitter) {
        super();
        if (!form) return;
        for (const el of form.elements) {
          if (!el.name || el.disabled || el instanceof FakeButton) continue;
          if ((el.type === "checkbox" || el.type === "radio") && !el.checked) continue;
          this.append(el.name, el.value);
        }
        if (submitter?.name) this.append(submitter.name, submitter.getAttribute("value") || "");
      }
    },
    DOMParser: class {
      /** @param {string} text */
      parseFromString(text) {
        return documentOf(text);
      }
    },
    CSS: { escape: (/** @type {string} */ s) => s.replace(/["\\]/g, "\\$&") },
    MutationObserver: class {
      observe() {}
      disconnect() {}
      takeRecords() {
        return [];
      }
    },
    IntersectionObserver: class {
      observe() {}
      disconnect() {}
    },
    ResizeObserver: undefined,
    // The channel to the page's other tabs: a test says what another tab says with `onmessage`.
    BroadcastChannel: class {
      constructor() {
        page.channels.push(this);
      }
      postMessage() {}
    },
    matchMedia: () => ({ matches: false, addEventListener() {} }),
    getComputedStyle: () => ({ direction: "ltr", position: "static" }),
    getSelection: () => ({ toString: () => "" }),
    scrollTo() {},
    requestAnimationFrame: (/** @type {any} */ fn) => setTimeout(fn, 0),
    navigator: { clipboard: { writeText: async (/** @type {string} */ text) => void page.copied.push(text) } },
    /**
     * @param {any} input @param {any} [init]
     */
    fetch: async (input, init) => {
      const href = new URL(String(input), url).href;
      page.fetched.push({ url: href, init });
      const next = page.answers.length ? page.answers.shift() : answer("");
      const made = typeof next === "function" ? await next(href, init) : next;
      if (made instanceof Error) throw made;
      return { ...made, url: made.url || href };
    },
  };
  // Some of these are getters of Node's own (`navigator`): each is set over what is there.
  for (const [name, value] of Object.entries(globals)) Object.defineProperty(g, name, { value, writable: true, configurable: true });
  page.copied = /** @type {string[]} */ ([]);
  // A poll runs for as long as the page does: here it never keeps the test's process alive.
  const interval = setInterval;
  g.setInterval = (/** @type {any} */ fn, /** @type {number} */ ms) => {
    const timer = interval(fn, ms);
    timer.unref?.();
    return timer;
  };
  // `window.addEventListener` and friends: the window is the global object here.
  for (const name of ["addEventListener", "removeEventListener", "dispatchEvent"]) {
    g[name] = (/** @type {any[]} */ ...args) => /** @type {any} */ (FakeEventTarget.prototype)[name].apply(g, args);
  }
  return page;
}

/** Fires `type` at `target`, as the browser would. @param {any} target @param {string} type @param {any} [extra] */
export function fire(target, type, extra = {}) {
  const event = new FakeEvent(type, { bubbles: true, cancelable: true, extra });
  target.dispatchEvent(event);
  return event;
}

/** Lets every promise and timer due now settle. */
export async function settle(times = 20) {
  for (let i = 0; i < times; i++) await new Promise((done) => setTimeout(done, 0));
}
