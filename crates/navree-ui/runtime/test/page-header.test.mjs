// An answer is taken into the page on screen only when it says, in the header every `View` sends,
// that it is a page of this app: a same-origin file with the same `data-build` is not one.
import { test } from "node:test";
import assert from "node:assert/strict";
import { answer, install, settle } from "./fake-dom.mjs";

const page = install({
  url: "https://app.test/deals",
  html: `<!doctype html><html data-build="b1"><head></head><body data-ui-boost>
    <main id="main"><a id="file" href="/uploads/note.html">Note</a><a id="next" href="/deals/2">Next</a></main>
    <div id="kpis" data-ui-region data-ui-src="/fragments/kpis">₹1,00,000</div>
  </body></html>`,
});

const { refresh } = await import("../src/nav.js");
await import("../src/core.js");
await settle();

const $ = (/** @type {string} */ id) => /** @type {any} */ (document.getElementById(id));
const PAGE = { "content-type": "text/html; charset=utf-8", "x-navree-page": "1" };
const html = (/** @type {string} */ main) => `<!doctype html><html data-build="b1"><head><title>T</title></head><body data-ui-boost><main id="main">${main}</main><div id="kpis" data-ui-region data-ui-src="/fragments/kpis">₹2,50,000</div></body></html>`;

test("a same-origin answer without the page header loads for real", async () => {
  page.answers.push(answer(html("Planted"), { headers: { "content-type": "text/html" } }));
  page.assigned.length = 0;
  $("file").click();
  await settle();
  assert.deepEqual(page.assigned, ["https://app.test/uploads/note.html"]);
  assert.equal($("main").textContent.includes("Planted"), false);
});

test("a page of this app is shown in place", async () => {
  page.answers.push(answer(html("Deal 2"), { headers: PAGE }));
  page.assigned.length = 0;
  $("next").click();
  await settle();
  assert.deepEqual(page.assigned, []);
  assert.equal($("main").textContent, "Deal 2");
});

test("a region's answer without the page header is not drawn", async () => {
  page.answers.push(answer(html("x"), { headers: { "content-type": "text/html" } }));
  await refresh("kpis");
  assert.equal($("kpis").textContent, "₹2,50,000");
  assert.equal($("kpis").hasAttribute("data-ui-failed"), true);
  page.answers.push(answer(`<div id="kpis" data-ui-region data-ui-src="/fragments/kpis">₹3,00,000</div>`, { headers: PAGE }));
  await refresh("kpis");
  assert.equal($("kpis").textContent, "₹3,00,000");
});
