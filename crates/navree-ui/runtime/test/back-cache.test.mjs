// A page the browser kept for the back button is drawn for whoever was signed in when it was drawn:
// when it comes back and someone else (or no one) is signed in, it loads again. Who it was drawn for is
// noted when the page loads and when a page is swapped in, never as it is left: by then the next
// document's cookies are already stored.
import { test } from "node:test";
import assert from "node:assert/strict";
import { answer, install, settle } from "./fake-dom.mjs";

const page = install({
  url: "https://app.test/accounts",
  cookie: "XSRF-TOKEN=harleen-token",
  html: `<!doctype html><html data-build="b1"><head></head><body data-ui-boost>
    <main id="main">Accounts <a id="next" href="/accounts/2">Next</a></main>
  </body></html>`,
});

await import("../src/core.js");
await settle();

const $ = (/** @type {string} */ id) => /** @type {any} */ (document.getElementById(id));
const PAGE = { "content-type": "text/html; charset=utf-8", "x-navree-page": "1" };
const signIn = (/** @type {string} */ token) => (document.cookie = `XSRF-TOKEN=${token}`);
const signOut = () => (document.cookie = "XSRF-TOKEN=");
/** What happens as a page goes into the cache and comes back: the next document stored its cookies first. */
const leaveAndReturn = () => {
  dispatchEvent(new Event("pagehide", { persisted: true }));
  dispatchEvent(new Event("pageshow", { persisted: true }));
};

test("a page that comes back from the cache for the person it was drawn for stays", () => {
  leaveAndReturn();
  assert.equal(page.reloaded, 0);
});

test("a page that comes back from the cache after a sign-out, and another person's sign-in, loads again", () => {
  // The document that signed out stored its cookies before this one was hidden.
  signIn("gurpreet-token");
  leaveAndReturn();
  assert.equal(page.reloaded, 1);
});

test("a page that comes back from the cache after a sign-out, with no one signed in, loads again", () => {
  signOut();
  leaveAndReturn();
  assert.equal(page.reloaded, 2);
  // The same person again: it is theirs.
  signIn("harleen-token");
  leaveAndReturn();
  assert.equal(page.reloaded, 2);
});

test("a page swapped in for the person signed in then is theirs when it comes back", async () => {
  page.reloaded = 0;
  // Another person's sign-in is stored by the answer to this boosted link.
  page.answers.push(() => {
    signIn("gurpreet-token");
    return answer(`<!doctype html><html data-build="b1"><head><title>Accounts</title></head><body data-ui-boost><main id="main">Gurpreet's accounts</main></body></html>`, { headers: PAGE });
  });
  $("next").click();
  await settle();
  assert.equal($("main").textContent, "Gurpreet's accounts");
  // Back from the cache with the person the swapped page was drawn for.
  dispatchEvent(new Event("pageshow", { persisted: true }));
  assert.equal(page.reloaded, 0);
  signIn("harleen-token");
  leaveAndReturn();
  assert.equal(page.reloaded, 1);
});
