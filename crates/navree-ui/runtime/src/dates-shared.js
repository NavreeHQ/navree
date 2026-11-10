// What the dates modules share: day numbers, the locale and the rules the server wrote, the
// digits and letters a person may type, and the box that stands before a picker's hidden
// native field. Each module carries what it uses of this file; nothing here runs on its own.
import { E, active, attr, has, make, set, unset } from "./dom.js";

/** @param {Element} el @param {string} sel @returns {any} */
export const q = (el, sel) => E.querySelector.call(el, sel);
/** @param {Element} el @param {string} sel @returns {any} */
export const near = (el, sel) => E.closest.call(el, sel);

// ---- days: civil dates as day numbers ----
//
// A day here is a number: days counted from 1970-01-01 by integer arithmetic, so the
// browser's zone, its clock and `Date` never touch one. "Today" is what the server wrote
// into the page.

/** @param {number} y @param {number} m @param {number} d */
export function days(y, m, d) {
  y -= m <= 2 ? 1 : 0;
  const era = Math.floor(y / 400), yoe = y - era * 400;
  const doy = Math.floor((153 * (m + (m > 2 ? -3 : 9)) + 2) / 5) + d - 1;
  return era * 146097 + yoe * 365 + Math.floor(yoe / 4) - Math.floor(yoe / 100) + doy - 719468;
}
/** @param {number} z @returns {number[]} */
export function civil(z) {
  z += 719468;
  const era = Math.floor(z / 146097), doe = z - era * 146097;
  const yoe = Math.floor((doe - Math.floor(doe / 1460) + Math.floor(doe / 36524) - Math.floor(doe / 146096)) / 365);
  const doy = doe - (365 * yoe + Math.floor(yoe / 4) - Math.floor(yoe / 100));
  const mp = Math.floor((5 * doy + 2) / 153);
  const m = mp < 10 ? mp + 3 : mp - 9;
  return [yoe + era * 400 + (m <= 2 ? 1 : 0), m, doy - Math.floor((153 * mp + 2) / 5) + 1];
}
/** @param {number} y @param {number} m */
export const dim = (y, m) => days(m === 12 ? y + 1 : y, m === 12 ? 1 : m + 1, 1) - days(y, m, 1);
/** Weekday, 0 for Sunday. @param {number} z */
export const dow = (z) => (((z + 4) % 7) + 7) % 7;
/** @param {number} n */
export const p2 = (n) => String(n).padStart(2, "0");
/** @param {number} y @param {number} m */
export const ym = (y, m) => `${String(y).padStart(4, "0")}-${p2(m)}`;
/** @param {number} z */
export const iso = (z) => {
  const [y, m, d] = civil(z);
  return `${ym(y, m)}-${p2(d)}`;
};
export const FIRST = /* @__PURE__ */ days(1, 1, 1), LAST = /* @__PURE__ */ days(9999, 12, 31);
/** @param {string | null | undefined} text @returns {number | null} */
export function fromIso(text) {
  const k = /^(\d{4})-(\d{2})-(\d{2})$/.exec(text || "");
  if (!k) return null;
  const y = +k[1], m = +k[2], d = +k[3];
  return y < 1 || m < 1 || m > 12 || d < 1 || d > dim(y, m) ? null : days(y, m, d);
}
/** A month as a number, for comparing and counting. @param {number} y @param {number} m */
export const mIndex = (y, m) => y * 12 + m - 1;
/** @param {string} text */
export const esc = (text) => String(text).replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);

// ---- the locale and the rules the server wrote ----

/** @type {Map<string, any>} */
const locales = new Map();
/** The locale written in `el` or below it, or null when it isn't a whole one. @param {Element} el */
export function locOf(el) {
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
/** The date as typed here. @param {any} L @param {number} z */
export function fmt(L, z) {
  const [y, m, d] = civil(z);
  const Y = String(y).padStart(4, "0"), M = p2(m), D = p2(d);
  const parts = L.o === "mdy" ? [M, D, Y] : L.o === "dmy" ? [D, M, Y] : [Y, M, D];
  return parts.join(L.s) + L.x;
}

/** @type {Map<string, any>} */
const ruleCache = new Map();
/** The rules of a calendar. @param {Element} el the calendar root */
export function rulesOf(el) {
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
export const blocked = (R, z) => z < FIRST || z > LAST || (R.min !== null && z < R.min) || (R.max !== null && z > R.max) || R.wd.has(dow(z)) || R.runs.some((/** @type {number[]} */ r) => z >= r[0] && z <= r[1]);

// ---- what a person types: the same digits and letters as the server's reader ----

const ZEROS = [0x660, 0x6f0, 0x7c0, 0x966, 0x9e6, 0xa66, 0xae6, 0xb66, 0xbe6, 0xc66, 0xce6, 0xd66, 0xe50, 0xed0, 0xf20, 0x1040, 0xff10];
/** A digit of any script as its value, or -1. @param {string} c */
export function digit(c) {
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
// Built with `new`, marked pure, so a module that reads nothing typed doesn't carry them.
const ALNUM = /* @__PURE__ */ new RegExp("[\\p{Alphabetic}\\p{N}]", "u");
export const ALPHA = /* @__PURE__ */ new RegExp("\\p{Alphabetic}", "u");
/** Lower case, letters and digits only, marks taken off. @param {string} text */
export const fold = (text) => [...text.toLowerCase()].filter((c) => ALNUM.test(c)).map(plain).join("");

// ---- the box a person types in, before the field that posts ----

/** @param {Element} h */
export const nativeOf = (h) => /** @type {HTMLInputElement | null} */ (q(h, "[data-ui-native]"));
/** @param {Element} h */
export const textOf = (h) => /** @type {HTMLInputElement | null} */ (q(h, "[data-ui-text]"));
/** @param {Element} h */
export function closePop(h) {
  try { /** @type {HTMLElement | null} */ (q(h, "[popover]"))?.hidePopover(); } catch {}
}
/**
 * Puts `v` in a field the form posts. WebKit keeps an emptied date or time field's old validity until it is asked
 * again, and then won't submit its form, for a reason it can't show on a hidden field: so it is asked again.
 * @param {HTMLInputElement | undefined | null} el @param {string} v
 */
export function put(el, v) {
  if (!el) return;
  el.value = v;
  el.setCustomValidity("");
}
/** The page hears one `input` and one `change`, as for any field. @param {Element | undefined | null} el */
export function tell(el) {
  if (!el) return;
  el.dispatchEvent(new Event("input", { bubbles: true }));
  el.dispatchEvent(new Event("change", { bubbles: true }));
}
/** @param {Element} n */
export function unlimit(n) {
  for (const name of ["min", "max", "step"]) unset(n, name);
}

/** Says a typed value is wrong. @param {Element} h @param {string} message */
export function bad(h, message) {
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
export function good(h) {
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

/** @type {WeakMap<Element, MutationObserver>} */
const observers = new WeakMap();
/**
 * Puts the box before the hidden field: the field is out of the tab order and out of sight
 * for assistive technology, its label and requirement move to the box, and the marks the
 * page puts on it are copied there. The hidden field carries no limits of its own while
 * the box stands before it: a stored value that is now out of bounds, or a step counted
 * from another start, would stop the form with no word said, and the box is where a person
 * is told. The server checks the value either way.
 * Returns the box when it may be written now: it has a locale, has no focus and says nothing wrong.
 * @param {Element} h @returns {HTMLInputElement | null}
 */
export function stand(h) {
  const n = nativeOf(h), t = textOf(h);
  if (!n) return null;
  set(n, "aria-hidden", "true");
  set(n, "tabindex", "-1");
  // The box is the field a person fills in, so it holds the requirement: the hidden one can't show a message.
  if (n.required && t) t.required = true;
  n.required = false;
  unlimit(n);
  if (!t) return null;
  if (!t.id) t.id = `${n.id || h.id}-text`;
  for (const label of [...(n.labels || [])]) label.htmlFor = t.id;
  mirror(h);
  if (!observers.has(h)) {
    const o = new MutationObserver(() => mirror(h));
    o.observe(n, { attributes: true, attributeFilter: ["aria-invalid", "aria-describedby"] });
    observers.set(h, o);
  }
  return locOf(h) && active() !== t && !has(h, "data-ui-bad") ? t : null;
}
