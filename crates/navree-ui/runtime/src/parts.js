// The parts module: the small behaviours of parts that are plain markup without them.
// The colour picker keeps its native colour field, its hex field and its swatches in step;
// a clear button empties its field; a checkbox the server calls indeterminate becomes one
// (no attribute does that); an avatar whose picture did not load shows its fallback. Every
// part works without this module: the colour field is a native one that posts, and the
// rest are drawn only where a script runs. It is delegated from `document` and reads the
// page through dom.js, so a morph needs no wiring.
import { E, T, all, attr, closest, every, on, set, toggle } from "./dom.js";

const COLOR = "[data-ui-color]";
const NATIVE = "[data-slot=color-native]";
const TEXT = "[data-slot=color-hex]";
const SWATCH = "[data-ui-color-value]";
const CLEAR = "[data-ui-clear]";
const HEX = /^#?([0-9a-f]{3}|[0-9a-f]{6})$/i;

/** @param {Element} root @param {string} selector @returns {HTMLInputElement | null} */
const part = (root, selector) => E.querySelector.call(root, selector);
/** Sends `type` from `field` as a person's own change would. @param {Element} field @param {string} type */
const send = (field, type) => T.dispatchEvent.call(field, new Event(type, { bubbles: true }));

/** The field a clear button belongs to: the clearable one in its group. @param {Element} node */
const boxOf = (node) => {
  const group = closest(node, "[data-slot=input-group]");
  return group && part(group, "[data-ui-clearable]");
};

/** `#rgb`, `rgb` or `#rrggbb` as `#rrggbb`, and "" for anything else. @param {string} text */
function hex(text) {
  const m = HEX.exec(text.trim());
  if (!m) return "";
  const h = m[1].toLowerCase();
  return "#" + (h.length === 3 ? h.replace(/./g, "$&$&") : h);
}

/** Marks the swatch that holds `value` as the one pressed. @param {Element} root @param {string} value */
function mark(root, value) {
  for (const swatch of all(root, SWATCH)) set(swatch, "aria-pressed", String(attr(swatch, "data-ui-color-value") === value));
}

/** Writes `value` into the hex field and the swatches. @param {Element} root @param {string} value */
function fill(root, value) {
  const text = part(root, TEXT);
  if (text) text.value = value;
  mark(root, value);
}

let own = false;

on(document, "click", (e) => {
  const swatch = closest(e.target, SWATCH);
  const root = swatch && closest(swatch, COLOR);
  const native = root && part(root, NATIVE);
  if (native && !native.disabled) {
    native.value = attr(swatch, "data-ui-color-value") || "";
    fill(root, native.value);
    send(native, "input");
    send(native, "change");
  }
  const clear = closest(e.target, CLEAR);
  const field = /** @type {HTMLInputElement | null} */ (clear && boxOf(clear));
  if (field && !field.disabled && !field.readOnly) {
    field.value = "";
    send(field, "input");
    field.focus();
  }
});

on(document, "input", (e) => {
  const native = closest(e.target, NATIVE);
  const group = native && closest(native, COLOR);
  if (group) {
    if (!own) fill(group, /** @type {HTMLInputElement} */ (native).value);
    return;
  }
  const text = /** @type {HTMLInputElement | null} */ (closest(e.target, TEXT));
  const root = text && closest(text, COLOR);
  const value = root ? hex(text.value) : "";
  const field = root && part(root, NATIVE);
  if (field && value && field.value !== value) {
    field.value = value;
    mark(root, value);
    own = true;
    send(field, "input");
    own = false;
  }
  const clearable = closest(e.target, "[data-ui-clearable]");
  if (clearable) sync(clearable);
});

// The hex field's own change is the person finishing: the colour field says it, once, and
// the hex field shows the colour in its full form, or goes back when it was not one.
on(document, "change", (e) => {
  const text = /** @type {HTMLInputElement | null} */ (closest(e.target, TEXT));
  const root = text && closest(text, COLOR);
  const native = root && part(root, NATIVE);
  if (!root || !native) return;
  e.stopImmediatePropagation();
  const value = hex(text.value);
  if (value) native.value = value;
  fill(root, native.value);
  send(native, "change");
}, { capture: true });

/** Shows a clear button only while its field holds something. @param {Element} field */
function sync(field) {
  const group = closest(field, "[data-slot=input-group]");
  if (group) for (const button of all(group, CLEAR)) toggle(button, "hidden", !(/** @type {HTMLInputElement} */ (field).value));
}

function enhance() {
  for (const box of every("input[data-indeterminate]")) /** @type {HTMLInputElement} */ (box).indeterminate = true;
  for (const field of every("[data-ui-clearable]")) sync(field);
  for (const image of every("[data-slot=avatar][data-ui-fallback] img")) {
    const picture = /** @type {HTMLImageElement} */ (image);
    if (picture.complete && picture.naturalWidth === 0) set(/** @type {Element} */ (closest(picture, "[data-slot=avatar]")), "data-failed", "");
  }
}

// An image's error does not bubble: it is heard on the way down.
on(document, "error", (e) => {
  const avatar = closest(e.target, "[data-slot=avatar][data-ui-fallback]");
  if (avatar && e.target instanceof HTMLImageElement) set(avatar, "data-failed", "");
}, { capture: true });

enhance();
on(document, "ui:swap", enhance);
