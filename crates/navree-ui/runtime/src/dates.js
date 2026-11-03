// Dates: calendars, date and range pickers, time pickers. Everything is delegated from
// `document`, so a morph or a swap needs no re-wiring.
//
// A day here is a number: days counted from 1970-01-01 by integer arithmetic, so the
// browser's zone, its clock and `Date` never touch one. "Today" is what the server wrote
// into the page. The server owns the rules and checks every value that comes back;
// everything below only helps a person get to a value, and says so when it can't.
import { active as activeElement, all, attr, every, has, make, on, set, unset } from "./dom.js";

const E = Element.prototype;
/** @param {Element} el @param {string} sel @returns {any} */
const q = (el, sel) => E.querySelector.call(el, sel);
/** @param {Element} el @param {string} sel @returns {any} */
const near = (el, sel) => E.closest.call(el, sel);

// ---- days: civil dates as day numbers ----

/** @param {number} y @param {number} m @param {number} d */
function days(y, m, d) {
  y -= m <= 2 ? 1 : 0;
  const era = Math.floor(y / 400), yoe = y - era * 400;
  const doy = Math.floor((153 * (m + (m > 2 ? -3 : 9)) + 2) / 5) + d - 1;
  return era * 146097 + yoe * 365 + Math.floor(yoe / 4) - Math.floor(yoe / 100) + doy - 719468;
}
/** @param {number} z @returns {number[]} */
function civil(z) {
  z += 719468;
  const era = Math.floor(z / 146097), doe = z - era * 146097;
  const yoe = Math.floor((doe - Math.floor(doe / 1460) + Math.floor(doe / 36524) - Math.floor(doe / 146096)) / 365);
  const doy = doe - (365 * yoe + Math.floor(yoe / 4) - Math.floor(yoe / 100));
  const mp = Math.floor((5 * doy + 2) / 153);
  const m = mp < 10 ? mp + 3 : mp - 9;
  return [yoe + era * 400 + (m <= 2 ? 1 : 0), m, doy - Math.floor((153 * mp + 2) / 5) + 1];
}
/** @param {number} y @param {number} m */
const dim = (y, m) => days(m === 12 ? y + 1 : y, m === 12 ? 1 : m + 1, 1) - days(y, m, 1);
/** Weekday, 0 for Sunday. @param {number} z */
const dow = (z) => (((z + 4) % 7) + 7) % 7;
/** @param {number} n */
const p2 = (n) => String(n).padStart(2, "0");
/** @param {number} y @param {number} m */
const ym = (y, m) => `${String(y).padStart(4, "0")}-${p2(m)}`;
/** @param {number} z */
const iso = (z) => {
  const [y, m, d] = civil(z);
  return `${ym(y, m)}-${p2(d)}`;
};
const FIRST = days(1, 1, 1), LAST = days(9999, 12, 31);
/** @param {string | null} text @returns {number | null} */
function fromIso(text) {
  const k = /^(\d{4})-(\d{2})-(\d{2})$/.exec(text || "");
  if (!k) return null;
  const y = +k[1], m = +k[2], d = +k[3];
  return y < 1 || m < 1 || m > 12 || d < 1 || d > dim(y, m) ? null : days(y, m, d);
}
/** A month as a number, for comparing and counting. @param {number} y @param {number} m */
const mIndex = (y, m) => y * 12 + m - 1;
const MIN_M = mIndex(1, 1), MAX_M = mIndex(9999, 12);
/** @param {number} index @returns {number[]} */
const ofIndex = (index) => [Math.floor(index / 12), (index % 12) + 1];

// ---- locale ----

/** @type {Map<string, any>} */
const locales = new Map();
/** The locale written in `el` or below it, or null when it isn't a whole one. @param {Element} el */
function locOf(el) {
  const holder = has(el, "data-ui-loc") ? el : q(el, "[data-ui-loc]");
  const text = holder ? attr(holder, "data-ui-loc") || "" : "";
  if (!locales.has(text)) {
    let l = null;
    try {
      l = JSON.parse(text);
      if (l.m.length !== 12 || l.ms.length !== 12 || l.w.length !== 7 || l.ws.length !== 7 || l.L.length < 20) l = null;
    } catch {
      l = null;
    }
    if (locales.size > 20) locales.clear();
    locales.set(text, l);
  }
  return locales.get(text);
}
/** @param {string} pattern @param {Record<string, string | number>} v */
const expand = (pattern, v) => pattern.replace(/\{(.)\}/g, (s, k) => (k in v ? String(v[k]) : s));
/** The date as typed here. @param {any} L @param {number} z */
function fmt(L, z) {
  const [y, m, d] = civil(z);
  const Y = String(y).padStart(4, "0"), M = p2(m), D = p2(d);
  const parts = L.o === "mdy" ? [M, D, Y] : L.o === "dmy" ? [D, M, Y] : [Y, M, D];
  return parts.join(L.s) + L.x;
}
/** A date read out in full. @param {any} L @param {number} z */
function longDate(L, z) {
  const [y, m, d] = civil(z);
  return expand(L.lp, { w: L.w[dow(z)], M: (L.md || L.m)[m - 1], d, m, y });
}
/** @param {any} L @param {number} y @param {number} m */
const caption = (L, y, m) => expand(L.cp, { M: L.m[m - 1], m, y });
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
/** A time of day as typed here. @param {any} L @param {number} t seconds since midnight @param {boolean} secs */
function fmtTime(L, t, secs) {
  const h = Math.floor(t / 3600), mi = Math.floor((t % 3600) / 60), s = t % 60;
  const tail = secs ? `${p2(mi)}:${p2(s)}` : p2(mi);
  if (L.c === 24) return `${p2(h)}:${tail}`;
  const shown = ((h + 11) % 12) + 1, mark = h < 12 ? L.am : L.pm;
  return L.mf ? `${mark}${shown}:${tail}` : `${shown}:${tail}${mark}`;
}

// ---- reading what a person types: the same rules as the server's reader ----

const ZEROS = [0x660, 0x6f0, 0x7c0, 0x966, 0x9e6, 0xa66, 0xae6, 0xb66, 0xbe6, 0xc66, 0xce6, 0xd66, 0xe50, 0xed0, 0xf20, 0x1040, 0xff10];
/** @param {string} c */
function digit(c) {
  const k = /** @type {number} */ (c.codePointAt(0));
  if (k >= 48 && k <= 57) return k - 48;
  if (k < 0x660) return -1;
  for (const zero of ZEROS) if (k >= zero && k <= zero + 9) return k - zero;
  return -1;
}
const PLAIN = ["àáâãäåāăą", "a", "çćč", "c", "ďđ", "d", "èéêëēėęěẽ", "e", "ğ", "g", "ìíîïīıį", "i", "ł", "l", "ñńň", "n", "òóôõöøōő", "o", "ř", "r", "śşšß", "s", "ţť", "t", "ùúûüūůűų", "u", "ýÿ", "y", "źżž", "z", "æ", "e", "œ", "e", "þ", "t", "ð", "d", "ħ", "h", "ĳ", "i", "ŋ", "n"];
/** @param {string} c */
function plain(c) {
  for (let i = 0; i < PLAIN.length; i += 2) if (PLAIN[i].includes(c)) return PLAIN[i + 1];
  return c;
}
const ALNUM = /[\p{Alphabetic}\p{N}]/u, ALPHA = /\p{Alphabetic}/u;
/** @param {string} text */
const fold = (text) => [...text.toLowerCase()].filter((c) => ALNUM.test(c)).map(plain).join("");
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
/** @param {any} L @param {string} word @returns {number} the month, 0 for none, -1 for several */
function monthOf(L, word) {
  const long = [...word].length >= 3;
  let found = 0;
  for (let m = 1; m <= 12; m++) {
    const hit = [L.m[m - 1], (L.md || L.m)[m - 1], L.ms[m - 1]].some((name) => {
      const f = fold(name);
      return f === word || (long && f.startsWith(word));
    });
    if (hit) {
      if (found) return -1;
      found = m;
    }
  }
  return found;
}
/** @param {any} L @param {string} word */
const isWeekday = (L, word) => [...L.w, ...L.ws].some((name) => { const f = fold(name); return f === word || ([...word].length >= 3 && f.startsWith(word)); });
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
      } else if (!CONNECTORS.includes(w) && !isWeekday(L, w)) return { err: "unreadable" };
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
    const at = (from, to) => n.slice(from, to);
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

/**
 * Reads a typed time into seconds since midnight.
 * @param {any} L @param {string} text @returns {{t?: number, err?: string}}
 */
function parseTime(L, text) {
  text = text.trim();
  if (!text) return { err: "empty" };
  if ([...text].length > 32) return { err: "unreadable" };
  const lower = text.toLowerCase().replaceAll("uhr", " ");
  /** @type {string[]} */
  const runs = [];
  let run = "", letters = "";
  for (const c of lower) {
    const v = digit(c);
    if (v >= 0) run += v;
    else {
      if (run) runs.push(run);
      run = "";
      if (ALPHA.test(c) && !"hu時点分秒시분초".includes(c)) letters += c;
    }
  }
  if (run) runs.push(run);
  letters = fold(letters);
  let marker = null;
  if (letters) {
    if (letters === "a" || letters === "am" || letters === fold(L.am)) marker = "am";
    else if (letters === "p" || letters === "pm" || letters === fold(L.pm)) marker = "pm";
    else return { err: "unreadable" };
  }
  /** @type {string[]} */
  let parts;
  const split = (s, sizes) => { let at = 0; return sizes.map((n) => { const p = s.slice(at, at + n); at += n; return p; }); };
  if (runs.length === 1) {
    const r = runs[0];
    parts = r.length <= 2 ? [r] : r.length === 3 ? split(r, [1, 2]) : r.length === 4 ? split(r, [2, 2]) : r.length === 5 ? split(r, [1, 2, 2]) : r.length === 6 ? split(r, [2, 2, 2]) : [];
  } else if (runs.length === 2 || runs.length === 3) parts = runs;
  else parts = [];
  if (!parts.length || parts.some((p) => !p || p.length > 2)) return { err: "unreadable" };
  let [h, m, s] = [+parts[0], +(parts[1] || 0), +(parts[2] || 0)];
  if (marker) {
    if (h < 1 || h > 12) return { err: "out_of_range" };
    h %= 12;
    if (marker === "pm") h += 12;
  }
  if (h > 23 || m > 59 || s > 59) return { err: "out_of_range" };
  return { t: h * 3600 + m * 60 + s };
}

// ---- the rules the server wrote ----

/** @type {Map<string, any>} */
const ruleCache = new Map();
/** @param {Element} el the calendar root */
function rulesOf(el) {
  const text = attr(el, "data-ui-rules") || "{}";
  if (!ruleCache.has(text)) {
    let o = {};
    try { o = JSON.parse(text) || {}; } catch {}
    const nav = Array.isArray(o.nav) ? o.nav.map((/** @type {string} */ s) => { const k = /^(\d{4})-(\d{2})$/.exec(s); return k ? mIndex(+k[1], +k[2]) : null; }) : null;
    if (ruleCache.size > 20) ruleCache.clear();
    ruleCache.set(text, {
      min: fromIso(o.min), max: fromIso(o.max), wd: new Set(o.wd || []),
      runs: (o.r || []).map((/** @type {string[]} */ r) => [fromIso(r[0]), fromIso(r[1])]),
      nav,
    });
  }
  return ruleCache.get(text);
}
/** Whether a day can't be chosen. @param {any} R @param {number} z */
const blocked = (R, z) => z < FIRST || z > LAST || (R.min !== null && z < R.min) || (R.max !== null && z > R.max) || R.wd.has(dow(z)) || R.runs.some((/** @type {number[]} */ r) => z >= r[0] && z <= r[1]);
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
/** @param {string} text */
const esc = (text) => String(text).replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);
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
    st = { first: y ? mIndex(y, m) : mIndex(...(/** @type {[number, number]} */ (civil(c.today).slice(0, 2)))), focus: f ? fromIso(attr(f, "data-date")) : null, hover: null, work: null, stash: null };
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

/** One month, as the server draws it. @param {any} c @param {State} st @param {number} y @param {number} m @param {number} focus */
function monthHtml(c, st, y, m, focus) {
  // The id is a page's own text, so it is escaped like every other word that goes into the markup.
  const L = c.L, id = esc(`${c.id}-cap-${ym(y, m)}`), editable = !c.ro && !c.off;
  let cap;
  if (c.dd) {
    const lo = Math.max(1, c.R.min === null ? y - 100 : civil(c.R.min)[0]), hi = Math.min(9999, c.R.max === null ? y + 100 : civil(c.R.max)[0]);
    const [from, to] = hi - lo > 200 ? [Math.max(lo, y - 100), Math.min(hi, y + 100)] : [lo, hi];
    const opts = (/** @type {string[]} */ list, /** @type {number} */ cur, /** @type {number} */ base) => list.map((t, i) => `<option value="${base + i}"${base + i === cur ? " selected" : ""}>${esc(t)}</option>`).join("");
    const years = Array.from({ length: to - from + 1 }, (_, i) => String(from + i));
    cap = `<div data-slot="calendar-caption" id="${id}" data-layout="dropdowns" aria-live="polite"><select data-slot="native-select" data-size="sm" data-ui-cal="month" aria-label="${esc(L.L[6])}">${opts(L.m, m, 1)}</select><select data-slot="native-select" data-size="sm" data-ui-cal="year" aria-label="${esc(L.L[7])}">${opts(years, y, from)}</select></div>`;
  } else cap = `<div data-slot="calendar-caption" id="${id}" aria-live="polite">${esc(caption(L, y, m))}</div>`;
  const head = Array.from({ length: 7 }, (_, i) => { const d = (c.ws + i) % 7; return `<th data-slot="calendar-weekday" scope="col" abbr="${esc(L.w[d])}">${esc(L.ws[d])}</th>`; }).join("");
  const first = days(y, m, 1);
  const lead = (dow(first) - c.ws + 7) % 7;
  const rows = c.fixed ? 6 : Math.ceil((lead + dim(y, m)) / 7);
  let body = "";
  for (let r = 0; r < rows; r++) {
    const row = first - lead + r * 7;
    let cells = "";
    if (c.wn) cells += `<th data-slot="calendar-weeknum" scope="row">${isoWeek(row + ((11 - dow(row)) % 7))}</th>`;
    for (let i = 0; i < 7; i++) cells += cellHtml(c, st, row + i, y, m, focus, editable);
    body += `<tr>${cells}</tr>`;
  }
  return `<div data-slot="calendar-month" data-month="${ym(y, m)}"><div data-slot="calendar-head">${cap}</div><table data-slot="calendar-grid" role="grid" aria-labelledby="${id}"><thead><tr>${c.wn ? `<th data-slot="calendar-weeknum" scope="col">${esc(L.L[8])}</th>` : ""}${head}</tr></thead><tbody>${body}</tbody></table></div>`;
}
/** The ISO week of the Thursday `z`. @param {number} z */
function isoWeek(z) {
  const [y] = civil(z);
  const jan4 = days(y, 1, 4);
  const week1 = jan4 - ((dow(jan4) + 6) % 7);
  return Math.floor((z - week1) / 7) + 1;
}
/** @param {any} c @param {State} st @param {number} z @param {number} y @param {number} m @param {number} focus @param {boolean} editable */
function cellHtml(c, st, z, y, m, focus, editable) {
  const [zy, zm, zd] = civil(z);
  if (zy !== y || zm !== m) return `<td role="gridcell">${c.noOutside ? "" : `<span data-slot="calendar-day" data-outside aria-hidden="true">${zd}</span>`}</td>`;
  const k = marks(c, st, z), off = blocked(c.R, z);
  const selected = c.mode === "range" ? !!(k.start || k.end || k.mid) : !!k.sel;
  const flags = (c.mode === "range" ? "" : k.sel ? " data-selected" : "") + (z === c.today ? " data-today" : "") + (off ? " data-disabled" : "") + (k.start ? " data-range-start" : "") + (k.end ? " data-range-end" : "") + (k.mid ? " data-range-middle" : "");
  const dis = off || !editable ? ' aria-disabled="true"' : "";
  const cur = z === c.today ? ' aria-current="date"' : "";
  return `<td role="gridcell"${c.ro ? "" : ` aria-selected="${selected}"`}><button data-slot="calendar-day" type="button" data-date="${iso(z)}" tabindex="${focus === z && editable ? 0 : -1}" aria-label="${esc(longDate(c.L, z))}"${cur}${dis}${flags}>${zd}</button></td>`;
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
  if (box) drawMonths(box, vis.map(([y, m]) => monthHtml(c, st, y, m, focus)));
  navState(root, c, st);
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
  tpl.innerHTML = months.join("");
  const fresh = /** @type {Element[]} */ ([...DocumentFragment.prototype.querySelectorAll.call(tpl.content, "[data-slot=calendar-month]")]);
  const caption = (/** @type {Element} */ m) => q(m, "[data-slot=calendar-caption]");
  if (now.length !== fresh.length || now.some((m, i) => !caption(m) || attr(caption(m), "data-layout") !== attr(caption(fresh[i]), "data-layout"))) {
    box.innerHTML = months.join("");
    return;
  }
  now.forEach((m, i) => {
    const f = fresh[i], old = caption(m), next = caption(f);
    if (!old || !next) return;
    set(m, "data-month", attr(f, "data-month") || "");
    set(old, "id", next.id);
    old.replaceChildren(...next.childNodes);
    const table = q(m, "table"), replacement = q(f, "table");
    if (table && replacement) table.replaceWith(replacement);
  });
}
/** Turns the previous and next buttons on and off. @param {Element} root @param {any} c @param {State} st */
function navState(root, c, st) {
  const [lo, hi] = navLimits(c.R);
  for (const b of /** @type {any} */ (all(root, "[data-ui-cal=prev]"))) b.disabled = st.first <= lo;
  for (const b of /** @type {any} */ (all(root, "[data-ui-cal=next]"))) b.disabled = st.first + c.months - 1 >= hi;
}
/** Brings the marks on the days up to date without redrawing them. @param {Element} root */
function paint(root) {
  const c = cfg(root);
  if (!c) return;
  const st = stateOf(root, c);
  const editable = !c.ro && !c.off;
  /** @param {Element} el @param {string} name @param {any} on */
  const flag = (el, name, on) => (on ? set(el, name, "") : unset(el, name));
  for (const b of /** @type {any} */ (all(root, "button[data-slot=calendar-day]"))) {
    const z = fromIso(attr(b, "data-date"));
    if (z === null) continue;
    const k = marks(c, st, z), r = c.mode === "range";
    const selected = r ? !!(k.start || k.end || k.mid) : !!k.sel;
    flag(b, "data-selected", !r && k.sel);
    flag(b, "data-range-start", r && k.start);
    flag(b, "data-range-end", r && k.end);
    flag(b, "data-range-middle", r && k.mid);
    flag(b, "data-preview", r && k.preview);
    flag(b, "data-pending", r && k.pending);
    const off = cannot(c, st, z);
    flag(b, "data-disabled", blocked(c.R, z));
    flag(b, "data-unreachable", off && !blocked(c.R, z));
    if (off || !editable) set(b, "aria-disabled", "true");
    else unset(b, "aria-disabled");
    const cell = b.parentElement;
    if (cell && !c.ro) set(cell, "aria-selected", String(selected));
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
    if (f.from) f.from.value = a;
    if (f.to) f.to.value = b;
    if (changed) tell(f.to || f.from);
  } else {
    const v = [...w.set].sort((a, b) => a - b).map(iso).join(",");
    const changed = (f.main?.value ?? "") !== v;
    if (f.main) f.main.value = v;
    if (changed) tell(f.main);
  }
  const h = host(root);
  if (h) hostChanged(h, "pick");
}
/** The page hears one `input` and one `change`, as for any field. @param {Element | undefined} el */
function tell(el) {
  if (!el) return;
  el.dispatchEvent(new Event("input", { bubbles: true }));
  el.dispatchEvent(new Event("change", { bubbles: true }));
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
        if (f.from) f.from.value = "";
        if (f.to) f.to.value = "";
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
    if (f.from) f.from.value = whole ? iso(back.from) : "";
    if (f.to) f.to.value = whole ? iso(back.to) : "";
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
    return;
  }
  const text = t.closest?.("[data-ui-text]");
  if (text && !e.isComposing) textKey(e, text);
  const list = t.closest?.("[data-slot=time-list]");
  if (list) listKey(e, list);
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
  const opt = t.closest?.("[data-slot=time-list] [role=option]");
  if (opt) {
    const h = near(opt, "[data-ui-dates=time]");
    if (h) chooseTime(h, +(attr(opt, "data-t") || 0));
  }
  // A label of a picker focuses its box.
});
// A month or year list, and a box that was changed and left.
on(document, "change", (e) => {
  const t = /** @type {Element} */ (e.target);
  const kind = attr(t, "data-ui-cal");
  if (kind === "month" || kind === "year") {
    const root = near(t, "[data-ui-dates=calendar]");
    if (!root) return;
    const pick = (/** @type {string} */ k) => +(q(root, `[data-ui-cal=${k}]`)?.value || 0);
    show(root, mIndex(pick("year"), pick("month")));
    e.stopPropagation();
    return;
  }
  const text = t.closest?.("[data-ui-text]");
  if (text) {
    e.stopPropagation();
    const h = near(text, "[data-ui-dates]");
    if (h) (attr(h, "data-ui-dates") === "time" ? textTime : textDate)(h);
  }
}, true);
// What is typed in a box belongs to the box until it is a date: the page hears the field.
on(document, "input", (e) => {
  if (/** @type {Element} */ (e.target).closest?.("[data-ui-text]")) e.stopPropagation();
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
on(document, "focusin", (e) => {
  const t = /** @type {Element} */ (e.target);
  // A label or an error focuses the hidden field: the box stands in for it.
  if (has(t, "data-ui-native")) {
    const h = near(t, "[data-ui-dates=picker],[data-ui-dates=time]");
    if (h) q(h, "[data-ui-text]")?.focus();
  }
});
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

// ---- pickers: the box, the field behind it, the calendar in the popover ----

/** @param {Element} h */
const calOf = (h) => q(h, "[data-ui-dates=calendar]");
/** @param {Element} h */
const nativeOf = (h) => /** @type {HTMLInputElement | null} */ (q(h, "[data-ui-native]"));
/** @param {Element} h */
const textOf = (h) => /** @type {HTMLInputElement | null} */ (q(h, "[data-ui-text]"));
/** @param {Element} h */
function closePop(h) {
  try { /** @type {HTMLElement | null} */ (q(h, "[popover]"))?.hidePopover(); } catch {}
}

/**
 * Makes the box and the button agree with the field behind them.
 * `pick`: a day was chosen in the calendar, so close it and go back to the box.
 * `draft`: a range waits for Apply. `sync`: only show what the field holds.
 * @param {Element} h @param {"pick" | "draft" | "sync"} how
 */
function hostChanged(h, how) {
  const kind = attr(h, "data-ui-dates"), cal = calOf(h), L = locOf(h);
  if (!L || !cal) return;
  if (kind === "picker") {
    const n = nativeOf(h), t = textOf(h), label = q(h, "[data-ui-label]");
    const z = fromIso(n?.value ?? null);
    if (t && activeElement() !== t) t.value = z === null ? "" : fmt(L, z);
    if (label) label.textContent = z === null ? attr(label, "data-empty") || "" : fmt(L, z);
    h.toggleAttribute("data-has-value", z !== null);
    good(h);
    const st = states.get(cal);
    if (st) st.work = null;
    if (how === "pick") {
      closePop(h);
      /** @type {HTMLElement | null} */ (t || q(h, "[data-ui-act=open]"))?.focus();
    }
  } else if (kind === "range") {
    if (how === "draft") {
      markPresets(h);
      return;
    }
    const f = fields(cal), a = fromIso(f.from?.value ?? null), b = fromIso(f.to?.value ?? null);
    const label = q(h, "[data-ui-label]"), trigger = q(h, "[data-ui-act=open]");
    const today = fromIso(attr(h, "data-today")) ?? 0;
    if (label) label.textContent = a !== null && b !== null ? fmtRange(L, a, b, today) : attr(label, "data-empty") || "";
    if (trigger) {
      if (a !== null && b !== null) set(trigger, "title", `${fmt(L, a)} – ${fmt(L, b)}`);
      else unset(trigger, "title");
    }
    h.toggleAttribute("data-has-value", a !== null && b !== null);
    markPresets(h);
    if (how === "pick") {
      closePop(h);
      /** @type {HTMLElement | null} */ (trigger)?.focus();
    }
  }
}
/** Marks the preset that matches the range. @param {Element} h */
function markPresets(h) {
  const cal = calOf(h), c = cal && cfg(cal);
  if (!cal || !c) return;
  const w = stateOf(cal, c).work;
  let any = false;
  for (const b of /** @type {any} */ (all(h, "[data-ui-preset]"))) {
    if (attr(b, "data-ui-preset") === "custom") continue;
    const on = w.from !== null && w.to !== null && fromIso(attr(b, "data-from")) === w.from && fromIso(attr(b, "data-to")) === w.to;
    any ||= on;
    set(b, "aria-pressed", String(on));
    b.toggleAttribute("data-active", on);
  }
  const custom = q(h, "[data-ui-preset=custom]");
  if (custom) {
    const on = w.from !== null && w.to !== null && !any;
    set(custom, "aria-pressed", String(on));
    custom.toggleAttribute("data-active", on);
  }
}

/** A button that does something to a picker. @param {Element} el */
function onAct(el) {
  const act = attr(el, "data-ui-act"), h = near(el, "[data-ui-dates=picker],[data-ui-dates=range],[data-ui-dates=time]");
  if (!h) return;
  const kind = attr(h, "data-ui-dates");
  if (act === "clear") {
    if (kind === "range") {
      const cal = calOf(h), c = cal && cfg(cal);
      if (!cal || !c) return;
      const st = stateOf(cal, c);
      st.work = { from: null, to: null };
      st.stash = null;
      commit(cal, c, st);
      paint(cal);
    } else if (kind === "time") {
      setTime(h, null);
      textOf(h)?.focus();
    } else {
      const n = nativeOf(h), t = textOf(h);
      if (n && n.value) { n.value = ""; tell(n); }
      if (t) t.value = "";
      hostChanged(h, "sync");
      closePop(h);
      /** @type {HTMLElement | null} */ (t || q(h, "[data-ui-act=open]"))?.focus();
    }
  } else if (act === "today" && kind === "picker") {
    const cal = calOf(h), c = cal && cfg(cal);
    if (cal && c) choose(cal, c.today);
  } else if (act === "apply" && kind === "range") {
    const cal = calOf(h), c = cal && cfg(cal);
    if (!cal || !c) return;
    const st = stateOf(cal, c);
    if (st.work.from !== null && st.work.to === null) st.work = { from: st.work.from, to: st.work.from };
    commit(cal, c, st);
  } else if (act === "cancel" && kind === "range") {
    const cal = calOf(h);
    if (cal) abandon(cal);
    closePop(h);
    /** @type {HTMLElement | null} */ (q(h, "[data-ui-act=open]"))?.focus();
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

/** Says a typed date is wrong. @param {Element} h @param {string} message */
function bad(h, message) {
  const t = textOf(h);
  if (!t) return;
  h.toggleAttribute("data-ui-bad", true);
  set(t, "aria-invalid", "true");
  t.setCustomValidity(message);
  let m = q(h, "[data-ui-msg]");
  if (!m) {
    m = make("p");
    set(m, "data-slot", "field-error");
    set(m, "data-ui-msg", "");
    set(m, "role", "alert");
    m.id = `${h.id}-msg`;
    h.append(m);
  }
  m.textContent = message;
  const ids = (attr(t, "aria-describedby") || "").split(" ").filter(Boolean);
  if (!ids.includes(m.id)) set(t, "aria-describedby", [...ids, m.id].join(" "));
}
/** Clears the mark `bad` made. @param {Element} h */
function good(h) {
  const t = textOf(h);
  if (!t || !has(h, "data-ui-bad")) return;
  h.removeAttribute("data-ui-bad");
  t.setCustomValidity("");
  q(h, "[data-ui-msg]")?.remove();
  mirror(h);
}
/** Copies the hidden field's marks to the box a person sees. @param {Element} h */
function mirror(h) {
  const n = nativeOf(h), t = textOf(h);
  if (!n || !t || has(h, "data-ui-bad")) return;
  for (const name of ["aria-invalid", "aria-describedby"]) {
    const v = attr(n, name);
    if (v === null) unset(t, name);
    else set(t, name, v);
  }
}

/** The box was changed and left, or Enter was pressed. @param {Element} h */
function textDate(h) {
  const L = locOf(h), t = textOf(h), n = nativeOf(h), cal = calOf(h), c = cal && cfg(cal);
  if (!L || !t || !n || !c) return;
  const raw = t.value.trim();
  if (!raw) {
    good(h);
    if (n.value) { n.value = ""; tell(n); }
    hostChanged(h, "sync");
    return;
  }
  const r = parseDate(L, raw, c.today);
  if (r.z === undefined) {
    n.value = "";
    bad(h, L.L[16].replace("{example}", fmt(L, days(2026, 11, 30))));
  } else if (blocked(c.R, r.z)) {
    n.value = "";
    bad(h, L.L[17]);
  } else {
    good(h);
    const v = iso(r.z);
    if (n.value !== v) { n.value = v; tell(n); }
    t.value = fmt(L, r.z);
    h.toggleAttribute("data-has-value", true);
    const label = q(h, "[data-ui-label]");
    if (label) label.textContent = t.value;
    const st = states.get(cal);
    if (st) st.work = null;
  }
}
/** Keys in a picker's box. @param {KeyboardEvent} e @param {Element} text */
function textKey(e, text) {
  const h = near(text, "[data-ui-dates]");
  if (!h) return;
  const kind = attr(h, "data-ui-dates");
  if (e.key === "Enter") {
    (kind === "time" ? textTime : textDate)(h);
  } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
    const open = /** @type {HTMLElement | null} */ (q(h, "[data-ui-act=open]"));
    if (kind === "time" && (!open || open.hidden)) {
      e.preventDefault();
      stepTime(h, e.key === "ArrowDown" ? -1 : 1);
    } else if (e.key === "ArrowDown" && open && !/** @type {any} */ (open).disabled) {
      e.preventDefault();
      try { /** @type {HTMLElement | null} */ (q(h, "[popover]"))?.showPopover(); } catch {}
    }
  }
}

// ---- time ----

/** @param {Element} h */
function trules(h) {
  /** @type {any} */
  let o = {};
  try { o = JSON.parse(attr(h, "data-ui-trules") || "{}") || {}; } catch {}
  const sec = (/** @type {string | undefined} */ s) => { const k = /^(\d\d):(\d\d):(\d\d)$/.exec(s || ""); return k ? +k[1] * 3600 + +k[2] * 60 + +k[3] : null; };
  return { st: o.st || 30, min: sec(o.min), max: sec(o.max), u: o.u || [], strict: !!o.strict };
}
/** Whether a time can't be chosen. @param {any} T @param {number} t */
function timeBlocked(T, t) {
  const minute = Math.floor(t / 60);
  return (T.min !== null && t < T.min) || (T.max !== null && t > T.max) || T.u.some((/** @type {number[]} */ r) => minute >= r[0] && minute <= r[1]) || (T.strict && (t % 60 !== 0 || minute % T.st !== 0));
}
/** The text of a native time value. @param {number} t @param {boolean} secs */
const timeValue = (t, secs) => `${p2(Math.floor(t / 3600))}:${p2(Math.floor((t % 3600) / 60))}${secs || t % 60 ? `:${p2(t % 60)}` : ""}`;
/** @param {string} v */
function nativeTime(v) {
  const k = /^(\d\d):(\d\d)(?::(\d\d))?$/.exec(v || "");
  return k && +k[1] < 24 && +k[2] < 60 && +(k[3] || 0) < 60 ? +k[1] * 3600 + +k[2] * 60 + +(k[3] || 0) : null;
}
/** Sets a time in the field and the box. @param {Element} h @param {number | null} t */
function setTime(h, t) {
  const L = locOf(h), n = nativeOf(h), x = textOf(h);
  if (!L || !n || !x) return;
  const secs = has(h, "data-seconds");
  const v = t === null ? "" : timeValue(t, secs);
  x.value = t === null ? "" : fmtTime(L, t, secs);
  good(h);
  if (n.value !== v) { n.value = v; tell(n); }
  h.toggleAttribute("data-has-value", !!v);
}
/** @param {Element} h @param {number} t */
function chooseTime(h, t) {
  setTime(h, t);
  closePop(h);
  textOf(h)?.focus();
}
/** The box was changed and left, or Enter was pressed. @param {Element} h */
function textTime(h) {
  const L = locOf(h), x = textOf(h), n = nativeOf(h);
  if (!L || !x || !n) return;
  const raw = x.value.trim();
  if (!raw) { setTime(h, null); return; }
  const r = parseTime(L, raw);
  if (r.t === undefined) {
    n.value = "";
    bad(h, L.L[18].replace("{example}", fmtTime(L, 14 * 3600 + 30 * 60, false)));
  } else if (timeBlocked(trules(h), r.t)) {
    n.value = "";
    bad(h, L.L[19]);
  } else setTime(h, r.t);
}
/** Up and Down on a time with no list. @param {Element} h @param {number} dir */
function stepTime(h, dir) {
  const n = nativeOf(h), T = trules(h);
  const cur = nativeTime(n?.value || "") ?? (dir > 0 ? -T.st * 60 : 86400);
  for (let t = cur + dir * T.st * 60; t >= 0 && t < 86400; t += dir * T.st * 60) {
    if (!timeBlocked(T, t)) { setTime(h, t); return; }
  }
}
/** Fills the list of times when the popover opens. @param {Element} h */
function fillList(h) {
  const L = locOf(h), T = trules(h), ul = q(h, "[data-slot=time-list]"), n = nativeOf(h);
  if (!L || !ul) return;
  const secs = has(h, "data-seconds"), cur = nativeTime(n?.value || "");
  /** @type {number[]} */
  const opts = [];
  for (let m = 0; m < 1440; m += T.st) if (!timeBlocked(T, m * 60)) opts.push(m * 60);
  ul.innerHTML = opts.map((t) => `<li role="option" id="${esc(ul.id)}-${t}" data-t="${t}" aria-selected="${t === cur}">${esc(fmtTime(L, t, secs))}</li>`).join("");
  let at = opts.findIndex((t) => t === cur);
  if (at < 0) at = Math.max(0, opts.findIndex((t) => cur !== null && t >= cur));
  activate(ul, at);
}
/** Makes the nth option the active one. @param {Element} ul @param {number} at */
function activate(ul, at) {
  const items = /** @type {Element[]} */ ([...all(ul, "[role=option]")]);
  for (const li of items) li.removeAttribute("data-active");
  const li = items[Math.max(0, Math.min(at, items.length - 1))];
  if (!li) {
    unset(ul, "aria-activedescendant");
    return;
  }
  li.setAttribute("data-active", "");
  set(ul, "aria-activedescendant", li.id);
  reveal(/** @type {HTMLElement} */ (ul), /** @type {HTMLElement} */ (li), false);
}
/**
 * Scrolls the list, never the page, until the option shows: the least that does it, or with
 * `middle` the option in the middle. It counts in the list's own units, so a popover that is
 * still growing into place doesn't skew it. A list that is not shown has no size and stays
 * as it is. @param {HTMLElement} ul @param {HTMLElement} li @param {boolean} middle
 */
function reveal(ul, li, middle) {
  const top = li.offsetTop, h = li.offsetHeight, view = ul.clientHeight;
  if (middle) ul.scrollTop = top - (view - h) / 2;
  else if (top < ul.scrollTop) ul.scrollTop = top;
  else if (top + h > ul.scrollTop + view) ul.scrollTop = top + h - view;
}
/** @param {KeyboardEvent} e @param {Element} ul */
function listKey(e, ul) {
  const h = near(ul, "[data-ui-dates=time]");
  if (!h || e.altKey || e.ctrlKey || e.metaKey) return;
  const items = /** @type {Element[]} */ ([...all(ul, "[role=option]")]);
  const at = Math.max(0, items.findIndex((li) => has(li, "data-active")));
  /** @type {Record<string, number>} */
  const to = { ArrowDown: at + 1, ArrowUp: at - 1, Home: 0, End: items.length - 1, PageDown: at + 8, PageUp: at - 8 };
  if (e.key in to) {
    e.preventDefault();
    activate(ul, to[e.key]);
  } else if (e.key === "Enter" || e.key === " ") {
    e.preventDefault();
    const li = items[at];
    if (li) chooseTime(h, +(attr(li, "data-t") || 0));
  }
}

// ---- popovers, setup, morphs ----

on(document, "beforetoggle", (e) => {
  const pop = /** @type {Element} */ (e.target);
  if (/** @type {ToggleEvent} */ (e).newState !== "open" || !has(pop, "popover")) return;
  const h = near(pop, "[data-ui-dates]");
  if (!h) return;
  const kind = attr(h, "data-ui-dates");
  if (kind === "time") fillList(h);
  else if (kind === "picker" || kind === "range") openCalendar(h);
}, true);
on(document, "toggle", (e) => {
  const pop = /** @type {Element} */ (e.target);
  const h = has(pop, "popover") && near(pop, "[data-ui-dates]");
  if (!h) return;
  const kind = attr(h, "data-ui-dates"), open = /** @type {ToggleEvent} */ (e).newState === "open";
  if (kind === "time") {
    const ul = /** @type {HTMLElement | null} */ (q(h, "[data-slot=time-list]"));
    const current = ul && q(ul, "[data-active]");
    if (open && ul) {
      ul.focus({ preventScroll: true });
      // The list had no size when it was filled: now it is shown, bring the value into view.
      if (current) reveal(ul, /** @type {HTMLElement} */ (current), true);
    }
    return;
  }
  const cal = calOf(h);
  if (!cal) return;
  if (open) {
    const st = states.get(cal);
    if (st) focusDay(cal, st.focus);
  } else if (kind === "range") abandon(cal);
}, true);

/** @type {WeakMap<Element, MutationObserver>} */
const observers = new WeakMap();
/**
 * The hidden field carries no limits of its own while the box stands before it: a stored value
 * that is now out of bounds, or a step counted from another start, would stop the form with no
 * word said, and the box is where a person is told. The server checks the value either way.
 * @param {Element} n
 */
function unlimit(n) {
  for (const name of ["min", "max", "step"]) unset(n, name);
}
/** Sets up one picker: the hidden field stands behind the box. @param {Element} h */
function enhance(h) {
  const kind = attr(h, "data-ui-dates");
  if (kind === "range") {
    for (const n of /** @type {any} */ (all(h, "[data-ui-native]"))) {
      n.required = false;
      unlimit(n);
    }
    return;
  }
  const n = nativeOf(h), t = textOf(h);
  if (!n) return;
  set(n, "aria-hidden", "true");
  set(n, "tabindex", "-1");
  // The box is the field a person fills in, so it holds the requirement: the hidden one can't show a message.
  if (n.required && t) t.required = true;
  n.required = false;
  unlimit(n);
  const L = locOf(h);
  if (kind === "time") {
    const open = /** @type {HTMLElement | null} */ (q(h, "[data-ui-act=open]"));
    if (open) open.hidden = 1440 / trules(h).st > 288;
  }
  if (!t) return;
  if (!t.id) t.id = `${n.id || h.id}-text`;
  for (const label of [...(n.labels || [])]) label.htmlFor = t.id;
  mirror(h);
  if (!observers.has(h)) {
    const o = new MutationObserver(() => mirror(h));
    o.observe(n, { attributes: true, attributeFilter: ["aria-invalid", "aria-describedby"] });
    observers.set(h, o);
  }
  if (L && activeElement() !== t && !has(h, "data-ui-bad")) {
    if (kind === "time") {
      const v = nativeTime(n.value);
      if (v !== null) t.value = fmtTime(L, v, has(h, "data-seconds"));
    } else {
      const z = fromIso(n.value);
      if (z !== null) t.value = fmt(L, z);
    }
    // A value the page was drawn with that its rules now refuse (stored before they changed):
    // say so at the box, where a person looks, unless the page has already said so.
    if (!has(h, "data-ui-bad") && attr(n, "aria-invalid") !== "true") {
      if (kind === "time") {
        const v = nativeTime(n.value);
        if (v !== null && timeBlocked(trules(h), v)) bad(h, L.L[19]);
      } else {
        const z = fromIso(n.value), cal = calOf(h), c = cal && cfg(cal);
        if (z !== null && c && blocked(c.R, z)) bad(h, L.L[17]);
      }
    }
  }
}

/** @type {WeakMap<Element, string>} */
const sigs = new WeakMap();
/** What a calendar is drawn from: when the page changes any of it, the months are drawn again. */
const SIGNED = ["data-ui-rules", "data-ui-loc", "data-today", "data-months", "data-mode", "data-week-start", "data-caption", "data-week-numbers", "data-fixed-weeks", "data-no-outside", "data-readonly", "data-disabled", "data-min-days", "data-max-days", "data-max-count"];
/** After a morph: pickers and calendars take what the page now says. */
function sync() {
  for (const h of /** @type {any} */ (every("[data-ui-dates=picker],[data-ui-dates=range],[data-ui-dates=time]"))) enhance(h);
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
  for (const h of /** @type {any} */ (all(form, "[data-ui-dates=picker]"))) {
    good(h);
    const t = textOf(h);
    if (t) t.value = "";
    enhance(h);
    hostChanged(h, "sync");
  }
  for (const h of /** @type {any} */ (all(form, "[data-ui-dates=time]"))) {
    const t = textOf(h), n = nativeOf(h);
    good(h);
    if (t) t.value = "";
    if (n) setTime(h, nativeTime(n.value));
  }
  for (const h of /** @type {any} */ (all(form, "[data-ui-dates=range]"))) {
    const cal = calOf(h);
    if (cal) states.delete(cal);
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
