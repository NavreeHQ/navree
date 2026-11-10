// Dates: calendars, range pickers and the calendar of a date picker. Everything is
// delegated from `document`, so a morph or a swap needs no re-wiring. The box a person
// types a date in is a module of its own (`date-text.js`), and so is the time picker.
//
// A day is a number (see `dates-shared.js`). The server owns the rules and checks every
// value that comes back; everything below only helps a person get to a value.
import { active as activeElement, all, attr, every, has, make, on, set, unset } from "./dom.js";
import { FIRST, LAST, blocked, civil, closePop, days, dim, dow, esc, fmt, fromIso, iso, locOf, mIndex, nativeOf, near, put, q, rulesOf, tell, textOf, unlimit, ym } from "./dates-shared.js";

// ---- months ----

const MIN_M = mIndex(1, 1), MAX_M = mIndex(9999, 12);
/** @param {number} index @returns {number[]} */
const ofIndex = (index) => [Math.floor(index / 12), (index % 12) + 1];

// ---- the locale's words ----

/** @param {string} pattern @param {Record<string, string | number>} v */
const expand = (pattern, v) => pattern.replace(/\{(.)\}/g, (s, k) => (k in v ? String(v[k]) : s));
/** A date read out in full. @param {any} L @param {number} z */
function longDate(L, z) {
  const [y, m, d] = civil(z);
  return expand(L.lp, { w: L.w[dow(z)], M: (L.md || L.m)[m - 1], d, m, y });
}
/** @param {any} L @param {number} z @param {boolean} withYear */
function short(L, z, withYear) {
  const [y, m, d] = civil(z);
  return expand(withYear ? L.yp : L.sp, { M: L.ms[m - 1], d, m, y });
}
/** A range as a chip reads it. @param {any} L @param {number} a @param {number} b @param {number} today */
function fmtRange(L, a, b, today) {
  const [ay, am, ad] = civil(a), [by, bm, bd] = civil(b);
  const withYear = ay !== by || ay !== civil(today)[0];
  if (a === b) return short(L, a, withYear);
  if (ay === by && am === bm) {
    const month = L.ms[am - 1];
    if (L.sp === "{M} {d}" && L.yp === "{M} {d}, {y}") return withYear ? `${month} ${ad} – ${bd}, ${ay}` : `${month} ${ad} – ${bd}`;
    if (L.sp === "{d} {M}" && L.yp === "{d} {M} {y}") return withYear ? `${ad} – ${bd} ${month} ${ay}` : `${ad} – ${bd} ${month}`;
  }
  return `${short(L, a, withYear)} – ${short(L, b, withYear)}`;
}

// ---- where a person may go ----

/** The month a day is in, as a number. @param {number} z */
const monthIdx = (z) => { const [y, m] = civil(z); return mIndex(y, m); };
/** The first and last day of a month number. @param {number} idx */
const startOf = (idx) => { const [y, m] = ofIndex(idx); return days(y, m, 1); };
/** @param {number} idx */
const endOf = (idx) => { const [y, m] = ofIndex(idx); return days(y, m, dim(y, m)); };
/** The first and last month a person may go to. @param {any} R */
function navLimits(R) {
  const lo = Math.max(MIN_M, R.nav?.[0] ?? MIN_M, R.min === null ? MIN_M : monthIdx(R.min));
  const hi = Math.min(MAX_M, R.nav?.[1] ?? MAX_M, R.max === null ? MAX_M : monthIdx(R.max));
  return [lo, hi];
}

// ---- a calendar ----

/** The picker or range picker a calendar sits in, if any. @param {Element} root */
const host = (root) => near(root, "[data-ui-dates=picker],[data-ui-dates=range]");
const NARROW = matchMedia("(max-width: 639px)");

/** @param {Element} root */
function cfg(root) {
  const L = locOf(root);
  if (!L) return null;
  const h = host(root);
  const responsive = h && has(root, "data-embedded") && NARROW.matches;
  return {
    root, L, R: rulesOf(root), id: root.id, mode: attr(root, "data-mode") || "single",
    ws: +(attr(root, "data-week-start") || 0), months: responsive ? 1 : +(attr(root, "data-months") || 1),
    today: fromIso(attr(root, "data-today")) ?? days(1970, 1, 1),
    wn: has(root, "data-week-numbers"), fixed: has(root, "data-fixed-weeks"), noOutside: has(root, "data-no-outside"),
    dd: attr(root, "data-caption") === "dropdowns", ro: has(root, "data-readonly"), off: has(root, "data-disabled"),
    req: has(root, "data-required"), minDays: +(attr(root, "data-min-days") || 0), maxDays: +(attr(root, "data-max-days") || 0),
    maxCount: +(attr(root, "data-max-count") || 0), rtl: !!L.rtl, host: h,
  };
}

/**
 * @typedef {{first: number, focus: number | null, hover: number | null, work: any, stash: any}} State
 * first: the first month on show, as a month number. work: what is chosen; a range
 * with no `to` is half picked.
 */
/** @type {WeakMap<Element, State>} */
const states = new WeakMap();

/** The fields that hold the value. @param {Element} root @returns {Record<string, HTMLInputElement>} */
function fields(root) {
  const h = host(root);
  /** @type {Record<string, HTMLInputElement>} */
  const out = {};
  for (const el of /** @type {any} */ (all(h || root, h ? "[data-ui-native]" : "[data-ui-val]"))) out[attr(el, h ? "data-ui-native" : "data-ui-val") || "main"] = el;
  return out;
}
/** What the fields hold. @param {any} c */
function readWork(c) {
  const f = fields(c.root);
  if (c.mode === "range") return { from: fromIso(f.from?.value), to: fromIso(f.to?.value) };
  const set = new Set((f.main?.value || "").split(",").map(fromIso).filter((/** @type {any} */ z) => z !== null));
  return { set: c.mode === "single" ? new Set([...set].slice(0, 1)) : set };
}
/** @param {Element} root @param {any} c */
function stateOf(root, c) {
  let st = states.get(root);
  if (!st) {
    const [y, m] = (attr(root, "data-month") || "").split("-").map(Number);
    const f = q(root, "[data-slot=calendar-day][tabindex='0']");
    st = { first: y ? mIndex(y, m) : monthIdx(c.today), focus: f ? fromIso(attr(f, "data-date")) : null, hover: null, work: null, stash: null };
    states.set(root, st);
  }
  st.work ||= readWork(c);
  return st;
}
/** Whether a day is chosen, and how it sits in a range. @param {any} c @param {State} st @param {number} z */
function marks(c, st, z) {
  const w = st.work;
  if (c.mode !== "range") return { sel: w.set.has(z) };
  const pending = w.from !== null && w.to === null;
  let a = w.from, b = w.to;
  const preview = pending && st.hover !== null;
  if (preview) [a, b] = [Math.min(w.from, st.hover), Math.max(w.from, st.hover)];
  const inside = a !== null && b !== null && z >= a && z <= b;
  return { start: a === z && inside, end: b === z && inside, mid: inside && z !== a && z !== b, preview: preview && inside, pending: pending && w.from === z };
}
/** Whether a day can't be chosen now: the rules, and a half-picked range's reach. @param {any} c @param {State} st @param {number} z */
function cannot(c, st, z) {
  if (blocked(c.R, z)) return true;
  const w = st.work;
  if (c.mode !== "range" || w.from === null || w.to !== null) return false;
  const span = Math.abs(z - w.from) + 1;
  if ((c.minDays && span < c.minDays) || (c.maxDays && span > c.maxDays)) return true;
  const [lo, hi] = z < w.from ? [z, w.from] : [w.from, z];
  for (let d = lo; d <= hi; d++) if (blocked(c.R, d)) return true;
  return false;
}
/** The day that holds the tab stop. @param {any} c @param {State} st @param {number[][]} vis */
function pickFocus(c, st, vis) {
  const shown = (/** @type {number} */ z) => { const [y, m] = civil(z); return vis.some(([vy, vm]) => vy === y && vm === m); };
  const w = st.work;
  const chosen = c.mode === "range" ? [w.from, w.to].filter((/** @type {any} */ z) => z !== null) : [...w.set];
  const ok = (/** @type {number} */ z) => shown(z) && !blocked(c.R, z);
  if (st.focus !== null && shown(st.focus)) return st.focus;
  const first = chosen.find(ok) ?? (ok(c.today) ? c.today : undefined);
  if (first !== undefined) return first;
  const [y, m] = vis[0];
  for (let d = 1; d <= dim(y, m); d++) if (!blocked(c.R, days(y, m, d))) return days(y, m, d);
  return days(y, m, 1);
}
/** The months on show. @param {any} c @param {State} st */
function visible(c, st) {
  let first = Math.min(Math.max(st.first, MIN_M), MAX_M - c.months + 1);
  st.first = first;
  return Array.from({ length: c.months }, (_, i) => ofIndex(first + i));
}

/**
 * One month, as the server draws it. Whether a day is chosen or can't be is not written
 * here: `paint` marks every day after each drawing.
 * @param {any} c @param {number} y @param {number} m @param {number} focus
 */
function monthHtml(c, y, m, focus) {
  // The id is a page's own text, so it is escaped like every other word that goes into the markup.
  const L = c.L, id = esc(`${c.id}-cap-${ym(y, m)}`), editable = !c.ro && !c.off;
  let cap;
  if (c.dd) {
    const lo = Math.max(1, c.R.min === null ? y - 100 : civil(c.R.min)[0]), hi = Math.min(9999, c.R.max === null ? y + 100 : civil(c.R.max)[0]);
    const [from, to] = hi - lo > 200 ? [Math.max(lo, y - 100), Math.min(hi, y + 100)] : [lo, hi];
    const opts = (/** @type {string[]} */ list, /** @type {number} */ cur, /** @type {number} */ base) => list.map((t, i) => `<option value="${base + i}"${base + i === cur ? " selected" : ""}>${esc(t)}</option>`).join("");
    const years = Array.from({ length: to - from + 1 }, (_, i) => String(from + i));
    cap = `<div data-slot="calendar-caption" id="${id}" data-layout="dropdowns"><select data-slot="native-select" data-size="sm" data-ui-cal="month" aria-label="${esc(L.L[6])}">${opts(L.m, m, 1)}</select><select data-slot="native-select" data-size="sm" data-ui-cal="year" aria-label="${esc(L.L[7])}">${opts(years, y, from)}</select></div>`;
  } else cap = `<div data-slot="calendar-caption" id="${id}" aria-live="polite">${esc(expand(L.cp, { M: L.m[m - 1], m, y }))}</div>`;
  const head = Array.from({ length: 7 }, (_, i) => { const d = (c.ws + i) % 7; return `<th data-slot="calendar-weekday" scope="col" abbr="${esc(L.w[d])}">${esc(L.ws[d])}</th>`; }).join("");
  const first = days(y, m, 1);
  const lead = (dow(first) - c.ws + 7) % 7;
  const rows = c.fixed ? 6 : Math.ceil((lead + dim(y, m)) / 7);
  let body = "";
  for (let r = 0; r < rows; r++) {
    const row = first - lead + r * 7;
    body += "<tr>";
    if (c.wn) body += `<th data-slot="calendar-weeknum" scope="row">${isoWeek(row + ((11 - dow(row)) % 7))}</th>`;
    for (let z = row; z < row + 7; z++) {
      const [zy, zm, zd] = civil(z);
      body += zy !== y || zm !== m
        ? `<td role="gridcell">${c.noOutside ? "" : `<span data-slot="calendar-day" data-outside aria-hidden="true">${zd}</span>`}</td>`
        : `<td role="gridcell"><button data-slot="calendar-day" type="button" data-date="${iso(z)}" tabindex="${focus === z && editable ? 0 : -1}" aria-label="${esc(longDate(L, z))}"${z === c.today ? ' aria-current="date" data-today' : ""}>${zd}</button></td>`;
    }
    body += "</tr>";
  }
  return `<div data-slot="calendar-month" data-month="${ym(y, m)}"><div data-slot="calendar-head">${cap}</div><table data-slot="calendar-grid" role="grid" aria-labelledby="${id}"><thead><tr>${c.wn ? `<th data-slot="calendar-weeknum" scope="col">${esc(L.L[8])}</th>` : ""}${head}</tr></thead><tbody>${body}</tbody></table></div>`;
}
/** The ISO week of the Thursday `z`. @param {number} z */
function isoWeek(z) {
  const jan4 = days(civil(z)[0], 1, 4);
  return Math.floor((z - jan4 + ((dow(jan4) + 6) % 7)) / 7) + 1;
}

/** Draws the months. @param {Element} root */
function render(root) {
  const c = cfg(root);
  if (!c) return;
  const st = stateOf(root, c);
  const vis = visible(c, st);
  const focus = pickFocus(c, st, vis);
  st.focus = focus;
  const box = q(root, "[data-slot=calendar-months]");
  if (box) drawMonths(box, vis.map(([y, m]) => monthHtml(c, y, m, focus)));
  const [lo, hi] = navLimits(c.R);
  for (const b of /** @type {any} */ (all(root, "[data-ui-cal=prev]"))) b.disabled = st.first <= lo;
  for (const b of /** @type {any} */ (all(root, "[data-ui-cal=next]"))) b.disabled = st.first + c.months - 1 >= hi;
  paint(root);
}
/**
 * Puts the months in the box. A month already there keeps its caption, which is a live
 * region: its words change in place, so a screen reader announces the new month. Only when
 * the number of months changes are they all drawn again.
 * @param {Element} box @param {string[]} months
 */
function drawMonths(box, months) {
  const now = /** @type {Element[]} */ ([...all(box, "[data-slot=calendar-month]")]);
  const tpl = /** @type {HTMLTemplateElement} */ (make("template"));
  // Markup from monthHtml only: each value in it is a number worked out from day numbers,
  // or text (the locale's names and labels, the calendar's id) escaped by esc().
  // nosemgrep: navree-raw-html
  tpl.innerHTML = months.join("");
  const fresh = /** @type {Element[]} */ ([...DocumentFragment.prototype.querySelectorAll.call(tpl.content, "[data-slot=calendar-month]")]);
  const caption = (/** @type {Element} */ m) => q(m, "[data-slot=calendar-caption]");
  if (now.length !== fresh.length || now.some((m, i) => !caption(m) || attr(caption(m), "data-layout") !== attr(caption(fresh[i]), "data-layout"))) {
    box.replaceChildren(...fresh);
    return;
  }
  now.forEach((m, i) => {
    const f = fresh[i], old = caption(m), next = caption(f);
    set(m, "data-month", attr(f, "data-month") || "");
    set(old, "id", next.id);
    if (attr(next, "data-layout") === "dropdowns") {
      // The lists of months and years are not a live region: they say the month by what they
      // hold. They stay, and only take their new value, unless the years on offer changed.
      const lists = /** @type {HTMLSelectElement[]} */ ([...all(old, "select")]), fresh2 = /** @type {HTMLSelectElement[]} */ ([...all(next, "select")]);
      lists.forEach((list, k) => {
        const to = fresh2[k];
        if (!to) return;
        const value = to.value;
        if (list.options.length === to.options.length && [...list.options].every((o, j) => o.value === to.options[j].value)) {
          for (const o of list.options) o.toggleAttribute("selected", o.value === value);
        } else list.replaceChildren(...to.childNodes);
        // Moved options leave a list on its last one in some browsers: say which is chosen.
        list.value = value;
      });
    } else old.replaceChildren(...next.childNodes);
    q(m, "table")?.replaceWith(q(f, "table"));
  });
}
/** Brings the marks on the days up to date without redrawing them. @param {Element} root */
function paint(root) {
  const c = cfg(root);
  if (!c) return;
  const st = stateOf(root, c);
  const editable = !c.ro && !c.off, r = c.mode === "range";
  for (const b of /** @type {any} */ (all(root, "button[data-slot=calendar-day]"))) {
    const z = fromIso(attr(b, "data-date"));
    if (z === null) continue;
    const k = marks(c, st, z), off = cannot(c, st, z), stop = blocked(c.R, z);
    /** @type {[string, any][]} */
    const flags = [["data-selected", !r && k.sel], ["data-range-start", r && k.start], ["data-range-end", r && k.end], ["data-range-middle", r && k.mid], ["data-preview", r && k.preview], ["data-pending", r && k.pending], ["data-disabled", stop], ["data-unreachable", off && !stop]];
    for (const [name, on] of flags) b.toggleAttribute(name, !!on);
    if (off || !editable) set(b, "aria-disabled", "true");
    else unset(b, "aria-disabled");
    if (!c.ro) set(b.parentElement, "aria-selected", String(r ? !!(k.start || k.end || k.mid) : !!k.sel));
  }
}

/** Moves the view, keeping focus where it was. @param {Element} root @param {number} first */
function show(root, first) {
  const c = cfg(root);
  if (!c) return;
  const st = stateOf(root, c);
  const [lo, hi] = navLimits(c.R);
  const active = /** @type {Element} */ (activeElement());
  const inside = !!active && near(active, "[data-slot=calendar-months]") === q(root, "[data-slot=calendar-months]");
  const kind = active && root.contains(active) ? attr(active, "data-ui-cal") : null;
  st.first = Math.max(lo, Math.min(first, hi - c.months + 1));
  st.focus = null;
  render(root);
  if (kind === "prev" || kind === "next") {
    const b = q(root, `[data-ui-cal=${kind}]`);
    if (b && !b.disabled) b.focus();
  } else if (kind === "month" || kind === "year") q(root, `[data-ui-cal=${kind}]`)?.focus();
  else if (inside) focusDay(root, st.focus);
}
/** @param {Element} root @param {number | null} z */
function focusDay(root, z) {
  if (z === null) return;
  /** @type {HTMLElement | null} */ (q(root, `[data-date="${iso(z)}"]`))?.focus({ preventScroll: true });
}
/** Gives a day the tab stop. @param {Element} root @param {number} z */
function rove(root, z) {
  const c = cfg(root);
  if (!c) return;
  const st = stateOf(root, c);
  for (const b of /** @type {any} */ (all(root, "button[data-slot=calendar-day][tabindex='0']"))) b.tabIndex = -1;
  st.focus = z;
  const b = q(root, `[data-date="${iso(z)}"]`);
  if (b && !c.ro && !c.off) b.tabIndex = 0;
}
/** Goes to a day, drawing its month if it isn't on show. @param {Element} root @param {number} z */
function go(root, z) {
  const c = cfg(root);
  if (!c) return;
  const st = stateOf(root, c);
  const [lo, hi] = navLimits(c.R);
  z = Math.max(Math.max(startOf(lo), c.R.min ?? FIRST), Math.min(z, Math.min(endOf(hi), c.R.max ?? LAST)));
  const idx = monthIdx(z);
  if (idx < st.first || idx > st.first + c.months - 1) {
    st.first = idx < st.first ? idx : idx - c.months + 1;
    st.focus = z;
    render(root);
  } else rove(root, z);
  focusDay(root, z);
}

/** Writes what is chosen into the fields and tells the page. @param {Element} root @param {any} c @param {State} st */
function commit(root, c, st) {
  const f = fields(root), w = st.work;
  if (c.mode === "range") {
    const a = w.from !== null && w.to !== null ? iso(w.from) : "", b = a ? iso(w.to) : "";
    const changed = (f.from?.value ?? "") !== a || (f.to?.value ?? "") !== b;
    put(f.from, a);
    put(f.to, b);
    if (changed) tell(f.to || f.from);
  } else {
    const v = [...w.set].sort((a, b) => a - b).map(iso).join(",");
    const changed = (f.main?.value ?? "") !== v;
    put(f.main, v);
    if (changed) tell(f.main);
  }
  const h = host(root);
  if (h) hostChanged(h, "pick");
}
/** @param {any} c */
const confirming = (c) => !!c.host && has(c.host, "data-confirm");

/** A person chose a day. @param {Element} root @param {number} z */
function choose(root, z) {
  const c = cfg(root);
  if (!c || c.ro || c.off) return;
  const st = stateOf(root, c);
  if (cannot(c, st, z)) return;
  const w = st.work;
  rove(root, z);
  if (c.mode === "range") {
    if (w.from === null || w.to !== null) {
      // The first click starts a range. Nothing posts until it has both ends.
      if (!confirming(c)) {
        st.stash = { from: w.from, to: w.to };
        const f = fields(root);
        put(f.from, "");
        put(f.to, "");
      }
      st.work = { from: z, to: null };
      st.hover = null;
      paint(root);
      return;
    }
    st.work = { from: Math.min(w.from, z), to: Math.max(w.from, z) };
    st.hover = null;
    st.stash = null;
    paint(root);
    if (confirming(c)) hostChanged(/** @type {Element} */ (c.host), "draft");
    else commit(root, c, st);
    return;
  }
  if (c.mode === "single") {
    if (w.set.has(z) && !c.req && !c.host) w.set.clear();
    else w.set = new Set([z]);
  } else if (w.set.has(z)) w.set.delete(z);
  else if (!c.maxCount || w.set.size < c.maxCount) w.set.add(z);
  else return;
  paint(root);
  commit(root, c, st);
}
/** Leaves a half-picked range, or an unapplied one, as it was. @param {Element} root */
function abandon(root) {
  const c = cfg(root);
  if (!c || c.mode !== "range") return;
  const st = stateOf(root, c);
  const half = st.work.from !== null && st.work.to === null;
  if (half && !confirming(c)) {
    const back = st.stash || { from: null, to: null };
    const f = fields(root), whole = back.from !== null && back.to !== null;
    put(f.from, whole ? iso(back.from) : "");
    put(f.to, whole ? iso(back.to) : "");
  }
  if (half || confirming(c)) {
    st.work = readWork(c);
    st.stash = null;
    st.hover = null;
    paint(root);
  }
}

// ---- keyboard, pointer ----

/** A day moved by months, keeping the day of the month where it fits. @param {number} z @param {number} by */
function addMonths(z, by) {
  const [y, m, d] = civil(z);
  const [ny, nm] = ofIndex(Math.max(MIN_M, Math.min(MAX_M, mIndex(y, m) + by)));
  return days(ny, nm, Math.min(d, dim(ny, nm)));
}
on(document, "keydown", (e) => {
  const t = /** @type {Element} */ (e.target);
  const day = t.closest?.("button[data-slot=calendar-day]");
  if (day && !e.altKey && !e.ctrlKey && !e.metaKey) {
    const root = near(day, "[data-ui-dates=calendar]"), c = root && cfg(root);
    const z = fromIso(attr(day, "data-date"));
    if (!root || !c || z === null) return;
    const left = c.rtl ? 1 : -1, row = (dow(z) - c.ws + 7) % 7;
    /** @type {Record<string, number>} */
    const moves = { ArrowLeft: z + left, ArrowRight: z - left, ArrowUp: z - 7, ArrowDown: z + 7, Home: z - row, End: z + 6 - row, PageUp: addMonths(z, e.shiftKey ? -12 : -1), PageDown: addMonths(z, e.shiftKey ? 12 : 1) };
    if (e.key in moves) {
      e.preventDefault();
      go(root, moves[e.key]);
    }
  }
});

on(document, "click", (e) => {
  const t = /** @type {Element} */ (e.target);
  const day = t.closest?.("button[data-slot=calendar-day]");
  if (day) {
    const root = near(day, "[data-ui-dates=calendar]"), z = fromIso(attr(day, "data-date"));
    if (root && z !== null && attr(day, "aria-disabled") !== "true") choose(root, z);
    return;
  }
  const nav = t.closest?.("[data-ui-cal]");
  if (nav) {
    const root = near(nav, "[data-ui-dates=calendar]"), c = root && cfg(root);
    if (!root || !c) return;
    const st = stateOf(root, c), which = attr(nav, "data-ui-cal");
    if (which === "prev" || which === "next") {
      e.preventDefault();
      show(root, st.first + (which === "next" ? 1 : -1));
    } else if (which === "today") {
      const idx = monthIdx(c.today);
      if (idx < st.first || idx > st.first + c.months - 1) {
        show(root, idx);
        rove(root, c.today);
      } else choose(root, c.today);
    }
    return;
  }
  const act = t.closest?.("[data-ui-act]");
  if (act) onAct(act);
  const preset = t.closest?.("[data-ui-preset]");
  if (preset) onPreset(preset);
});
// A month or year list.
on(document, "change", (e) => {
  const t = /** @type {Element} */ (e.target), kind = attr(t, "data-ui-cal");
  const root = (kind === "month" || kind === "year") && near(t, "[data-ui-dates=calendar]");
  if (!root) return;
  const pick = (/** @type {string} */ k) => +(q(root, `[data-ui-cal=${k}]`)?.value || 0);
  show(root, mIndex(pick("year"), pick("month")));
  e.stopPropagation();
}, true);

/** Shows a half-picked range's reach as the pointer or focus moves. @param {Event} e */
function reach(e) {
  const day = /** @type {Element} */ (e.target).closest?.("button[data-slot=calendar-day]");
  if (!day) return;
  const root = near(day, "[data-ui-dates=calendar]"), c = root && cfg(root);
  if (!root || !c) return;
  const z = fromIso(attr(day, "data-date"));
  if (z === null) return;
  if (e.type === "focusin") rove(root, z);
  if (c.mode !== "range") return;
  const st = stateOf(root, c);
  if (st.work.from !== null && st.work.to === null && st.hover !== z) {
    st.hover = z;
    paint(root);
  }
}
on(document, "pointerover", reach);
on(document, "focusin", reach);
on(document, "pointerout", (e) => {
  const day = /** @type {Element} */ (e.target).closest?.("button[data-slot=calendar-day]");
  const root = day && near(day, "[data-ui-dates=calendar]");
  const to = /** @type {PointerEvent} */ (e).relatedTarget;
  if (!root || (to instanceof Node && root.contains(to))) return;
  const st = states.get(root);
  if (st && st.hover !== null) {
    st.hover = null;
    paint(root);
  }
});

// ---- pickers: the field that posts, the button, the calendar in the popover ----

/** @param {Element} h */
const calOf = (h) => q(h, "[data-ui-dates=calendar]");

/**
 * Makes the button agree with the field behind it.
 * `pick`: a day was chosen in the calendar, so close it and go back to the box.
 * `draft`: a range waits for Apply. `sync`: only show what the field holds.
 * A date picker's box is the box module's: the picker is told (`ui:date`), and it follows.
 * @param {Element} h @param {"pick" | "draft" | "sync"} how
 */
function hostChanged(h, how) {
  const kind = attr(h, "data-ui-dates"), cal = calOf(h), L = locOf(h);
  if (!L || !cal) return;
  const label = q(h, "[data-ui-label]"), trigger = q(h, "[data-ui-act=open]");
  if (kind === "picker") {
    const z = fromIso(nativeOf(h)?.value);
    if (label) label.textContent = z === null ? attr(label, "data-empty") || "" : fmt(L, z);
    h.toggleAttribute("data-has-value", z !== null);
    h.dispatchEvent(new Event("ui:date"));
    const st = states.get(cal);
    if (st) st.work = null;
    if (how === "pick") {
      closePop(h);
      /** @type {HTMLElement | null} */ (textOf(h) || trigger)?.focus();
    }
    return;
  }
  if (how !== "draft") {
    const f = fields(cal), a = fromIso(f.from?.value ?? null), b = fromIso(f.to?.value ?? null), whole = a !== null && b !== null;
    if (label) label.textContent = whole ? fmtRange(L, a, b, fromIso(attr(h, "data-today")) ?? 0) : attr(label, "data-empty") || "";
    if (trigger) {
      if (whole) set(trigger, "title", `${fmt(L, a)} – ${fmt(L, b)}`);
      else unset(trigger, "title");
    }
    h.toggleAttribute("data-has-value", whole);
  }
  markPresets(h);
  if (how === "pick") {
    closePop(h);
    /** @type {HTMLElement | null} */ (trigger)?.focus();
  }
}
/** Marks the preset that matches the range. @param {Element} h */
function markPresets(h) {
  const cal = calOf(h), c = cal && cfg(cal);
  if (!cal || !c) return;
  const w = stateOf(cal, c).work, whole = w.from !== null && w.to !== null;
  let any = false;
  /** @param {Element} b @param {boolean} on */
  const press = (b, on) => {
    set(b, "aria-pressed", String(on));
    b.toggleAttribute("data-active", on);
  };
  for (const b of /** @type {any} */ (all(h, "[data-ui-preset]:not([data-ui-preset=custom])"))) {
    const on = whole && fromIso(attr(b, "data-from")) === w.from && fromIso(attr(b, "data-to")) === w.to;
    any ||= on;
    press(b, on);
  }
  const custom = q(h, "[data-ui-preset=custom]");
  if (custom) press(custom, whole && !any);
}

/** A button that does something to a picker. @param {Element} el */
function onAct(el) {
  const act = attr(el, "data-ui-act"), h = near(el, "[data-ui-dates=picker],[data-ui-dates=range]");
  const cal = h && calOf(h), c = cal && cfg(cal);
  if (!c) return;
  const st = stateOf(cal, c);
  if (attr(h, "data-ui-dates") === "range") {
    if (act === "clear") {
      st.work = { from: null, to: null };
      st.stash = null;
      commit(cal, c, st);
      paint(cal);
    } else if (act === "apply") {
      if (st.work.from !== null && st.work.to === null) st.work = { from: st.work.from, to: st.work.from };
      commit(cal, c, st);
    } else if (act === "cancel") {
      abandon(cal);
      closePop(h);
      q(h, "[data-ui-act=open]")?.focus();
    }
  } else if (act === "today") choose(cal, c.today);
  else if (act === "clear") {
    const n = nativeOf(h), t = textOf(h);
    if (n && n.value) { put(n, ""); tell(n); }
    hostChanged(h, "sync");
    closePop(h);
    /** @type {HTMLElement | null} */ (t || q(h, "[data-ui-act=open]"))?.focus();
  }
}
/** A preset button of a range picker. @param {Element} el */
function onPreset(el) {
  const h = near(el, "[data-ui-dates=range]"), cal = h && calOf(h), c = cal && cfg(cal);
  if (!h || !cal || !c || /** @type {any} */ (el).disabled) return;
  const st = stateOf(cal, c);
  if (attr(el, "data-ui-preset") === "custom") {
    focusDay(cal, st.focus);
    return;
  }
  const a = fromIso(attr(el, "data-from")), b = fromIso(attr(el, "data-to"));
  if (a === null || b === null) return;
  st.work = { from: a, to: b };
  st.stash = null;
  st.hover = null;
  st.first = Math.max(navLimits(c.R)[0], Math.min(monthIdx(a), navLimits(c.R)[1] - c.months + 1));
  st.focus = null;
  render(cal);
  if (has(h, "data-confirm")) hostChanged(h, "draft");
  else commit(cal, c, st);
}

/** Opens a picker's calendar on the right month. @param {Element} h */
function openCalendar(h) {
  const cal = calOf(h), c = cal && cfg(cal);
  if (!cal || !c) return;
  const st = stateOf(cal, c);
  st.work = readWork(c);
  st.hover = null;
  st.stash = null;
  st.focus = null;
  const w = st.work;
  const anchor = c.mode === "range" ? w.from ?? c.today : [...w.set][0] ?? c.today;
  const [lo, hi] = navLimits(c.R);
  st.first = Math.max(lo, Math.min(monthIdx(anchor), hi - c.months + 1));
  render(cal);
  if (attr(h, "data-ui-dates") === "range") markPresets(h);
}

// ---- popovers, setup, morphs ----

const HOSTS = "[data-ui-dates=picker],[data-ui-dates=range]";
on(document, "beforetoggle", (e) => {
  const pop = /** @type {Element} */ (e.target), h = has(pop, "popover") && near(pop, HOSTS);
  if (h && /** @type {ToggleEvent} */ (e).newState === "open") openCalendar(h);
}, true);
on(document, "toggle", (e) => {
  const pop = /** @type {Element} */ (e.target), h = has(pop, "popover") && near(pop, HOSTS);
  const cal = h && calOf(h);
  if (!cal) return;
  if (/** @type {ToggleEvent} */ (e).newState === "open") {
    const st = states.get(cal);
    if (st) focusDay(cal, st.focus);
  } else if (attr(h, "data-ui-dates") === "range") abandon(cal);
}, true);

/** @type {WeakMap<Element, string>} */
const sigs = new WeakMap();
/** What a calendar is drawn from: when the page changes any of it, the months are drawn again. */
const SIGNED = ["data-ui-rules", "data-ui-loc", "data-today", "data-months", "data-mode", "data-week-start", "data-caption", "data-week-numbers", "data-fixed-weeks", "data-no-outside", "data-readonly", "data-disabled", "data-min-days", "data-max-days", "data-max-count"];
/** After a morph: pickers and calendars take what the page now says. */
function sync() {
  // The hidden fields of a range picker, and of a date picker drawn as a button, carry no requirement or
  // limits while the button stands before them, and are out of the tab order and out of sight for assistive
  // technology. A date picker's box does the same for its field (`date-text.js`).
  for (const n of /** @type {any} */ (every("[data-ui-dates=range] [data-ui-native],[data-ui-dates=picker][data-style=button] > [data-ui-native]"))) {
    n.required = false;
    unlimit(n);
    set(n, "aria-hidden", "true");
    set(n, "tabindex", "-1");
  }
  for (const root of /** @type {any} */ (every("[data-ui-dates=calendar]"))) {
    const st = states.get(root), sig = SIGNED.map((name) => attr(root, name)).join("|");
    if (!st || !sigs.has(root)) {
      sigs.set(root, sig);
      continue;
    }
    const c = cfg(root);
    if (!c) continue;
    if (sigs.get(root) !== sig) {
      sigs.set(root, sig);
      states.delete(root);
      render(root);
    } else if (!(st.work.from !== null && st.work.to === null)) {
      st.work = readWork(c);
      paint(root);
    }
  }
}
on(document, "ui:swap", sync);
// A form's reset puts the fields back; the boxes follow.
on(document, "reset", (e) => setTimeout(() => {
  const form = /** @type {Element} */ (e.target);
  for (const h of /** @type {any} */ (all(form, HOSTS))) {
    const cal = calOf(h);
    if (cal && attr(h, "data-ui-dates") === "range") states.delete(cal);
    hostChanged(h, "sync");
  }
  for (const root of /** @type {any} */ (all(form, "[data-ui-dates=calendar]"))) {
    const st = states.get(root), c = cfg(root);
    if (st && c) {
      st.work = readWork(c);
      paint(root);
    }
  }
}, 0));
sync();
