// The slider, tree and toolbar modules against hostile markup and a hostile server, in Chromium and
// Firefox: the committed dist/ under the page's exact content security policy, served by
// tests/support/fixture.mjs, with a second origin that records anything sent to it. Named elements
// that stand in for DOM methods, a lazy branch pointed off the site, answers full of markup and of ids
// the page already has, a thousand items at once, and values that are not numbers.
//
//   UI_BROWSERS=chromium,firefox,webkit node --test tests/flow-hostile.test.mjs

import assert from "node:assert/strict";
import { after, before, describe, test } from "node:test";
import { chromium, firefox, webkit } from "playwright";
import { CSP, start } from "./support/fixture.mjs";

const ENGINES = { chromium, firefox, webkit };
const NAMES = (process.env.UI_BROWSERS || "chromium,firefox").split(/[ ,]+/).filter((n) => ENGINES[n]);
let unique = 0;
const key = (name) => `${name}-${++unique}`;
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const until = async (done, ms = 5000) => {
  for (const end = Date.now() + ms; !done() && Date.now() < end; ) await sleep(20);
};

/** Names that, on an element of a form or on the document, stand in for what the modules call. */
const NAMED = ["querySelector", "querySelectorAll", "closest", "getAttribute", "setAttribute", "hasAttribute", "removeAttribute", "toggleAttribute", "contains", "matches", "focus", "children", "id", "fonts", "body", "cookie", "forms", "reset"].map((n) => `<img name="${n}" src="data:image/gif;base64,R0lGODlhAQABAAAAACw=">`).join("");

/** A tree as the server writes it. */
const item = (text, { open = false, lazy = "", kids = "", level = 1, checked = false } = {}) =>
  `<li data-slot="tree-item" data-level="${level}" data-setsize="9" data-posinset="1"${kids || lazy ? " data-branch" : ""}${open ? " data-open" : ""}${lazy ? " data-lazy" : ""}><div data-slot="tree-row">${kids || lazy ? `<span data-slot="tree-toggle" aria-hidden="true"><svg width="12" height="12" viewBox="0 0 12 12"><path d="M3 2l6 4-6 4z"/></svg></span>` : ""}<label data-slot="tree-label"><input data-slot="checkbox" type="checkbox" name="n" value="${text}"${checked ? " checked" : ""}><span data-slot="tree-text">${text}</span></label></div>${kids ? `<ul data-slot="tree-group">${kids}</ul>` : lazy ? `<ul data-slot="tree-group" data-src="${lazy}"></ul>` : ""}</li>`;
const tree = (id, inner, cascade = true) =>
  `<div data-slot="tree" id="${id}" data-ui-tree data-select="multiple"${cascade ? " data-cascade" : ""} data-label="Tree" data-l='{"loading":"Loading","failed":"Failed","retry":"Again","loaded":"{n} loaded"}'><ul data-slot="tree-root">${inner}</ul><div data-slot="tree-status" role="status" aria-live="polite"></div></div>`;
const module = (names) => names.map((name) => `<script type="module" src="/_navree/ui/${name}.js"></script>`).join("");
const shell = (body, modules, head = "") => `<!doctype html><html lang="en"><head><meta charset="utf-8"><title>t</title>${module(modules)}${head}</head><body>${body}</body></html>`;

for (const name of NAMES) {
  describe(name, { concurrency: false }, () => {
    let browser, f;
    before(async () => {
      browser = await ENGINES[name].launch();
      f = await start();
    });
    after(async () => {
      await browser.close();
      f.close();
    });

    const route = (path, body, headers) => f.routes.set(path, (req, res) => f.html(res, typeof body === "function" ? body() : body, headers));
    const fragment = (path, text) =>
      f.routes.set(path, (req, res) => {
        res.writeHead(200, { "content-type": "text/html; charset=utf-8", "content-security-policy": CSP, "cache-control": "no-store" });
        res.end(typeof text === "function" ? text() : text);
      });
    const hits = (path) => f.log.filter((entry) => entry.path.split("?")[0] === path);

    async function fresh(path) {
      const context = await browser.newContext();
      const page = await context.newPage();
      const errors = [];
      const reports = [];
      page.on("pageerror", (error) => errors.push(String(error)));
      await page.addInitScript(() => {
        document.addEventListener("securitypolicyviolation", (e) => (window.__csp ||= []).push(e.violatedDirective));
      });
      await page.goto(f.origin + path, { waitUntil: "load" });
      await sleep(100);
      return { context, page, errors, reports };
    }
    /** Focuses by the prototype: the test's own selector engine would read a name in the page instead of the method it wants. */
    const focus = (page, selector) => page.evaluate((s) => HTMLElement.prototype.focus.call(Document.prototype.querySelector.call(document, s)), selector);
    const q = (page, selector) => page.evaluate((s) => Document.prototype.querySelectorAll.call(document, s).length, selector);

    test(`names that stand in for DOM methods leave the tree, the slider and the toolbar working (${name})`, async () => {
      const path = key("named");
      const slider = `<form id="sf" method="post" action="/x"><input name="elements"><input name="querySelectorAll"><div data-slot="slider" data-ui-slider data-decimals="0"><div data-slot="slider-control"><svg data-slot="slider-graphic"><rect data-slot="slider-track"/><rect data-slot="slider-fill" x="0%" width="40%"/></svg><input data-slot="slider-input" id="s1" type="range" name="s" min="0" max="100" step="5" value="40"></div><output for="s1">40</output></div></form>`;
      const bar = `<div data-slot="toolbar" id="tb" role="group" aria-label="Bar" data-ui-toolbar data-orientation="horizontal" data-loop><div data-slot="toolbar-tool" data-tool="0" data-kind="tool"><button id="b1" type="button">One</button></div><div data-slot="toolbar-tool" data-tool="1" data-kind="tool"><button id="b2" type="button">Two</button></div></div>`;
      route(`/${path}`, shell(NAMED + tree("t", item("A", { open: true, kids: item("A1", { level: 2 }) + item("A2", { level: 2 }) }) + item("B")) + slider + bar, ["tree", "slider", "toolbar"]));
      const { context, page, errors } = await fresh(`/${path}`);
      // The tree: roles, one stop, keys.
      assert.equal(await q(page, "#t [role=treeitem]"), 4);
      await focus(page, "#t [role=treeitem][tabindex='0']");
      await page.keyboard.press("ArrowDown");
      assert.equal(await page.evaluate(() => Document.prototype.querySelector.call(document, "#t [role=treeitem]:focus [data-slot=tree-text]")?.textContent), "A1");
      await page.keyboard.press("Space");
      assert.equal(await page.evaluate(() => Document.prototype.querySelector.call(document, "#t input:checked")?.value), "A1");
      assert.equal(await page.evaluate(() => Document.prototype.querySelector.call(document, "#t [role=treeitem]").getAttribute("aria-checked")), "mixed");
      // The slider: a key moves it and the written value follows; a reset puts it back.
      await focus(page, "#s1");
      await page.keyboard.press("ArrowRight");
      assert.equal(await page.evaluate(() => Document.prototype.querySelector.call(document, "output").textContent), "45");
      await page.evaluate(() => HTMLFormElement.prototype.reset.call(Document.prototype.getElementById.call(document, "sf")));
      await sleep(100);
      assert.equal(await page.evaluate(() => Document.prototype.querySelector.call(document, "output").textContent), "40");
      // The toolbar: one stop and the arrows.
      assert.equal(await page.evaluate(() => Document.prototype.getElementById.call(document, "tb").getAttribute("role")), "toolbar");
      await focus(page, "#b1");
      await page.keyboard.press("ArrowRight");
      assert.equal(await page.evaluate(() => Document.prototype.getElementById.call(document, "b2") === document.activeElement), true);
      assert.deepEqual(errors, []);
      assert.deepEqual(await page.evaluate(() => window.__csp || []), []);
      await context.close();
    });

    test(`a lazy branch pointed off the site asks nobody, and says it failed (${name})`, async () => {
      const path = key("offsite");
      route(`/${path}`, shell(tree("t", item("A", { lazy: `${f.evilOrigin}/children` }) + item("B", { lazy: `//127.0.0.1:1/x` }) + item("C", { lazy: `javascript:alert(1)` })), ["tree"]));
      const { context, page, errors } = await fresh(`/${path}`);
      for (const text of ["A", "B", "C"]) {
        await page.locator(`#t li:has(> [data-slot=tree-row] [data-slot=tree-text]:text-is("${text}")) > [data-slot=tree-row] > [data-slot=tree-toggle]`).click();
        await sleep(150);
      }
      assert.equal(f.evil.length, 0, "the other site was never asked");
      assert.equal(await q(page, "#t [data-slot=tree-note] [data-slot=tree-retry]"), 3);
      assert.deepEqual(errors, []);
      assert.deepEqual(await page.evaluate(() => window.__csp || []), []);
      await context.close();
    });

    test(`an answer full of markup brings in the items and nothing else, and no id the page has (${name})`, async () => {
      const path = key("answer");
      route(`/${path}`, shell(`<p id="victim">the page's own</p>` + tree("t", item("A", { lazy: `/${path}-kids` })), ["tree"]));
      fragment(
        `/${path}-kids`,
        () =>
          `<script>window.__pwned=1</script><meta http-equiv="refresh" content="0;url=${f.evilOrigin}/"><base href="${f.evilOrigin}/">` +
          item("ok").replace('<li ', '<li onclick="window.__pwned=2" ') +
          `<li data-slot="tree-item" id="victim"><div data-slot="tree-row"><span data-slot="tree-text">takes an id</span></div></li>` +
          `<li data-slot="tree-item"><div data-slot="tree-row"><span data-slot="tree-text" id="victim">hides one</span></div></li>` +
          `<div data-slot="tree-item">not a list item</div><img src="${f.evilOrigin}/pixel" name="body"><iframe src="${f.evilOrigin}/frame"></iframe>`,
      );
      const { context, page, errors } = await fresh(`/${path}`);
      await page.locator("#t [data-slot=tree-toggle]").click();
      await until(() => hits(`/${path}-kids`).length);
      await sleep(300);
      const texts = await page.evaluate(() => [...Document.prototype.querySelectorAll.call(document, "#t [data-slot=tree-text]")].map((e) => e.textContent));
      assert.deepEqual(texts, ["A", "ok"], "only the list item that brought nothing the page has");
      assert.equal(await page.evaluate(() => window.__pwned), undefined, "no script ran");
      assert.equal(await page.evaluate(() => Document.prototype.getElementById.call(document, "victim").textContent), "the page's own");
      assert.equal(f.evil.length, 0, "nothing was fetched from the other site");
      assert.equal(await page.evaluate(() => location.origin), f.origin, "the page did not leave");
      assert.equal(await q(page, "base, meta[http-equiv]"), 0);
      assert.deepEqual(errors, []);
      await context.close();
    });

    test(`an answer that redirects, is not a page, or has the wrong type is a failure, not a swap (${name})`, async () => {
      const path = key("bad");
      route(`/${path}`, shell(tree("t", item("A", { lazy: `/${path}-redirect` }) + item("B", { lazy: `/${path}-json` }) + item("C", { lazy: `/${path}-500` })), ["tree"]));
      f.routes.set(`/${path}-redirect`, (req, res) => {
        res.writeHead(302, { location: `${f.evilOrigin}/x` });
        res.end();
      });
      f.routes.set(`/${path}-json`, (req, res) => {
        res.writeHead(200, { "content-type": "application/json" });
        res.end(JSON.stringify({ html: "<li>x</li>" }));
      });
      f.routes.set(`/${path}-500`, (req, res) => {
        res.writeHead(500, { "content-type": "text/html" });
        res.end(item("never"));
      });
      const { context, page, errors } = await fresh(`/${path}`);
      for (const text of ["A", "B", "C"]) {
        await page.locator(`#t li:has(> [data-slot=tree-row] [data-slot=tree-text]:text-is("${text}")) > [data-slot=tree-row] > [data-slot=tree-toggle]`).click();
        await sleep(250);
      }
      assert.equal(await q(page, "#t [data-slot=tree-retry]"), 3);
      assert.equal(await q(page, "#t [data-slot=tree-text]"), 3, "none of the answers was put in");
      assert.equal(f.evil.length, 0);
      // WebKit reports the refused cross-site redirect itself; the module has caught it and says it failed.
      assert.deepEqual(errors.filter((e) => !/access control checks/.test(e)), []);
      await context.close();
    });

    test(`a thousand items in one answer are put in at once, and the tree still answers its keys (${name})`, async () => {
      const path = key("many");
      route(`/${path}`, shell(tree("t", item("A", { lazy: `/${path}-kids` })), ["tree"]));
      fragment(`/${path}-kids`, () => Array.from({ length: 5000 }, (_, n) => item(`child ${n}`, { level: 2 })).join(""));
      const { context, page, errors } = await fresh(`/${path}`);
      const began = Date.now();
      await page.locator("#t [data-slot=tree-toggle]").click();
      await page.waitForSelector("#t [data-slot=tree-text]:text-is('child 4999')", { state: "attached", timeout: 15000 });
      assert.ok(Date.now() - began < 8000, `${Date.now() - began} ms`);
      assert.equal(await q(page, "#t [role=treeitem]"), 5001);
      assert.equal(await page.evaluate(() => [...Document.prototype.querySelectorAll.call(document, "#t [role=treeitem]")].filter((e) => e.getAttribute("tabindex") === "0").length), 1);
      await page.keyboard.press("End");
      assert.deepEqual(errors, []);
      await context.close();
    });

    test(`values that are not numbers, hostile names and markup in a slider's data are text, and the fill stays inside (${name})`, async () => {
      const path = key("slider");
      route(
        `/${path}`,
        shell(
          `<div data-slot="slider" data-ui-slider data-decimals="9999999999" data-prefix="&lt;img src=x onerror=window.__pwned=1&gt;" data-suffix="&quot;&gt;&lt;b&gt;" data-names='["__proto__","constructor",{"a":1},null,"&lt;i&gt;"]' data-big="Infinity" data-gap="-5" data-range data-to="&lt;u&gt;"><div data-slot="slider-control"><svg data-slot="slider-graphic"><rect data-slot="slider-track"/><rect data-slot="slider-fill" x="0%" width="0%"/></svg><input id="lo" data-slot="slider-input" type="range" min="0" max="4" step="1" value="1"><input id="hi" data-slot="slider-input" type="range" min="0" max="4" step="1" value="3"></div><output>x</output></div><div data-slot="slider" data-ui-slider data-names="not json {"><div data-slot="slider-control"><svg data-slot="slider-graphic"><rect data-slot="slider-fill" x="0%" width="0%"/></svg><input id="odd" type="range" min="5" max="5" step="any" value="5"></div><output>y</output></div>`,
          ["slider"],
        ),
      );
      const { context, page, errors } = await fresh(`/${path}`);
      await page.focus("#hi");
      for (const keyName of ["ArrowRight", "ArrowRight", "PageDown", "Home", "End", "Shift+ArrowLeft"]) await page.keyboard.press(keyName);
      await page.focus("#odd");
      await page.keyboard.press("ArrowRight");
      await page.keyboard.press("PageUp");
      assert.equal(await page.evaluate(() => window.__pwned), undefined);
      assert.equal(await q(page, "output img, output b, output i, output u"), 0, "the value is text");
      const widths = await page.evaluate(() => [...Document.prototype.querySelectorAll.call(document, "[data-slot=slider-fill]")].map((e) => [e.getAttribute("x"), e.getAttribute("width")]));
      for (const [x, width] of widths) {
        assert.ok(parseFloat(x) >= 0 && parseFloat(x) + parseFloat(width) <= 100.001, `${x} ${width}`);
        assert.ok(!/NaN|Infinity/.test(x + width), `${x} ${width}`);
      }
      assert.deepEqual(errors, []);
      assert.deepEqual(await page.evaluate(() => window.__csp || []), []);
      await context.close();
    });

    test(`a toolbar with twins that name nothing, or name something odd, forwards nothing it should not (${name})`, async () => {
      const path = key("toolbar");
      const real = (n, key) => `<div data-slot="toolbar-tool" data-tool="${n}" data-kind="tool"><label data-slot="toolbar-toggle"><input type="checkbox" name="t${n}" data-twin-key="${key}" aria-label="T${n}"></label></div>`;
      const twin = (n, key) => `<label data-slot="menu-item" data-choice="check" data-twin="${n}" hidden><input type="checkbox" role="menuitemcheckbox" name="" data-twin-key='${key}'><span data-slot="menu-text">T${n}</span></label>`;
      route(
        `/${path}`,
        shell(
          `<div data-slot="toolbar" id="tb" role="group" aria-label="Bar" data-ui-toolbar data-orientation="horizontal" data-loop>${real(0, "0.1")}${real(1, "1.1")}<div data-slot="toolbar-more"><button id="more" data-slot-more hidden type="button" popovertarget="m">More</button><div id="m" popover data-ui-menu role="menu">${twin(0, "0.1")}${twin(1, '1.1"] , body {x:y} [x="')}</div></div></div><form id="f"><input name="elements"></form>`,
          ["toolbar", "menus", "overlay"],
        ),
      );
      const { context, page, errors } = await fresh(`/${path}`);
      await page.evaluate(() => Document.prototype.getElementById.call(document, "m").querySelector("input[data-twin-key^='1']").click());
      await sleep(100);
      assert.equal(await page.evaluate(() => Document.prototype.querySelector.call(document, "input[name=t1]").checked), false, "a key that is not the module's own forwards nothing");
      await page.evaluate(() => Document.prototype.getElementById.call(document, "m").querySelector("input[data-twin-key='0.1']").click());
      assert.equal(await page.evaluate(() => Document.prototype.querySelector.call(document, "input[name=t0]").checked), true, "the one that is its own does");
      assert.deepEqual(errors, []);
      await context.close();
    });
  });
}
