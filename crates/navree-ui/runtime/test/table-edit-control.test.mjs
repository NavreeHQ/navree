// A table cell edited in place is a form holding one field named `value`. The change of that field posts
// the cell, and its Enter and Escape are the cell's. A cell edited by a control of the app's or a plugin's
// own can hold other elements (a search box, buttons): their changes and keys are the control's, and
// reach the cell not at all. The text editor and the select editor, which are the field alone, work as before.
import { test } from "node:test";
import assert from "node:assert/strict";
import { fire, install } from "./fake-dom.mjs";

const page = install({ url: "https://app.test/deliveries", html: "" });

await import("../src/table.js");

const $ = (/** @type {string} */ id) => /** @type {any} */ (document.getElementById(id));

/** Draws `inside` as the body of one cell's form, with the token the server draws first. @param {string} inside */
function draw(inside) {
  page.errors.length = 0;
  page.submitted.length = 0;
  document.body.innerHTML = `
    <form id="cell" method="post" action="/deliveries/42/place" data-ui-form data-ui-edit>
      <input type="hidden" name="_csrf" value="token">
      ${inside}
    </form>`;
}

/** A control with a search box of its own, then the field. */
const searchThenField = `
  <div id="control">
    <input id="search" type="search" name="q" aria-label="Search places" value="Lud">
    <button id="use" type="button">Use</button>
    <input id="field" type="text" name="value" aria-label="Place" value="Ludhiana">
  </div>`;

/** The same, with the search box after the field. */
const fieldThenSearch = `
  <div id="control">
    <input id="field" type="text" name="value" aria-label="Place" value="Ludhiana">
    <input id="search" type="search" name="q" aria-label="Search places" value="Lud">
  </div>`;

/** A control that keeps its value in a hidden input, beside a search box of its own. */
const hiddenValue = `
  <div id="control">
    <input id="search" type="search" name="q" aria-label="Search places" value="Lud">
    <input id="field" type="hidden" name="value" value="Ludhiana">
  </div>`;

const sent = (/** @type {boolean} */ ok) => fire($("cell"), "ui:sent", { detail: { form: $("cell"), ok } });

test("a change of the field named value posts the cell, once", () => {
  draw(searchThenField);
  $("field").value = "Patiala";
  fire($("field"), "change");
  assert.deepEqual(page.errors, []);
  assert.equal(page.submitted.length, 1);
  assert.equal(page.submitted[0].form, $("cell"));
});

test("a change of the control's own search box posts nothing", () => {
  draw(searchThenField);
  $("search").value = "Pat";
  fire($("search"), "change");
  fire($("use"), "change");
  assert.deepEqual(page.errors, []);
  assert.deepEqual(page.submitted, []);
});

test("a change of a hidden field named value posts the cell, and one of the search box beside it does not", () => {
  draw(hiddenValue);
  fire($("search"), "change");
  assert.deepEqual(page.submitted, []);
  $("field").value = "Patiala";
  fire($("field"), "change");
  assert.equal(page.submitted.length, 1);
});

test("a change in a form that is not an edited cell posts nothing", () => {
  document.body.innerHTML = `<form id="other" method="post" action="/x"><input id="field" name="value" value="a"></form>`;
  page.submitted.length = 0;
  fire($("field"), "change");
  assert.deepEqual(page.submitted, []);
});

test("Enter in the field named value leaves the field, and the browser's own Enter is stopped", () => {
  draw(searchThenField);
  $("field").focus();
  assert.equal(document.activeElement, $("field"));
  const key = fire($("field"), "keydown", { key: "Enter" });
  assert.deepEqual(page.errors, []);
  assert.equal(key.defaultPrevented, true);
  assert.notEqual(document.activeElement, $("field"));
});

test("Enter in the control's own search box is left to the control", () => {
  draw(searchThenField);
  $("search").focus();
  const key = fire($("search"), "keydown", { key: "Enter" });
  assert.deepEqual(page.errors, []);
  assert.equal(key.defaultPrevented, false);
  assert.equal(document.activeElement, $("search"));
  assert.deepEqual(page.submitted, []);
});

test("Escape in the field puts back the value the server drew, and leaves the field", () => {
  draw(searchThenField);
  $("field").focus();
  $("field").value = "Patiala";
  fire($("field"), "keydown", { key: "Escape" });
  assert.deepEqual(page.errors, []);
  assert.equal($("field").value, "Ludhiana");
  assert.notEqual(document.activeElement, $("field"));
  // What was typed in the search box is the control's own.
  assert.equal($("search").value, "Lud");
});

test("Escape in the control's own search box changes nothing", () => {
  draw(searchThenField);
  $("field").value = "Patiala";
  $("search").focus();
  $("search").value = "Pat";
  const key = fire($("search"), "keydown", { key: "Escape" });
  assert.deepEqual(page.errors, []);
  assert.equal(key.defaultPrevented, false);
  assert.equal($("field").value, "Patiala");
  assert.equal($("search").value, "Pat");
  assert.equal(document.activeElement, $("search"));
});

test("Escape in the field puts back its value when the search box comes after it", () => {
  draw(fieldThenSearch);
  $("field").value = "Patiala";
  fire($("field"), "keydown", { key: "Escape" });
  assert.equal($("field").value, "Ludhiana");
  assert.equal($("search").value, "Lud");
});

test("after an accepted post the field goes back to the new value, with the search box first in the form", () => {
  draw(searchThenField);
  $("field").value = "Patiala";
  fire($("field"), "change");
  sent(true);
  assert.deepEqual(page.errors, []);
  // The new value is the one the cell goes back to; the search box's default is not touched.
  assert.equal($("field").defaultValue, "Patiala");
  assert.equal($("search").defaultValue, "Lud");
  $("field").value = "Amritsar";
  fire($("field"), "keydown", { key: "Escape" });
  assert.equal($("field").value, "Patiala");
});

test("after a refused post the field goes back to the value it had, with the search box first in the form", () => {
  draw(searchThenField);
  $("field").value = "Patiala";
  fire($("field"), "change");
  sent(false);
  assert.deepEqual(page.errors, []);
  assert.equal($("field").value, "Ludhiana");
  assert.equal($("search").value, "Lud");
});

test("after a post the field goes back to the right value with the search box after it", () => {
  draw(fieldThenSearch);
  $("field").value = "Patiala";
  sent(true);
  $("field").value = "Amritsar";
  sent(false);
  assert.equal($("field").value, "Patiala");
  assert.equal($("search").defaultValue, "Lud");
});

test("a control that keeps its value in a hidden field is put back too", () => {
  draw(hiddenValue);
  $("field").value = "Patiala";
  fire($("field"), "change");
  assert.equal(page.submitted.length, 1);
  sent(true);
  assert.equal($("field").defaultValue, "Patiala");
  $("field").value = "Amritsar";
  sent(false);
  assert.deepEqual(page.errors, []);
  assert.equal($("field").value, "Patiala");
  assert.equal($("search").defaultValue, "Lud");
});

test("the text editor posts on change, and Escape and Enter are its own", () => {
  draw(`<input id="field" type="text" name="value" aria-label="Deal" value="Harleen">`);
  $("field").focus();
  $("field").value = "Gurpreet";
  fire($("field"), "change");
  assert.deepEqual(page.errors, []);
  assert.equal(page.submitted.length, 1);

  const enter = fire($("field"), "keydown", { key: "Enter" });
  assert.equal(enter.defaultPrevented, true);
  assert.notEqual(document.activeElement, $("field"));

  $("field").focus();
  fire($("field"), "keydown", { key: "Escape" });
  assert.equal($("field").value, "Harleen");
  assert.notEqual(document.activeElement, $("field"));

  sent(true);
  $("field").value = "Meera";
  sent(false);
  assert.equal($("field").value, "Harleen");
  $("field").value = "Simran";
  sent(true);
  $("field").value = "Arjun";
  fire($("field"), "keydown", { key: "Escape" });
  assert.equal($("field").value, "Simran");
  assert.deepEqual(page.errors, []);
});

test("the select editor posts on change, and goes back to the option the server drew", () => {
  draw(`
    <select id="field" name="value" aria-label="Status">
      <option id="open" value="open" selected>Open</option>
      <option id="done" value="done">Done</option>
    </select>`);
  const choose = (/** @type {string} */ id) => {
    for (const option of $("field").options) option.selected = option.id === id;
  };
  choose("done");
  fire($("field"), "change");
  assert.deepEqual(page.errors, []);
  assert.equal(page.submitted.length, 1);

  fire($("field"), "keydown", { key: "Escape" });
  assert.equal($("open").selected, true);
  assert.equal($("done").selected, false);

  choose("done");
  sent(true);
  choose("open");
  fire($("field"), "keydown", { key: "Escape" });
  assert.equal($("done").selected, true);
  assert.equal($("open").selected, false);

  choose("open");
  sent(false);
  assert.equal($("done").selected, true);
  assert.deepEqual(page.errors, []);
});
