// The one-time code module. The field is one real text input drawn as boxes by the
// stylesheet, so it already posts, takes a code from a text message and works with no
// script. This keeps what is typed or pasted to the characters a code may hold, puts a
// pasted code in whole (the input's own length limit would cut "482 915" before the
// space was taken out), keeps the caret after the last character when it is clicked,
// and tells the page when the code is whole: a `change` event, and the form sent when
// the field asks for that. It is delegated from `document`, so a morph needs no wiring.
import { T, attr, closest, has, on, toggle } from "./dom.js";

const FIELD = "[data-ui-otp]";

/** What of `text` a code of this field's kind may hold, as many characters as it has room for. @param {HTMLInputElement} field @param {string} text */
function clean(field, text) {
  const kept = text.replace(/[^0-9A-Za-z]/g, "");
  return (attr(field, "data-ui-otp") === "alnum" ? kept.toUpperCase() : kept.replace(/\D/g, "")).slice(0, field.maxLength);
}

/** Tells the page the code is whole, once each time it becomes so. @param {HTMLInputElement} field */
function finish(field) {
  const whole = field.value.length === field.maxLength;
  const was = has(field, "data-done");
  toggle(field, "data-done", whole);
  if (!whole || was) return;
  T.dispatchEvent.call(field, new Event("change", { bubbles: true }));
  const form = field.form;
  if (has(field, "data-ui-submit") && form && field.checkValidity()) HTMLFormElement.prototype.requestSubmit.call(form);
}

on(document, "input", (e) => {
  const field = /** @type {HTMLInputElement | null} */ (closest(e.target, FIELD));
  if (!field || e.isComposing) return;
  const value = clean(field, field.value);
  if (value !== field.value) field.value = value;
  finish(field);
});

// A paste is put in whole, cleaned, over what is selected.
on(document, "paste", (e) => {
  const field = /** @type {HTMLInputElement | null} */ (closest(e.target, FIELD));
  if (!field || field.readOnly || field.disabled) return;
  e.preventDefault();
  const text = e.clipboardData ? e.clipboardData.getData("text") : "";
  const value = clean(field, field.value.slice(0, field.selectionStart ?? 0) + text + field.value.slice(field.selectionEnd ?? 0));
  field.value = value;
  T.dispatchEvent.call(field, new Event("input", { bubbles: true }));
});

// A click puts the caret after the last character, so a code is typed in order; a range
// that is selected stays, and the arrow keys move the caret as they always do.
on(document, "click", (e) => {
  const field = /** @type {HTMLInputElement | null} */ (closest(e.target, FIELD));
  if (field && field.selectionStart === field.selectionEnd) field.setSelectionRange(field.value.length, field.value.length);
});
