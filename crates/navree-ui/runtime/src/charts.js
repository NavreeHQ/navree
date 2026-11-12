// Charts: the tooltip, the crosshair and the keys of a chart drawn on the server.
// Everything shown is read from the page: the values from the chart's own table, the
// places from attributes on the drawing, the tooltip's lines from the skeleton the
// server wrote. Nothing is fetched and nothing from the page is written as markup: text
// goes in as text. It is delegated from `document`, so a chart that a region morphs in
// needs no wiring. A chart's own elements are a `div` and SVG, which no named element
// can shadow, so they are called directly; the document and what an event names go
// through dom.js.
import { active, closest, every, holds, on } from "./dom.js";

const BODY = "[data-ui-chart] > [data-slot=chart-body]";
/** The value each chart body is on now. @type {WeakMap<Element, number>} */
const current = new WeakMap();
/** @param {Element} el @param {string} selector */
const $ = (el, selector) => el.querySelector(selector);
/** @param {Element} el @param {string} selector */
const $$ = (el, selector) => el.querySelectorAll(selector);
/** @param {number} low @param {number} n @param {number} high */
const clamp = (low, n, high) => Math.max(low, Math.min(n, high));
/** Where each value sits across the plot, in percent: a chart of rows has none. @param {HTMLElement} body */
const places = (body) => body.dataset.xs?.split(" ").map(Number);
/** @param {HTMLElement} body */
const rows = (body) => +(body.dataset.rows ?? 0);

/** Makes each chart one tab stop that says how to move through it. */
function enhance() {
  for (const body of /** @type {Iterable<HTMLElement>} */ (every(BODY))) {
    if (body.tabIndex >= 0) continue;
    body.tabIndex = 0;
    const keys = body.dataset.keys;
    if (keys) body.setAttribute("aria-describedby", ((body.getAttribute("aria-describedby") ?? "") + " " + keys).trim());
  }
}
enhance();
on(document, "ui:swap", enhance);

/** A cell's text as a person sees it: a gap's dash, not its words for a screen reader. @param {Element} cell */
const shown = (cell) => ($(cell, "[aria-hidden=true]") || cell).textContent?.trim() ?? "";

/** @param {HTMLElement} body */
function hide(body) {
  for (const marked of $$(body, "[data-active]")) marked.removeAttribute("data-active");
  for (const el of $$(body, "[data-slot=chart-cursor],[data-slot=chart-dot]")) el.setAttribute("visibility", "hidden");
  $(body, "[data-slot=chart-tip]")?.removeAttribute("data-open");
  // The chart's own height, before any tooltip grew it.
  body.style.minHeight = "";
}

/** @param {HTMLElement} body @param {number} i @param {boolean} [keyed] */
function show(body, i, keyed) {
  const n = rows(body);
  if (!n) return;
  i = clamp(0, i, n - 1);
  current.set(body, i);
  hide(body);
  const figure = /** @type {Element} */ (body.parentElement);
  const box = body.getBoundingClientRect();
  const xs = places(body);
  const marks = $$(body, `[data-i="${i}"]`);
  for (const mark of marks) mark.setAttribute("data-active", "");
  let ax = 0, ay = 0;
  if (xs) {
    const plot = $(body, "[data-slot=chart-plot]");
    const p = (plot ?? body).getBoundingClientRect();
    const at = xs[i] ?? 0, x = at + "%";
    ax = p.left - box.left + (p.width * at) / 100;
    ay = p.top - box.top + 8;
    for (const cursor of $$(body, "[data-slot=chart-cursor]")) {
      cursor.setAttribute("x1", x);
      cursor.setAttribute("x2", x);
      cursor.setAttribute("visibility", "visible");
    }
    // A series switched off in the legend is not drawn, so it has no dot to point at.
    let top = Infinity;
    for (const dot of /** @type {Iterable<SVGElement>} */ ($$(body, "[data-slot=chart-dot]"))) {
      const y = dot.dataset.ys?.split(" ")[i];
      if (!y || y === "-" || !dot.checkVisibility()) continue;
      dot.setAttribute("visibility", "visible");
      dot.setAttribute("cx", x);
      dot.setAttribute("cy", y);
      top = Math.min(top, +y);
    }
    if (top < Infinity && plot) ay = p.top - box.top + top;
  } else if (marks[0]) {
    const r = marks[0].getBoundingClientRect();
    ax = r.right - box.left;
    ay = r.top - box.top;
  }
  // The tooltip: the table row's name, then a value for each line the server wrote.
  const table = /** @type {HTMLTableElement | null} */ ($(figure, "[data-slot=chart-table] table"));
  const cells = table?.tBodies[0]?.rows[i]?.cells;
  const tip = /** @type {HTMLElement | null} */ ($(body, "[data-slot=chart-tip]"));
  if (!cells || !tip) return;
  const [head, ...lines] = /** @type {HTMLElement[]} */ ([...tip.children]);
  head.textContent = shown(cells[0]);
  tip.setAttribute("data-open", "");
  const said = [head.textContent];
  lines.forEach((line, c) => {
    const value = $(line, "strong"), cell = cells[c + 1];
    if (!value || !cell) return;
    value.textContent = shown(cell);
    // A value coloured by its band gives the line its colour.
    const ink = $(cell, "[data-color],[data-tone]");
    if (ink) for (const name of ["data-color", "data-tone"]) {
      const v = ink.getAttribute(name);
      v ? line.setAttribute(name, v) : line.removeAttribute(name);
    }
    const unit = $(line, "em")?.textContent;
    if (line.checkVisibility()) said.push(`${line.lastElementChild?.textContent} ${value.textContent}${unit ? " " + unit : ""}`);
  });
  // Beside the value, on whichever side has room; never past the chart's edges. A chart
  // shorter than its tooltip (a few bars with many series) grows to hold it.
  const w = tip.offsetWidth, h = tip.offsetHeight;
  if (body.hasAttribute("data-float") && marks[0]) {
    // A thin chart (a meter, a strip) floats the tooltip over the page, above the mark
    // and centred on it, or below it when the window has no room above; it never grows.
    const r = marks[0].getBoundingClientRect();
    tip.style.left = clamp(0, r.left - box.left + (r.width - w) / 2, box.width - w) + "px";
    tip.style.top = (r.top < h + 12 ? r.bottom - box.top + 6 : r.top - box.top - h - 6) + "px";
  } else {
    if (h > box.height) body.style.minHeight = h + "px";
    tip.style.left = clamp(0, ax + 12 + w > box.width ? ax - 12 - w : ax + 12, box.width - w) + "px";
    tip.style.top = clamp(0, ay - h / 2, Math.max(box.height, h) - h) + "px";
  }
  const say = keyed && $(figure, "[data-slot=chart-announce]");
  if (say) say.textContent = said.join(", ");
}

/** The value under the pointer. @param {HTMLElement} body @param {PointerEvent} e */
function under(body, e) {
  const xs = places(body);
  if (!xs) {
    const mark = /** @type {HTMLElement | null} */ (closest(e.target, "[data-i]"));
    return mark && holds(body, mark) ? +(mark.dataset.i ?? -1) : -1;
  }
  const r = $(body, "[data-slot=chart-plot]")?.getBoundingClientRect();
  if (!r?.width) return -1;
  const x = ((e.clientX - r.left) / r.width) * 100;
  const gaps = xs.map((at) => Math.abs(at - x));
  return gaps.indexOf(Math.min(...gaps));
}

/** The last move over a chart, drawn once per frame: moves that arrive faster end where the pointer stopped. @type {PointerEvent | null} */
let latest = null;
let frame = 0;
/** @type {Record<string, (e: any) => void>} */
const events = {
  pointermove(e) {
    if (!closest(e.target, BODY)) return;
    latest = e;
    frame ||= requestAnimationFrame(() => {
      frame = 0;
      const body = /** @type {HTMLElement | null} */ (latest && closest(latest.target, BODY));
      if (!body) return;
      const i = under(body, /** @type {PointerEvent} */ (latest));
      if (i >= 0) show(body, i);
      else if (!places(body)) hide(body);
    });
  },
  pointerout(e) {
    const body = /** @type {HTMLElement | null} */ (closest(e.target, BODY));
    if (!body || holds(body, e.relatedTarget)) return;
    // A move that came in the same frame as the leave is not drawn after it.
    if (latest && closest(latest.target, BODY) === body) latest = null;
    if (body !== active()) hide(body);
  },
  focusin(e) {
    const body = /** @type {HTMLElement | null} */ (closest(e.target, BODY));
    if (body) show(body, current.get(body) ?? 0, true);
  },
  focusout(e) {
    const body = /** @type {HTMLElement | null} */ (closest(e.target, BODY));
    if (body) hide(body);
  },
  keydown(e) {
    const body = /** @type {HTMLElement | null} */ (closest(e.target, BODY));
    if (!body || e.altKey || e.ctrlKey || e.metaKey) return;
    if (e.key === "Escape") return hide(body);
    const n = rows(body), at = current.get(body) ?? 0, step = Math.max(1, Math.round(n / 10)), way = $(body, "[data-rtl]") ? -1 : 1;
    const to = { ArrowRight: at + way, ArrowLeft: at - way, ArrowDown: at + 1, ArrowUp: at - 1, Home: 0, End: n - 1, PageDown: at + step, PageUp: at - step }[e.key];
    if (to === undefined) return;
    e.preventDefault();
    show(body, to, true);
  },
};
for (const type in events) on(document, type, events[type], { passive: type.startsWith("pointer") });
