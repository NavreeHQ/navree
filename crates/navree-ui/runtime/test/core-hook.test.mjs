// A module that needs core's own write path, op applier and announcer asks for them with a `ui:core`
// event, and keeps no copy of their state. And a page that starts clean (it carries `data-ui-fresh-page`) is
// never drawn into a plain page: it is entered by a full load, whichever fetch brought it (a link, a
// form, a prefetched answer, a region, a lazy part), and a feed takes no rows from one.
import { test } from "node:test";
import assert from "node:assert/strict";
import { answer, fire, install, settle } from "./fake-dom.mjs";

const page = install({
  url: "https://app.test/deliveries",
  html: `<!doctype html><html data-build="b1"><head></head><body data-ui-boost>
    <main id="main">
      <a id="to-sign-in" href="/sign-in" data-ui-prefetch>Sign in</a>
      <form id="out" data-ui-form method="post" action="/sign-out"><button id="sign-out" type="submit">Sign out</button></form>
      <div id="security" data-ui-region data-ui-src="/account/password" data-ui-lazy>Loading</div>
      <div id="recent" data-ui-region data-ui-src="/activity" data-ui-feed="recent-rows"><ul id="recent-rows"></ul></div>
      <div id="zones" data-ui-region data-ui-src="/account/zones"><a id="zone-link" href="/account/zones" data-ui-in-region>Zones</a></div>
    </main>
  </body></html>`,
});

// The fake document keeps what is written to a template out of its content, where a feed reads its rows.
const written = /** @type {PropertyDescriptor} */ (Object.getOwnPropertyDescriptor(Element.prototype, "innerHTML"));
Object.defineProperty(HTMLTemplateElement.prototype, "innerHTML", {
  configurable: true,
  get: written.get,
  set(/** @type {string} */ text) {
    const holder = document.createElement("div");
    written.set?.call(holder, text);
    /** @type {any} */ (this).content.replaceChildren(...holder.childNodes);
  },
});

const PAGE = { "content-type": "text/html; charset=utf-8", "x-navree-page": "1" };
const SIGN_IN = `<!doctype html><html data-build="b1" data-ui-fresh-page><head><title>Sign in</title></head><body data-ui-boost>
  <main id="main"><h1>Sign in</h1><form method="post" action="/sign-in"><input type="password" name="password"></form></main>
</body></html>`;
/** What a page that starts clean answers. @param {{url?: string, redirected?: boolean}} [init] */
const freshPage = (init = {}) => answer(SIGN_IN, { headers: PAGE, ...init });

// The lazy part of the page asks as the page starts, and is answered by a page that starts clean.
page.answers.push(freshPage());
await import("../src/core.js");
await settle();

const $ = (/** @type {string} */ id) => /** @type {any} */ (document.getElementById(id));
const paths = () => page.fetched.map((f) => new URL(f.url).pathname);
const noPassword = () => document.querySelector("input[type=password]") === null;

test("ui:core gives an object detail the three functions and ignores any other detail", async () => {
  const { applyEnvelope } = await import("../src/ops.js");
  const { whileWriting } = await import("../src/nav.js");
  const { announce } = await import("../src/ui.js");
  const ask = (/** @type {any} */ detail) => {
    document.dispatchEvent(new CustomEvent("ui:core", { detail }));
    return detail;
  };
  const thrown = page.errors.length;

  const given = ask({});
  assert.equal(typeof given.apply, "function");
  assert.equal(typeof given.writing, "function");
  assert.equal(typeof given.say, "function");
  assert.equal(given.apply, applyEnvelope);
  assert.equal(given.writing, whileWriting);
  assert.equal(given.say, announce);
  assert.equal(await given.writing(async () => 5), 5);

  // A module that already holds an `apply` keeps it, and is given nothing besides.
  const held = ask({ apply: 1 });
  assert.equal(held.apply, 1);
  assert.equal("writing" in held, false);
  assert.equal("say" in held, false);

  // A server-drawn `Cmd::dispatch("ui:core")` sends no detail; nothing else that is no object is taken either.
  for (const other of [null, "x", 7]) ask(other);
  assert.equal(page.errors.length, thrown);
});

test("a page swap between a fresh and a plain page is a full load; two fresh pages boost", async () => {
  // A link.
  page.assigned.length = 0;
  page.answers.push(freshPage());
  $("to-sign-in").click();
  await settle();
  assert.deepEqual(page.assigned, ["https://app.test/sign-in"], "a link to a page that starts clean must load it in full");

  // A form whose answer is such a page, after a redirect.
  page.assigned.length = 0;
  page.answers.push(freshPage({ url: "https://app.test/sign-in", redirected: true }));
  $("sign-out").click();
  await settle();
  assert.deepEqual(page.assigned, ["https://app.test/sign-in"], "a form's answer that starts clean must load in full");

  // An answer fetched ahead, by a hover, is judged as any other is.
  page.assigned.length = 0;
  page.fetched.length = 0;
  page.answers.push(freshPage());
  fire($("to-sign-in"), "mouseover");
  await new Promise((done) => setTimeout(done, 150));
  assert.equal(page.fetched.length, 1);
  assert.equal(page.fetched[0].init.headers["x-navree-prefetch"], "1");
  $("to-sign-in").click();
  await settle();
  assert.equal(page.fetched.length, 1, "the click used the answer fetched ahead");
  assert.deepEqual(page.assigned, ["https://app.test/sign-in"], "an answer fetched ahead that starts clean must load in full");

  // A region's own link: the region of a plain page is not given a page that starts clean.
  page.assigned.length = 0;
  page.answers.push(freshPage());
  $("zone-link").click();
  await settle();
  assert.deepEqual(page.assigned, ["https://app.test/account/zones"], "a region's answer that starts clean must load in full");

  assert.equal(noPassword(), true, "a field of a page that starts clean was drawn into a plain page");
});

test("a fresh page's answer to a region or lazy part of a plain page is not drawn", async () => {
  // The lazy part asked as the page started.
  assert.equal($("security").getAttribute("data-ui-failed"), "403");
  assert.equal(noPassword(), true);

  page.fetched.length = 0;
  page.answers.push(freshPage());
  fire($("security"), "ui:refresh");
  await settle();
  assert.deepEqual(paths(), ["/account/password"]);
  assert.equal($("security").getAttribute("data-ui-failed"), "403");
  assert.equal(noPassword(), true);
});

test("a fresh answer to a feed is not drawn", async () => {
  const rows = (/** @type {Record<string, string>} */ headers) => ({ headers: { "content-type": "text/html; charset=utf-8", ...headers } });
  page.answers.push(answer(`<li id="r1">Harpreet Sandhu signed in</li>`, rows({ "x-navree-fresh": "1" })));
  fire($("recent"), "ui:refresh");
  await settle();
  assert.equal($("recent-rows").children.length, 0);
  assert.equal($("recent").getAttribute("data-ui-failed"), "403");

  // The same feed takes the rows of a plain answer.
  page.answers.push(answer(`<li id="r2">Meera Sharma signed in</li>`, rows({})));
  fire($("recent"), "ui:refresh");
  await settle();
  assert.deepEqual([...$("recent-rows").children].map((li) => li.id), ["r2"]);
});
