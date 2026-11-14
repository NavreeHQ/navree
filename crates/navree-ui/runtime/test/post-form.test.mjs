// The page's one post form (`ui-post`) posts, with the page's token, only for a button the server
// drew for it; a button in stored content sends no form at all.
import { test } from "node:test";
import assert from "node:assert/strict";
import { answer, install, settle } from "./fake-dom.mjs";

const page = install({
  url: "https://app.test/deals",
  cookie: "XSRF-TOKEN=tok",
  html: `<!doctype html><html data-build="b1"><head></head><body data-ui-boost>
    <main id="main">
      <button id="archive" type="submit" form="ui-post" formaction="/deals/1/archive" formmethod="post" data-ui-post>Archive</button>
      <button id="unmarked" type="submit" form="ui-post" formaction="/account/delete" formmethod="post">Delete</button>
      <form id="edit" method="post" action="/deals/1" data-ui-form>
        <input type="hidden" name="_csrf" value="tok">
        <div data-ui-stored><button id="planted" type="submit" formaction="/account/delete">Go</button></div>
        <button id="save" type="submit">Save</button>
      </form>
    </main>
    <div data-ui-stored><form id="planted-form" method="post" action="/account/email" data-ui-form><button id="planted-send" type="submit">Send</button></form></div>
    <form id="ui-post" method="post" novalidate hidden data-ui-form><input type="hidden" name="_csrf" value="tok"></form>
  </body></html>`,
});

await import("../src/core.js");

const $ = (/** @type {string} */ id) => /** @type {any} */ (document.getElementById(id));
const sent = () => page.fetched.map((f) => `${f.init?.method || "GET"} ${new URL(f.url).pathname}`);

test("a post button the server marked posts where it says, with the token", async () => {
  page.fetched.length = 0;
  page.answers.push(answer(`{"ops":[]}`, { headers: { "content-type": "application/vnd.navree.ops+json" } }));
  $("archive").click();
  await settle();
  assert.deepEqual(sent(), ["POST /deals/1/archive"]);
  assert.equal(page.fetched[0].init.headers["x-xsrf-token"], "tok");
});

test("a button that names the post form without the server's mark sends nothing", async () => {
  page.fetched.length = 0;
  page.submitted.length = 0;
  $("unmarked").click();
  await settle();
  assert.deepEqual(sent(), []);
  // Nor does the browser send it in the runtime's place.
  assert.deepEqual(page.submitted, []);
});

test("a form in stored content is the browser's to send, with no token in a header", async () => {
  page.fetched.length = 0;
  page.submitted.length = 0;
  $("planted-send").click();
  await settle();
  assert.deepEqual(sent(), []);
  assert.equal(page.submitted.length, 1);
});

test("a button in stored content sends no form", async () => {
  page.fetched.length = 0;
  page.submitted.length = 0;
  $("planted").click();
  await settle();
  assert.deepEqual(sent(), []);
  assert.deepEqual(page.submitted, []);
  page.answers.push(answer(`{"ops":[]}`, { headers: { "content-type": "application/vnd.navree.ops+json" } }));
  $("save").click();
  await settle();
  assert.deepEqual(sent(), ["POST /deals/1"]);
});
