// The select module: Select, Combobox, MultiSelect and the command palette.
//
// The server draws everything. Focus stays on the control (a button or an input),
// the list is followed with `aria-activedescendant`, and the value lives in a native
// <select> (`select-native`) that forms, `required`, reset and `change` already
// understand. Placement of the popup is the overlay module's (`ui:place`).
import { active, all, clean, cookie, every, fetchHere, make, on, one, sameOrigin, who } from "./dom.js";

// The words, in English. The server sends only those a page changes (`data-l`), and a
// test holds this list equal to the server's defaults.
const EN = {
  empty: "No results",
  none: "No options",
  searching: "Searching…",
  failed: "Couldn't load the options. Press Enter to try again.",
  retry: "Try again",
  lost: "You no longer have access to these options.",
  clear: "Clear",
  remove: "Remove {label}",
  focused: "{label} focused. Press Backspace to remove.",
  added: "{label} added",
  removed: "{label} removed",
  cleared: "Selection cleared",
  count: "{n} selected",
  result: "1 result available",
  results: "{n} results available",
  create: "Create “{q}”",
  creating: "Creating…",
  createFailed: "Couldn't create it.",
  limit: "You can choose at most {n}.",
  more: "Show more",
  shown: "Showing {n} of {total}. Keep typing to narrow the list.",
  min: "Type at least {n} characters to search.",
  selectAll: "Select all",
  deselectAll: "Deselect all",
  toggle: "Show options",
  recent: "Recent",
  list: "Results",
  required: "Choose an option.",
  palette: "Command palette",
};

const SHOWN = 200;
// How long typing waits for a pause before a search goes out. Choosing never waits.
const WAIT = 100;
const ROWS = 500;
const PAGE = 10;
const OPTION = "[role=option]";
const ITEM = "[role=option]:not([data-action])";
const LIVE = ":not([hidden]):not([aria-disabled=true])";

/** @param {string} s */
const norm = (s) => s.normalize("NFD").replace(/\p{M}/gu, "").toLowerCase();
/** @param {string} s @param {Record<string, any>} [v] */
const fmt = (s, v = {}) => s.replace(/\{(\w+)\}/g, (_, k) => (k in v ? v[k] : ""));
/** @param {Element} e @param {string} q */
const $ = (e, q) => /** @type {HTMLElement | null} */ (e.querySelector(q));
/** @param {Element} e @param {string} name @param {boolean} on */
const flag = (e, name, on) => e.toggleAttribute(name, on);
/** The wait after the last key, in ms: the picker's own (0 is none), or the default. @param {Element} root */
const waitOf = (root) => {
  const v = root.getAttribute("data-ui-debounce"), n = v === null || v === "" ? NaN : Number(v);
  return Number.isFinite(n) && n >= 0 ? Math.min(n, 5000) : WAIT;
};

/** @typedef {{open: boolean, active: HTMLElement | null, seq: number, ctrl: AbortController | null, cache: Map<string, string>, buffer: string, q: string, n: number, timer: number, busy: number, more: string, failed: boolean, creating: boolean, said: number, hold: number, words: Record<string, string> | null, index: {el: HTMLElement, t: string | null}[] | null, count: number}} State */
/** @type {WeakMap<Element, State>} */
const states = new WeakMap();

/** @param {Element} root @returns {State} */
function stateOf(root) {
  let s = states.get(root);
  if (!s) {
    s = { open: false, active: null, seq: 0, ctrl: null, cache: new Map(), buffer: "", q: "", n: 0, timer: 0, busy: 0, more: "", failed: false, creating: false, said: 0, hold: 0, words: null, index: null, count: 0 };
    states.set(root, s);
  }
  return s;
}

/** @param {Element} root */
function words(root) {
  const s = stateOf(root);
  if (!s.words) {
    try {
      s.words = { ...EN, ...JSON.parse(root.getAttribute("data-l") || "{}") };
    } catch {
      s.words = EN;
    }
  }
  return /** @type {Record<string, string>} */ (s.words);
}

/** @param {Element} root */
function parts(root) {
  const palette = root.hasAttribute("data-ui-palette");
  const control = $(root, palette ? "[data-slot=command-input]" : "[data-slot=select-trigger],[data-slot=combobox-input],[data-slot=multiselect-input]");
  return {
    palette,
    kind: root.getAttribute("data-ui-picker") || "palette",
    control: /** @type {HTMLInputElement} */ (control),
    popup: $(root, "[data-slot=select-popup]"),
    list: /** @type {HTMLElement} */ ($(root, palette ? "[data-slot=command-list]" : "[data-slot=select-list]")),
    native: /** @type {HTMLSelectElement | null} */ ($(root, "[data-slot=select-native]")),
    note: $(root, "[data-slot=select-note]"),
    status: $(root, "[data-slot=select-status]"),
    chips: $(root, "[data-slot=chips]"),
    typed: !!control && control.nodeName === "INPUT",
  };
}

/**
 * Tells assistive technology `text`, after `wait` ms; a newer message replaces one that
 * hasn't been said. A `soft` message (a count of results) gives way to one said in the
 * last 1.5 s about what the person just did.
 * @param {Element} root @param {string} text @param {number} [wait] @param {boolean} [soft]
 */
function say(root, text, wait = 0, soft = false) {
  const live = $(root, "[data-slot=select-status]");
  if (!live) return;
  const s = stateOf(root);
  if (soft && performance.now() < s.hold) return;
  if (!soft) s.hold = performance.now() + 1500;
  clearTimeout(s.said);
  s.said = setTimeout(() => {
    live.textContent = "";
    requestAnimationFrame(() => { live.textContent = text; });
  }, wait);
}

/** The words of an option as a person reads them. @param {Element} o */
const labelOf = (o) => (o.querySelector("[data-slot=option-text]") || o).textContent || "";
/** What the control shows when `o` is chosen. @param {Element} o */
const shownOf = (o) => o.getAttribute("data-selected-label") || labelOf(o);
/** @param {Element} o */
const valueOf = (o) => o.getAttribute("data-value") ?? "";

/** @param {Element} root @param {string} v */
const searched = (root) => root.hasAttribute("data-ui-search");

/** The normalised text of every option, kept until the list changes. @param {Element} root */
function index(root) {
  const { list } = parts(root), s = stateOf(root);
  const els = /** @type {HTMLElement[]} */ ([...list.querySelectorAll(ITEM)]);
  if (!s.index || s.count !== els.length || (els[0] && s.index[0]?.el !== els[0])) {
    s.index = els.map((el) => ({ el, t: null }));
    s.count = els.length;
  }
  return s.index;
}

/** Rows a person can land on. @param {Element} root */
function live(root) {
  return /** @type {HTMLElement[]} */ ([...parts(root).list.querySelectorAll(OPTION + LIVE)]);
}

/** @param {HTMLElement} list @param {HTMLElement} o */
function reveal(list, o) {
  const top = o.offsetTop, bottom = top + o.offsetHeight;
  if (top < list.scrollTop) list.scrollTop = top;
  else if (bottom > list.scrollTop + list.clientHeight) list.scrollTop = bottom - list.clientHeight;
}

/** @param {Element} root @param {HTMLElement | null} o @param {boolean} [scroll] */
function activate(root, o, scroll = true) {
  const s = stateOf(root), p = parts(root);
  if (s.active && s.active !== o) s.active.removeAttribute("data-active");
  s.active = o;
  if (!o) {
    p.control?.removeAttribute("aria-activedescendant");
    return;
  }
  o.setAttribute("data-active", "");
  if (!o.id) o.id = (p.list.id || "ui") + "-o" + ++s.n;
  p.control?.setAttribute("aria-activedescendant", o.id);
  if (scroll) reveal(p.list, o);
}

/** Moves the active row by `by`, or to an end. @param {Element} root @param {number} by @param {"first" | "last"} [end] */
function move(root, by, end) {
  const rows = live(root), s = stateOf(root);
  if (!rows.length) return;
  const at = s.active ? rows.indexOf(s.active) : -1;
  let to = end === "first" ? 0 : end === "last" ? rows.length - 1 : at + by;
  if (!end) {
    const loop = root.hasAttribute("data-ui-palette");
    if (to < 0) to = loop ? rows.length - 1 : 0;
    else if (to >= rows.length) to = loop ? 0 : rows.length - 1;
  }
  activate(root, rows[to]);
}

// ---------------------------------------------------------------- the value

/** The values chosen. @param {Element} root */
function chosen(root) {
  const n = parts(root).native;
  return n ? [...n.selectedOptions].map((o) => o.value).filter((v) => v !== "") : [];
}

/** Draws what is chosen into the control and the list, from the native select. @param {Element} root */
function sync(root) {
  const p = parts(root), n = p.native;
  if (!n || !p.control) return;
  const values = chosen(root);
  const set = new Set(values);
  const rows = p.list.querySelectorAll(ITEM);
  const multi = p.kind === "multi";
  for (const o of rows) {
    const on = set.has(valueOf(o));
    if (on) o.setAttribute("aria-selected", "true");
    else if (multi) o.setAttribute("aria-selected", "false");
    else o.removeAttribute("aria-selected");
  }
  if (p.kind === "select") {
    const o = [...rows].find((r) => valueOf(r) === values[0]);
    const label = values.length ? (o ? shownOf(o) : n.selectedOptions[0]?.text || values[0]) : n.querySelector('option[value=""]')?.textContent || "";
    const out = $(root, "[data-slot=select-value]");
    if (out && out.textContent !== label) out.textContent = label;
    flag(p.control, "data-placeholder", !values.length);
    // The icon of the chosen option leads; the chevron is last.
    const lead = p.control.firstElementChild;
    if (lead?.matches("[data-slot=icon]") && lead.nextElementSibling) lead.remove();
    const icon = o?.querySelector(":scope > [data-slot=icon]");
    if (icon) p.control.prepend(icon.cloneNode(true));
  } else if (p.kind === "combobox") {
    const text = values.length ? n.selectedOptions[0]?.text || values[0] : "";
    if (active() !== p.control || !stateOf(root).open) p.control.value = text;
  } else if (p.kind === "multi") chips(root, values);
  const clear = $(root, "[data-slot$=-clear]");
  if (clear) clear.hidden = !values.length;
  if (root.hasAttribute("data-ui-max") || root.hasAttribute("data-limit")) limit(root, atMax(root));
}

/** Whether a multi-select has chosen all it may. @param {Element} root */
function atMax(root) {
  const max = Number(root.getAttribute("data-ui-max")) || 0;
  return !!max && chosen(root).length >= max;
}

/** Says the limit is reached. @param {Element} root */
function full(root) {
  say(root, fmt(words(root).limit, { n: root.getAttribute("data-ui-max") }));
}

/**
 * Switches the options off at the limit, and on again when it lets go. It runs for
 * every list that comes in, so a new search's rows are off too. A row the server drew
 * disabled is left as drawn: only what this switched off is switched on.
 * @param {Element} root @param {boolean} on
 */
function limit(root, on) {
  flag(root, "data-limit", on);
  const list = parts(root).list;
  if (on) {
    for (const o of list.querySelectorAll(OPTION + '[aria-selected="false"]:not([aria-disabled="true"])')) {
      o.setAttribute("data-limit-off", "");
      o.setAttribute("aria-disabled", "true");
    }
  } else {
    for (const o of list.querySelectorAll("[data-limit-off]")) {
      o.removeAttribute("data-limit-off");
      o.removeAttribute("aria-disabled");
    }
  }
}

/** Makes the chips match `values`. @param {Element} root @param {string[]} values */
function chips(root, values) {
  const { chips: box, native } = parts(root);
  if (!box || !native) return;
  const have = new Map();
  for (const c of box.querySelectorAll("[data-slot=chip]")) have.set(c.getAttribute("data-value"), c);
  const tpl = /** @type {HTMLTemplateElement | null} */ (root.querySelector("template"));
  for (const [v, c] of have) if (!values.includes(v)) c.remove();
  for (const v of values) {
    if (have.has(v) || !tpl) continue;
    const c = /** @type {HTMLElement} */ (tpl.content.firstElementChild?.cloneNode(true));
    const label = [...native.options].find((o) => o.value === v)?.text || v;
    c.setAttribute("data-value", v);
    const t = c.querySelector("[data-slot=chip-text]");
    if (t) t.textContent = label;
    c.querySelector("[data-slot=chip-remove]")?.setAttribute("aria-label", fmt(words(root).remove, { label }));
    box.append(c);
  }
  // Chips follow the order chosen; one that has focus isn't moved if it needn't be.
  const now = [...box.querySelectorAll("[data-slot=chip]")].map((c) => c.getAttribute("data-value"));
  if (now.join("\u{1f}") !== values.join("\u{1f}")) for (const v of values) { const c = box.querySelector(`[data-slot=chip][data-value="${CSS.escape(v)}"]`); if (c) box.append(c); }
  const after = Number(root.getAttribute("data-ui-summary")) || 0;
  box.querySelector("[data-slot=chip-summary]")?.remove();
  flag(root, "data-summary", !!after && values.length > after);
  if (after && values.length > after) {
    const sum = make("span");
    sum.setAttribute("data-slot", "chip-summary");
    sum.setAttribute("role", "listitem");
    sum.textContent = fmt(words(root).count, { n: values.length });
    box.append(sum);
  }
}

/** Puts `value` in the native select, adding the option when a search found it. @param {Element} root @param {string} value @param {string} label @param {boolean} [on] */
function store(root, value, label, on = true) {
  const n = /** @type {HTMLSelectElement} */ (parts(root).native);
  let o = [...n.options].find((x) => x.value === value);
  if (!o && on && value !== "") {
    o = new Option(label, value);
    n.append(o);
  }
  if (!o) return;
  if (n.multiple) o.selected = on;
  else n.value = on ? value : "";
}

/** Tells the page the value changed. @param {Element} root */
function changed(root) {
  const n = /** @type {HTMLSelectElement} */ (parts(root).native);
  n.dispatchEvent(new Event("input", { bubbles: true }));
  n.dispatchEvent(new Event("change", { bubbles: true }));
  root.removeAttribute("data-invalid");
  parts(root).control?.removeAttribute("aria-invalid");
  root.closest("[data-slot=field]")?.querySelector("[data-picker-error]")?.remove();
}

/** Chooses (or in a multi-select toggles) row `o`. @param {Element} root @param {HTMLElement} o */
function choose(root, o) {
  const p = parts(root), s = stateOf(root);
  if (o.getAttribute("aria-disabled") === "true") {
    if (o.hasAttribute("data-limit-off")) full(root);
    return;
  }
  const value = valueOf(o), label = shownOf(o);
  if (p.kind === "multi") {
    const on = !chosen(root).includes(value);
    store(root, value, label, on);
    sync(root);
    changed(root);
    say(root, fmt(words(root)[on ? "added" : "removed"], { label }));
    p.control.value = "";
    s.q = "";
    // A search typed ahead of this pick is of no use now, and its answer must not
    // replace the list the pick was made from.
    halt(root);
    if (s.open) view(root);
    return;
  }
  store(root, value, label);
  sync(root);
  changed(root);
  close(root, true);
}

/** Empties the choice. @param {Element} root */
function clear(root) {
  const p = parts(root);
  for (const v of chosen(root)) store(root, v, "", false);
  if (p.native && !p.native.multiple) p.native.value = "";
  if (p.typed) {
    p.control.value = "";
    stateOf(root).q = "";
  }
  sync(root);
  changed(root);
  say(root, words(root).cleared);
  halt(root);
  if (stateOf(root).open) view(root);
  p.control?.focus();
}

// ------------------------------------------------------------ open and close

/** @param {Element} root @param {"selected" | "first" | "last" | "none"} [where] */
function open(root, where = "selected") {
  const p = parts(root), s = stateOf(root);
  if (s.open || !p.popup || root.hasAttribute("data-disabled") || root.hasAttribute("data-readonly")) return;
  s.open = true;
  root.setAttribute("data-open", "");
  p.control.setAttribute("aria-expanded", "true");
  const label = /** @type {HTMLLabelElement | undefined} */ (p.control.labels?.[0]);
  if (label && !p.list.hasAttribute("aria-label")) {
    if (!label.id) label.id = (p.list.id || "ui") + "-label";
    p.list.setAttribute("aria-labelledby", label.id);
  }
  try { p.popup.showPopover(); } catch {}
  p.popup.dispatchEvent(new Event("ui:place"));
  // The server's answers are this list's rows while it is open: a morph of the page, which draws the list as it
  // first was, leaves them as they are.
  if (searched(root)) {
    p.list.setAttribute("data-ui-keep", "");
    queue(root, s.q, 0);
  } else view(root);
  const rows = live(root);
  if (where === "first") activate(root, rows[0] || null);
  else if (where === "last") activate(root, rows[rows.length - 1] || null);
  else if (where === "selected") activate(root, p.list.querySelector('[aria-selected="true"]' + LIVE) || (p.typed ? null : rows[0]) || null);
}

/** @param {Element} root @param {boolean} [refocus] */
function close(root, refocus) {
  const p = parts(root), s = stateOf(root);
  if (!s.open) return;
  s.open = false;
  halt(root);
  s.cache.clear();
  root.removeAttribute("data-open");
  p.control?.setAttribute("aria-expanded", "false");
  p.control?.removeAttribute("aria-activedescendant");
  p.list.removeAttribute("aria-busy");
  p.list.removeAttribute("data-ui-keep");
  try { p.popup?.hidePopover(); } catch {}
  activate(root, null);
  s.q = "";
  s.buffer = "";
  if (p.typed) p.control.value = "";
  // A combobox shows what is chosen again.
  if (p.kind === "combobox") sync(root);
  if (refocus) p.control?.focus({ preventScroll: true });
}

// ----------------------------------------------------------------- filtering

/**
 * Shows the rows that match the typed text, at most SHOWN of them. A searched list
 * is the server's answer already: it is not filtered again. `quiet`: the rows are drawn
 * again as they were (a swap), so how many there are is not said again.
 * @param {Element} root @param {boolean} [quiet]
 */
function view(root, quiet = false) {
  const p = parts(root), s = stateOf(root);
  const q = norm(s.q).trim(), tokens = q ? q.split(/\s+/) : [];
  const pal = p.palette;
  const server = !pal && searched(root);
  const rows = index(root);
  let found = 0, shown = 0;
  const cap = p.kind === "select" ? Infinity : SHOWN;
  /** @type {{el: HTMLElement, score: number}[]} */
  const scored = [];
  const recent = pal ? recents(root) : null;
  for (const row of rows) {
    const { el } = row;
    let score = 1;
    const remote = pal && el.closest("[data-slot=command-remote]");
    const mine = pal && el.closest("[data-slot=command-recent]");
    // The text to search is made the first time it is needed.
    if (tokens.length) score = mine ? 0 : server || remote ? 1 : rank((row.t ??= norm(labelOf(el)) + " " + norm(el.getAttribute("data-keywords") || "")), tokens);
    else if (pal) score = mine ? (recent?.size ? 1 : 0) : recent?.has(valueOf(el)) ? 0 : 1;
    if (score > 0) found++;
    // With no text, a chosen row is shown wherever it stands, so the list opens on it.
    const keep = !tokens.length && el.getAttribute("aria-selected") === "true";
    const off = score <= 0 || (shown >= cap && !keep);
    if (!off) shown++;
    if (el.hidden !== off) el.hidden = off;
    scored.push({ el, score });
  }
  if (pal) order(scored);
  for (const g of p.list.querySelectorAll("[role=group]")) {
    const any = g.querySelector(ITEM + ":not([hidden])");
    if (g.hidden === !!any) g.hidden = !any;
  }
  for (const sep of p.list.querySelectorAll("[data-slot=option-separator]")) /** @type {HTMLElement} */ (sep).hidden = !!tokens.length;
  // Rows that are not options: create, more, pick all, retry.
  actions(root, q, found);
  // The active row stays if it is still there; else the first match for typed text.
  const act = s.active;
  if (!act || act.hidden || !act.isConnected) {
    const first = live(root)[0] || null;
    activate(root, (p.typed || pal) && (tokens.length || pal) ? first : null, false);
  }
  const total = found;
  let text = "", say_ = "";
  if (s.failed) text = say_ = words(root).failed;
  else if (!total) text = say_ = words(root)[rows.length || server ? "empty" : "none"];
  else if (shown < total) text = fmt(words(root).shown, { n: shown.toLocaleString("en-US"), total: total.toLocaleString("en-US") });
  if (!text && total) say_ = fmt(words(root)[total === 1 ? "result" : "results"], { n: total.toLocaleString("en-US") });
  if (p.note) {
    p.note.textContent = text;
    p.note.hidden = !text;
  }
  if (say_ && !quiet) say(root, say_, 250, true);
  highlight(root, tokens);
  // The list got taller or shorter: the popup follows, on the side it opened on.
  if (s.open) p.popup?.dispatchEvent(new Event("ui:place"));
}

/** How well `text` answers every token: higher is better, 0 is no. @param {string} text @param {string[]} tokens */
function rank(text, tokens) {
  let total = 0;
  for (const t of tokens) {
    const at = text.indexOf(t);
    if (at < 0) return 0;
    total += at === 0 ? 3 : text[at - 1] === " " ? 2 : 1;
  }
  return total / tokens.length;
}

/** Orders items within each group by score, best first, ties as the page wrote them. @param {{el: HTMLElement, score: number}[]} scored */
function order(scored) {
  /** @type {Map<Element, {el: HTMLElement, score: number}[]>} */
  const by = new Map();
  for (const x of scored) {
    const g = x.el.parentElement;
    if (!g) continue;
    if (!by.has(g)) by.set(g, []);
    /** @type {any[]} */ (by.get(g)).push(x);
  }
  for (const [g, items] of by) {
    const sorted = items.filter((x) => x.score > 0).sort((a, b) => b.score - a.score);
    let anchor = g.querySelector(":scope > [data-slot=option-group-label]");
    for (const x of sorted) {
      const next = anchor ? anchor.nextElementSibling : g.firstElementChild;
      if (next !== x.el) g.insertBefore(x.el, next);
      anchor = x.el;
    }
  }
}

// The match, drawn with the Custom Highlight API, so no markup changes.
/** @param {Element} root @param {string[]} tokens */
function highlight(root, tokens) {
  const css = /** @type {any} */ (globalThis.CSS);
  if (!css?.highlights || typeof Highlight === "undefined") return;
  css.highlights.delete("ui-match");
  if (!tokens.length) return;
  const ranges = [];
  for (const o of parts(root).list.querySelectorAll(ITEM + ":not([hidden])")) {
    const node = (o.querySelector("[data-slot=option-text]") || o).firstChild;
    if (!node || node.nodeType !== 3) continue;
    const text = /** @type {string} */ (node.nodeValue), n = norm(text);
    if (n.length !== text.length) continue;
    for (const t of tokens) {
      const at = n.indexOf(t);
      if (at < 0) continue;
      const r = new Range();
      r.setStart(node, at);
      r.setEnd(node, at + t.length);
      ranges.push(r);
    }
    if (ranges.length > 600) break;
  }
  if (ranges.length) css.highlights.set("ui-match", new Highlight(...ranges));
}

/** Create, more, pick all and retry: rows that are not options. @param {Element} root @param {string} q @param {number} found */
function actions(root, q, found) {
  const p = parts(root), s = stateOf(root);
  for (const a of p.list.querySelectorAll("[data-action]")) a.remove();
  const row = (action, text, extra) => {
    const r = make("div");
    r.setAttribute("role", "option");
    r.setAttribute("data-action", action);
    r.setAttribute("data-slot", "select-action");
    r.textContent = text;
    if (extra) r.setAttribute("aria-disabled", "true");
    p.list.append(r);
    return r;
  };
  if (s.failed) row("retry", words(root).retry);
  if (s.more) row("more", words(root).more);
  if (root.hasAttribute("data-ui-pick-all") && found && !s.failed) {
    const all = [...p.list.querySelectorAll(ITEM + ":not([hidden])")];
    const done = all.every((o) => o.getAttribute("aria-selected") === "true");
    const r = row("all", words(root)[done ? "deselectAll" : "selectAll"]);
    p.list.prepend(r);
  }
  const make_ = root.getAttribute("data-ui-create");
  const text = (p.control?.value || "").trim().slice(0, 256);
  if (make_ && text && p.typed && !s.failed) {
    const n = norm(text);
    const same = [...(p.native?.options || [])].some((o) => norm(o.text) === n || norm(o.value) === n) || [...p.list.querySelectorAll(ITEM)].some((o) => norm(labelOf(o)) === n);
    if (!same) {
      const denied = root.getAttribute("data-ui-create-denied");
      const maxed = atMax(root);
      const r = row("create", s.creating ? words(root).creating : fmt(words(root).create, { q: text }), s.creating || denied !== null || maxed);
      // The limit's switch only: a row that is off for another reason isn't switched on when it lets go.
      if (maxed && denied === null && !s.creating) r.setAttribute("data-limit-off", "");
      if (denied !== null) {
        const why = make("span");
        why.setAttribute("data-slot", "option-description");
        why.textContent = denied;
        r.append(why);
      }
    }
  }
}

// -------------------------------------------------------------- the server

/**
 * Drops a search that is waiting or on its way: what was typed ahead of a choice, or
 * of closing. Its answer never lands.
 * @param {Element} root
 */
function halt(root) {
  const s = stateOf(root);
  clearTimeout(s.timer);
  s.ctrl?.abort();
  s.seq++;
  parts(root).list?.removeAttribute("aria-busy");
}

/** Searches for `q` after `wait` ms, or at once for 0. @param {Element} root @param {string} q @param {number} wait */
function queue(root, q, wait) {
  const s = stateOf(root);
  clearTimeout(s.timer);
  const min = Number(root.getAttribute("data-ui-min")) || 0;
  if (q.trim().length < min) {
    s.ctrl?.abort();
    s.seq++;
    s.more = "";
    s.failed = false;
    parts(root).list.replaceChildren();
    index(root);
    const { note } = parts(root);
    if (note) { note.textContent = fmt(words(root).min, { n: min }); note.hidden = false; }
    say(root, fmt(words(root).min, { n: min }));
    return;
  }
  const cached = s.cache.get(q);
  if (cached !== undefined) {
    s.seq++;
    s.ctrl?.abort();
    land(root, cached, q);
    return;
  }
  if (wait > 0) s.timer = setTimeout(() => find(root, q, 0), wait);
  else find(root, q, 0);
}

const TAGS = new Set(["DIV", "SPAN", "A", "IMG", "KBD", "svg", "path", "circle", "ellipse", "rect", "line", "polyline", "polygon", "g", "defs", "clipPath", "use"]);
const ATTRS = /^(role|dir|href|src|alt|loading|focusable|aria-(selected|disabled|label|hidden)|data-(value|keywords|selected-label|more|no-recent|lead|shape|state|size)|d|cx|cy|r|rx|ry|x|y|x1|x2|y1|y2|width|height|points|viewBox|transform|overflow|opacity|color|fill(-.+)?|stroke(-.+)?)$/;
const SLOTS = /^(option-(body|text|description|avatar|group-label|separator|page)|command-(item|shortcut|reason)|icon|flag)$/;
/** The ids a flag's art is drawn with, and refers to: `f1-by-by-a`, or scoped, `menu-f1-by-by-a`. */
const FLAG_ID = /^([\w-]+-)?f[14](-[a-z0-9]+)+$/i;
/** Whether attribute `name` of `e`, worth `value`, may stay. @param {Element} e @param {string} name @param {string} value */
function kept(e, name, value) {
  if (name === "data-slot") return SLOTS.test(value);
  // A flag's own ids, inside the flag only, and its clip by one of them: nothing else names an element.
  if (name === "id") return FLAG_ID.test(value) && !!e.closest("svg[data-slot=flag]");
  if (name === "clip-path") return /^url\(#[\w-]+\)$/.test(value) && FLAG_ID.test(value.slice(5, -1));
  // A transform is numbers in its functions; a paint is a color: no function (a url() reaches another
  // element or a file) and no escape.
  if (name === "transform") return /^[\w.,\s()+-]*$/.test(value);
  return ATTRS.test(name) && !(/^(fill|stroke|color)/.test(name) && /[()\\]/.test(value));
}

/**
 * A reply as options, and nothing else: only the elements and attributes a list is
 * drawn with survive, so a reply can't bring a script, a form, an id that takes over
 * the page's own, a link off this site or a picture that isn't an option's.
 * @param {string} html
 */
function tidy(html) {
  const t = /** @type {HTMLTemplateElement} */ (make("template"));
  // The first step of the sanitizer below: a template's content is inert (no script runs, no
  // picture loads), and nothing in it reaches the page but the elements and attributes that
  // survive the allowlist.
  // nosemgrep: navree-raw-html
  t.innerHTML = html;
  clean(t.content);
  for (const e of [...t.content.querySelectorAll("*")]) {
    if (!TAGS.has(e.nodeName) || (e.nodeName === "IMG" && e.getAttribute("data-slot") !== "option-avatar")) {
      e.remove();
      continue;
    }
    for (const { name, value } of [...e.attributes]) if (!kept(e, name, value)) e.removeAttribute(name);
    for (const k of ["href", "src"]) {
      const v = e.getAttribute(k);
      if (v && !sameOrigin(v) && !(k === "src" && v.startsWith("data:image/"))) e.removeAttribute(k);
    }
    // A flag's `use` draws a part of its own art, or a flag the page holds once: never a file.
    if (e.nodeName === "use" && !/^#[\w-]+$/.test(e.getAttribute("href") || "")) e.removeAttribute("href");
  }
  return t.content;
}

/** Puts a reply into the list. @param {Element} root @param {string} html @param {string} q @param {boolean} [append] */
function land(root, html, q, append) {
  const p = parts(root), s = stateOf(root);
  const page = tidy(html);
  const holder = page.querySelector("[data-slot=option-page]");
  const rows = [...(holder || page).childNodes].slice(0, ROWS);
  s.more = holder?.getAttribute("data-more") || "";
  if (s.more && !sameOrigin(s.more)) s.more = "";
  s.failed = false;
  p.list.removeAttribute("aria-busy");
  if (p.note) p.note.hidden = true;
  const target = p.palette ? $(root, "[data-slot=command-remote]") || p.list : p.list;
  if (append) { for (const a of target.querySelectorAll("[data-action]")) a.remove(); target.append(...rows); }
  else target.replaceChildren(...rows);
  // The palette's group of answers is empty as the server draws the page: a morph leaves it as it is.
  if (target !== p.list) target.toggleAttribute("data-ui-keep", !!target.firstChild);
  s.index = null;
  if (!append) activate(root, null, false);
  sync(root);
  if (p.palette) {
    s.q = q;
  }
  view(root);
  if (p.typed && !p.palette && !append && !live(root).some((o) => o === s.active)) activate(root, live(root)[0] || null, false);
}

/** Asks the server. @param {Element} root @param {string} q @param {number} after */
async function find(root, q, after, more = "") {
  const s = stateOf(root), p = parts(root);
  const src = more || root.getAttribute("data-ui-search");
  if (!src || !sameOrigin(src)) return;
  s.ctrl?.abort();
  const mine = ++s.seq;
  const ctrl = (s.ctrl = new AbortController());
  clearTimeout(s.said);
  const slow = setTimeout(() => {
    if (s.seq !== mine) return;
    p.list.setAttribute("aria-busy", "true");
    if (p.note) { p.note.textContent = words(root).searching; p.note.hidden = false; }
    say(root, words(root).searching);
  }, 150);
  try {
    const url = new URL(src, location.href);
    if (!more) url.searchParams.set("q", q);
    const res = await fetchHere(url, { headers: { "x-navree-ui": "1", accept: "text/html" }, signal: ctrl.signal });
    if (s.seq !== mine) return;
    if (res.redirected && sameOrigin(res.url)) {
      location.assign(res.url);
      return;
    }
    if (res.status === 401 || res.status === 403) {
      s.failed = false;
      p.list.replaceChildren();
      s.index = null;
      p.list.removeAttribute("aria-busy");
      if (p.note) { p.note.textContent = words(root).lost; p.note.hidden = false; }
      say(root, words(root).lost);
      return;
    }
    if (!res.ok || !(res.headers.get("content-type") || "").startsWith("text/html")) throw 0;
    const html = await res.text();
    if (s.seq !== mine) return;
    if (!more) {
      if (s.cache.size > 20) s.cache.delete(s.cache.keys().next().value);
      s.cache.set(q, html);
    }
    land(root, html, q, !!more);
  } catch (e) {
    if (s.seq !== mine || /** @type {any} */ (e)?.name === "AbortError") return;
    s.failed = true;
    p.list.removeAttribute("aria-busy");
    if (!more) { p.list.replaceChildren(); s.index = null; }
    view(root);
  } finally {
    clearTimeout(slow);
  }
}

/** Makes the typed text a new option. @param {Element} root */
async function create(root) {
  const p = parts(root), s = stateOf(root);
  const text = (p.control.value || "").trim().slice(0, 256);
  const how = root.getAttribute("data-ui-create");
  if (!text || s.creating) return;
  if (atMax(root)) return full(root);
  if (how === "free") {
    store(root, text, text);
    if (p.kind === "multi") { p.control.value = ""; s.q = ""; }
    sync(root);
    changed(root);
    say(root, fmt(words(root).added, { label: text }));
    if (p.kind === "multi") view(root);
    else close(root, true);
    return;
  }
  if (!how || !sameOrigin(how)) return;
  s.creating = true;
  view(root);
  try {
    /** @type {Record<string, string>} */
    const headers = { "x-navree-ui": "1", accept: "text/html", "content-type": "application/x-www-form-urlencoded" };
    const token = cookie("XSRF-TOKEN");
    if (token) headers["x-xsrf-token"] = token;
    const res = await fetchHere(how, { method: "POST", headers, body: new URLSearchParams({ name: text }) });
    if (res.redirected && sameOrigin(res.url)) {
      location.assign(res.url);
      return;
    }
    if (!res.ok) throw 0;
    const row = tidy(await res.text()).querySelector(ITEM);
    if (!row) throw 0;
    // The limit may have been reached while this was on its way.
    if (atMax(root)) {
      s.creating = false;
      view(root);
      return full(root);
    }
    const value = valueOf(row), label = shownOf(row);
    store(root, value, label);
    if (p.kind === "multi") { p.control.value = ""; s.q = ""; }
    s.cache.clear();
    sync(root);
    changed(root);
    say(root, fmt(words(root).added, { label }));
    s.creating = false;
    if (p.kind === "multi") { if (s.open) { searched(root) ? find(root, "", 0) : view(root); } } else close(root, true);
  } catch {
    s.creating = false;
    view(root);
    say(root, words(root).createFailed);
  } finally {
    s.creating = false;
  }
}

// ---------------------------------------------------------------- the actions

/** @param {Element} root @param {HTMLElement} o */
function act(root, o) {
  const action = o.getAttribute("data-action");
  const p = parts(root), s = stateOf(root);
  if (!action) return choose(root, o);
  if (o.getAttribute("aria-disabled") === "true") {
    if (o.hasAttribute("data-limit-off")) full(root);
    return;
  }
  if (action === "create") create(root);
  else if (action === "retry") find(root, s.q, 0);
  else if (action === "more") find(root, s.q, 0, s.more);
  else if (action === "all") {
    const rows = [...p.list.querySelectorAll(ITEM + ":not([hidden])")].filter((r) => r.getAttribute("aria-disabled") !== "true");
    const done = rows.every((r) => r.getAttribute("aria-selected") === "true");
    const max = Number(root.getAttribute("data-ui-max")) || Infinity;
    let n = chosen(root).length;
    for (const r of rows) {
      const on = r.getAttribute("aria-selected") === "true";
      if (done) { if (on) store(root, valueOf(r), "", false); }
      else if (!on && n < max) { store(root, valueOf(r), shownOf(r)); n++; }
    }
    sync(root);
    changed(root);
    view(root);
  }
}

// ------------------------------------------------------------------ keyboard

/** Typeahead for a select. @param {Element} root @param {string} key */
function type(root, key) {
  const s = stateOf(root);
  s.buffer += key.toLowerCase();
  clearTimeout(s.timer);
  s.timer = setTimeout(() => { s.buffer = ""; }, 500);
  const rows = live(root), now = s.active ? rows.indexOf(s.active) : -1;
  const again = s.buffer.length > 1 && [...s.buffer].every((c) => c === s.buffer[0]);
  const word = again ? s.buffer[0] : s.buffer;
  const order = [...rows.slice(now + (again || s.buffer.length === 1 ? 1 : 0)), ...rows.slice(0, now + 1)];
  const hit = order.find((r) => norm(labelOf(r)).startsWith(word));
  if (hit) { if (!stateOf(root).open) open(root, "none"); activate(root, hit); }
}

/** @param {KeyboardEvent} e @param {Element} root */
function selectKey(e, root) {
  const s = stateOf(root), key = e.key;
  const isOpen = s.open;
  const printable = key.length === 1 && !e.ctrlKey && !e.metaKey && !e.altKey;
  if (!isOpen) {
    if (key === "ArrowDown" || key === "Enter" || (key === " " && !s.buffer)) open(root, "selected");
    else if (key === "ArrowUp") open(root, "first");
    else if (key === "Home") open(root, "first");
    else if (key === "End") open(root, "last");
    else if ((key === "Delete" || key === "Backspace") && root.hasAttribute("data-ui-clearable")) clear(root);
    else if (printable && key !== " ") { open(root, "none"); type(root, key); }
    else return;
    e.preventDefault();
    return;
  }
  if (key === "ArrowDown") move(root, 1);
  else if (key === "ArrowUp" && e.altKey) { if (s.active) choose(root, s.active); else close(root, true); }
  else if (key === "ArrowUp") move(root, -1);
  else if (key === "Home") move(root, 0, "first");
  else if (key === "End") move(root, 0, "last");
  else if (key === "PageDown") move(root, PAGE);
  else if (key === "PageUp") move(root, -PAGE);
  else if (key === "Enter" || (key === " " && !s.buffer)) { if (s.active) act(root, s.active); }
  else if (key === "Escape") close(root, true);
  else if (key === "Tab") { if (s.active) choose(root, s.active); else close(root); return; }
  else if (printable) type(root, key);
  else return;
  e.preventDefault();
}

/** The combobox and the multi-select. @param {KeyboardEvent} e @param {Element} root */
function typedKey(e, root) {
  const s = stateOf(root), p = parts(root), key = e.key, input = p.control;
  const composing = e.isComposing || e.keyCode === 229;
  if (composing) return;
  if (key === "ArrowDown" && e.altKey) { open(root, "none"); e.preventDefault(); return; }
  if (key === "ArrowUp" && e.altKey) { close(root); e.preventDefault(); return; }
  if (key === "ArrowDown") {
    if (!s.open) open(root, "selected");
    else move(root, 1);
    if (!s.active) move(root, 1);
  } else if (key === "ArrowUp") {
    if (!s.open) open(root, "last");
    else move(root, -1);
  } else if (key === "PageDown" && s.open) move(root, PAGE);
  else if (key === "PageUp" && s.open) move(root, -PAGE);
  else if (key === "Enter") {
    if (s.open) {
      if (s.active) act(root, s.active);
      e.preventDefault();
    }
    return;
  } else if (key === "Escape") {
    if (s.open) { close(root); e.preventDefault(); e.stopPropagation(); }
    else if (root.hasAttribute("data-ui-clearable") && chosen(root).length && p.kind === "combobox") { clear(root); e.preventDefault(); e.stopPropagation(); }
    return;
  } else if (key === "Tab") {
    if (s.open) close(root);
    return;
  } else if (p.kind === "multi" && (key === "Backspace" || key === "ArrowLeft" && input.selectionStart === 0 && input.selectionEnd === 0 && (getComputedStyle(root).direction !== "rtl")) && !input.value) {
    const last = [...root.querySelectorAll("[data-slot=chip]")].pop();
    if (last) { /** @type {HTMLElement} */ (last).focus(); say(root, fmt(words(root).focused, { label: labelOf(last) })); e.preventDefault(); }
    return;
  } else if (p.kind === "multi" && key === "ArrowRight" && !input.value && getComputedStyle(root).direction === "rtl" && input.selectionStart === 0) {
    const last = [...root.querySelectorAll("[data-slot=chip]")].pop();
    if (last) { /** @type {HTMLElement} */ (last).focus(); e.preventDefault(); }
    return;
  } else return;
  e.preventDefault();
}

/** A chip has focus. @param {KeyboardEvent} e @param {Element} root @param {HTMLElement} chip */
function chipKey(e, root, chip) {
  const key = e.key, rtl = getComputedStyle(root).direction === "rtl";
  const prev = /** @type {HTMLElement | null} */ (chip.previousElementSibling?.matches("[data-slot=chip]") ? chip.previousElementSibling : null);
  const next = /** @type {HTMLElement | null} */ (chip.nextElementSibling?.matches("[data-slot=chip]") ? chip.nextElementSibling : null);
  const input = parts(root).control;
  if (key === "Backspace" || key === "Delete") {
    const label = labelOf(chip);
    store(root, chip.getAttribute("data-value") || "", "", false);
    sync(root);
    changed(root);
    say(root, fmt(words(root).removed, { label }));
    (next || prev || input).focus();
  } else if (key === (rtl ? "ArrowRight" : "ArrowLeft")) (prev || chip).focus();
  else if (key === (rtl ? "ArrowLeft" : "ArrowRight")) (next || input).focus();
  else if (key === "Escape" || key === "Enter") input.focus();
  else return;
  e.preventDefault();
}

// ---------------------------------------------------------------- the palette

/** Where a palette's recent items are kept: for the person, so the next one in the browser sees none of them. @param {Element} dlg */
const recentKey = (dlg) => "navree-ui:recent:" + who() + ":" + dlg.id;

/** @param {Element} dlg */
function recents(dlg) {
  try {
    return new Set(JSON.parse(localStorage.getItem(recentKey(dlg)) || "[]"));
  } catch {
    return new Set();
  }
}

/** Fills the Recent group from what was chosen before. @param {Element} dlg */
function fill(dlg) {
  const box = $(dlg, "[data-slot=command-recent]");
  const n = Number(dlg.getAttribute("data-ui-recent")) || 0;
  const ids = n ? [.../** @type {Set<string>} */ (recents(dlg))].slice(0, n) : [];
  if (box) {
    box.replaceChildren();
    const label = make("div");
    label.setAttribute("data-slot", "option-group-label");
    label.setAttribute("aria-hidden", "true");
    label.textContent = words(dlg).recent;
    box.setAttribute("aria-label", words(dlg).recent);
    for (const id of ids) {
      const from = [...dlg.querySelectorAll("[data-slot=command-list] " + ITEM)].find((o) => valueOf(o) === id && !o.closest("[data-slot=command-recent],[data-slot=command-remote]"));
      if (!from) continue;
      const c = /** @type {HTMLElement} */ (from.cloneNode(true));
      c.removeAttribute("data-shortcut");
      c.removeAttribute("id");
      c.removeAttribute("data-active");
      box.append(c);
    }
    if (box.children.length) box.prepend(label);
    box.hidden = !box.children.length;
  }
  const apple = /Mac|iPhone|iPad/.test(navigator.platform || "");
  if (apple) for (const k of dlg.querySelectorAll("[data-slot=command-shortcut]")) k.textContent = (k.textContent || "").replace(/Ctrl\+/g, "⌘").replace(/Alt\+/g, "⌥").replace(/Shift\+/g, "⇧").replace(/Meta\+/g, "⌘");
  stateOf(dlg).index = null;
}

/** Makes a closed palette ready to open: the Recent group filled, no text, no row on. @param {Element} dlg @param {boolean} [quiet] */
function prepare(dlg, quiet = false) {
  fill(dlg);
  stateOf(dlg).q = "";
  const input = parts(dlg).control;
  if (input) input.value = "";
  view(dlg, quiet);
  activate(dlg, null, false);
}

/**
 * A swap while the palette is open: what the server drew is filled and filtered again, and the text
 * typed and the row on stay. The Recent group is made again, so the row is found again by its value.
 * @param {Element} dlg
 */
function refresh(dlg) {
  const s = stateOf(dlg), was = s.active, input = parts(dlg).control;
  if (input) s.q = input.value;
  fill(dlg);
  view(dlg, true);
  const rows = live(dlg), row = was && (rows.find((o) => o === was) || rows.find((o) => valueOf(o) === valueOf(was)));
  // With no row on, none is: only typing or the arrows put one there.
  if (row || !was) activate(dlg, row || null, false);
}

/** Remembers an item as the latest chosen. @param {Element} dlg @param {Element} item */
function remember(dlg, item) {
  const n = Number(dlg.getAttribute("data-ui-recent")) || 0;
  if (!n || item.hasAttribute("data-no-recent") || item.closest("[data-slot=command-remote]")) return;
  const id = valueOf(item);
  const ids = [id, ...[...recents(dlg)].filter((x) => x !== id)].slice(0, n);
  try { localStorage.setItem(recentKey(dlg), JSON.stringify(ids)); } catch {}
}

/** Runs the item: closes the palette and clicks it. @param {HTMLDialogElement} dlg @param {HTMLElement} item */
function run(dlg, item) {
  if (item.getAttribute("aria-disabled") === "true") return;
  remember(dlg, item);
  dlg.close();
  if (!item.matches("[data-action]")) item.click();
}

/** Whether chord text matches the key event. @param {string} chord @param {KeyboardEvent} e */
function chordMatches(chord, e) {
  const parts_ = chord.split("+"), key = /** @type {string} */ (parts_.pop());
  const apple = /Mac|iPhone|iPad/.test(navigator.platform || "");
  const want = { ctrl: false, meta: false, alt: false, shift: false };
  for (const m of parts_) {
    if (m === "mod") want[apple ? "meta" : "ctrl"] = true;
    else if (m in want) want[/** @type {keyof typeof want} */ (m)] = true;
  }
  return e.ctrlKey === want.ctrl && e.metaKey === want.meta && e.altKey === want.alt && e.shiftKey === want.shift && e.key.toLowerCase() === key;
}

/** @param {KeyboardEvent} e @param {HTMLDialogElement} dlg */
function paletteKey(e, dlg) {
  const key = e.key;
  if (e.isComposing || e.keyCode === 229) return;
  if (key === "ArrowDown") move(dlg, 1);
  else if (key === "ArrowUp") move(dlg, -1);
  else if (key === "Enter") { const a = stateOf(dlg).active; if (a) run(dlg, a); }
  else if (key === "Tab") { return; }
  else return;
  e.preventDefault();
}

// ------------------------------------------------------------------- events

/** @param {Event} e */
const rootOf = (e) => /** @type {Element | null} */ (/** @type {Element} */ (e.target)?.closest?.("[data-ui-picker],[data-ui-palette]"));

on(document, "keydown", (e) => {
  // Opening the palette, and the shortcuts of its items.
  if (!e.defaultPrevented && !e.repeat && (e.ctrlKey || e.metaKey || e.altKey)) {
    for (const dlg of /** @type {NodeListOf<HTMLDialogElement>} */ (every("dialog[data-ui-palette]"))) {
      if (dlg.hasAttribute("data-ui-key") && e.key.toLowerCase() === "k" && (e.ctrlKey || e.metaKey) && !e.altKey && !e.shiftKey) {
        e.preventDefault();
        if (dlg.open) dlg.close();
        else if (!one("dialog[open]")) { prepare(dlg); dlg.showModal(); }
        return;
      }
      for (const item of dlg.querySelectorAll("[data-shortcut]")) {
        // What the server answered while the palette was open binds no key of the page.
        if (item.closest("[data-slot=command-remote]")) continue;
        if (chordMatches(/** @type {string} */ (item.getAttribute("data-shortcut")), e) && !one("dialog[open]")) {
          e.preventDefault();
          run(dlg, /** @type {HTMLElement} */ (item));
          return;
        }
      }
    }
  }
  const root = rootOf(e);
  if (!root) return;
  const t = /** @type {HTMLElement} */ (e.target);
  if (root.hasAttribute("data-ui-palette")) {
    if (t.matches("[data-slot=command-input]")) paletteKey(e, /** @type {HTMLDialogElement} */ (root));
    return;
  }
  const chip = t.closest("[data-slot=chip]");
  if (chip) chipKey(e, root, /** @type {HTMLElement} */ (chip));
  else if (t.matches("[data-slot=select-trigger]")) selectKey(e, root);
  else if (t.matches("[data-slot=combobox-input],[data-slot=multiselect-input]")) typedKey(e, root);
});

on(document, "input", (e) => {
  const root = rootOf(e), t = /** @type {HTMLInputElement} */ (e.target);
  if (!root || !t.matches?.("[data-slot=combobox-input],[data-slot=multiselect-input],[data-slot=command-input]")) return;
  const s = stateOf(root);
  s.q = t.value;
  if (root.hasAttribute("data-ui-palette")) {
    if (root.getAttribute("data-ui-search")) queue(root, t.value, waitOf(root));
    view(root);
    return;
  }
  // The key that opens the list is asked for by the opening, once; the next ones wait.
  const opening = !s.open;
  if (opening) open(root, "none");
  if (searched(root)) { if (!opening) queue(root, t.value, waitOf(root)); }
  else view(root);
});

on(document, "click", (e) => {
  const t = /** @type {Element} */ (e.target);
  const root = rootOf(e);
  if (!root) return;
  if (root.hasAttribute("data-ui-palette")) {
    const item = /** @type {HTMLElement | null} */ (t.closest("[data-slot=command-item]"));
    if (item) {
      if (item.getAttribute("aria-disabled") === "true") { e.preventDefault(); return; }
      remember(/** @type {HTMLDialogElement} */ (root), item);
      /** @type {HTMLDialogElement} */ (root).close();
    }
    return;
  }
  const row = /** @type {HTMLElement | null} */ (t.closest(OPTION));
  if (row && root.contains(row)) return void act(root, row);
  if (t.closest("[data-slot$=-clear]")) return void clear(root);
  const remove = t.closest("[data-slot=chip-remove]");
  if (remove) {
    const chip = /** @type {HTMLElement} */ (remove.closest("[data-slot=chip]"));
    const label = labelOf(chip);
    store(root, chip.getAttribute("data-value") || "", "", false);
    sync(root);
    changed(root);
    say(root, fmt(words(root).removed, { label }));
    parts(root).control.focus();
    return;
  }
  const p = parts(root);
  if (t.closest("[data-slot=select-trigger]")) {
    // Safari doesn't focus a button that is clicked: the keys must still reach it.
    p.control.focus({ preventScroll: true });
    if (stateOf(root).open) close(root, true);
    else open(root, "selected");
  } else if (t.closest("[data-slot=combobox-toggle]")) {
    if (stateOf(root).open) close(root, true);
    else { p.control.focus(); open(root, "none"); }
  } else if (t.closest("[data-slot=combobox-control],[data-slot=multiselect-control]") && !t.closest("[data-slot=chip]")) {
    p.control.focus();
    if (!stateOf(root).open && t.closest("[data-slot=combobox-input],[data-slot=multiselect-input],[data-slot=combobox-control],[data-slot=multiselect-control]")) open(root, "none");
  }
});

// Focus stays where it is when the popup is pressed (so the input doesn't blur).
on(document, "mousedown", (e) => {
  const t = /** @type {Element} */ (e.target);
  if (t.closest?.("[data-slot=select-popup],[data-slot=command-list]")) e.preventDefault();
  // A press outside closes any open list.
  for (const root of every("[data-ui-picker][data-open]")) if (!root.contains(t)) close(root);
});

on(document, "pointermove", (e) => {
  const o = /** @type {HTMLElement | null} */ (/** @type {Element} */ (e.target).closest?.(OPTION));
  if (!o || o.getAttribute("aria-disabled") === "true") return;
  const root = rootOf(e);
  if (root && (stateOf(root).open || root.hasAttribute("data-ui-palette")) && stateOf(root).active !== o) activate(root, o, false);
}, { passive: true });

on(document, "focusout", (e) => {
  const root = rootOf(e), to = /** @type {Element | null} */ (/** @type {FocusEvent} */ (e).relatedTarget);
  if (!root || root.hasAttribute("data-ui-palette")) return;
  if (to && root.contains(to)) return;
  if (stateOf(root).open) close(root);
  else if (parts(root).kind === "combobox") sync(root);
});

on(document, "focusin", (e) => {
  // A chip or the carrier got focus from outside (a label, a script): it goes to the control.
  const t = /** @type {Element} */ (e.target);
  if (t.matches?.("[data-slot=select-native]")) {
    const root = rootOf(e);
    if (root) parts(root).control?.focus();
  }
});

let asked = false;
// A required picker that is empty: say so where the person is looking.
on(document, "invalid", (e) => {
  const t = /** @type {Element} */ (e.target);
  if (!t.matches?.("[data-slot=select-native]")) return;
  e.preventDefault();
  const root = rootOf(e);
  if (!root) return;
  const p = parts(root);
  root.setAttribute("data-invalid", "");
  p.control.setAttribute("aria-invalid", "true");
  const field = root.closest("[data-slot=field]");
  if (field && !field.querySelector("[data-picker-error]")) {
    const msg = make("p");
    msg.setAttribute("data-slot", "field-error");
    msg.setAttribute("data-picker-error", "");
    msg.id = (p.control.id || root.id) + "-error";
    msg.textContent = /** @type {HTMLSelectElement} */ (t).validationMessage || words(root).required || "";
    field.append(msg);
    p.control.setAttribute("aria-describedby", [p.control.getAttribute("aria-describedby"), msg.id].filter(Boolean).join(" "));
  }
  // The first empty one takes the focus; the others in the same check only show it.
  if (!asked) {
    asked = true;
    p.control.focus();
    setTimeout(() => { asked = false; }, 0);
  }
}, true);

on(document, "reset", (e) => {
  const form = /** @type {HTMLFormElement} */ (e.target);
  setTimeout(() => { for (const root of all(form, "[data-ui-picker]")) sync(root); }, 0);
});

on(document, "close", (e) => {
  const dlg = /** @type {Element} */ (e.target);
  if (dlg.matches?.("dialog[data-ui-palette]")) {
    // An item picked while a search was typed ahead: nothing is asked after the palette is gone,
    // and what the server answered for the last text isn't there when it opens again.
    halt(dlg);
    stateOf(dlg).cache.clear();
    const remote = $(dlg, "[data-slot=command-remote]");
    remote?.replaceChildren();
    remote?.removeAttribute("data-ui-keep");
    prepare(dlg);
  }
}, true);

/** Starts what a page's pickers need: the values the browser restored, the palettes. */
function boot() {
  for (const root of every("[data-ui-picker]")) sync(root);
  // An open palette keeps what was typed in it and its row: a swap changes the rows, not where the person is.
  for (const dlg of /** @type {NodeListOf<HTMLDialogElement>} */ (every("dialog[data-ui-palette]"))) {
    if (dlg.open) refresh(dlg);
    else prepare(dlg, true);
  }
}
boot();
on(window, "pageshow", boot);
// A morph rewrites attributes to what the server drew: an open list is drawn again as it is.
on(document, "ui:swap", () => {
  // A row may have been relabelled where it stands: what was read of the rows is read again.
  for (const root of every("[data-ui-picker],dialog[data-ui-palette]")) stateOf(root).index = null;
  boot();
  for (const root of every("[data-ui-picker][data-open]")) {
    const p = parts(root), s = stateOf(root);
    if (!s.open) continue;
    p.control.setAttribute("aria-expanded", "true");
    view(root, true);
    if (s.active?.isConnected) activate(root, s.active, false);
  }
});
