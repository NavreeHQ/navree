// Charts: the tooltip, the crosshair and the keys of a chart drawn on the server.
// Everything shown is read from the page: the values from the chart's own table, the
// places from attributes on the drawing. Nothing is fetched and nothing from the page
// is written as markup: text goes in with text nodes. It is delegated from `document`,
// so a chart that a region morphs in needs no wiring.
import { active, all, attr, closest, every, has, holds, make, on, set, textNode, unset } from "./dom.js";

const E = Element.prototype;
/** @param {Element} el @param {string} selector @returns {Element | null} */
const one = (el, selector) => E.querySelector.call(el, selector);
const BODY = "[data-ui-chart] > [data-slot=chart-body]";

/** The value each chart body is on now. @type {WeakMap<Element, number>} */
const current = new WeakMap();
/** Parsed `data-xs`, kept while the attribute stays the same. @type {WeakMap<Element, {text: string, xs: number[]}>} */
const places = new WeakMap();

/** Makes each chart one tab stop that says how to move through it. */
function enhance() {
  for (const body of every(BODY)) {
    if (!has(body, "tabindex")) set(body, "tabindex", "0");
    const keys = attr(body, "data-keys");
    const described = (attr(body, "aria-describedby") || "").split(" ");
    if (keys && !described.includes(keys)) set(body, "aria-describedby", [...described, keys].join(" ").trim());
  }
}
enhance();
on(document, "ui:swap", enhance);

/** @param {Element} body */
const count = (body) => Math.max(0, Number(attr(body, "data-rows")) || 0);
/** @param {Element} body */
const mode = (body) => attr(body, "data-mode") || "point";
/** @param {Element} body */
const rtl = (body) => !!one(body, "[data-rtl]");

/** Where each value sits across the plot, in percent. @param {Element} body */
function xs(body) {
  const text = attr(body, "data-xs") || "";
  const kept = places.get(body);
  if (kept && kept.text === text) return kept.xs;
  const parsed = text.split(" ").map(Number);
  places.set(body, { text, xs: parsed });
  return parsed;
}

/** Whether series `s` is switched off in the legend. @param {Element} figure @param {number} s */
const off = (figure, s) => {
  const toggle = /** @type {HTMLInputElement | null} */ (one(figure, `[data-slot=chart-toggle][data-s="${s}"]`));
  return !!toggle && !toggle.checked;
};

/** A cell's text as a person sees it: a gap's dash, not its words for a screen reader. @param {Element} cell */
function shown(cell) {
  const dash = one(cell, "[aria-hidden=true]");
  return (dash || cell).textContent?.trim() || "";
}

/**
 * The tooltip's lines for value `i`: the row's name, then each column's value, name
 * and colour, from the table.
 * @param {Element} figure @param {number} i
 */
function lines(figure, i) {
  const table = one(figure, "[data-slot=chart-table] table");
  if (!table) return null;
  const heads = [...all(table, "thead th")];
  const row = all(table, "tbody tr")[i];
  if (!row) return null;
  const cells = [...all(row, "th,td")];
  const out = { head: shown(cells[0]), rows: /** @type {{value: string, name: string, color: string | null, tone: string | null}[]} */ ([]) };
  const kind = attr(figure, "data-kind");
  for (let c = 1; c < cells.length; c++) {
    const th = heads[c];
    if (!th) continue;
    // A donut's and a funnel's columns are a value and its shares, not series.
    if (kind !== "donut" && kind !== "funnel" && off(figure, c - 1)) continue;
    out.rows.push({ value: shown(cells[c]), name: th.textContent?.trim() || "", color: attr(th, "data-color"), tone: attr(th, "data-tone") });
  }
  return out;
}

/** @param {Element} body @param {number} i @param {boolean} keyed */
function show(body, i, keyed) {
  const n = count(body);
  if (!n) return;
  i = Math.max(0, Math.min(n - 1, i));
  current.set(body, i);
  const figure = /** @type {Element} */ (body.parentElement);
  for (const marked of all(body, "[data-active]")) unset(marked, "data-active");
  const how = mode(body);
  const box = body.getBoundingClientRect();
  let ax = 0, ay = 0;
  if (how === "point" || how === "band") {
    const plot = one(body, "[data-slot=chart-plot]");
    const x = xs(body)[i] ?? 0;
    const p = plot ? plot.getBoundingClientRect() : box;
    ax = p.left - box.left + (p.width * x) / 100;
    ay = p.top - box.top + 8;
    const cursor = one(body, "[data-slot=chart-cursor]");
    if (cursor) {
      set(cursor, "x1", x + "%");
      set(cursor, "x2", x + "%");
      set(cursor, "visibility", "visible");
    }
    let top = Infinity;
    for (const dot of all(body, "[data-slot=chart-dot]")) {
      const y = (attr(dot, "data-ys") || "").split(" ")[i];
      const hidden = !y || y === "-" || off(figure, Number(attr(dot, "data-s")));
      set(dot, "visibility", hidden ? "hidden" : "visible");
      if (hidden) continue;
      set(dot, "cx", x + "%");
      set(dot, "cy", y);
      top = Math.min(top, Number(y));
    }
    if (top < Infinity && plot) ay = p.top - box.top + top;
    for (const bar of all(body, `[data-i="${i}"]`)) set(bar, "data-active", "");
  } else {
    let first = null;
    for (const mark of all(body, `[data-i="${i}"]`)) {
      set(mark, "data-active", "");
      first ??= mark;
    }
    if (first) {
      const r = first.getBoundingClientRect();
      ax = r.right - box.left;
      ay = r.top - box.top;
    }
  }
  const text = lines(figure, i);
  const tip = one(body, "[data-slot=chart-tip]");
  if (!text || !tip) return;
  const parts = [];
  const head = make("div");
  set(head, "data-tip", "head");
  head.append(textNode(text.head));
  parts.push(head);
  for (const row of text.rows) {
    const line = make("div");
    set(line, "data-tip", "row");
    const key = make("span");
    set(key, "data-slot", "chart-key");
    set(key, "data-key", "line");
    if (row.color) set(key, "data-color", row.color);
    if (row.tone) set(key, "data-tone", row.tone);
    const value = make("strong");
    value.append(textNode(row.value));
    const name = make("span");
    name.append(textNode(row.name));
    line.append(key, value, name);
    parts.push(line);
  }
  tip.replaceChildren(...parts);
  set(tip, "data-open", "");
  // Beside the value, on whichever side has room; never past the chart's edges. A chart
  // shorter than its tooltip (a few bars with many series) grows to hold it.
  const w = tip.offsetWidth, h = tip.offsetHeight;
  if (h > box.height) /** @type {HTMLElement} */ (body).style.minHeight = h + "px";
  let left = ax + 12;
  if (left + w > box.width) left = ax - 12 - w;
  tip.style.left = Math.max(0, Math.min(left, box.width - w)) + "px";
  tip.style.top = Math.max(0, Math.min(ay - h / 2, Math.max(box.height, h) - h)) + "px";
  if (keyed) {
    const say = one(figure, "[data-slot=chart-announce]");
    if (say) say.textContent = [text.head, ...text.rows.map((r) => `${r.name} ${r.value}`)].join(", ");
  }
}

/** @param {Element} body */
function hide(body) {
  for (const marked of all(body, "[data-active]")) unset(marked, "data-active");
  for (const el of all(body, "[data-slot=chart-cursor],[data-slot=chart-dot]")) set(el, "visibility", "hidden");
  const tip = one(body, "[data-slot=chart-tip]");
  if (tip) unset(tip, "data-open");
}

/** The value under the pointer. @param {Element} body @param {PointerEvent} e */
function under(body, e) {
  const how = mode(body);
  if (how === "point" || how === "band") {
    const plot = one(body, "[data-slot=chart-plot]");
    if (!plot) return -1;
    const r = plot.getBoundingClientRect();
    if (!r.width) return -1;
    const x = ((e.clientX - r.left) / r.width) * 100;
    let best = -1, gap = Infinity;
    xs(body).forEach((at, i) => {
      const d = Math.abs(at - x);
      if (d < gap) {
        gap = d;
        best = i;
      }
    });
    return best;
  }
  const mark = closest(e.target, "[data-i]");
  return mark && holds(body, mark) ? Number(attr(mark, "data-i")) : -1;
}

/** The last move over a chart, drawn once per frame: moves that arrive faster end where the pointer stopped. @type {PointerEvent | null} */
let latest = null;
let frame = 0;
on(document, "pointermove", (e) => {
  if (!closest(e.target, BODY)) return;
  latest = e;
  if (frame) return;
  frame = requestAnimationFrame(() => {
    frame = 0;
    const body = latest && closest(latest.target, BODY);
    if (!body) return;
    const i = under(body, /** @type {PointerEvent} */ (latest));
    if (i >= 0) show(body, i, false);
    else if (mode(body) === "rows" || mode(body) === "parts") hide(body);
  });
}, { passive: true });

on(document, "pointerout", (e) => {
  const body = closest(e.target, BODY);
  if (body && !holds(body, /** @type {Node | null} */ (e.relatedTarget)) && body !== active()) hide(body);
}, { passive: true });

on(document, "focusin", (e) => {
  const body = /** @type {Element} */ (e.target);
  if (body instanceof Element && E.matches.call(body, BODY)) show(body, current.get(body) ?? 0, true);
});

on(document, "focusout", (e) => {
  const body = /** @type {Element} */ (e.target);
  if (body instanceof Element && E.matches.call(body, BODY)) hide(body);
});

on(document, "keydown", (e) => {
  const body = /** @type {Element} */ (e.target);
  if (!(body instanceof Element) || !E.matches.call(body, BODY) || e.altKey || e.ctrlKey || e.metaKey) return;
  const n = count(body);
  const at = current.get(body) ?? 0;
  const back = rtl(body);
  const step = Math.max(1, Math.round(n / 10));
  /** @type {Record<string, number>} */
  const moves = {
    ArrowRight: back ? at - 1 : at + 1,
    ArrowLeft: back ? at + 1 : at - 1,
    ArrowDown: at + 1,
    ArrowUp: at - 1,
    Home: 0,
    End: n - 1,
    PageDown: at + step,
    PageUp: at - step,
  };
  if (e.key === "Escape") {
    hide(body);
    return;
  }
  if (!(e.key in moves)) return;
  e.preventDefault();
  show(body, moves[e.key], true);
});
