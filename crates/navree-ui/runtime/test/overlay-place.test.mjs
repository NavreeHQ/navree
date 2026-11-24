// A popup is placed from the size it keeps. Until it has a place the stylesheet lays a popup out as an
// unplaced one, centred in the window, and one as wide as its content (a data table's column menu) is
// stretched there to its widest. Read then, its width is not the width it ends up with, and a menu
// lined up by its end lands that much short of its button. The overlay module gives a popup a place
// before it reads its size.
//
// This page has no layout, so each popup's width is told by the test: one width while it has no place,
// another once it has one, as the stylesheet makes it. The browser suites under e2e/ui see the real one.
import { test } from "node:test";
import assert from "node:assert/strict";
import { fire, install } from "./fake-dom.mjs";

const page = install({ url: "https://app.test/deals", html: "" });
// The window: 1440 by 900, with no keyboard or pinch taking any of it.
Object.assign(globalThis, { innerWidth: 1440, innerHeight: 900, visualViewport: undefined });

await import("../src/overlay.js");

const $ = (/** @type {string} */ id) => /** @type {any} */ (document.getElementById(id));
const direction = (/** @type {"ltr" | "rtl"} */ dir) => {
  /** @type {any} */ (globalThis).getComputedStyle = () => ({ direction: dir, position: "static", maxHeight: "none" });
};

/**
 * A button at `at` and its popup, which is `unplaced` wide until it has a place and `placed` wide from
 * then on. What the popup's place was each time its width was read is kept in `read`.
 * @param {{align?: string, at: {left: number, right: number}, unplaced: number, placed: number}} options
 */
function draw({ align, at, unplaced, placed }) {
  document.body.innerHTML = `
    <button id="b" type="button" popovertarget="menu">Stage options</button>
    <div id="menu" popover="auto" data-ui-place data-side="bottom"${align ? ` data-align="${align}"` : ""}></div>`;
  $("b").getBoundingClientRect = () => ({ top: 170, bottom: 194, left: at.left, right: at.right, width: at.right - at.left, height: 24 });
  /** @type {(string | undefined)[]} */
  const read = [];
  Object.defineProperty($("menu"), "offsetWidth", {
    get() {
      read.push(this.style.left);
      return this.style.left ? placed : unplaced;
    },
  });
  Object.defineProperty($("menu"), "offsetHeight", { get: () => 40 });
  return read;
}
const open = () => fire($("menu"), "toggle", { newState: "open" });
const close = () => fire($("menu"), "toggle", { newState: "closed" });

test("a menu lined up by its end opens with its end on its button's, the first time it opens", () => {
  direction("ltr");
  // 22rem wide while it has no place, 11rem once it has: a column menu that only says "Hide column".
  const read = draw({ align: "end", at: { left: 732, right: 756 }, unplaced: 352, placed: 176 });
  open();
  assert.deepEqual(page.errors, []);
  assert.deepEqual(read, ["0px"], "its width is read once, with a place");
  assert.equal($("menu").style.left, "580px", "756 less its own 176");
  assert.equal($("menu").style.top, "198px", "under the button, a gap away");
});

test("it opens in the same place the second time", () => {
  direction("ltr");
  const read = draw({ align: "end", at: { left: 732, right: 756 }, unplaced: 352, placed: 176 });
  open();
  close();
  open();
  assert.deepEqual(read, ["0px", "580px"], "the second time it is read where it was left");
  assert.equal($("menu").style.left, "580px");
});

test("a centred popup is centred on its button by the width it keeps", () => {
  direction("ltr");
  draw({ at: { left: 700, right: 800 }, unplaced: 352, placed: 176 });
  open();
  assert.equal($("menu").style.left, "662px", "750, the button's middle, less half of 176");
});

test("right to left, a menu lined up by its start ends on its button's right edge", () => {
  direction("rtl");
  draw({ align: "start", at: { left: 732, right: 756 }, unplaced: 352, placed: 176 });
  open();
  assert.equal($("menu").style.left, "580px");
});

test("a menu near the window's edge is held in by the width it keeps, not by its widest", () => {
  direction("ltr");
  // Its button is 200 px from the left: 352 wide it would be pushed to the margin, 176 wide it fits under its button.
  draw({ align: "end", at: { left: 176, right: 200 }, unplaced: 352, placed: 176 });
  open();
  assert.equal($("menu").style.left, "24px");
});

test("a popup whose width does not depend on its place goes where it did", () => {
  direction("ltr");
  draw({ align: "end", at: { left: 732, right: 756 }, unplaced: 320, placed: 320 });
  open();
  assert.equal($("menu").style.left, "436px");
});

test("a popup opened by script and placed in the same frame is read with a place too", () => {
  direction("ltr");
  const read = draw({ align: "end", at: { left: 732, right: 756 }, unplaced: 352, placed: 176 });
  fire($("menu"), "ui:place");
  assert.deepEqual(page.errors, []);
  assert.deepEqual(read, ["0px"]);
  assert.equal($("menu").style.left, "580px");
});
