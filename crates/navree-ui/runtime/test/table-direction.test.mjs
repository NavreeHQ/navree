// The table's grid keys and its column resizing (by pointer and by the arrow keys on the handle) read the
// table's direction: left to right, and right to left (Urdu, Arabic), where the keys and the drag mean
// the other way round.
import { test } from "node:test";
import assert from "node:assert/strict";
import { fire, install } from "./fake-dom.mjs";

const page = install({ url: "https://app.test/customers", html: "" });

await import("../src/table.js");

const $ = (/** @type {string} */ id) => /** @type {any} */ (document.getElementById(id));
const direction = (/** @type {"ltr" | "rtl"} */ dir) => {
  /** @type {any} */ (globalThis).getComputedStyle = () => ({ direction: dir, position: "static" });
};

/** A table with the grid keys on, and a heading with a handle to resize its first column by. */
function draw() {
  page.errors.length = 0;
  page.assigned.length = 0;
  document.body.innerHTML = `
    <div id="t" data-ui-table data-ui-grid>
      <div data-slot="table-container"><table data-slot="table">
        <colgroup><col data-col="name"><col data-col="city"><col data-col="due"></colgroup>
        <thead><tr>
          <th id="name-head" data-col="name">Name<span id="handle" data-slot="col-resize" role="separator" tabindex="0" aria-valuenow="200"></span></th>
          <th data-col="city">City</th><th data-col="due">Due</th>
        </tr></thead>
        <tbody>
          <tr data-key="1"><td id="a1" tabindex="0">Harleen</td><td id="a2" tabindex="-1">Ludhiana</td><td id="a3" tabindex="-1">₹1,00,000</td></tr>
          <tr data-key="2"><td id="b1" tabindex="-1">Gurpreet</td><td id="b2" tabindex="-1">Patiala</td><td id="b3" tabindex="-1">₹2,50,000</td></tr>
        </tbody>
      </table></div>
    </div>`;
  // The heading is 200 px wide, as a layout would say.
  $("name-head").getBoundingClientRect = () => ({ top: 0, bottom: 0, left: 0, right: 200, width: 200, height: 0 });
}

/** The width the first column has now, and the one the address asks for. */
const widthOf = () => document.querySelector("col[data-col=name]")?.getAttribute("width");

test("the grid keys go to the next cell the way the table reads: left to right", () => {
  direction("ltr");
  draw();
  fire($("a2"), "keydown", { key: "ArrowRight" });
  assert.deepEqual(page.errors, []);
  assert.equal(document.activeElement, $("a3"));
  fire($("a3"), "keydown", { key: "ArrowLeft" });
  assert.equal(document.activeElement, $("a2"));
  fire($("a2"), "keydown", { key: "ArrowDown" });
  assert.equal(document.activeElement, $("b2"));
});

test("the grid keys go to the next cell the way the table reads: right to left", () => {
  direction("rtl");
  draw();
  fire($("a2"), "keydown", { key: "ArrowRight" });
  assert.deepEqual(page.errors, []);
  assert.equal(document.activeElement, $("a1"));
  fire($("a1"), "keydown", { key: "ArrowLeft" });
  assert.equal(document.activeElement, $("a2"));
  fire($("a2"), "keydown", { key: "ArrowUp" });
  assert.equal(document.activeElement, $("a2"));
});

test("a column grows with the pointer's drag to the right in a left-to-right table", () => {
  direction("ltr");
  draw();
  fire($("handle"), "pointerdown", { clientX: 100, pointerId: 1 });
  fire($("handle"), "pointermove", { clientX: 130 });
  assert.deepEqual(page.errors, []);
  assert.equal(widthOf(), "230");
  assert.equal($("handle").getAttribute("aria-valuenow"), "230");
  fire($("handle"), "pointerup");
  assert.deepEqual(page.errors, []);
  // The width is kept in the address, through the runtime's own link.
  assert.equal(new URL(page.assigned.at(-1) || "").searchParams.get("w")?.startsWith("name:230"), true);
});

test("a column grows with the pointer's drag to the left in a right-to-left table", () => {
  direction("rtl");
  draw();
  fire($("handle"), "pointerdown", { clientX: 100, pointerId: 1 });
  fire($("handle"), "pointermove", { clientX: 70 });
  assert.deepEqual(page.errors, []);
  assert.equal(widthOf(), "230");
  fire($("handle"), "pointermove", { clientX: 130 });
  assert.equal(widthOf(), "170");
});

test("the arrow keys on a column's handle widen it the way the table reads: left to right", () => {
  direction("ltr");
  draw();
  fire($("handle"), "keydown", { key: "ArrowRight" });
  assert.deepEqual(page.errors, []);
  assert.equal(widthOf(), "216");
  fire($("handle"), "keydown", { key: "ArrowLeft" });
  assert.equal(widthOf(), "184");
});

test("the arrow keys on a column's handle widen it the way the table reads: right to left", () => {
  direction("rtl");
  draw();
  fire($("handle"), "keydown", { key: "ArrowLeft" });
  assert.deepEqual(page.errors, []);
  assert.equal(widthOf(), "216");
  fire($("handle"), "keydown", { key: "ArrowRight" });
  assert.equal(widthOf(), "184");
});
