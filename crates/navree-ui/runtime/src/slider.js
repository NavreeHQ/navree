// The slider module. A native range input already posts, answers the slider keys and
// works with no script. This adds what it lacks: the filled part, the value as text,
// a larger step, a gap and an order between the two thumbs of a range, and a click
// on the track of a range. Everything is delegated from `document` and read through
// dom.js, so a morph needs no wiring and a name in stored markup can't shadow a method.
import { all, attr, closest, every, has, on, set } from "./dom.js";

const ROOT = "[data-ui-slider]";
const SECOND = "input[type=range]";

/** @param {Element} root @returns {HTMLInputElement[]} */
const thumbs = (root) => /** @type {any} */ ([...all(root, SECOND)]);
/** @param {Element} root @param {string} selector */
const first = (root, selector) => all(root, selector)[0] || null;
/** @param {number} n */
const clamp = (n) => (n < 0 ? 0 : n > 100 ? 100 : n);
/** @param {string} text @returns {number} */
const num = (text) => (text === "" || !Number.isFinite(Number(text)) ? NaN : Number(text));

/** The decimals the page writes a value with, held to what a toolkit number can have: 0 to 9. @param {Element} root */
const decimalsOf = (root) => Math.min(9, Math.max(0, Math.floor(Number(attr(root, "data-decimals")) || 0)));

/** @type {WeakMap<Element, string[]>} */
const named = new WeakMap();
/** The names of the steps, or none. @param {Element} root */
function namesOf(root) {
  let names = named.get(root);
  if (!names) {
    names = [];
    try {
      const parsed = JSON.parse(attr(root, "data-names") || "[]");
      if (Array.isArray(parsed)) names = parsed.slice(0, 1000).map(String);
    } catch {}
    named.set(root, names);
  }
  return names;
}

/**
 * A value as the number it is, written the way the page writes numbers: the decimals the grid has (the
 * shortest text, up to nine, for a slider that takes any value) and the page's decimal point. A browser
 * writes a small value as `1e-7`; the person never reads that.
 * @param {Element} root @param {string} text
 */
function shown(root, text) {
  const value = num(text);
  if (!Number.isFinite(value)) return text;
  const decimals = decimalsOf(root);
  let out = decimals > 0 ? value.toFixed(decimals) : value.toFixed(9).replace(/\.?0+$/, "");
  // A small negative that rounds to nothing is nothing.
  if (/^-0(\.0*)?$/.test(out)) out = out.slice(1);
  const point = attr(root, "data-point");
  return point ? out.replace(".", point) : out;
}

/** The value of `input` as the page writes it: a step's name, or the number with its prefix and suffix. @param {Element} root @param {HTMLInputElement} input */
function say(root, input) {
  const names = namesOf(root);
  const min = num(input.min), step = num(input.step), value = num(input.value);
  if (names.length && step > 0 && Number.isFinite(value)) {
    const at = Math.round((value - min) / step);
    if (names[at] !== undefined) return names[at];
  }
  return (attr(root, "data-prefix") || "") + shown(root, input.value) + (attr(root, "data-suffix") || "");
}

/** Whether the page writes a value as more than the bare number. @param {Element} root */
const worded = (root) => has(root, "data-prefix") || has(root, "data-suffix") || has(root, "data-names");

/** Brings the filled part, the value and the thumbs' text in line with the inputs. @param {Element} root */
function sync(root) {
  const inputs = thumbs(root);
  if (!inputs.length) return;
  const range = has(root, "data-range") && inputs.length > 1;
  const [a, b] = inputs;
  const min = num(a.min), max = num(a.max), span = max - min;
  const at = (/** @type {HTMLInputElement} */ input) => (span > 0 ? clamp(((num(input.value) - min) / span) * 100) : 0);
  const lo = range ? at(a) : 0, hi = range ? at(b) : at(a);
  const fill = first(root, "[data-slot=slider-fill]");
  if (fill) {
    set(fill, "x", lo + "%");
    set(fill, "width", Math.max(0, hi - lo) + "%");
  }
  const text = range ? say(root, a) + " " + (attr(root, "data-to") || "–") + " " + say(root, b) : say(root, a);
  const out = first(root, "output");
  if (out && out.textContent !== text) out.textContent = text;
  if (worded(root)) for (const input of inputs) set(input, "aria-valuetext", say(root, input));
  // When both thumbs sit at the same place, the one that can move away is on top.
  if (range) set(root, "data-top", num(a.value) >= num(b.value) && num(a.value) >= max ? "min" : "max");
}

/** Says a value changed, as typing would. @param {HTMLInputElement} input */
function announce(input) {
  input.dispatchEvent(new Event("input", { bubbles: true }));
  input.dispatchEvent(new Event("change", { bubbles: true }));
}

/**
 * Moves a value by one step, on the browser's grid. An input with `step="any"` has none, so it
 * moves by a hundredth of its track.
 * @param {HTMLInputElement} input @param {number} by 1 or -1
 */
function nudge(input, by) {
  try {
    if (by > 0) input.stepUp();
    else input.stepDown();
    return;
  } catch {}
  const min = num(input.min), max = num(input.max);
  if (!(max > min)) return;
  const next = Math.min(max, Math.max(min, num(input.value) + (by * (max - min)) / 100));
  // To nine places, which is what the server reads.
  input.value = String(Number(next.toFixed(9)));
}

/** The gap between the thumbs of a range, as a number. @param {Element} root @param {HTMLInputElement} input */
function gapOf(root, input) {
  const steps = Number(attr(root, "data-gap")) || 0;
  const step = num(input.step);
  return steps > 0 && step > 0 ? steps * step : 0;
}

/** Keeps the two thumbs in order and apart. @param {Element} root @param {HTMLInputElement} moved */
function keepApart(root, moved) {
  const [a, b] = thumbs(root);
  if (!b || !has(root, "data-range")) return;
  const gap = gapOf(root, moved);
  const decimals = decimalsOf(root);
  const write = (/** @type {HTMLInputElement} */ input, /** @type {number} */ value) => {
    input.value = value.toFixed(decimals);
    // The browser has put it on its grid; if the edge fell between two steps, take the one inside.
    if (input === a && num(a.value) > num(b.value) - gap) nudge(input, -1);
    if (input === b && num(b.value) < num(a.value) + gap) nudge(input, 1);
  };
  if (moved === a && num(a.value) > num(b.value) - gap) write(a, num(b.value) - gap);
  if (moved === b && num(b.value) < num(a.value) + gap) write(b, num(a.value) + gap);
}

on(document, "input", (e) => {
  const root = closest(e.target, ROOT);
  if (!root) return;
  keepApart(root, e.target);
  sync(root);
});

/** What Page Up, Page Down and Shift with an arrow move by. @param {Element} root @param {HTMLInputElement} input */
function bigStep(root, input) {
  const given = num(attr(root, "data-big") || "");
  if (given > 0) return given;
  const tenth = (num(input.max) - num(input.min)) / 10;
  const step = num(input.step);
  // A slider that takes any value has no step to round to: a tenth of the track.
  if (!(step > 0)) return tenth;
  return Math.max(step, Math.round(tenth / step) * step);
}

on(document, "keydown", (e) => {
  const input = closest(e.target, ROOT + " " + SECOND);
  const root = input && closest(input, ROOT);
  if (!input || !root || e.altKey || e.ctrlKey || e.metaKey || /** @type {HTMLInputElement} */ (input).disabled) return;
  const field = /** @type {HTMLInputElement} */ (input);
  const rtl = getComputedStyle(field).direction === "rtl";
  const horizontal = e.key === "ArrowLeft" || e.key === "ArrowRight";
  // `step="any"` has no step: the browsers disagree on how far an arrow goes (Firefox goes to the end).
  const anyStep = !(num(field.step) > 0);
  // The arrows move the thumb the way it looks: in a right-to-left page the least is at the right, and
  // browsers differ on whether Right means more there, so the sideways arrows are done here.
  const toward = e.key === (rtl ? "ArrowLeft" : "ArrowRight") || e.key === "ArrowUp" ? 1 : -1;
  let by = 0;
  let large = false;
  if (e.key === "PageUp" || e.key === "PageDown") {
    by = e.key === "PageUp" ? 1 : -1;
    large = true;
  } else if (e.shiftKey && (horizontal || e.key === "ArrowUp" || e.key === "ArrowDown")) {
    by = toward;
    large = true;
  } else if (!e.shiftKey && ((rtl && horizontal) || (anyStep && (horizontal || e.key === "ArrowUp" || e.key === "ArrowDown")))) {
    // Done here: a slider that takes any value moves by a hundredth of its track, and in a right-to-left page the sideways arrows are the page's.
    by = toward;
  }
  if (!by) return;
  e.preventDefault();
  const big = bigStep(root, field);
  // A step, or for a slider that takes any value a hundredth of the track, which is what `nudge` moves it by.
  const unit = anyStep ? (num(field.max) - num(field.min)) / 100 : num(field.step);
  // Move by whole steps: the browser puts the value on its grid and inside its bounds.
  const times = large ? Math.max(1, Math.round(big / unit)) : 1;
  const before = field.value;
  for (let n = 0; n < times; n++) nudge(field, by);
  keepApart(root, field);
  if (field.value !== before) announce(field);
});

// A click on the track of a range moves the nearer thumb there. (A single slider's
// input is the track, and the browser does it.)
on(document, "pointerdown", (e) => {
  if (e.button !== 0) return;
  const control = closest(e.target, "[data-range] [data-slot=slider-control]");
  const root = control && closest(control, ROOT);
  if (!control || !root || has(root, "data-disabled") || has(root, "data-readonly")) return;
  const inputs = thumbs(root);
  const [a, b] = inputs;
  if (!a || !b || e.target instanceof HTMLInputElement) return;
  const graphic = first(control, "[data-slot=slider-graphic]") || control;
  const box = graphic.getBoundingClientRect();
  if (box.width <= 0) return;
  let along = (e.clientX - box.left) / box.width;
  if (getComputedStyle(control).direction === "rtl") along = 1 - along;
  along = along < 0 ? 0 : along > 1 ? 1 : along;
  const min = num(a.min), max = num(a.max);
  const value = min + along * (max - min);
  const near = Math.abs(value - num(a.value)) <= Math.abs(value - num(b.value)) ? (value > num(b.value) ? b : a) : b;
  const step = num(near.step);
  const decimals = decimalsOf(root);
  near.value = (step > 0 ? min + Math.round((value - min) / step) * step : value).toFixed(decimals);
  keepApart(root, near);
  near.focus();
  sync(root);
  announce(near);
  e.preventDefault();
});

// Form reset puts the inputs back after the event; look again once it has.
on(document, "reset", (e) => {
  const form = /** @type {Element} */ (e.target);
  setTimeout(() => {
    for (const root of all(form, ROOT)) sync(root);
  }, 0);
});

function start() {
  for (const root of every(ROOT)) {
    // Markup that claims to be a slider and has no input is left alone.
    if (thumbs(root).length) sync(root);
  }
}
start();
on(document, "ui:swap", start);
// A form restored by the browser on back and forward brings its values after the first run.
on(window, "pageshow", start);
