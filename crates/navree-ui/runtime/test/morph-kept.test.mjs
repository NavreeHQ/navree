// A morph matches the page's elements to the server's by id and by `data-key`. What lies inside an element
// the morph keeps (`data-ui-keep`), below the element being morphed, is not the page's to match: an element a
// component's script, a library or an island's script wrote there with one of the page's ids or keys never
// takes the place of the page's own. A kept element itself still moves whole.
import { test } from "node:test";
import assert from "node:assert/strict";
import { install } from "./fake-dom.mjs";

/** The page as the server draws it: the bodies hold what the server drew in them, nothing. */
const MAIN = `<main id="main">
      <form id="new-delivery" method="post" action="/deliveries">
        <input id="charge" name="charge" type="number" value="40">
        <div id="map" data-slot="proof:map">
          <div id="map-body" data-slot="proof:map-body" data-ui-keep></div>
        </div>
      </form>
      <ul id="cards">
        <li data-key="42">Gurpreet Singh</li>
        <li id="board" data-slot="proof:board"><div id="board-body" data-slot="proof:board-body" data-ui-keep></div></li>
      </ul>
      <p id="total">₹1,00,000</p>
      <div id="depots" data-slot="island" data-ui-island="/_navree/ui/island/depots-0123456789ab.js">
        <div id="depots-body" data-slot="island-body" data-ui-keep></div>
      </div>
      <section id="first">Ludhiana</section>
      <div id="kept" data-ui-keep></div>
    </main>`;

install({ url: "https://app.test/deliveries/new", html: `<!doctype html><html data-build="b1"><head></head><body>${MAIN}</body></html>` });

const { morph } = await import("../src/morph.js");
const $ = (/** @type {string} */ id) => /** @type {any} */ (document.getElementById(id));

/** A detached element of `html`, as a fetched page gives one. @param {string} html */
function server(html) {
  const box = document.createElement("div");
  box.innerHTML = html;
  return /** @type {any} */ (box.firstElementChild);
}

// What the scripts wrote, after the page's own elements of the same id or key.
$("map-body").innerHTML = `<input id="charge" name="charge" type="number" value="0">`;
$("board-body").innerHTML = `<ol><li data-key="42">Planted</li></ol>`;
$("depots-body").innerHTML = `<p id="total">Planted</p>`;
$("kept").innerHTML = `<p>Drawn by the script</p>`;

const charge = $("charge");
const writtenCharge = $("map-body").firstElementChild;
const card = /** @type {any} */ (document.querySelector("#cards > li[data-key='42']"));
const writtenCard = $("board-body").querySelector("li");
const total = $("total");
const writtenTotal = $("depots-body").firstElementChild;
const kept = $("kept");

// The server answers with the same page, the kept element now ahead of the section.
morph($("main"), server(MAIN.replace(`<section id="first">Ludhiana</section>\n      <div id="kept" data-ui-keep></div>`, `<div id="kept" data-ui-keep></div>\n      <section id="first">Ludhiana</section>`)));

test("a field written into a component's body does not take the place of the page's field with its id", () => {
  assert.equal($("new-delivery").firstElementChild, charge, "the page's own field was replaced");
  assert.equal(charge.getAttribute("value"), "40");
  assert.equal(writtenCharge.parentNode, $("map-body"), "the written field left the body");
  assert.equal(writtenCharge.getAttribute("value"), "0");
});

test("an element written into a body does not take the place of the page's element with its key", () => {
  assert.equal($("cards").firstElementChild, card, "the page's own card was replaced");
  assert.equal(card.textContent, "Gurpreet Singh");
  assert.equal(writtenCard.parentNode.parentNode, $("board-body"), "the written card left the body");
  assert.equal(writtenCard.textContent, "Planted");
});

test("an element an island's script wrote does not take the place of the page's element with its id", () => {
  assert.equal(total.isConnected, true, "the page's own element was replaced");
  assert.equal(total.textContent, "₹1,00,000");
  assert.equal(writtenTotal.parentNode, $("depots-body"));
  assert.equal(writtenTotal.textContent, "Planted");
});

test("a kept element itself still moves whole", () => {
  assert.equal($("kept"), kept);
  assert.equal(kept.nextElementSibling, $("first"));
  assert.equal(kept.textContent, "Drawn by the script");
});
