// Phone numbers: a searchable country list built from the server's select,
// formatting as people type, a pasted international number choosing its
// country, and the number in E.164 in a hidden field. The server re-reads
// what was typed in the chosen country and is the authority; this module only
// helps people type. The numbering plans load on first use, once.
// Everything it reads from the document goes through dom.js, so named
// elements in a page (`<img name="createTextNode">`) can't change what it does.
import { active, all, attr, every, has, make, on, root as documentRoot, set, textNode, unset } from "./dom.js";

/**
 * A territory of the plans: its id, calling code, international and national
 * prefixes, the pattern stripping the national prefix and its rewrite, its
 * leading digits, general pattern, line pattern, lengths, example number,
 * formats and international formats.
 * @typedef {{id: string, cc: number, idd?: string, np?: string, npp?: string, npt?: string, ld?: string, gd?: string, tp?: string, len?: number[], ex?: string, f?: any[][], i?: any[][]}} Territory
 */

/** The plans, beside this module (the build writes their hashed name). */
// @ts-ignore: defined by build.mjs
const DATA = new URL(PHONE_DATA_FILE, import.meta.url).href;

/** @type {Map<string, Territory>} */
const byId = new Map();
/** Calling code to territories, the one with the formats first. @type {Map<number, Territory[]>} */
const byCode = new Map();
/** @type {Promise<void> | null} */
let loading = null;

function load() {
  loading ||= fetch(DATA, { credentials: "same-origin" })
    .then((r) => (r.ok ? r.json() : null))
    .then((data) => {
      for (const t of data?.t || []) {
        byId.set(t.id, t);
        const list = byCode.get(t.cc) || [];
        if (t.f) list.unshift(t);
        else list.push(t);
        byCode.set(t.cc, list);
      }
    })
    .catch(() => {});
  return loading;
}

/** @type {Map<string, RegExp>} */
const cache = new Map();
/** A pattern of the plans, anchored at the start, or at both ends. @param {string} source @param {boolean} [full] */
function re(source, full) {
  const key = (full ? "$" : "^") + source;
  let found = cache.get(key);
  if (!found) cache.set(key, (found = new RegExp(`^(?:${source})${full ? "$" : ""}`, "d")));
  return found;
}

const ND = /\p{Nd}/u;
/**
 * The value of a decimal digit of any script, or -1. Unicode writes each
 * script's digits as one run of ten from zero, and runs that touch (the
 * mathematical digits) are whole runs, so the value is the distance from the
 * run's start, modulo ten.
 * @param {string} c
 */
function digit(c) {
  const code = /** @type {number} */ (c.codePointAt(0));
  if (code >= 48 && code <= 57) return code - 48;
  if (!ND.test(c)) return -1;
  let start = code;
  while (start > code - 60 && ND.test(String.fromCodePoint(start - 1))) start--;
  return (code - start) % 10;
}

const PUNCTUATION = /[\s\-‐-―−ー－-／().\[\]（）［］~⁓∼～]/;
const PLUS = /^\s*[+＋]/;

/** What was typed: a leading plus, the digits, and whether anything else (an extension, letters) is there. @param {string} text */
function read(text) {
  let digits = "", other = false;
  for (const c of text.replace(PLUS, "")) {
    const d = digit(c);
    if (d >= 0) digits += d;
    else if (!PUNCTUATION.test(c)) other = true;
  }
  return { plus: PLUS.test(text), digits, other };
}

/** The territory whose formats serve calling code `cc`. @param {number} cc */
const mainOf = (cc) => byCode.get(cc)?.[0];

const SLOT = " ";
/** Formats whose text is groups and punctuation only can be filled as digits arrive. @param {string} format */
const eligible = (format) => !/[\p{L}\p{N}]/u.test(format.replace(/\$\d/g, ""));

/** The format's pattern with every digit class as `\d`, to measure its groups on nines. @param {string} p */
function nines(p) {
  let out = "";
  for (let i = 0; i < p.length; i++) {
    const c = p[i];
    if (c === "{") {
      const end = p.indexOf("}", i);
      out += p.slice(i, end + 1);
      i = end;
    } else if (c === "[") {
      i = p.indexOf("]", i);
      out += "\\d";
    } else if (c === "\\") {
      out += p.slice(i, i + 2);
      i++;
    } else out += c >= "0" && c <= "9" ? "\\d" : c;
  }
  return out;
}

/** `text` with each `$n` as many slots as group `n` takes on the longest number. @param {string} pattern @param {string} text */
function template(pattern, text) {
  const m = new RegExp(nines(pattern)).exec("99999999999999999");
  return m ? text.replace(/\$(\d)/g, (_, n) => SLOT.repeat((m[+n] || "").length)) : "";
}

/** The digits written into the template's slots, up to the last digit. @param {string} t @param {string} digits */
function fill(t, digits) {
  let out = "", at = 0;
  for (const c of t) {
    if (c === SLOT) {
      if (at === digits.length) break;
      out += digits[at++];
    } else if (at === digits.length && at > 0) break;
    else out += c;
  }
  return out;
}

/**
 * A national number written the way its plan writes it, so far as typed: by
 * the first format whose leading digits fit and that holds this many digits.
 * `prefix` is the national prefix typed before it, if any.
 * @param {Territory} main @param {string} nsn @param {boolean} intl @param {string} prefix
 */
function writeNsn(main, nsn, intl, prefix) {
  if (nsn.length < 3) return prefix + nsn;
  for (const [pattern, format, ld, national, optional] of (intl && main.i) || main.f || []) {
    if (!eligible(format)) continue;
    // With a national prefix typed, skip formats for numbers dialled without one.
    if (!intl && prefix && !national && !optional) continue;
    if (ld?.length && !re(ld[Math.min(nsn.length - 3, ld.length - 1)]).test(nsn)) continue;
    const withPrefix = !intl && prefix && national && prefix === main.np;
    const text = withPrefix ? national : format;
    // A whole number takes its exact groups; a partial one fills a template.
    if (re(pattern, true).test(nsn)) {
      const written = nsn.replace(re(pattern, true), text);
      if (withPrefix) return written;
      return prefix ? prefix + (/[- ]/.test(national || "") ? " " : "") + written : written;
    }
    const t = template(pattern, text);
    if (t.split(SLOT).length - 1 < nsn.length) continue;
    const written = fill(t, nsn);
    if (withPrefix) return written;
    return prefix ? prefix + (/[- ]/.test(national || "") ? " " : "") + written : written;
  }
  return prefix + nsn;
}

/** A whole national number as its country writes it, as the server does. @param {Territory} t @param {string} nsn */
function national(t, nsn) {
  const main = mainOf(t.cc) || t;
  for (const [pattern, format, ld, withPrefix] of main.f || []) {
    if ((!ld?.length || re(ld[ld.length - 1]).test(nsn)) && re(pattern, true).test(nsn)) {
      return nsn.replace(re(pattern, true), withPrefix || format);
    }
  }
  return nsn;
}

/**
 * The national significant number of `digits` dialled in `t`: its national
 * prefix stripped (or rewritten) as the server does, unless that turns a
 * number that fits the plan into one that doesn't.
 * @param {Territory} t @param {string} digits
 */
function significant(t, digits) {
  const m = t.npp && re(t.npp).exec(digits);
  if (!m) return digits;
  const fits = (/** @type {string} */ n) => !t.gd || re(t.gd, true).test(n);
  const last = m[m.length - 1];
  const next = !t.npt || last === undefined
    ? digits.slice(m[0].length)
    : t.npt.replace(/\$(\d)/g, (_, g) => m[+g] || "") + digits.slice(m[0].length);
  if (fits(digits) && !fits(next)) return digits;
  const lengths = t.len || [];
  return lengths.includes(next.length) || next.length > (lengths.at(-1) || 0) ? next : digits;
}

/**
 * The territory a number of code `cc` belongs to, in the server's order (the
 * one the code's formats belong to first): the first whose leading digits it
 * starts with, or whose lines it fits. While it fits none (not whole yet), the
 * chosen `current` stays if it shares the code.
 * @param {number} cc @param {string} nsn @param {Territory | undefined} current
 */
function territoryOf(cc, nsn, current) {
  const list = byCode.get(cc) || [];
  if (list.length < 2) return list[0];
  for (const t of list) if (t.ld ? re(t.ld).test(nsn) : t.tp && re(t.tp, true).test(nsn)) return t;
  return current?.cc === cc ? current : list[0];
}

/** A calling code at the start of `digits`, and the rest. @param {string} digits @returns {[number, string] | null} */
function splitCode(digits) {
  if (digits[0] === "0") return null;
  for (let n = 1; n <= 3 && n <= digits.length; n++) {
    const cc = +digits.slice(0, n);
    if (byCode.has(cc)) return [cc, digits.slice(n)];
  }
  return null;
}

/**
 * What the box shows, the number in E.164 (empty until it has a possible
 * length) and the territory a number with its calling code belongs to.
 * @param {string} text @param {Territory | undefined} region
 * @returns {{text: string, e164: string, found?: Territory}}
 */
export function format(text, region) {
  const { plus, digits, other } = read(text);
  if (other || (!plus && !region)) return { text, e164: "" };
  /** @param {string} rest @param {string} lead */
  const international = (rest, lead) => {
    const split = splitCode(rest);
    if (!split) return { text: lead + rest, e164: "" };
    const [cc, nsn] = split;
    const main = /** @type {Territory} */ (mainOf(cc));
    const found = territoryOf(cc, nsn, region);
    const n = significant(main, nsn);
    const complete = (found?.len || main.len || []).includes(n.length);
    return {
      text: lead + cc + (nsn ? ` ${writeNsn(main, nsn, true, "")}` : ""),
      e164: complete ? `+${cc}${n}` : "",
      found,
    };
  };
  if (plus) return international(digits, "+");
  const t = /** @type {Territory} */ (region);
  const idd = t.idd && re(t.idd).exec(digits);
  if (idd && idd[0] && digits.length > idd[0].length && digits[idd[0].length] !== "0") {
    // Written with `+`: the number then reads the same in any country chosen.
    return international(digits.slice(idd[0].length), "+");
  }
  const main = mainOf(t.cc) || t;
  let prefix = "";
  if (t.cc === 1 && digits[0] === "1" && digits[1] !== "0" && digits[1] !== "1") prefix = "1";
  else if (t.npp) {
    const m = re(t.npp).exec(digits);
    // Only what comes before a captured group is a typed prefix.
    const end = m ? (m.indices?.[1]?.[0] ?? m[0].length) : 0;
    if (m && end > 0 && end < digits.length) prefix = digits.slice(0, end);
  }
  const nsn = digits.slice(prefix.length);
  const n = significant(t, digits);
  return {
    // A North American number typed with its 1 takes the international groups.
    text: prefix === "1" ? `1 ${writeNsn(main, nsn, true, "")}` : writeNsn(main, nsn, false, prefix),
    e164: (t.len || []).includes(n.length) ? `+${t.cc}${n}` : "",
  };
}

// ---------------------------------------------------------------------------
// The field
// ---------------------------------------------------------------------------

/** @param {Element} root */
const parts = (root) => ({
  select: /** @type {HTMLSelectElement | null} */ (root.querySelector("[data-slot=phone-country]")),
  trigger: /** @type {HTMLButtonElement | null} */ (root.querySelector("[data-slot=phone-country-trigger]")),
  list: /** @type {HTMLElement | null} */ (root.querySelector("[data-slot=phone-countries]")),
  input: /** @type {HTMLInputElement | null} */ (root.querySelector("input[type=tel]")),
  e164: /** @type {HTMLInputElement | null} */ (root.querySelector("[data-slot=phone-e164]")),
  status: root.querySelector("[data-slot=phone-status]"),
});

/** The flag of a region: the page's own drawing if it has one, else its emoji. @param {Element} root @param {string} id */
function flag(root, id) {
  const own = /** @type {HTMLTemplateElement | null} */ (root.querySelector("template[data-slot=phone-flags]"))
    ?.content.querySelector(`[data-flag="${CSS.escape(id)}"]`);
  if (own) return own.cloneNode(true);
  return textNode(id.length === 2 ? String.fromCodePoint(...[...id].map((c) => 0x1f1e6 + c.charCodeAt(0) - 65)) : "");
}

/** @param {Element} root */
function enhance(root) {
  if (has(root, "data-enhanced")) return;
  const { e164 } = parts(root);
  set(root, "data-enhanced", "");
  if (e164 && !has(root, "data-disabled")) e164.disabled = false;
}

/** @param {Element} root @param {string} text */
function announce(root, text) {
  const { status } = parts(root);
  if (!status) return;
  status.textContent = "";
  setTimeout(() => { status.textContent = text; }, 50);
}

/** The option of the select for `id`. @param {HTMLSelectElement} select @param {string} id */
const optionOf = (select, id) => [...select.options].find((o) => o.value === id);

/** Shows `id` as the chosen country, on the select and the trigger. @param {Element} root @param {string} id */
function choose(root, id) {
  const { select, trigger, input } = parts(root);
  if (!select || !trigger) return;
  select.value = id;
  const option = optionOf(select, id);
  const t = byId.get(id);
  const [flagBox, dial] = all(trigger, "[data-slot=phone-flag],[data-slot=phone-dial]");
  flagBox?.replaceChildren(flag(root, id));
  if (dial) dial.textContent = t ? `+${t.cc}` : "";
  if (option) set(trigger, "aria-label", `${attr(select, "aria-label") || ""}: ${option.text}`);
  const list = parts(root).list;
  if (list) for (const row of all(list, "[role=option]")) set(row, "aria-selected", String(attr(row, "data-value") === id));
  if (input && t?.ex && !has(input, "data-own-placeholder")) input.placeholder = national(t, t.ex);
  select.dispatchEvent(new Event("change", { bubbles: true }));
}

/**
 * Rewrites the box from what it holds, keeping the caret after the same digit.
 * With `detect` (only when someone types), a number with its calling code
 * chooses its own country; a country picked by hand is never overruled.
 * @param {Element} root @param {boolean} [detect]
 */
function reformat(root, detect = false) {
  const { select, input, e164 } = parts(root);
  if (!input || !select) return;
  const region = byId.get(select.value);
  const before = input.value;
  const caret = input.selectionStart ?? before.length;
  const result = format(before, region);
  if (detect && result.found && result.found.id !== select.value && optionOf(select, result.found.id)) {
    choose(root, result.found.id);
    const text = attr(/** @type {Element} */ (parts(root).status), "data-label-country-set") || "{country}";
    announce(root, text.replace("{country}", optionOf(select, result.found.id)?.text || result.found.id));
  }
  if (e164) e164.value = result.e164;
  if (result.text === before) return;
  const counted = before.slice(0, caret).replace(/[^+\p{Nd}]/gu, "").length;
  input.value = result.text;
  let at = 0, seen = 0;
  while (at < result.text.length && seen < counted) {
    if (/[+\d]/.test(result.text[at])) seen++;
    at++;
  }
  if (active() === input) input.setSelectionRange(at, at);
}

/** The list of countries, built from the select the first time it opens. @param {Element} root */
function build(root) {
  const { select, list } = parts(root);
  if (!select || !list || list.childElementCount) return;
  const base = list.id;
  const search = make("input");
  search.type = "search";
  set(search, "data-slot", "phone-search");
  set(search, "role", "combobox");
  set(search, "aria-autocomplete", "list");
  set(search, "aria-expanded", "true");
  set(search, "aria-controls", `${base}-list`);
  set(search, "autocomplete", "off");
  search.placeholder = attr(list, "data-label-search") || "";
  set(search, "aria-label", search.placeholder);
  const box = make("ul");
  box.id = `${base}-list`;
  set(box, "role", "listbox");
  set(box, "data-slot", "phone-options");
  set(box, "aria-label", attr(select, "aria-label") || "");
  const seen = new Set();
  for (const option of select.options) {
    if (!option.value || seen.has(option.value)) continue;
    seen.add(option.value);
    const t = byId.get(option.value);
    const item = make("li");
    item.id = `${base}-${option.value}`;
    set(item, "role", "option");
    set(item, "data-value", option.value);
    set(item, "aria-selected", String(option.selected));
    const name = option.text.replace(/\s\+\d+$/, "");
    const code = option.text.slice(name.length).trim();
    const flagBox = make("span");
    set(flagBox, "data-slot", "phone-flag");
    set(flagBox, "aria-hidden", "true");
    flagBox.append(flag(root, option.value));
    const label = make("span");
    set(label, "data-slot", "phone-option-name");
    label.textContent = name;
    const dial = make("span");
    set(dial, "data-slot", "phone-option-code");
    // Dial codes read left to right in any page.
    const digits = make("bdi");
    set(digits, "dir", "ltr");
    digits.textContent = code || (t ? `+${t.cc}` : "");
    dial.append(digits);
    item.append(flagBox, label, dial);
    set(item, "data-search", `${fold(name)} ${option.value.toLowerCase()} ${code.replace("+", "")}`);
    box.append(item);
  }
  const empty = make("p");
  set(empty, "data-slot", "phone-empty");
  empty.hidden = true;
  empty.textContent = attr(list, "data-label-empty") || "";
  const searchBox = make("div");
  set(searchBox, "data-slot", "phone-search-box");
  searchBox.append(search);
  list.append(searchBox, box, empty);
}

/** Lower case, without accents, for search. @param {string} text */
const fold = (text) => text.normalize("NFD").replace(/\p{M}/gu, "").toLowerCase();

/** @param {HTMLElement} list */
const items = (list) => /** @type {HTMLElement[]} */ ([...all(list, "[role=option]")]);
/** @param {HTMLElement} list */
const shown = (list) => items(list).filter((item) => !item.hidden);

/** Marks `item` as the one Enter picks. @param {HTMLElement} list @param {HTMLElement | undefined} item @param {ScrollLogicalPosition} [block] */
function activate(list, item, block = "nearest") {
  const search = list.querySelector("[data-slot=phone-search]");
  for (const other of all(list, "[data-active]")) unset(other, "data-active");
  if (!item || !search) {
    if (search) unset(search, "aria-activedescendant");
    return;
  }
  set(item, "data-active", "");
  set(search, "aria-activedescendant", item.id);
  item.scrollIntoView({ block });
}

/** Shows the countries the search finds. @param {Element} root @param {HTMLElement} list @param {string} query */
function filter(root, list, query) {
  const q = fold(query.trim().replace(/^\+/, ""));
  let count = 0;
  for (const item of items(list)) {
    const hit = !q || (attr(item, "data-search") || "").split(" ").some((word, at, words) => word.startsWith(q) || words.slice(at).join(" ").startsWith(q));
    item.hidden = !hit;
    if (hit) count++;
  }
  const empty = /** @type {HTMLElement | null} */ (list.querySelector("[data-slot=phone-empty]"));
  if (empty) empty.hidden = count > 0;
  activate(list, shown(list)[0]);
  clearTimeout(Number(attr(list, "data-timer")));
  set(list, "data-timer", String(setTimeout(() => {
    const message = count ? (attr(list, "data-label-count") || "{n}").replace("{n}", String(count)) : attr(list, "data-label-empty") || "";
    announce(root, message);
  }, 400)));
}

/** Picks the country of `item`, closes the list and returns to the number. @param {Element} root @param {HTMLElement} item */
function pick(root, item) {
  const { list, input, trigger } = parts(root);
  const id = attr(item, "data-value") || "";
  for (const other of all(/** @type {HTMLElement} */ (list), "[aria-selected=true]")) set(other, "aria-selected", "false");
  set(item, "aria-selected", "true");
  choose(root, id);
  try { list?.hidePopover(); } catch {}
  reformat(root);
  (input || trigger)?.focus();
}

// An event's target may be a form, whose named fields shadow its methods
// (`<input name="closest">`): read them through the prototype.
const EP = Element.prototype;
/** @param {EventTarget | null} el @param {string} selector @returns {Element | null} */
const closest = (el, selector) => (el instanceof Element ? EP.closest.call(el, selector) : null);
/** @param {EventTarget | null} el @param {string} selector */
const matches = (el, selector) => el instanceof Element && EP.matches.call(el, selector);

/** @param {Event} e */
const rootOf = (e) => closest(e.target, "[data-ui-phone]");

function enhanceAll() {
  for (const root of every("[data-ui-phone]:not([data-enhanced])")) enhance(root);
}
enhanceAll();
new MutationObserver((records) => {
  if (records.some((r) => r.addedNodes.length)) enhanceAll();
}).observe(documentRoot(), { childList: true, subtree: true });

// Plans load when someone first reaches for a phone field.
for (const type of ["focusin", "pointerdown"]) {
  on(document, type, (e) => {
    const root = rootOf(e);
    if (root) load().then(() => reformat(root));
  }, { passive: true });
}

on(document, "input", (e) => {
  const root = rootOf(e);
  if (!root) return;
  const target = /** @type {HTMLElement} */ (e.target);
  if (matches(target, "[data-slot=phone-search]")) {
    filter(root, /** @type {HTMLElement} */ (parts(root).list), /** @type {HTMLInputElement} */ (target).value);
  } else if (matches(target, "input[type=tel]")) {
    load().then(() => reformat(root, true));
  }
});

on(document, "change", (e) => {
  const root = rootOf(e);
  if (root && matches(e.target, "[data-slot=phone-country]") && e.isTrusted) {
    load().then(() => {
      choose(root, /** @type {HTMLSelectElement} */ (e.target).value);
      reformat(root);
    });
  }
});

// Backspace just after a space or bracket the formatting added takes the digit
// before it, so the caret never sticks.
on(document, "beforeinput", (e) => {
  const input = /** @type {HTMLInputElement} */ (e.target);
  if (e.inputType !== "deleteContentBackward" || !matches(input, "[data-ui-phone] input[type=tel]")) return;
  const at = input.selectionStart ?? 0;
  if (at !== input.selectionEnd || at === 0 || /[+\p{Nd}]/u.test(input.value[at - 1])) return;
  let from = at - 1;
  while (from > 0 && !/[+\p{Nd}]/u.test(input.value[from - 1])) from--;
  if (from === 0) return;
  e.preventDefault();
  input.setRangeText("", from - 1, at, "end");
  input.dispatchEvent(new Event("input", { bubbles: true }));
});

// The list is built, and aligned to the text's direction, before it opens, so
// it is placed at its full size.
on(document, "beforetoggle", (e) => {
  const list = /** @type {HTMLElement} */ (e.target);
  if (/** @type {ToggleEvent} */ (e).newState !== "open" || !matches(list, "[data-slot=phone-countries]")) return;
  const root = /** @type {Element} */ (closest(list, "[data-ui-phone]"));
  build(root);
  set(list, "data-align", getComputedStyle(root).direction === "rtl" ? "end" : "start");
  const search = /** @type {HTMLInputElement | null} */ (list.querySelector("[data-slot=phone-search]"));
  if (search) {
    search.value = attr(list, "data-typed") || "";
    unset(list, "data-typed");
    filter(root, list, search.value);
    const chosen = /** @type {HTMLElement | null} */ (list.querySelector("[aria-selected=true]:not([hidden])"));
    if (chosen && !search.value) activate(list, chosen, "center");
  }
}, true);

on(document, "toggle", (e) => {
  const list = /** @type {HTMLElement} */ (e.target);
  if (!matches(list, "[data-slot=phone-countries]")) return;
  const root = /** @type {Element} */ (closest(list, "[data-ui-phone]"));
  const { trigger } = parts(root);
  const open = /** @type {ToggleEvent} */ (e).newState === "open";
  if (trigger) set(trigger, "aria-expanded", String(open));
  if (!open) return;
  const search = /** @type {HTMLInputElement | null} */ (list.querySelector("[data-slot=phone-search]"));
  if (search && active() !== search) {
    search.focus();
    // In view now that the list is shown.
    const current = /** @type {HTMLElement | null} */ (list.querySelector("[data-active]"));
    if (current) current.scrollIntoView({ block: search.value ? "nearest" : "center" });
  }
}, true);

on(document, "keydown", (e) => {
  const target = /** @type {HTMLElement} */ (e.target);
  const root = rootOf(e);
  if (!root) return;
  const { list, trigger } = parts(root);
  if (!list) return;
  // A letter or an arrow on the closed trigger opens the list, searching.
  if (target === trigger && !list.matches(":popover-open")) {
    if (e.key === "ArrowDown" || e.key === "ArrowUp" || (e.key.length === 1 && /\S/.test(e.key) && !e.ctrlKey && !e.metaKey && !e.altKey)) {
      e.preventDefault();
      if (e.key.length === 1) set(list, "data-typed", e.key);
      try { list.showPopover(); } catch {}
    }
    return;
  }
  if (!matches(target, "[data-slot=phone-search]")) return;
  const visible = shown(list);
  const current = /** @type {HTMLElement | null} */ (list.querySelector("[data-active]"));
  const at = current ? visible.indexOf(current) : -1;
  const move = (/** @type {number} */ to) => {
    e.preventDefault();
    activate(list, visible[Math.max(0, Math.min(visible.length - 1, to))]);
  };
  if (e.key === "ArrowDown") move(at + 1);
  else if (e.key === "ArrowUp") move(at - 1);
  else if (e.key === "PageDown") move(at + 8);
  else if (e.key === "PageUp") move(at - 8);
  else if ((e.key === "Home" || e.key === "End") && e.ctrlKey) move(e.key === "Home" ? 0 : visible.length - 1);
  else if (e.key === "Enter") {
    e.preventDefault();
    if (current) pick(root, current);
  } else if (e.key === "Escape") {
    e.preventDefault();
    try { list.hidePopover(); } catch {}
    trigger?.focus();
  } else if (e.key === "Tab") {
    try { list.hidePopover(); } catch {}
  }
});

on(document, "click", (e) => {
  const item = /** @type {HTMLElement | null} */ (closest(e.target, "[data-ui-phone] [role=option]"));
  const root = item && rootOf(e);
  if (item && root) pick(root, item);
});

on(document, "pointermove", (e) => {
  const item = /** @type {HTMLElement | null} */ (closest(e.target, "[data-ui-phone] [role=option]"));
  if (item && !has(item, "data-active")) activate(/** @type {HTMLElement} */ (closest(item, "[data-slot=phone-countries]")), item);
}, { passive: true });

// A rejected form shows its errors on the number, the control people use.
on(document, "focusin", (e) => {
  if (matches(e.target, "[data-enhanced] [data-slot=phone-country]")) parts(/** @type {Element} */ (closest(e.target, "[data-ui-phone]"))).trigger?.focus();
});
