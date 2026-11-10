// Time pickers: a box to type a time of day in, a list of times in a popover, and the
// native time field behind them that posts. Everything is delegated from `document`, so
// a morph or a swap needs no re-wiring. A time here is seconds since midnight on a wall
// clock: no date, no zone. The server owns the rules and checks every value that comes
// back; everything below only helps a person get to a value, and says so when it can't.
import { all, attr, every, has, on, set, unset } from "./dom.js";
import { ALPHA, bad, closePop, days, digit, esc, fmt, fold, good, locOf, nativeOf, near, p2, put, q, stand, tell, textOf, unlimit } from "./dates-shared.js";

const PICKER = "[data-ui-dates=time]", BOX = `${PICKER} [data-ui-text]`;

/** A time of day as typed here. @param {any} L @param {number} t seconds since midnight @param {boolean} secs */
function fmtTime(L, t, secs) {
  const h = Math.floor(t / 3600), mi = Math.floor((t % 3600) / 60), s = t % 60;
  const tail = secs ? `${p2(mi)}:${p2(s)}` : p2(mi);
  if (L.c === 24) return `${p2(h)}:${tail}`;
  const shown = ((h + 11) % 12) + 1, mark = h < 12 ? L.am : L.pm;
  return L.mf ? `${mark}${shown}:${tail}` : `${shown}:${tail}${mark}`;
}

/**
 * Reads a typed time into seconds since midnight: the same rules as the server's reader.
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
  const split = (/** @type {string} */ s, /** @type {number[]} */ sizes) => { let at = 0; return sizes.map((n) => { const p = s.slice(at, at + n); at += n; return p; }); };
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

/** The rules the server wrote. @param {Element} h */
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
  if (n.value !== v) { put(n, v); tell(n); }
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
    put(n, "");
    bad(h, L.L[18].replace("{example}", fmtTime(L, 14 * 3600 + 30 * 60, false)));
  } else if (timeBlocked(trules(h), r.t)) {
    put(n, "");
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
  // Each value is a number of seconds (m * 60), or text escaped by esc(): the list's id and the
  // time as the locale writes it.
  // nosemgrep: navree-raw-html
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

on(document, "keydown", (e) => {
  const t = /** @type {Element} */ (e.target), h = t.closest?.(PICKER);
  if (!h) return;
  if (has(t, "data-ui-text") && !e.isComposing) {
    if (e.key === "Enter") textTime(h);
    else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      const open = /** @type {HTMLElement | null} */ (q(h, "[data-ui-act=open]"));
      if (!open || open.hidden) {
        e.preventDefault();
        stepTime(h, e.key === "ArrowDown" ? -1 : 1);
      } else if (e.key === "ArrowDown" && !/** @type {any} */ (open).disabled) {
        e.preventDefault();
        try { /** @type {HTMLElement | null} */ (q(h, "[popover]"))?.showPopover(); } catch {}
      }
    }
  }
  const ul = t.closest("[data-slot=time-list]");
  if (!ul || e.altKey || e.ctrlKey || e.metaKey) return;
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
});

on(document, "click", (e) => {
  const t = /** @type {Element} */ (e.target), h = t.closest?.(PICKER);
  if (!h) return;
  if (t.closest("[data-ui-act=clear]")) {
    setTime(h, null);
    textOf(h)?.focus();
  }
  const opt = t.closest("[data-slot=time-list] [role=option]");
  if (opt) chooseTime(h, +(attr(opt, "data-t") || 0));
});
// A box that was changed and left. What is typed belongs to the box until it is a time: the page hears the field.
on(document, "change", (e) => {
  const t = /** @type {Element} */ (e.target), h = t.closest?.(PICKER);
  if (h && t.closest(BOX)) {
    e.stopPropagation();
    textTime(h);
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

on(document, "beforetoggle", (e) => {
  const pop = /** @type {Element} */ (e.target), h = near(pop, PICKER);
  if (h && has(pop, "popover") && /** @type {ToggleEvent} */ (e).newState === "open") fillList(h);
}, true);
on(document, "toggle", (e) => {
  const pop = /** @type {Element} */ (e.target), h = near(pop, PICKER);
  const ul = h && has(pop, "popover") && /** @type {ToggleEvent} */ (e).newState === "open" && /** @type {HTMLElement | null} */ (q(h, "[data-slot=time-list]"));
  if (!ul) return;
  ul.focus({ preventScroll: true });
  // The list had no size when it was filled: now it is shown, bring the value into view.
  const current = q(ul, "[data-active]");
  if (current) reveal(ul, current, true);
}, true);

/** Sets up one picker: the hidden field stands behind the box. @param {Element} h */
function enhance(h) {
  const open = /** @type {HTMLElement | null} */ (q(h, "[data-ui-act=open]"));
  if (open) open.hidden = 1440 / trules(h).st > 288;
  const t = stand(h), n = nativeOf(h), L = locOf(h);
  const v = n && nativeTime(n.value);
  if (!t || !n || v == null) return;
  t.value = fmtTime(L, v, has(h, "data-seconds"));
  // A value the page was drawn with that its rules now refuse (stored before they changed):
  // say so at the box, where a person looks, unless the page has already said so.
  if (attr(n, "aria-invalid") !== "true" && timeBlocked(trules(h), v)) bad(h, L.L[19]);
}

// ---- a date and a time together ----
//
// A joint control is a date picker and a time picker side by side, and one native
// date-and-time field that posts. The halves' own fields post nothing: whenever one of
// them changes, the joint field takes the date and the time together (or nothing while a
// half is empty), and the page hears the joint field alone.

const JOINT = "[data-ui-dates=datetime]";
/** The joint field, the date half and the time half. @param {Element} j @returns {any[]} */
const halves = (j) => [q(j, "[data-ui-joint]"), q(j, "[data-ui-dates=picker]"), q(j, PICKER)];
/**
 * Marks the box of an empty half whose other half is filled, so the form is not sent with
 * half a value: the browser says why when the form is sent. A box that already says what is
 * wrong with what was typed in it keeps that.
 * @param {Element} h the half @param {any} wanted the message, or nothing
 */
function need(h, wanted) {
  const box = textOf(h);
  if (box && !has(h, "data-ui-bad")) box.setCustomValidity(wanted || "");
}
/** The joint field takes both halves. @param {Element} j @param {boolean} [quiet] say nothing to the page */
function join(j, quiet) {
  const [out, dp, tp] = halves(j);
  const L = out && dp && tp && locOf(tp), d = L && nativeOf(dp)?.value, t = L && nativeOf(tp)?.value;
  if (!L) return;
  const was = out.value;
  put(out, d && t ? `${d}T${t}` : "");
  if (!quiet && out.value !== was) tell(out);
  // The examples are the ones the halves give for text they can't read.
  need(tp, d && !t && L.L[18].replace("{example}", fmtTime(L, 52200, false)));
  need(dp, t && !d && L.L[16].replace("{example}", fmt(L, days(2026, 11, 30))));
}
// What the halves' fields say is the joint field's to say.
for (const type of ["input", "change"]) {
  on(document, type, (e) => {
    const t = /** @type {Element} */ (e.target), j = t.closest?.(`${JOINT} [data-ui-native]`) && near(t, JOINT);
    if (!j) return;
    e.stopPropagation();
    if (type === "change") join(j);
  }, true);
}
/** Sets up one joint control: the joint field stands behind the two boxes. @param {Element} j */
function joint(j) {
  const [out, dp, tp] = halves(j), boxes = [textOf(dp), textOf(tp)];
  if (!out || !boxes[0] || !boxes[1]) return;
  set(out, "aria-hidden", "true");
  set(out, "tabindex", "-1");
  if (out.required) for (const box of boxes) box.required = true;
  out.required = false;
  unlimit(out);
  // The halves carry the joint field's marks, and their boxes show them. Its label names the
  // whole group and points at the date box.
  for (const name of ["aria-invalid", "aria-describedby"]) {
    const v = attr(out, name);
    for (const n of [nativeOf(dp), nativeOf(tp)]) if (n && v !== null) set(n, name, v);
  }
  boxes[0].id ||= `${dp.id}-text`;
  for (const label of [...(out.labels || [])]) {
    label.id ||= `${j.id}-label`;
    set(j, "aria-labelledby", label.id);
    label.htmlFor = boxes[0].id;
  }
  join(j, true);
}
const sync = () => {
  for (const h of /** @type {any} */ (every(PICKER))) enhance(h);
  for (const j of /** @type {any} */ (every(JOINT))) joint(j);
};
on(document, "ui:swap", sync);
// A form's reset puts the fields back; the boxes follow.
on(document, "reset", (e) => setTimeout(() => {
  const form = /** @type {Element} */ (e.target);
  for (const h of /** @type {any} */ (all(form, PICKER))) {
    const t = textOf(h), n = nativeOf(h);
    good(h);
    if (t) t.value = "";
    if (n) setTime(h, nativeTime(n.value));
  }
  for (const j of /** @type {any} */ (all(form, JOINT))) join(j, true);
}, 0));
// A label or an error focuses the joint field: the date box stands in for it.
on(document, "focusin", (e) => {
  const t = /** @type {Element} */ (e.target), j = has(t, "data-ui-joint") && near(t, JOINT);
  if (j) textOf(halves(j)[1])?.focus();
});
sync();
