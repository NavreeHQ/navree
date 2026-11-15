// A lazy region is marked busy until its content arrives. One whose fetch failed is busy no longer:
// it shows what it says when a fetch gets no answer, and an assistive tool is not told to wait for more.
import { test } from "node:test";
import assert from "node:assert/strict";
import { answer, install, settle } from "./fake-dom.mjs";

const region = (/** @type {string} */ id) =>
  `<div id="${id}" data-ui-region data-ui-src="/fragments/${id}" data-ui-lazy aria-busy="true"><div data-slot="skeleton"></div><div data-slot="region-failed">Could not load.</div></div>`;

const page = install({
  url: "https://app.test/deals",
  html: `<!doctype html><html data-build="b1"><head></head><body data-ui-boost>${region("down")}${region("planted")}${region("fine")}</body></html>`,
});
const PAGE = { "content-type": "text/html; charset=utf-8", "x-navree-page": "1" };
// The three lazy regions are fetched in the order they stand: no connection, an answer that is no page of this app, a page.
page.answers.push(
  new TypeError("Failed to fetch"),
  answer("<p>somebody's file</p>", { status: 502, headers: { "content-type": "text/html" } }),
  answer(`<p id="total">₹1,00,000</p>`, { headers: { ...PAGE, "x-navree-fragment": "1" } }),
);

const { refresh } = await import("../src/nav.js");
await import("../src/core.js");
await settle();

const $ = (/** @type {string} */ id) => /** @type {any} */ (document.getElementById(id));

test("a lazy region whose fetch got no answer is failed, and no longer busy", () => {
  assert.equal($("down").getAttribute("data-ui-failed"), "0");
  assert.equal($("down").hasAttribute("aria-busy"), false);
});

test("a lazy region answered by something that is no page of this app is failed, and no longer busy", () => {
  assert.equal($("planted").getAttribute("data-ui-failed"), "502");
  assert.equal($("planted").hasAttribute("aria-busy"), false);
});

test("a lazy region that got its content is not busy either, and is not failed", () => {
  assert.equal($("fine").hasAttribute("aria-busy"), false);
  assert.equal($("fine").hasAttribute("data-ui-failed"), false);
  assert.equal($("total").textContent, "₹1,00,000");
});

test("a failed region asked again draws its answer", async () => {
  page.answers.push(answer(`<p id="back">₹2,50,000</p>`, { headers: { ...PAGE, "x-navree-fragment": "1" } }));
  await refresh("down");
  assert.equal($("back").textContent, "₹2,50,000");
  assert.equal($("down").hasAttribute("data-ui-failed"), false);
  assert.equal($("down").hasAttribute("aria-busy"), false);
});
