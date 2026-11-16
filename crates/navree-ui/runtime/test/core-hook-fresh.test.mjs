// A page that starts clean (it carries `data-ui-fresh-page`) keeps its readers on pages that start clean: a
// swap to another such page is boosted as any is, and a swap to a plain page is a full load.
import { test } from "node:test";
import assert from "node:assert/strict";
import { answer, install, settle } from "./fake-dom.mjs";

const links = `<a id="to-reset" href="/forgot-password">Forgot your password?</a><a id="to-home" href="/">Home</a>`;
const page = install({
  url: "https://app.test/sign-in",
  html: `<!doctype html><html data-build="b1" data-ui-fresh-page><head></head><body data-ui-boost>
    <main id="main"><h1 id="heading">Sign in</h1>${links}</main>
  </body></html>`,
});

await import("../src/core.js");
await settle();

const $ = (/** @type {string} */ id) => /** @type {any} */ (document.getElementById(id));
const PAGE = { "content-type": "text/html; charset=utf-8", "x-navree-page": "1" };
/** A page of the app, one that starts clean or a plain one. @param {boolean} fresh @param {string} heading */
const html = (fresh, heading) =>
  `<!doctype html><html data-build="b1"${fresh ? " data-ui-fresh-page" : ""}><head><title>${heading}</title></head><body data-ui-boost><main id="main"><h1 id="heading">${heading}</h1>${links}</main></body></html>`;

test("a page swap between a fresh and a plain page is a full load; two fresh pages boost", async () => {
  // Another page that starts clean is drawn in place.
  page.answers.push(answer(html(true, "Reset your password"), { headers: PAGE }));
  $("to-reset").click();
  await settle();
  assert.deepEqual(page.assigned, [], "pages that start clean lost their boosting");
  assert.equal($("heading").textContent, "Reset your password");
  assert.deepEqual(page.pushed, ["https://app.test/forgot-password"]);

  // A plain page is a new document: what the page that starts clean held does not follow the reader into it.
  page.answers.push(answer(html(false, "Deliveries"), { headers: PAGE }));
  $("to-home").click();
  await settle();
  assert.deepEqual(page.assigned, ["https://app.test/"], "a plain page was drawn into a page that starts clean");
  assert.equal($("heading").textContent, "Reset your password");
});
