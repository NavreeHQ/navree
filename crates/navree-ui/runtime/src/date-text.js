// The box a person types a date in: a date picker's text box before the native date field
// that posts. It reads what is typed by the same rules as the server's reader, writes the
// day into the field, and says at the box what it can't take. A module of its own, loaded
// with a date picker that has a box, so a page of calendars and range pickers never loads it.
// The calendar in the popover is the dates module's; when it changes the field, it tells
// the picker (`ui:date`) and the box follows.
import { active, all, attr, every, has, on } from "./dom.js";
import { ALPHA, bad, blocked, civil, days, digit, dim, fmt, fold, fromIso, good, iso, locOf, nativeOf, near, put, q, rulesOf, stand, tell, textOf } from "./dates-shared.js";

const PICKER = "[data-ui-dates=picker]", BOX = `${PICKER} [data-ui-text]`;
const CONNECTORS = ["de", "del", "of", "the", "st", "nd", "rd", "th", "г", "um"];
const MARKERS = "年月日년월일";

/** @param {string} text @returns {(string | {w: string})[]} numbers as strings, words as objects */
function tokens(text) {
  const out = [];
  let num = "", word = "";
  const flush = () => {
    if (num) out.push(num);
    if (word) out.push({ w: word });
    num = word = "";
  };
  for (const c of text) {
    const v = digit(c);
    if (v >= 0) {
      if (word) flush();
      num += v;
    } else if (ALPHA.test(c) && !MARKERS.includes(c)) {
      if (num) flush();
      word += c;
    } else flush();
  }
  flush();
  return out;
}
/** Whether a word is one of `names`, or begins one when it has three letters or more. @param {string[]} names @param {string} word */
const named = (names, word) => names.some((name) => { const f = fold(name); return f === word || ([...word].length >= 3 && f.startsWith(word)); });
/** @param {any} L @param {string} word @returns {number} the month, 0 for none, -1 for several */
function monthOf(L, word) {
  let found = 0;
  for (let m = 1; m <= 12; m++) {
    if (named([L.m[m - 1], (L.md || L.m)[m - 1], L.ms[m - 1]], word)) {
      if (found) return -1;
      found = m;
    }
  }
  return found;
}
/** @param {string} digits @param {number} today @returns {number} the year, or 0 */
function yearOf(digits, today) {
  const v = +digits;
  let y = 0;
  if (digits.length === 4) y = v;
  else if (digits.length === 2) {
    const now = civil(today)[0];
    y = Math.floor(now / 100) * 100 + v;
    if (y > now + 50) y -= 100;
    else if (y <= now - 50) y += 100;
  }
  return y >= 1 && y <= 9999 ? y : 0;
}
/** @param {string} digits */
const part = (digits) => (digits.length >= 1 && digits.length <= 2 ? +digits : -1);

/**
 * Reads a typed date. `{z}` is the day, `{err}` one of the server's codes.
 * @param {any} L @param {string} text @param {number} today
 * @returns {{z?: number, err?: string}}
 */
function parseDate(L, text, today) {
  text = text.trim();
  if (!text) return { err: "empty" };
  if ([...text].length > 64) return { err: "unreadable" };
  /** @type {string[]} */
  const nums = [];
  let month = 0;
  for (const t of tokens(text)) {
    if (typeof t === "string") {
      if (t.length > 8 || nums.length === 3) return { err: "unreadable" };
      nums.push(t);
    } else {
      const w = fold(t.w);
      if (!w) continue;
      const m = monthOf(L, w);
      if (m < 0) return { err: "unreadable" };
      if (m > 0) {
        if (month) return { err: "unreadable" };
        month = m;
      } else if (!CONNECTORS.includes(w) && !named([...L.w, ...L.ws], w)) return { err: "unreadable" };
    }
  }
  /** @param {number} y @param {number} m @param {number} d */
  const build = (y, m, d) => (y < 1 ? { err: "bad_year" } : m < 1 || m > 12 || d < 1 || d > dim(y, m) ? { err: "not_a_date" } : { z: days(y, m, d) });
  /** Year, month, day from digit strings, checking in the server's order. @param {string} ys @param {string} ms @param {string} ds */
  const ymd = (ys, ms, ds) => {
    const m = part(ms), d = part(ds);
    if (m < 0 || d < 0) return { err: "unreadable" };
    return build(yearOf(ys, today), m, d);
  };
  if (nums.length === 3 && !month) {
    const [a, b, c] = nums;
    if (a.length === 4) return ymd(a, b, c);
    return L.o === "mdy" ? ymd(c, a, b) : L.o === "dmy" ? ymd(c, b, a) : ymd(a, b, c);
  }
  if (nums.length === 2 && month) {
    const [a, b] = nums;
    let ys, ds;
    if (a.length === 4 && b.length === 4) return { err: "unreadable" };
    if (a.length === 4) [ys, ds] = [a, b];
    else if (b.length === 4) [ys, ds] = [b, a];
    else if (L.o === "ymd") [ys, ds] = [a, b];
    else [ys, ds] = [b, a];
    const d = part(ds);
    if (d < 0) return { err: yearOf(ys, today) ? "unreadable" : "bad_year" };
    return build(yearOf(ys, today), month, d);
  }
  if (nums.length === 1 && !month) {
    const n = nums[0];
    const at = (/** @type {number} */ from, /** @type {number} */ to) => n.slice(from, to);
    if (n.length === 8) {
      const iso8 = (() => { const y = yearOf(at(0, 4), today); return y ? build(y, +at(4, 6), +at(6, 8)) : { err: "bad_year" }; })();
      if (iso8.z !== undefined) return iso8;
      if (L.o === "mdy") return ymd(at(4, 8), at(0, 2), at(2, 4));
      if (L.o === "dmy") return ymd(at(4, 8), at(2, 4), at(0, 2));
      return iso8;
    }
    if (n.length === 6) return L.o === "mdy" ? ymd(at(4, 6), at(0, 2), at(2, 4)) : L.o === "dmy" ? ymd(at(4, 6), at(2, 4), at(0, 2)) : ymd(at(0, 2), at(2, 4), at(4, 6));
    return { err: "unreadable" };
  }
  return { err: nums.length === 2 && !month ? "missing_year" : "unreadable" };
}

/** The rules of the picker's calendar. @param {Element} h */
const rules = (h) => { const cal = q(h, "[data-ui-dates=calendar]"); return cal && rulesOf(cal); };

/** The box was changed and left, or Enter was pressed. @param {Element} h */
function textDate(h) {
  const L = locOf(h), t = textOf(h), n = nativeOf(h), R = rules(h);
  if (!L || !t || !n || !R) return;
  const raw = t.value.trim();
  if (!raw) {
    good(h);
    if (n.value) { put(n, ""); tell(n); }
    h.toggleAttribute("data-has-value", false);
    return;
  }
  const z = parseDate(L, raw, fromIso(attr(h, "data-today")) ?? 0).z;
  if (z === undefined || blocked(R, z)) {
    put(n, "");
    bad(h, z === undefined ? L.L[16].replace("{example}", fmt(L, days(2026, 11, 30))) : L.L[17]);
    return;
  }
  good(h);
  const v = iso(z);
  if (n.value !== v) { put(n, v); tell(n); }
  t.value = fmt(L, z);
  h.toggleAttribute("data-has-value", true);
}

on(document, "keydown", (e) => {
  const t = /** @type {Element} */ (e.target), h = !e.isComposing && t.closest?.(BOX) && near(t, PICKER);
  if (!h) return;
  const open = q(h, "[data-ui-act=open]");
  if (e.key === "Enter") textDate(h);
  else if (e.key === "ArrowDown" && open && !open.disabled) {
    e.preventDefault();
    try { q(h, "[popover]")?.showPopover(); } catch {}
  }
});
// A box that was changed and left. What is typed belongs to the box until it is a date: the page hears the field.
on(document, "change", (e) => {
  const t = /** @type {Element} */ (e.target);
  if (t.closest?.(BOX)) {
    e.stopPropagation();
    textDate(near(t, PICKER));
  }
}, true);
on(document, "input", (e) => {
  if (/** @type {Element} */ (e.target).closest?.(BOX)) e.stopPropagation();
}, true);
// A label or an error focuses the hidden field: the box stands in for it.
on(document, "focusin", (e) => {
  const t = /** @type {Element} */ (e.target), h = has(t, "data-ui-native") && near(t, PICKER);
  if (h) textOf(h)?.focus();
});
// The calendar changed the field, or emptied it: the box shows what it holds, and the box's words go.
on(document, "ui:date", (e) => {
  const h = /** @type {Element} */ (e.target), t = textOf(h), L = locOf(h), z = fromIso(nativeOf(h)?.value);
  if (t && L && active() !== t) t.value = z === null ? "" : fmt(L, z);
  good(h);
}, true);

/** Sets up one picker: the hidden field stands behind the box. @param {Element} h */
function enhance(h) {
  const t = stand(h), n = nativeOf(h), z = fromIso(n?.value), L = locOf(h), R = rules(h);
  if (!t || !n || z === null) return;
  t.value = fmt(L, z);
  // A value the page was drawn with that its rules now refuse (stored before they changed):
  // say so at the box, where a person looks, unless the page has already said so.
  if (attr(n, "aria-invalid") !== "true" && R && blocked(R, z)) bad(h, L.L[17]);
}
const sync = () => { for (const h of /** @type {any} */ (every(PICKER))) enhance(h); };
on(document, "ui:swap", sync);
// A form's reset puts the fields back: the boxes are emptied and take what the fields hold.
on(document, "reset", (e) => setTimeout(() => {
  for (const h of /** @type {any} */ (all(/** @type {Element} */ (e.target), PICKER))) {
    good(h);
    const t = textOf(h);
    if (t) t.value = "";
    enhance(h);
  }
}, 0));
sync();
