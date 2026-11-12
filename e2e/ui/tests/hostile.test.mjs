// The runtime against a hostile server and hostile markup, in Chromium and
// Firefox (UI_BROWSERS=chromium,firefox,webkit adds WebKit). The committed dist/ runs under the page's exact content security
// policy, served by tests/support/fixture.mjs; the last section uses the proof app.
// Each test states what must hold, so taking the protection away fails it.

import assert from "node:assert/strict";
import { after, before, describe, test } from "node:test";
import { chromium, firefox, webkit } from "playwright";
import { CSP, page as doc, start } from "./support/fixture.mjs";
import { launch } from "./support/launch.mjs";
import { lateBody } from "./support/late.mjs";

const PROOF = process.env.UI_PROOF_URL || "http://127.0.0.1:8090";
// The short waits are for something to have happened: room for a busy machine.
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms <= 300 ? ms * 2.5 : ms));
const until = async (done, ms = 5000) => {
  for (const end = Date.now() + ms; !done() && Date.now() < end; ) await new Promise((resolve) => setTimeout(resolve, 20));
};
const GIF = "data:image/gif;base64,R0lGODlhAQABAAAAACw=";
let unique = 0;
const key = (name) => `${name}-${++unique}`;

// Chromium and Firefox; UI_BROWSERS=chromium,firefox,webkit adds WebKit.
const ENGINES = { chromium, firefox, webkit };
const NAMES = (process.env.UI_BROWSERS || "chromium,firefox").split(",").filter((name) => ENGINES[name]);
for (const [engine, launcher] of NAMES.map((name) => [name, ENGINES[name]])) {
  describe(engine, () => {
    let browser, f;

    before(async () => {
      // Firefox slows a page's requests while another load runs; the saves here are timed.
      browser = await launch(launcher, engine === "firefox" ? { firefoxUserPrefs: { "network.http.throttle.enable": false } } : {});
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

    test(`named elements can't break the phone input (${engine})`, async () => {
      const path = key("phone");
      // Images named after what the module reads from the document, and form
      // fields named after the methods it calls on event targets.
      const clobber = `<img name="createTextNode" src="${GIF}" alt=""><img name="documentElement" src="${GIF}" alt=""><img name="documentElement" src="${GIF}" alt=""><img name="activeElement" src="${GIF}" alt="">`;
      const phone = `<div data-slot="phone-input" data-ui-phone data-ui-keep-attrs="data-enhanced">
        <select data-slot="phone-country" name="phone_country" aria-label="Country" data-field-skip><option value="">Country</option><option value="FR">France +33</option><option value="GB" selected>United Kingdom +44</option></select>
        <button type="button" id="trigger" data-slot="phone-country-trigger" popovertarget="phone-countries" aria-haspopup="listbox" aria-expanded="false" aria-label="Country: United Kingdom +44" data-field-skip><span data-slot="phone-flag" aria-hidden="true"></span><span data-slot="phone-dial">+44</span></button>
        <div data-slot="phone-countries" id="phone-countries" popover="auto" data-side="bottom" data-align="start" data-ui-place data-ui-keep data-label-search="Search countries" data-label-empty="No country found." data-label-count="{n} countries" data-preferred=""></div>
        <input data-slot="input" id="tel" type="tel" name="phone" autocomplete="tel" inputmode="tel" dir="ltr">
        <input data-slot="phone-e164" type="hidden" name="phone_e164" value="" disabled data-ui-keep-attrs="disabled value">
        <span data-slot="phone-status" role="status" aria-live="polite" data-label-country-set="Country set to {country}"></span>
      </div>`;
      // What a page with a phone input loads: the phone and overlay modules.
      route(`/${path}`, `<!doctype html><html lang="en"><head><meta charset="utf-8"><title>t</title><script type="module" src="/_navree/ui/overlay.js"></script><script type="module" src="/_navree/ui/phone.js"></script></head><body>${clobber}<form id="f" method="post" action="/${path}-post"><input name="closest" value="x"><input name="matches" value="x">${phone}</form></body></html>`);
      const { context, page, errors } = await fresh(`/${path}`);
      const read = () => page.evaluate(() => {
        const q = (s) => Document.prototype.querySelector.call(document, s);
        return {
          enhanced: Element.prototype.hasAttribute.call(q("[data-ui-phone]"), "data-enhanced"),
          country: q("select[name=phone_country]").value,
          text: q("#tel").value,
          e164: q("input[name=phone_e164]").value,
          options: Document.prototype.querySelectorAll.call(document, "#phone-countries [role=option]").length,
          flag: q("[data-slot=phone-flag]").textContent,
        };
      });
      const eventually = async (check) => {
        for (const end = Date.now() + 5000; Date.now() < end; await sleep(20)) if (await check()) return;
      };
      await eventually(async () => (await read()).enhanced);
      // A click on the form itself goes through a target whose fields shadow its methods.
      await page.evaluate(() => Document.prototype.getElementById.call(document, "f").click());
      // The list opens, draws its flags and picks a country.
      await page.click("#trigger");
      await eventually(async () => (await read()).options > 0);
      await page.click("#phone-countries [role=option][data-value=FR]");
      await eventually(async () => (await read()).country === "FR");
      // A pasted number of another country formats and picks its own.
      await page.click("#tel");
      await page.keyboard.type("+447400123456");
      await eventually(async () => (await read()).country === "GB");
      const state = await read();
      assert.deepEqual(
        { enhanced: state.enhanced, country: state.country, text: state.text, e164: state.e164, options: state.options, flag: state.flag.length > 0 },
        { enhanced: true, country: "GB", text: "+44 7400 123456", e164: "+447400123456", options: 2, flag: true },
      );
      assert.deepEqual(errors, []);
      await context.close();
    });

    test(`named elements and a hostile answer can't break a split, a hotkey or a lazy list (${engine})`, async () => {
      const path = key("panes");
      // Images named after what the modules read from the document, and form fields named
      // after the methods they call on elements and event targets.
      const clobber = ["querySelector", "querySelectorAll", "activeElement", "documentElement", "scrollingElement", "cookie", "body", "head", "title", "hidden", "createElement", "createTextNode", "getElementById", "importNode", "dispatchEvent", "addEventListener"].map((name) => `<img name="${name}" src="${GIF}" alt="">`).join("");
      const items = (from, n) => Array.from({ length: n }, (_, i) => `<li data-key="${from + i}" data-cursor="${String(1000 - from - i).padStart(6, "0")}"><a href="/x/${from + i}">Row ${from + i}</a></li>`).join("");
      const split = `<div data-slot="split" id="s" data-ui-split data-orientation="horizontal" data-ui-split-save="/${path}-save">
        <section data-slot="split-pane" id="a" aria-label="List" data-split-basis="30" data-split-size="300" data-split-min="200" data-split-max="500"><div data-slot="split-pane-body"><div data-slot="split-pane-content">
          <form id="f"><input name="closest" value="x"><input name="matches" value="x"><input name="querySelector" value="x"><input name="contains" value="x"></form>
          <div data-slot="lazy-list" id="l" data-ui-lazy-list data-ui-src="/${path}-rows" data-ui-seen="/${path}-seen" data-ui-order="desc" data-ui-max="50" data-l-loaded="{n} more" data-l-new="{n} new rows">
            <a data-slot="lazy-more" data-dir="start" href="#" hidden>Newer</a>
            <ul data-slot="lazy-items" id="l-v" data-ui-keep data-ui-start-done data-ui-since="5" role="list" aria-label="Rows">${items(0, 10)}</ul>
            <a data-slot="lazy-more" data-dir="end" id="more" href="/${path}?after=000991">More</a>
            <p data-slot="lazy-status" role="status" aria-live="polite"></p>
          </div>
        </div></div></section>
        <div data-slot="split-handle" id="h" role="separator" aria-orientation="vertical" aria-controls="a" aria-label="List" data-ui-keep-attrs="tabindex aria-valuenow aria-valuemin aria-valuemax"></div>
        <section data-slot="split-pane" id="b" aria-label="Detail" data-split-fill data-split-min="300" data-split-max="900"><div data-slot="split-pane-body"><div data-slot="split-pane-content"><a id="hot" href="/${path}-hot" data-ui-hotkey="j" aria-keyshortcuts="j">Next</a></div></div></section>
      </div>`;
      route(`/${path}`, `<!doctype html><html lang="en"><head><meta charset="utf-8"><title>t</title><link rel="stylesheet" href="/_navree/ui/navree-ui.css"><link rel="stylesheet" href="/_navree/ui/split-styles.css"><link rel="stylesheet" href="/_navree/ui/lazy-styles.css"><script type="module" src="/_navree/ui/split.js"></script><script type="module" src="/_navree/ui/keys.js"></script><script type="module" src="/_navree/ui/lazy.js"></script></head><body>${clobber}${split}</body></html>`);
      route(`/${path}-hot`, `<!doctype html><html lang="en"><head><meta charset="utf-8"><title>hot</title></head><body>hot</body></html>`);
      f.routes.set(`/${path}-save`, (req, res) => {
        res.writeHead(204);
        res.end();
      });
      // The next page answers rows, a script, a handler, a duplicate key and a key that is markup.
      // A live ask is answered with row 2 changed and moved to the top, a new row, a key
      // with a comma in it, and row 4 and that key gone.
      f.routes.set(`/${path}-rows`, (req, res, body, url) => {
        if (url.searchParams.get("dir") === "live") {
          res.writeHead(200, { "content-type": "text/html", "x-ui-more": "0", "x-ui-since": "6", "x-ui-gone": "4,a%2Cb", "content-security-policy": CSP });
          return res.end(`<li data-key="2" data-cursor="001005"><a href="/x/2">Row 2, changed</a></li><li data-key="99" data-cursor="001001" data-ui-say="Row 99 came"><a href="/x/99">Row 99</a></li>`);
        }
        res.writeHead(200, { "content-type": "text/html", "x-ui-more": "0", "content-security-policy": CSP });
        res.end(`${items(10, 5)}<li data-key="3" data-cursor="000500">a duplicate</li><li data-key='"]&gt;<img src=x>' data-cursor="000100"><script>document.documentElement.setAttribute("data-attacker-ran","1")</script><img src="x" onerror="document.documentElement.setAttribute('data-attacker-ran','1')" alt=""></li><li data-key="a,b" data-cursor="000050">a key with a comma</li>`);
      });
      f.routes.set(`/${path}-seen`, (req, res) => {
        res.writeHead(204);
        res.end();
      });
      const { context, page, errors } = await fresh(`/${path}`);
      const q = (s) => page.evaluate((s) => Document.prototype.querySelector.call(document, s) && Element.prototype.getAttribute.call(Document.prototype.querySelector.call(document, s), "aria-valuenow"), s);
      await until(() => true, 50);
      // Focused through the prototype: Playwright's own selector reads querySelectorAll, which an image here names.
      await page.evaluate(() => HTMLElement.prototype.focus.call(Document.prototype.getElementById.call(document, "h")));
      await page.keyboard.press("ArrowRight");
      assert.equal(await q("#h"), "32", "the separator moves by its keys");
      // Within the save's own time limit.
      await until(() => hits(`/${path}-save`).length, 10000);
      assert.equal(hits(`/${path}-save`).length, 1, "and saves once");
      assert.match(hits(`/${path}-save`)[0].body, /split=s&sizes=32%2C68&collapsed=/);
      // The lazy list's end is in view, so its next page comes by itself: the rows arrive
      // once each, and nothing in them runs.
      await until(() => f.log.filter((entry) => entry.path.startsWith(`/${path}-rows`)).length);
      await sleep(200);
      const rows = await page.evaluate(() => [...Document.prototype.querySelectorAll.call(document, "#l-v > li")].map((li) => Element.prototype.getAttribute.call(li, "data-key")));
      assert.equal(rows.filter((k) => k === "3").length, 1, "a duplicate key is not added twice");
      assert.ok(rows.includes("14"));
      assert.equal(await attr(page, "data-attacker-ran"), null, "no script or handler from the answer ran");
      assert.equal(await page.evaluate(() => Element.prototype.hasAttribute.call(Document.prototype.getElementById.call(document, "more"), "hidden")), true, "the end is done");
      assert.match(f.log.filter((entry) => entry.path.startsWith(`/${path}-rows`))[0].path, /dir=end&cursor=000991/);
      assert.ok(rows.includes("a,b"), "a key with a comma is a key");
      // The list was seen once it showed.
      await until(() => hits(`/${path}-seen`).length);
      assert.equal(hits(`/${path}-seen`)[0].method, "POST", "seen is told by a post");
      // A live merge with row 2's link focused: the row changes and moves to the top, and
      // keeps the focus; the new row is announced; the gone ones go.
      await page.evaluate(() => HTMLElement.prototype.focus.call(Document.prototype.querySelector.call(document, "#l-v > li[data-key='2'] a")));
      const before = hits(`/${path}-seen`).length;
      await page.evaluate(() => EventTarget.prototype.dispatchEvent.call(Document.prototype.getElementById.call(document, "l"), new CustomEvent("ui:refresh", { bubbles: true })));
      await until(() => f.log.some((entry) => entry.path.startsWith(`/${path}-rows?dir=live`)));
      await sleep(200);
      const merged = await page.evaluate(() => {
        const q = (s) => Document.prototype.querySelector.call(document, s);
        const keys = [...Document.prototype.querySelectorAll.call(document, "#l-v > li")].map((li) => Element.prototype.getAttribute.call(li, "data-key"));
        const focus = Document.prototype.querySelector.call(document, ":focus");
        return {
          first: keys.slice(0, 3),
          gone: !keys.includes("4") && !keys.includes("a,b"),
          text: q("#l-v > li[data-key='2']").textContent,
          focused: focus ? Element.prototype.getAttribute.call(Element.prototype.closest.call(focus, "li"), "data-key") : null,
          since: Element.prototype.getAttribute.call(q("#l-v"), "data-ui-since"),
        };
      });
      assert.deepEqual(merged, { first: ["2", "99", "0"], gone: true, text: "Row 2, changed", focused: "2", since: "6" });
      const live = f.log.find((entry) => entry.path.startsWith(`/${path}-rows?dir=live`));
      assert.match(live.path, /since=5&from=001000&to=000050/, "the live ask names its revision and its window");
      await until(() => hits(`/${path}-seen`).length > before);
      assert.ok(hits(`/${path}-seen`).length > before, "a new row is seen again");
      // Said once the rows stop coming: a second after the last.
      let said = "";
      for (const end = Date.now() + 4000; Date.now() < end && !/Row 99 came/.test(said); await new Promise((r) => setTimeout(r, 50))) {
        said = await page.evaluate(() => Document.prototype.querySelector.call(document, "#l [data-slot=lazy-status]").textContent);
      }
      assert.match(said, /Row 99 came/, "the new row is announced");
      // A hotkey still clicks what names it.
      // Focused through the prototype: the test's own tools read `matches` off the form.
      await page.evaluate(() => HTMLElement.prototype.focus.call(Document.prototype.querySelector.call(document, "#f input[name=closest]")));
      await page.keyboard.press("j");
      assert.equal(page.url(), f.origin + `/${path}`, "no hotkey while a field has focus");
      // Focused through the prototype: Playwright's own selector reads querySelectorAll, which an image here names.
      await page.evaluate(() => HTMLElement.prototype.focus.call(Document.prototype.getElementById.call(document, "h")));
      await page.keyboard.press("j");
      await page.waitForURL(`**/${path}-hot`);
      assert.deepEqual(errors, []);
      await context.close();
    });

    test(`named elements can't break a sortable list (${engine})`, async () => {
      const path = key("sortable");
      // Names the module could read from the document, and fields named after what it calls on a form.
      const clobber = `<img name="elementFromPoint" src="${GIF}" alt=""><img name="body" src="${GIF}" alt=""><img name="activeElement" src="${GIF}" alt=""><form name="getElementById"></form><img name="documentElement" src="${GIF}" alt="">`;
      const words = JSON.stringify({ picked: "Picked up {item}.", moved: "Position {position} of {total} in {list}.", cancelled: "Back at position {position} of {total} in {list}." }).replaceAll('"', "&quot;");
      const item = (key, name) =>
        `<li data-slot="sortable-item" id="s-item-${key}" data-key="${key}" data-name="${name}"><button type="button" data-slot="sortable-handle" id="h${key}" aria-label="Move ${name}">Move</button><div data-slot="sortable-body">${name}</div><form data-ui-move data-ui-form method="post" action="/${path}-move"><input type="hidden" name="requestSubmit"><input type="hidden" name="elements"><input type="hidden" name="item" value="${key}"><input type="hidden" name="from_list"><input type="hidden" name="to_list"><input type="hidden" name="before"><input type="hidden" name="after"><input type="hidden" name="position"></form></li>`;
      const list = (key, title, items) =>
        `<section data-slot="sortable-list" id="s-list-${key}" data-list="${key}"><div data-slot="sortable-list-header"><h2 data-slot="sortable-list-title">${title}</h2></div><ol data-slot="sortable-items" id="s-list-${key}-items">${items}</ol></section>`;
      const sortable = `<div data-slot="sortable" id="s" data-layout="board" role="group" aria-label="Board" data-ui-sortable data-l="${words}"><div data-slot="sortable-status" id="s-status" aria-live="assertive"></div><div data-slot="sortable-lists">${list("a", "First", item("1", "One") + item("2", "Two"))}${list("b", "Second", "")}</div></div>`;
      route(`/${path}`, doc(`${clobber}${sortable}`).replace("</head>", `<script type="module" src="/_navree/ui/sortable.js"></script></head>`));
      const { context, page: tab, errors } = await fresh(`/${path}`);
      const read = () => tab.evaluate(() => {
        const q = (s) => Document.prototype.querySelector.call(document, s);
        return { status: q("#s-status").textContent, in: Element.prototype.closest.call(q("#s-item-1"), "[data-slot=sortable-list]").id };
      });
      await tab.focus("#h1");
      await tab.keyboard.press("Space");
      assert.equal((await read()).status, "Picked up One.");
      await tab.keyboard.press("ArrowRight");
      assert.deepEqual(await read(), { status: "Position 1 of 1 in Second.", in: "s-list-b" });
      await tab.keyboard.press("Escape");
      assert.deepEqual(await read(), { status: "Back at position 1 of 2 in First.", in: "s-list-a" });
      assert.deepEqual(errors, []);
      await context.close();
    });

    /** Waits until `check`, which may be async, holds, or `ms` pass. */
    const eventually = async (check, ms = 5000) => {
      for (const end = Date.now() + ms; Date.now() < end; await new Promise((resolve) => setTimeout(resolve, 20))) if (await check()) return;
    };

    test(`invalidate fetches a region, a plain element with a source, and a feed again (${engine})`, async () => {
      const name = key("inv");
      const go = (t) => `<form id="f-${t}" data-ui-form method="post" action="/${name}-inv-${t}"><button id="go-${t}">go ${t}</button></form>`;
      route(`/${name}`, doc(`<div id="rg" data-ui-region data-ui-src="/${name}-rg">r0</div><div id="pl" data-ui-src="/${name}-pl">p0</div><div id="fd" data-ui-src="/${name}-fd" data-ui-feed="into" data-ui-after="0"><ul id="into"></ul></div>${go("rg")}${go("pl")}${go("fd")}`));
      route(`/${name}-rg`, () => doc(`<div id="rg" data-ui-region data-ui-src="/${name}-rg">r${hits(`/${name}-rg`).length}</div>`));
      route(`/${name}-pl`, () => doc(`<div id="pl" data-ui-src="/${name}-pl">p${hits(`/${name}-pl`).length}</div>`));
      f.routes.set(`/${name}-fd`, (req, res) => f.html(res, `<li id="row-${hits(`/${name}-fd`).length}">row</li>`, { "x-ui-cursor": String(hits(`/${name}-fd`).length) }));
      for (const t of ["rg", "pl", "fd"]) f.routes.set(`/${name}-inv-${t}`, (req, res) => f.ops(res, { ops: [{ op: "invalidate", target: t }] }));
      const { context, page, errors } = await fresh(`/${name}`);
      // A feed asks for what follows its cursor, in the query.
      const fetched = (t) => f.log.filter((entry) => entry.path.split("?")[0] === `/${name}-${t}`).length;
      for (const t of ["rg", "pl", "fd"]) {
        const before = fetched(t);
        await page.click(`#go-${t}`);
        await until(() => fetched(t) > before, 4000);
        assert.equal(fetched(t), before + 1, `${t} is fetched again, once`);
      }
      await eventually(async () => (await page.textContent("#rg")) === "r1" && (await page.textContent("#pl")) === "p1");
      assert.equal(await page.textContent("#rg"), "r1", "the region is drawn again");
      assert.equal(await page.textContent("#pl"), "p1", "the plain element is drawn again");
      assert.deepEqual(errors, []);
      await context.close();
    });

    test(`a form that says how long it waits gives up then, and says so (${engine})`, async () => {
      const name = key("deadline");
      route(`/${name}`, doc(`<form id="f" data-ui-form data-ui-deadline="700" method="post" action="/${name}-post"><button id="go">go</button></form>`));
      // An answer that never comes.
      const held = [];
      f.routes.set(`/${name}-post`, (req, res) => held.push(res));
      const { context, page, errors } = await fresh(`/${name}`);
      await page.evaluate(() => {
        window.__sent = [];
        document.addEventListener("ui:sent", (e) => window.__sent.push({ ok: e.detail.ok, status: e.detail.status }));
      });
      const start = Date.now();
      await page.click("#go");
      await eventually(async () => (await page.evaluate(() => window.__sent.length)) > 0, 5000);
      const waited = Date.now() - start;
      assert.deepEqual(await page.evaluate(() => window.__sent), [{ ok: false, status: 0 }]);
      assert.ok(waited >= 600 && waited < 4000, `gave up after ${waited} ms`);
      await eventually(async () => (await toasts(page)).some((t) => /No connection/.test(t)));
      assert.equal(await page.evaluate(() => Document.prototype.querySelector.call(document, "#f").hasAttribute("data-pending")), false, "nothing left pending");
      for (const res of held) res.destroy();
      assert.deepEqual(errors, []);
      await context.close();
    });

    /** A sortable as `Sortable` draws one, its move forms posting to `/${path}-move`, with fields named after what the module and the runtime call. */
    const sortablePage = (path, clobber = "") => {
      const words = JSON.stringify({ picked: "Picked up {item}.", moved: "Position {position} of {total} in {list}.", dropped: "Dropped {item}.", cancelled: "Back at position {position} of {total} in {list}.", returned: "{item} is back at position {position} of {total} in {list}.", failed: "That move wasn't saved. Try again.", offline: "No connection. That move may not have been saved." }).replaceAll('"', "&quot;");
      const fields = ["requestSubmit", "elements", "isConnected", "action", "closest", "querySelector"].map((n) => `<input type="hidden" name="${n}">`).join("");
      const item = (key, name) =>
        `<li data-slot="sortable-item" id="s-item-${key}" data-key="${key}" data-name="${name}"><button type="button" data-slot="sortable-handle" id="h${key}" aria-label="Move ${name}">Move</button><div data-slot="sortable-body">${name}</div><form data-ui-move data-ui-form method="post" action="/${path}-move">${fields}<input type="hidden" name="item" value="${key}"><input type="hidden" name="from_list" value="a"><input type="hidden" name="to_list" value="a"><input type="hidden" name="before"><input type="hidden" name="after"><input type="hidden" name="position"></form></li>`;
      const list = (key, title, items) =>
        `<section data-slot="sortable-list" id="s-list-${key}" data-list="${key}"><div data-slot="sortable-list-header"><h2 data-slot="sortable-list-title">${title}</h2></div><ol data-slot="sortable-items" id="s-list-${key}-items">${items}</ol></section>`;
      const sortable = `<div data-slot="sortable" id="s" data-layout="board" role="group" aria-label="Board" data-ui-sortable data-l="${words}"><div data-slot="sortable-status" id="s-status" aria-live="assertive"></div><div data-slot="sortable-lists">${list("a", "First", item("1", "One") + item("2", "Two"))}${list("b", "Second", item("3", "Three"))}</div></div>`;
      return doc(`${clobber}${sortable}`).replace("</head>", `<script type="module" src="/_navree/ui/sortable.js"></script></head>`);
    };
    const field = (body, name) => new URLSearchParams(body).get(name);

    test(`a sortable under clobbering names drops, posts its move, and says a server's failure apart from no connection (${engine})`, async () => {
      const path = key("sortpost");
      const clobber = `<img name="elementFromPoint" src="${GIF}" alt=""><img name="body" src="${GIF}" alt=""><img name="activeElement" src="${GIF}" alt=""><form name="getElementById"></form><img name="documentElement" src="${GIF}" alt="">`;
      route(`/${path}`, sortablePage(path, clobber));
      let answer = 200;
      f.routes.set(`/${path}-move`, (req, res) => (answer === 200 ? f.ops(res, { ops: [] }) : (res.writeHead(answer, { "content-type": "text/plain" }), res.end("broken"))));
      const { context, page: tab, errors } = await fresh(`/${path}`);
      const read = () => tab.evaluate(() => {
        const q = (s) => Document.prototype.querySelector.call(document, s);
        return { status: q("#s-status").textContent, in: Element.prototype.closest.call(q("#s-item-1"), "[data-slot=sortable-list]").id };
      });
      // 1 to the second list, before 3: dropped, posted, taken.
      await tab.focus("#h1");
      await tab.keyboard.press("Space");
      await tab.keyboard.press("ArrowRight");
      await tab.keyboard.press("Space");
      await until(() => hits(`/${path}-move`).length === 1);
      const posted = hits(`/${path}-move`)[0];
      assert.equal(posted.headers["x-navree-ui"], "1", "by the runtime");
      for (const [name, value] of Object.entries({ item: "1", from_list: "a", to_list: "b", before: "3", after: "", position: "" })) {
        assert.equal(field(posted.body, name), value, name);
      }
      await eventually(async () => !(await tab.evaluate(() => Document.prototype.querySelector.call(document, "#s-item-1").hasAttribute("data-pending"))));
      assert.deepEqual(await read(), { status: "Dropped One.", in: "s-list-b" });
      assert.deepEqual(await toasts(tab), []);
      // Back to the first list, and the server fails: no region to ask, so it goes back and the toast says the server failed it.
      answer = 503;
      await tab.focus("#h1");
      await tab.keyboard.press("Space");
      await tab.keyboard.press("ArrowLeft");
      await tab.keyboard.press("Space");
      await until(() => hits(`/${path}-move`).length === 2);
      await eventually(async () => (await toasts(tab)).length > 0);
      assert.deepEqual(await toasts(tab), ["That move wasn't saved. Try again."]);
      await eventually(async () => (await read()).in === "s-list-b");
      assert.match((await read()).status, /^One is back at position 1 of 2 in Second\./);
      assert.deepEqual(errors, []);
      await context.close();
    });

    test(`a sortable silences only its own failed move: another form's failure on the page still says so (${engine})`, async () => {
      const path = key("sortcancel");
      route(`/${path}`, sortablePage(path).replace("</body>", `<form id="other" data-ui-form method="post" action="/${path}-other"><button id="go">go</button></form></body>`));
      f.routes.set(`/${path}-move`, (req, res) => req.socket.destroy());
      f.routes.set(`/${path}-other`, (req, res) => (res.writeHead(500, { "content-type": "text/plain" }), res.end("broken")));
      const { context, page: tab, errors } = await fresh(`/${path}`);
      // The move gets no answer: the sortable tells, once, in its own words.
      await tab.focus("#h1");
      await tab.keyboard.press("Space");
      await tab.keyboard.press("ArrowDown");
      await tab.keyboard.press("Space");
      await eventually(async () => (await toasts(tab)).length > 0);
      assert.deepEqual(await toasts(tab), ["No connection. That move may not have been saved."]);
      // Another form that fails is the runtime's to tell.
      await tab.click("#go");
      await eventually(async () => (await toasts(tab)).length > 1);
      assert.deepEqual((await toasts(tab)).sort(), ["No connection. That move may not have been saved.", "That didn't work. Try again."]);
      assert.deepEqual(errors, []);
      await context.close();
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
        // The answer shows, however slow the machine (its toast lasts 4 s): then nothing
        // more may come.
        await page.locator("[data-slot=toast]", { hasText: "done" }).waitFor({ timeout: 15000 });
        await sleep(200);
        assert.equal(hits(`/${path}-post`).length, 1, value);
        assert.equal(hits(`/${path}-post`)[0].headers["x-xsrf-token"], undefined, value);
        assert.equal(await page.evaluate(() => document.getElementById("f").hasAttribute("data-pending")), false, value);
        assert.deepEqual((await toasts(page)).filter((text) => text !== "done"), [], value);
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

    test(`a form aimed at another origin is refused by the runtime, whatever the content policy (${engine})`, async () => {
      f.evil.length = 0;
      const path = key("xorigin-open");
      // A policy that lets the page fetch and post anywhere: only the runtime stands in the way.
      const open = { "content-security-policy": "script-src 'self'; style-src 'self'", "set-cookie": "XSRF-TOKEN=tok; Path=/" };
      route(`/${path}`, doc(`<form id="f" data-ui-form method="post" action="${f.evilOrigin}/steal"><input name="secret" value="s"><button id="go">go</button></form><form id="g" data-ui-form method="post" action="/${path}-post"><button id="here" formaction="${f.evilOrigin}/steal-by-button">go</button></form>`), open);
      const { context, page } = await fresh(`/${path}`);
      await page.click("#go");
      await page.click("#here");
      await sleep(200);
      assert.deepEqual(f.evil, [], "nothing, not even a preflight, reached the other origin");
      assert.deepEqual(hits(`/${path}-post`), []);
      assert.ok(page.url().startsWith(f.origin), page.url());
      assert.equal(await page.evaluate(() => document.querySelector("[data-pending]")), null);
      await context.close();
    });

    /** Two links, a form; the form's answer changes what `/x-private` shows. */
    function staleScene(answer) {
      const name = key("stale");
      const state = { user: "aarti", version: 1 };
      route(`/${name}`, doc(`<a id="link" data-ui-prefetch href="/${name}-private">account</a><form id="f" data-ui-form method="post" action="/${name}-write"><button id="go">go</button></form>`));
      route(`/${name}-private`, () => doc(`<h1 id="who">${state.user} v${state.version}</h1>`));
      f.routes.set(`/${name}-write`, (req, res) => {
        state.user = "bal";
        state.version = 2;
        f.ops(res, answer(name), 200, { "set-cookie": "user=bal; Path=/" });
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
      assert.equal(await page.textContent("#who"), "bal v2");
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
        assert.equal(await page.textContent("#who"), "bal v2", `absolute=${absolute}`);
        await context.close();
      }
    });

    test(`a prefetch started during a write is not shown after it (${engine})`, async () => {
      const name = key("during");
      const state = { user: "aarti" };
      route(`/${name}`, doc(`<a id="link" data-ui-prefetch href="/${name}-private">account</a><form id="f" data-ui-form method="post" action="/${name}-write"><button id="go">go</button></form>`));
      route(`/${name}-private`, () => doc(`<h1 id="who">${state.user}</h1>`));
      f.routes.set(`/${name}-write`, (req, res) => setTimeout(() => {
        state.user = "bal";
        f.ops(res, { ops: [] });
      }, 300));
      const { context, page } = await fresh(`/${name}`);
      await page.click("#go");
      await page.hover("#link");
      await sleep(450);
      await page.click("#link");
      await page.waitForSelector("#who");
      assert.equal(await page.textContent("#who"), "bal");
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
      assert.equal(await one.textContent("#who"), "bal v2");
      await context.close();
    });

    test(`a plain form's post drops the prefetch of other tabs (${engine})`, async () => {
      const name = key("plainpost");
      const state = { user: "aarti" };
      route(`/${name}`, doc(`<a id="link" data-ui-prefetch href="/${name}-private">account</a><form id="f" method="post" action="/${name}-write"><button id="go">go</button></form>`));
      route(`/${name}-private`, () => doc(`<h1 id="who">${state.user}</h1>`));
      f.routes.set(`/${name}-write`, (req, res) => {
        state.user = "bal";
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
      assert.equal(await one.textContent("#who"), "bal");
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

    test(`a region's own links are fetched ahead only when the region asks (${engine})`, async () => {
      const name = key("regionahead");
      route(`/${name}`, doc(`<div id="quiet" data-ui-region><a id="a" data-ui-in-region href="/${name}-a">a</a></div><div id="asks" data-ui-region data-ui-prefetch><a id="b" data-ui-in-region href="/${name}-b">b</a><a id="c" href="/${name}-c">c</a></div>`));
      for (const path of ["a", "b", "c"]) route(`/${name}-${path}`, doc(`<h1>${path}</h1>`));
      const { context, page } = await fresh(`/${name}`);
      for (const id of ["a", "b", "c"]) {
        await page.hover(`#${id}`);
        await sleep(200);
      }
      assert.equal(hits(`/${name}-a`).length, 0, "a region that didn't ask");
      assert.equal(hits(`/${name}-b`).length, 1, "a region that asked");
      assert.equal(hits(`/${name}-b`)[0].headers["x-navree-prefetch"], "1");
      assert.equal(hits(`/${name}-c`).length, 0, "a link of the page, in that region but not its own");
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

    test(`only a link marked eager is fetched with no hover, at most four a page (${engine})`, async () => {
      const name = key("eager");
      const links = [1, 2, 3, 4, 5].map((n) => `<a id="e${n}" data-ui-prefetch="eager" href="/${name}-e${n}">e${n}</a>`).join("");
      route(`/${name}`, doc(`<a id="hover" data-ui-prefetch href="/${name}-hover">hover</a><a id="plain" href="/${name}-plain">plain</a><a id="off" data-ui-prefetch="eager" data-ui-noboost href="/${name}-off">off</a><a id="away" data-ui-prefetch="eager" href="${E()}/eager">away</a>${links}`));
      for (const path of ["hover", "plain", "off", "e1", "e2", "e3", "e4", "e5"]) route(`/${name}-${path}`, doc(`<h1 id="h">${path}</h1>`), { "cache-control": "private, max-age=10" });
      f.evil.length = 0;
      const { context, page } = await fresh(`/${name}`);
      await sleep(1200);
      const fetched = (path) => hits(`/${name}-${path}`);
      for (const path of ["e1", "e2", "e3", "e4"]) {
        assert.equal(fetched(path).length, 1, path);
        assert.equal(fetched(path)[0].headers["x-navree-prefetch"], "1", path);
      }
      for (const path of ["e5", "hover", "plain", "off"]) assert.equal(fetched(path).length, 0, path);
      assert.deepEqual(f.evil, []);
      await page.click("#e2");
      await page.waitForSelector("#h");
      assert.equal(await page.textContent("#h"), "e2");
      assert.equal(fetched("e2").length, 1, "the click used what was fetched");
      await context.close();
    });

    test(`an eager prefetch waits out a write and is never shown after one (${engine})`, async () => {
      const name = key("eagerwrite");
      const state = { user: "aarti" };
      route(`/${name}`, doc(`<a id="link" data-ui-prefetch="eager" href="/${name}-private">account</a><form id="f" data-ui-form method="post" action="/${name}-write"><button id="go">go</button></form>`));
      // A user's own page is no-store: the HTTP cache, which no guard of the runtime's
      // reaches, never holds it.
      route(`/${name}-private`, () => doc(`<h1 id="who">${state.user}</h1>`));
      f.routes.set(`/${name}-write`, (req, res) => {
        state.user = "bal";
        f.ops(res, { ops: [{ op: "toast", tone: "success", text: "saved" }] });
      });
      const { context, page } = await fresh(`/${name}`);
      await sleep(1200);
      assert.equal(hits(`/${name}-private`).length, 1);
      await page.click("#go");
      await sleep(250);
      await page.click("#link");
      await page.waitForSelector("#who");
      assert.equal(await page.textContent("#who"), "bal");
      await context.close();
    });

    test(`a page reached by a boosted link fetches its own eager links (${engine})`, async () => {
      const name = key("eagerswap");
      route(`/${name}`, doc(`<a id="next" href="/${name}-next">next</a>`));
      route(`/${name}-next`, doc(`<h1 id="h">next</h1><a data-ui-prefetch="eager" href="/${name}-later">later</a>`));
      route(`/${name}-later`, doc(`<h1>later</h1>`));
      const { context, page } = await fresh(`/${name}`);
      await sleep(600);
      assert.equal(hits(`/${name}-later`).length, 0);
      await page.click("#next");
      await page.waitForSelector("#h");
      await sleep(1200);
      assert.equal(hits(`/${name}-later`).length, 1);
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
          // WebKit applies a style attribute the page's policy forbids when it comes in
          // by the DOM (main's runtime too): there the check is that nothing it names is
          // fetched, below.
          if (engine !== "webkit") assert.notEqual(state.styled, "rgb(1, 2, 3)", label);
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

    // Answers that come out of order. The older request's body is held (tests/support/late.mjs) until the newer one
    // has been shown, then let through: it must leave the page, the address and the history as the newer one made them.
    /** Asks the region `id` to fetch again, as a module does. The promise is the fetch's end, kept on the page as `window[mark]` when it is not to be waited for. */
    const ask = (page, id, mark) => page.evaluate(([target, name]) => {
      const detail = {};
      document.getElementById(target).dispatchEvent(new CustomEvent("ui:refresh", { bubbles: true, detail }));
      if (name) window[name] = detail.done;
      else return detail.done;
    }, [id, mark]);

    test(`a region's older answer, read after a newer one was drawn, is not drawn over it (${engine})`, async () => {
      const name = key("older");
      route(`/${name}`, doc(`<div id="r" data-ui-region data-ui-src="/${name}-frag">start</div>`));
      route(`/${name}-frag`, () => doc(`<div id="r" data-ui-region data-ui-src="/${name}-frag">answer ${hits(`/${name}-frag`).length}</div>`));
      const { context, page, errors } = await fresh(`/${name}`);
      const late = await lateBody(page, { url: new RegExp(`/${name}-frag`) });
      await ask(page, "r", "__first");
      await late.arrived();
      await ask(page, "r");
      assert.equal(await page.textContent("#r"), "answer 2", "the newer answer is drawn");
      await late.release();
      await page.evaluate(() => window.__first);
      assert.equal(await page.textContent("#r"), "answer 2", "the older one, read after it, leaves it as it is");
      assert.deepEqual(errors, []);
      await context.close();
    });

    test(`a region's link followed while a fetch of it is on its way is not undone by that fetch (${engine})`, async () => {
      const name = key("followed");
      const region = (text) => `<div id="r" data-ui-region data-ui-src="/${name}-frag"><span id="what">${text}</span><a id="next" href="/${name}-next" data-ui-in-region>next</a></div>`;
      route(`/${name}`, doc(region("start")));
      route(`/${name}-frag`, doc(region("refreshed")));
      route(`/${name}-next`, doc(region("followed")));
      const { context, page, errors } = await fresh(`/${name}`);
      const late = await lateBody(page, { url: new RegExp(`/${name}-frag`) });
      await ask(page, "r", "__first");
      await late.arrived();
      await page.click("#next");
      await page.waitForFunction(() => document.getElementById("what").textContent === "followed");
      await late.release();
      await page.evaluate(() => window.__first);
      assert.equal(await page.textContent("#what"), "followed", "the fetch that began before the link was followed is out of date");
      assert.deepEqual(errors, []);
      await context.close();
    });

    test(`a sort of one table that a sort of another overtook still shows, and leaves neither table busy (${engine})`, async () => {
      const name = key("tables");
      const table = (id, text) => `<div id="${id}" data-ui-region data-ui-table data-ui-src="/${name}"><span id="${id}-what">${text}</span><a id="${id}-sort" href="/${name}-${id}" data-ui-in-region>sort</a></div>`;
      route(`/${name}`, doc(table("ta", "start") + table("tb", "start")));
      route(`/${name}-ta`, doc(table("ta", "a sorted") + table("tb", "start")));
      route(`/${name}-tb`, doc(table("ta", "start") + table("tb", "b sorted")));
      const { context, page, errors } = await fresh(`/${name}`);
      const late = await lateBody(page, { url: new RegExp(`/${name}-ta`) });
      const busy = () => page.evaluate(() => ["ta", "tb"].map((id) => document.getElementById(id).hasAttribute("data-pending")));
      await page.click("#ta-sort");
      await late.arrived();
      await page.click("#tb-sort");
      await page.waitForFunction(() => document.getElementById("tb-what").textContent === "b sorted");
      assert.deepEqual(await busy(), [true, false], "the second table is done; the first is still waiting for its own answer");
      assert.equal(await attr(page, "data-ui-loading"), "", "the page still loads while the first table's answer is on its way");
      await late.release();
      await page.waitForFunction(() => document.getElementById("ta-what").textContent === "a sorted");
      assert.equal(await page.textContent("#tb-what"), "b sorted", "the first table's answer leaves the second as it is");
      assert.deepEqual(await busy(), [false, false], "neither table is left busy");
      assert.equal(await attr(page, "data-ui-loading"), null, "nor the page loading");
      assert.equal(await page.evaluate(() => document.getElementById("ta").getAttribute("aria-busy")), null);
      assert.deepEqual(errors, []);
      await context.close();
    });

    test(`a region's answer brings the styles and the module its parts need, and is drawn once its styles are in (${engine})`, async () => {
      const name = key("brings");
      let release;
      const held = new Promise((resolve) => (release = resolve));
      f.routes.set(`/${name}.css`, async (req, res) => {
        await held;
        res.writeHead(200, { "content-type": "text/css" });
        res.end("#r [data-part]{outline:3px solid rgb(1, 2, 3)}");
      });
      route(`/${name}`, doc(`<div id="r" data-ui-region data-ui-src="/${name}-frag">start</div>`));
      // A fragment is a page with no script of its own: its head names what its parts need.
      route(`/${name}-frag`, `<!doctype html><html lang="en"><head><meta charset="utf-8"><title>f</title><link rel="stylesheet" href="/${name}.css"><script type="module" src="/_navree/ui/keys.js"></script></head><body><div id="r" data-ui-region data-ui-src="/${name}-frag"><span data-part>part</span></div></body></html>`);
      const { context, page, errors } = await fresh(`/${name}`);
      await ask(page, "r", "__done");
      await until(() => hits(`/${name}.css`).length === 1);
      await sleep(100);
      assert.equal(await page.textContent("#r"), "start", "the part waits for its styles");
      release();
      await page.evaluate(() => window.__done);
      assert.equal(await page.textContent("#r"), "part");
      assert.equal(await page.evaluate(() => getComputedStyle(document.querySelector("#r [data-part]")).outlineColor), "rgb(1, 2, 3)");
      assert.equal(await page.locator('head script[src="/_navree/ui/keys.js"]').count(), 1, "the module it needs is loaded, once");
      // The next answer names the same: nothing is added twice.
      await ask(page, "r");
      assert.equal(await page.locator(`head link[href="/${name}.css"]`).count(), 1);
      assert.equal(await page.locator('head script[src="/_navree/ui/keys.js"]').count(), 1);
      assert.deepEqual(errors, []);
      await context.close();
    });

    test(`a page followed while a table's sort is on its way leaves that sort's answer undrawn (${engine})`, async () => {
      const name = key("leave");
      const table = (text) => `<div id="ta" data-ui-region data-ui-table data-ui-src="/${name}"><span id="ta-what">${text}</span><a id="ta-sort" href="/${name}-sort" data-ui-in-region>sort</a></div>`;
      route(`/${name}`, doc(`${table("start")}<a id="away" href="/${name}-away">away</a>`));
      route(`/${name}-sort`, doc(table("sorted")));
      route(`/${name}-away`, doc(`<h1 id="h">away</h1>${table("away")}`, { title: "away" }));
      const { context, page, errors } = await fresh(`/${name}`);
      const late = await lateBody(page, { url: new RegExp(`/${name}-sort`) });
      await page.click("#ta-sort");
      await late.arrived();
      await page.click("#away");
      await page.waitForFunction(() => document.title === "away");
      await late.release();
      await sleep(200);
      assert.equal(await page.textContent("#ta-what"), "away", "the page followed after the sort is the newer");
      assert.equal(await page.evaluate(() => location.pathname), `/${name}-away`);
      assert.equal(await attr(page, "data-ui-loading"), null);
      assert.deepEqual(errors, []);
      await context.close();
    });

    /** A page with two links, `a` and `b`, which answer with `a` and `b` (`more` for the options of a's page); a's carries a toast. */
    function twoLinks(more = {}) {
      const name = key("twice");
      route(`/${name}`, doc(`<a id="a" href="/${name}-a">a</a><a id="b" href="/${name}-b">b</a>`, { title: "start" }));
      const toast = `<section data-slot="toaster" aria-label="Notifications" data-position="bottom-end" data-close="Dismiss"><div data-slot="toast" data-tone="info" data-duration="sticky" role="status"><div data-slot="toast-body"><p data-slot="toast-text">from a</p></div></div></section>`;
      route(`/${name}-a`, doc(`<h1 id="h">page a</h1>${toast}`, { title: "a", ...more }));
      route(`/${name}-b`, doc(`<h1 id="h">page b</h1>`, { title: "b" }));
      return name;
    }
    /** Follows a, whose body is held, then b, which is shown. */
    async function overtaken(page, late) {
      await page.click("#a");
      await late.arrived();
      await page.click("#b");
      await page.waitForFunction(() => document.title === "b");
    }

    test(`a page that a newer one overtook while its body was read leaves the address, the page and the history as the newer one made them (${engine})`, async () => {
      const name = twoLinks();
      const { context, page, errors } = await fresh(`/${name}`);
      const entries = await page.evaluate(() => history.length);
      const late = await lateBody(page, { url: new RegExp(`/${name}-a`) });
      await overtaken(page, late);
      await late.release();
      assert.equal(await page.evaluate(() => location.pathname), `/${name}-b`, "the address is the newer page's");
      assert.equal(await page.textContent("#h"), "page b");
      assert.equal(await page.title(), "b");
      assert.equal(await page.evaluate(() => history.length), entries + 1, "only the newer page made an entry");
      assert.deepEqual(await toasts(page), ["from a"], "the toast the older page brought is still shown");
      assert.deepEqual(errors, []);
      await context.close();
    });

    test(`a page from another build that a newer page overtook doesn't take the browser to itself (${engine})`, async () => {
      const name = twoLinks({ build: "newer" });
      const { context, page, errors } = await fresh(`/${name}`);
      const late = await lateBody(page, { url: new RegExp(`/${name}-a`) });
      await page.evaluate(() => { window.__stays = true; });
      await overtaken(page, late);
      await late.release();
      await sleep(300);
      assert.equal(hits(`/${name}-a`).length, 1, "it was asked for once, by the boost, and not gone to");
      assert.equal(await page.evaluate(() => window.__stays), true, "the page was not loaded again");
      assert.equal(await page.evaluate(() => location.pathname), `/${name}-b`);
      assert.deepEqual(errors, []);
      await context.close();
    });

    test(`a feed answer that a newer fetch overtook doesn't take the page to where it was redirected (${engine})`, async () => {
      const name = key("feedlate");
      let asked = 0;
      let release;
      const held = new Promise((resolve) => (release = resolve));
      route(`/${name}`, doc(`<div id="fd" data-ui-region data-ui-src="/${name}-feed" data-ui-feed="into" data-ui-after="0"><ul id="into"></ul></div>`));
      route(`/${name}-login`, doc(`<h1 id="login">Sign in</h1>`));
      f.routes.set(`/${name}-feed`, async (req, res) => {
        // The first fetch waits at the server; the second is answered, then the first ends in a redirect.
        if (++asked > 1) return f.html(res, `<li id="row-1">row</li>`, { "x-ui-cursor": "1" });
        await held;
        res.writeHead(303, { location: `/${name}-login` });
        res.end();
      });
      const { context, page, errors } = await fresh(`/${name}`);
      await ask(page, "fd", "__first");
      await until(() => asked === 1);
      // The cursor has moved on, so the newer fetch is for another address: no browser holds it back behind the first.
      await page.evaluate(() => document.getElementById("fd").setAttribute("data-ui-after", "5"));
      await ask(page, "fd");
      assert.equal(await page.locator("#into #row-1").count(), 1, "the newer fetch's row is drawn");
      release();
      await until(() => hits(`/${name}-login`).length > 0);
      await sleep(300);
      const gone = hits(`/${name}-login`).filter((hit) => !hit.headers["x-navree-region"]);
      assert.deepEqual(gone, [], "the page was not sent to the login page");
      assert.equal(await page.evaluate(() => location.pathname), `/${name}`);
      assert.equal(await page.locator("#into #row-1").count(), 1);
      assert.deepEqual(errors, []);
      await context.close();
    });

    test(`an island imports only a script of the app's own, takes its props as data, and one that throws leaves the fallback (${engine})`, async () => {
      f.evil.length = 0;
      const name = key("isle");
      const good = `/_navree/ui/island/${name}-0123456789ab.js`;
      const boom = `/_navree/ui/island/${name}-boom-0123456789ab.js`;
      const js = (res, text) => {
        res.writeHead(200, { "content-type": "text/javascript" });
        res.end(text);
      };
      f.routes.set(good, (req, res) => js(res, "export function mount(body, props) { body.textContent = 'drawn ' + typeof props?.city + ' ' + (props?.city ?? ''); }"));
      f.routes.set(boom, (req, res) => js(res, "export function mount() { throw new Error('no'); }"));
      const island = (id, src, props = "{}") =>
        `<div id="${id}" data-ui-island="${src}" data-ui-props="${props.replaceAll("&", "&amp;").replaceAll('"', "&quot;")}" data-ui-keep-attrs="data-ui-mounted data-ui-failed"><div data-slot="island-body" data-ui-keep>fallback</div></div>`;
      route(
        `/${name}`,
        doc(
          island("ok", good, JSON.stringify({ city: "<img src=x onerror=document.documentElement.setAttribute('data-pwned','island')>" })) +
            island("evil", `${E()}/_navree/ui/island/x-0123456789ab.js`) +
            island("up", "/_navree/ui/island/../core.js") +
            island("query", `${good}?x=1`) +
            island("named", "/_navree/ui/island/Not_Ours-0123456789ab.js") +
            island("boom", boom) +
            island("junk", good, "{not json"),
          { head: `<script type="module" src="/_navree/ui/islands.js"></script>` },
        ),
      );
      const { context, page, errors } = await fresh(`/${name}`);
      await page.waitForFunction(() => ["ok", "junk"].every((id) => document.getElementById(id).hasAttribute("data-ui-mounted")));
      await page.waitForFunction(() => document.getElementById("boom").hasAttribute("data-ui-failed"));
      assert.match(await page.textContent("#ok [data-slot=island-body]"), /^drawn string <img/, "the props are text to the script, never markup");
      assert.equal(await attr(page, "data-pwned"), null);
      assert.equal(await page.textContent("#junk [data-slot=island-body]"), "drawn undefined ", "props that aren't JSON are none");
      assert.equal(await page.textContent("#boom [data-slot=island-body]"), "fallback", "the server's fallback stays");
      for (const id of ["evil", "up", "query", "named"]) {
        const state = await page.evaluate((i) => ["data-ui-mounted", "data-ui-failed"].map((n) => document.getElementById(i).hasAttribute(n)), id);
        assert.deepEqual(state, [false, false], `${id} was never imported`);
      }
      assert.deepEqual(f.evil, [], "nothing asked of another site");
      assert.equal(hits(`${good}?x=1`).length, 0);
      assert.equal(hits(good).length, 1, "one script, imported once for two islands");
      assert.deepEqual(errors, []);
      await context.close();
    });

    test(`commands reach only state attributes (${engine})`, async () => {
      f.evil.length = 0;
      const name = key("cmd");
      const refused = [["a1", "href", "javascript:1"], ["a1", "HREF", "javascript:1"], ["a1", "onclick", "1"], ["a1", "ONCLICK", "1"], ["a1", "style", "color:red"], ["a1", "srcdoc", "x"], ["a1", "ping", `${E()}/ping`], ["a1", "target", "_blank"], ["v1", "poster", `${E()}/poster`], ["i1", "srcset", `${E()}/srcset 1x`], ["o1", "data", `${E()}/object`], ["a1", "xlink:href", "javascript:1"], ["r1", "data-ui-src", `/${name}-admin`], ["b2", "data-ui-click", `[["set_attr","a1","title","rewritten"]]`], ["a1", "data-ui-persist", ""], ["secret", "form", "evilform"], ["secret", "name", "stolen"], ["evilform", "method", "post"], ["a1", "popover", "auto"], ["a1", "DATA-UI-CLICK", "[]"]];
      const allowed = [["a1", "data-open", "1"], ["a1", "aria-expanded", "true"], ["a1", "hidden", ""], ["d1", "open", ""], ["secret", "disabled", ""]];
      const button = (id, command) => `<button id="${id}" data-ui-click='${JSON.stringify([command]).replace(/'/g, "&#39;")}'>x</button>`;
      const buttons = [...refused.map((step, at) => button(`no${at}`, ["set_attr", ...step])), ...refused.map((step, at) => button(`rm${at}`, ["remove_attr", step[0], step[1]])), ...allowed.map((step, at) => button(`yes${at}`, ["set_attr", ...step]))].join("");
      route(`/${name}`, doc(`<a id="a1" href="/ok" onclick="" data-ui-click="[]">a</a><details id="d1"></details><video id="v1"></video><img id="i1" src="${GIF}"><object id="o1"></object><div id="r1" data-ui-src="/${name}-frag"></div><button id="b2">b2</button><input id="secret" name="secret"><form id="evilform"></form>${buttons}`));
      const { context, page } = await fresh(`/${name}`);
      const before = await page.evaluate(() => Object.fromEntries(["a1", "v1", "i1", "o1", "r1", "b2", "secret", "evilform"].map((id) => [id, document.getElementById(id).outerHTML])));
      for (let at = 0; at < refused.length; at++) await page.evaluate((id) => document.getElementById(id).click(), `no${at}`);
      for (let at = 0; at < refused.length; at++) await page.evaluate((id) => document.getElementById(id).click(), `rm${at}`);
      const afterwards = await page.evaluate(() => Object.fromEntries(["a1", "v1", "i1", "o1", "r1", "b2", "secret", "evilform"].map((id) => [id, document.getElementById(id).outerHTML])));
      assert.deepEqual(afterwards, before);
      for (let at = 0; at < allowed.length; at++) await page.evaluate((id) => document.getElementById(id).click(), `yes${at}`);
      for (const [id, attribute] of allowed) assert.notEqual(await page.getAttribute(`#${id}`, attribute), null, attribute);
      assert.deepEqual(f.evil, []);
      await context.close();
    });

    /** A page of command buttons: each `[id, commands]` is a button running those commands. */
    const commandPage = (name, body, buttons) => {
      const button = ([id, commands]) => `<button id="${id}" type="button" data-ui-click='${JSON.stringify(commands).replace(/&/g, "&amp;").replace(/'/g, "&#39;")}'>x</button>`;
      route(`/${name}`, doc(body + buttons.map(button).join("")));
    };
    const pwned = (page) => page.evaluate(() => document.documentElement.getAttribute("data-pwned"));

    test(`text, count and value write text and nothing else (${engine})`, async () => {
      f.evil.length = 0;
      const name = key("text");
      const markup = `<img src="${E()}/img" onerror="document.documentElement.setAttribute('data-pwned','text')">`;
      const breakout = `"]],[["set_attr","a1","href","javascript:1"]],["`;
      commandPage(name,
        `<p id="t">start</p><a id="a1" href="/ok">a</a><script id="s">/* old */</script><svg><script id="svgs">/* old */</script></svg>` +
        `<input id="vis" name="vis" value="typed"><textarea id="ta" name="ta">area</textarea><select id="se" name="se"><option>one</option></select>` +
        `<input type="hidden" id="h" name="h" value="old" data-ui-value><input type="HIDDEN" id="h2" name="h2" value="old" data-ui-value><div id="d">div</div>` +
        `<textarea id="bio" name="bio" data-ui-input='[["count","n",10],["count","h",null],["count","vis",null]]'></textarea><span id="n">0/10</span>`,
        [
          ["b1", [["text", "t", markup]]],
          ["b2", [["text", "t", breakout]]],
          ["b3", [["text", "s", "document.documentElement.setAttribute('data-pwned','script')"], ["text", "svgs", "document.documentElement.setAttribute('data-pwned','svg')"]]],
          ["b4", [["text", "vis", "x"], ["text", "ta", "x"], ["text", "se", "x"]]],
          ["b5", [["value", "vis", "x"], ["value", "ta", "x"], ["value", "d", "x"], ["value", "se", "x"]]],
          ["b6", [["value", "h", markup], ["value", "h2", "two"]]],
          ["b7", [["text", "t", { toString: 1 }], ["value", "h", ["array"]], ["text", "t"], ["count"], ["value"], ["text", null, "x"]]],
          ["b8", [["text", "__proto__", "x"], ["value", "constructor", "x"]]],
        ]);
      const { context, page, errors } = await fresh(`/${name}`);
      const click = (id) => page.evaluate((b) => document.getElementById(b).click(), id);
      await click("b1");
      assert.equal(await page.textContent("#t"), markup);
      assert.equal(await page.locator("#t *").count(), 0, "no element made");
      await click("b2");
      assert.equal(await page.textContent("#t"), breakout);
      assert.equal(await page.getAttribute("#a1", "href"), "/ok");
      await click("b3");
      assert.equal(await page.evaluate(() => document.getElementById("s").textContent), "/* old */");
      assert.equal(await page.evaluate(() => document.getElementById("svgs").textContent), "/* old */");
      await click("b4");
      await click("b5");
      assert.deepEqual(await page.evaluate(() => ({
        vis: document.getElementById("vis").value,
        ta: document.getElementById("ta").value,
        se: document.getElementById("se").options.length,
        d: document.getElementById("d").textContent,
      })), { vis: "typed", ta: "area", se: 1, d: "div" });
      await click("b6");
      assert.equal(await page.inputValue("#h"), markup);
      assert.equal(await page.inputValue("#h2"), "two");
      await click("b7");
      await click("b8");
      assert.equal(await page.inputValue("#h"), markup, "only a string sets a value");
      assert.equal(await page.textContent("#t"), breakout, "only a string sets text");
      // Characters as the server counts them: one emoji is one.
      await page.fill("#bio", "a\u{1F600}");
      assert.equal(await page.textContent("#n"), "2/10");
      assert.equal(await page.inputValue("#h"), markup, "a count never writes a field");
      assert.equal(await page.inputValue("#vis"), "typed");
      assert.equal(await pwned(page), null);
      assert.deepEqual(errors, []);
      assert.deepEqual(f.evil, []);
      await context.close();
    });

    test(`a text command can't write a style sheet (${engine})`, async () => {
      f.evil.length = 0;
      const name = key("style");
      commandPage(name, `<style id="st"></style><p id="p">p</p>`, [["c1", [["text", "st", `#p{background:url(${E()}/css);color:rgb(1,2,3)}`]]]]);
      const { context, page, errors } = await fresh(`/${name}`);
      await page.evaluate(() => document.getElementById("c1").click());
      await sleep(100);
      assert.equal(await page.evaluate(() => document.getElementById("st").textContent), "");
      assert.notEqual(await page.evaluate(() => getComputedStyle(document.getElementById("p")).color), "rgb(1, 2, 3)");
      assert.deepEqual(errors, []);
      assert.deepEqual(f.evil, []);
      await context.close();
    });

    test(`fill copies a row's text into a dialog, and a key only into a hidden field (${engine})`, async () => {
      f.evil.length = 0;
      const name = key("fill");
      const hostile = `<img src="${E()}/fill" onerror="document.documentElement.setAttribute('data-pwned','fill')">`;
      const escaped = hostile.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
      const row = (k, title, button) => `<tr data-key="${k}"><td>${title}</td><td>${k}0</td><td>${button}</td></tr>`;
      route(`/${name}`, doc(
        `<table><thead><tr><th data-col="title">Title</th><th data-col="amount">Amount</th><th></th></tr></thead><tbody>` +
        row("7", escaped, `<button id="first" type="button" data-ui-click='[["fill","dlg"]]'>open</button>`) +
        row(`8&quot; onclick=&quot;1`, "Second", `<button id="second" type="button" data-ui-click='[["fill","dlg"]]'>open</button>`) +
        `</tbody></table><button id="outside" type="button" data-ui-click='[["fill","dlg"]]'>no row</button>` +
        `<dialog id="dlg"><h2 id="title" data-ui-cell="title">-</h2><p id="amount" data-ui-cell="amount">-</p><p data-ui-cell='"],img[src=x'>bad</p><p data-ui-cell="none">none</p>` +
        `<script id="s" data-ui-cell="title">/* old */</script><input id="vis" name="vis" data-ui-row-key value="typed">` +
        `<form method="post" action="/x"><input type="hidden" id="k" name="id" data-ui-row-key value=""><input type="hidden" id="kt" name="t" data-ui-cell="title"></form></dialog>`,
      ));
      const { context, page, errors } = await fresh(`/${name}`);
      await page.evaluate(() => document.getElementById("outside").click());
      assert.equal(await page.evaluate(() => document.getElementById("dlg").open), false, "no row, no fill");
      await page.evaluate(() => document.getElementById("first").click());
      const state = await page.evaluate(() => ({
        open: document.getElementById("dlg").open,
        title: document.getElementById("title").textContent,
        amount: document.getElementById("amount").textContent,
        made: document.querySelectorAll("#dlg img").length,
        script: document.getElementById("s").textContent,
        vis: document.getElementById("vis").value,
        key: document.getElementById("k").value,
        keyText: document.getElementById("kt").value,
      }));
      assert.deepEqual(state, { open: true, title: hostile, amount: "70", made: 0, script: "/* old */", vis: "typed", key: "7", keyText: hostile });
      await page.evaluate(() => document.getElementById("dlg").close());
      await page.evaluate(() => document.getElementById("second").click());
      assert.equal(await page.inputValue("#k"), `8" onclick="1`);
      assert.equal(await page.evaluate(() => document.getElementById("k").getAttribute("onclick")), null);
      assert.equal(await pwned(page), null);
      assert.deepEqual(errors, []);
      assert.deepEqual(f.evil, []);
      await context.close();
    });

    test(`an envelope loads only the toolkit's modules (${engine})`, async () => {
      for (const url of ["/attacker.js", "/_navree/ui/../../attacker.js", `${E()}/mod.js`, "data:text/javascript,document.documentElement.setAttribute('data-attacker-ran','1')"]) {
        f.evil.length = 0;
        const { context, page } = await afterOps({ modules: [url], ops: [] });
        assert.equal(await attr(page, "data-attacker-ran"), null, url);
        assert.deepEqual(f.evil, [], url);
        await context.close();
      }
      const { context, page } = await afterOps({ modules: ["/_navree/ui/stand-in.js"], ops: [] });
      assert.equal(await attr(page, "data-stand-in-ran"), "1");
      await context.close();
    });

    test(`hostile markup with a field's id doesn't adopt what was typed (${engine})`, async () => {
      for (const evil of [`<input id="secret" name="stolen">`, `<input id="secret" name="secret">`]) {
        const name = key("steal");
        route(`/${name}`, doc(`<div id="t"><form id="good" action="/good"><input id="secret" name="secret" value="server"></form></div><form id="f" data-ui-form method="post" action="/${name}-post"><button id="go">go</button></form>`));
        f.routes.set(`/${name}-post`, (req, res) => f.ops(res, { ops: [{ op: "morph", target: "t", html: `<div id="t"><form id="evilform" action="/evil">${evil}</form></div>` }] }));
        const { context, page } = await fresh(`/${name}`);
        await page.fill("#secret", "typed secret");
        await page.click("#go");
        await sleep(200);
        assert.equal(await page.inputValue("#secret"), "", evil);
        await context.close();
      }
    });

    test(`a field that keeps its name and form keeps what was typed (${engine})`, async () => {
      const name = key("samefield");
      route(`/${name}`, doc(`<div id="t"><form id="good" action="/good"><input id="v" name="v" value="server"></form></div><form id="f" data-ui-form method="post" action="/${name}-post"><button id="go">go</button></form>`));
      f.routes.set(`/${name}-post`, (req, res) => f.ops(res, { ops: [{ op: "morph", target: "t", html: `<div id="t"><form id="good" action="/good"><input id="v" name="v" value="server 2"></form></div>` }] }));
      const { context, page } = await fresh(`/${name}`);
      await page.fill("#v", "typed");
      await page.click("#go");
      await sleep(200);
      assert.equal(await page.inputValue("#v"), "typed");
      await context.close();
    });

    test(`stored markup can't make itself stay on every page (${engine})`, async () => {
      const name = key("persist");
      route(`/${name}`, doc(`<div id="stored" data-ui-persist>content of user aarti</div><a id="n" href="/${name}-next">n</a>`));
      route(`/${name}-next`, doc(`<h1 id="h">other page</h1>`));
      const { context, page } = await fresh(`/${name}`);
      await page.click("#n");
      await page.waitForSelector("#h");
      assert.equal(await page.locator("#stored").count(), 0);
      await context.close();
    });

    test(`the runtime's own toaster and announcer stay through a navigation (${engine})`, async () => {
      const name = key("toaster");
      route(`/${name}`, doc(`<form id="f" data-ui-form method="post" action="/${name}-post"><button id="go">go</button></form><a id="n" href="/${name}-next">n</a>`));
      route(`/${name}-next`, doc(`<h1 id="h">other page</h1>`));
      f.routes.set(`/${name}-post`, (req, res) => f.ops(res, { ops: [{ op: "toast", tone: "info", text: "still here" }] }));
      const { context, page } = await fresh(`/${name}`);
      await page.click("#go");
      await sleep(150);
      await page.click("#n");
      await page.waitForSelector("#h");
      assert.deepEqual(await toasts(page), ["still here"]);
      assert.equal(await page.locator("[data-slot=announcer]").count(), 1);
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

    test(`an error on a radio group shows under the group (${engine})`, async () => {
      const name = key("radio");
      route(`/${name}`, doc(`<form id="f" data-ui-form method="post" action="/${name}-post"><div data-slot="field"><fieldset data-slot="radio-group"><input type="radio" name="stage" value="a"><input type="radio" name="stage" value="b"></fieldset></div><button id="go">go</button></form>`));
      f.routes.set(`/${name}-post`, (req, res) => f.ops(res, { ops: [{ op: "errors", form: "f", fields: { stage: ["Choose a stage"] } }] }, 422));
      const { context, page } = await fresh(`/${name}`);
      await page.click("#go");
      await sleep(200);
      assert.equal(await page.textContent("[data-slot=field] [data-slot=field-error]"), "Choose a stage");
      assert.deepEqual(await toasts(page), []);
      await context.close();
    });

    test(`a failed op doesn't stop the rest or blame the connection (${engine})`, async () => {
      const { context, page, errors } = await afterOps({ ops: [{ op: "morph", target: "t", html: { a: 1 } }, { op: "url", href: `${E()}/x`, mode: "push" }, { op: "errors", form: "t", fields: { a: ["x"] } }, { op: "eval", code: "1" }, { op: "toast", tone: "info", text: "after" }] });
      const shown = await toasts(page);
      assert.ok(shown.includes("after"), JSON.stringify(shown));
      assert.ok(!shown.some((text) => text.includes("connection")), JSON.stringify(shown));
      assert.deepEqual(errors, []);
      assert.ok(page.url().startsWith(f.origin));
      await context.close();
    });

    test(`a toast's and an error's text stays text (${engine})`, async () => {
      const { context, page } = await afterOps(() => JSON.parse(`{"ops":[{"op":"toast","tone":"x\\" onclick=\\"1","text":"<img src=x onerror=1>"},{"op":"errors","form":"f","fields":{"__proto__":["<img src=x>"],"constructor":["y"]}}]}`));
      assert.equal(await page.locator("[data-slot=toast] img, [data-slot=field-error] img").count(), 0);
      assert.equal(await page.evaluate(() => ({}).polluted), undefined);
      await context.close();
    });

    test(`an envelope from another build reloads and applies nothing (${engine})`, async () => {
      const { context, page } = await afterOps({ build: "other", ops: [{ op: "toast", tone: "info", text: "should not show" }] });
      assert.deepEqual(await toasts(page), []);
      await context.close();
    });

    test(`a form being sent ignores a second submit (${engine})`, async () => {
      for (const how of ["double click", "enter twice", "requestSubmit twice"]) {
        const name = key("double");
        let writes = 0;
        // The answer waits until both presses are done, however slow the machine.
        let release;
        const held = new Promise((resolve) => (release = resolve));
        route(`/${name}`, doc(`<form id="f" data-ui-form method="post" action="/${name}-post"><input id="i" name="a" value="1"><button id="go">go</button></form>`));
        f.routes.set(`/${name}-post`, async (req, res) => {
          writes++;
          await held;
          f.ops(res, { ops: [{ op: "toast", tone: "info", text: "saved" }] });
        });
        const { context, page } = await fresh(`/${name}`);
        if (how === "double click") await page.dblclick("#go");
        else if (how === "enter twice") {
          await page.focus("#i");
          await page.keyboard.press("Enter");
          await sleep(40);
          await page.keyboard.press("Enter");
        } else await page.evaluate(() => { const form = document.getElementById("f"); form.requestSubmit(); form.requestSubmit(); });
        await sleep(100);
        release();
        await sleep(450);
        assert.equal(writes, 1, how);
        assert.deepEqual(await toasts(page), ["saved"], how);
        assert.equal(await page.evaluate(() => document.getElementById("f").hasAttribute("data-pending")), false, how);
        // Afterwards it submits again.
        await page.click("#go");
        await sleep(300);
        assert.equal(writes, 2, how);
        await context.close();
      }
    });

    test(`what is typed while a form is on its way is sent after it, the latest only (${engine})`, async () => {
      const name = key("latest");
      const bodies = [];
      route(`/${name}`, doc(`<form id="f" data-ui-form method="post" action="/${name}-post"><input id="i" name="a" value=""><input type="hidden" name="intent" value="check"></form>`));
      f.routes.set(`/${name}-post`, (req, res, body) => {
        bodies.push(body);
        setTimeout(() => f.ops(res, { ops: [] }), 300);
      });
      const { context, page, errors } = await fresh(`/${name}`);
      // A field check sent on every keystroke, as `Cmd::submit` on input does.
      await page.evaluate(() => {
        const form = document.getElementById("f");
        const input = document.getElementById("i");
        for (const text of ["a", "ab", "abc"]) {
          input.value = text;
          form.requestSubmit();
        }
        // The same again: a repeat, not news.
        form.requestSubmit();
      });
      await until(() => bodies.length >= 2, 3000);
      await sleep(500);
      assert.deepEqual(bodies, ["a=a&intent=check", "a=abc&intent=check"]);
      assert.deepEqual(errors, []);
      await context.close();
    });

    test(`a save's answer doesn't send what waited if it leaves the page (${engine})`, async () => {
      const name = key("leaves");
      const bodies = [];
      route(`/${name}`, doc(`<form id="f" data-ui-form method="post" action="/${name}-post"><input id="i" name="a" value="one"></form>`));
      route(`/${name}-done`, doc(`<h1 id="done">done</h1>`));
      f.routes.set(`/${name}-post`, (req, res, body) => {
        bodies.push(body);
        setTimeout(() => f.ops(res, { ops: [{ op: "navigate", href: `/${name}-done` }] }), 250);
      });
      const { context, page } = await fresh(`/${name}`);
      await page.evaluate(() => {
        const form = document.getElementById("f");
        form.requestSubmit();
        document.getElementById("i").value = "two";
        form.requestSubmit();
      });
      await page.waitForSelector("#done");
      await sleep(400);
      assert.deepEqual(bodies, ["a=one"]);
      await context.close();
    });

    test(`field errors take focus only for a save the user is still waiting on (${engine})`, async () => {
      const name = key("focus");
      const form = `<form id="f" data-ui-form method="post" action="/${name}-post"><div data-slot="field"><input id="title" name="title"></div><div data-slot="field"><input id="email" name="email"></div><button id="save">Save</button></form>`;
      route(`/${name}`, doc(form));
      f.routes.set(`/${name}-post`, (req, res) => setTimeout(() => f.ops(res, { ops: [{ op: "errors", form: "f", fields: { title: ["Enter a title."] } }] }, 422), 250));
      const focused = (page) => page.evaluate(() => document.activeElement?.id || document.activeElement?.nodeName);
      // A check sent by a command while the user types elsewhere: errors show, focus stays.
      let { context, page } = await fresh(`/${name}`);
      await page.focus("#email");
      await page.evaluate(() => document.getElementById("f").requestSubmit());
      await page.keyboard.type("anu");
      await page.waitForSelector("[data-slot=field-error]");
      assert.equal(await focused(page), "email");
      assert.equal(await page.inputValue("#email"), "anu");
      await context.close();
      // A save the user clicked and waited on: the first error takes focus.
      ({ context, page } = await fresh(`/${name}`));
      await page.click("#save");
      await page.waitForSelector("[data-slot=field-error]");
      await sleep(50);
      assert.equal(await focused(page), "title");
      await context.close();
      // A save the user clicked and then left for another field: focus stays there.
      ({ context, page } = await fresh(`/${name}`));
      await page.click("#save");
      await page.focus("#email");
      await page.waitForSelector("[data-slot=field-error]");
      await sleep(50);
      assert.equal(await focused(page), "email");
      await context.close();
    });

    test(`a form works again after a failed send (${engine})`, async () => {
      const name = key("retry");
      route(`/${name}`, doc(`<form id="f" data-ui-form method="post" action="/${name}-post"><button id="go">go</button></form>`));
      f.routes.set(`/${name}-post`, (req, res) => f.ops(res, { ops: [{ op: "toast", tone: "info", text: "saved" }] }));
      const { context, page } = await fresh(`/${name}`);
      await page.route(`**/${name}-post`, (request) => request.abort(), { times: 1 });
      await page.click("#go");
      await sleep(200);
      assert.ok((await toasts(page)).some((text) => text.includes("connection")));
      await page.click("#go");
      await sleep(200);
      assert.ok((await toasts(page)).includes("saved"));
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

    test(`the proof app sends exactly the policy (${engine})`, async () => {
      for (const path of ["/", "/plain", "/deals/new"]) {
        const response = await fetch(PROOF + path);
        assert.equal(response.headers.get("content-security-policy"), CSP, path);
      }
      // An answer with no page behind it allows less still.
      const missing = await fetch(`${PROOF}/nothing`);
      assert.match(missing.headers.get("content-security-policy"), /^default-src 'none'/);
    });

    test(`hovering Sign out in the proof app asks for nothing, and it signs out by POST (${engine})`, async () => {
      const context = await browser.newContext();
      const page = await context.newPage();
      const sent = [];
      page.on("request", (request) => sent.push(request));
      await page.goto(`${PROOF}/deals/new`);
      await page.waitForLoadState("networkidle");
      await page.click("[data-slot=sidebar-user]");
      const out = page.getByRole("button", { name: "Sign out" });
      await out.waitFor();
      sent.length = 0;
      await out.hover();
      await sleep(200);
      await out.focus();
      await sleep(200);
      assert.deepEqual(sent.map((request) => request.url()), []);
      await out.click();
      await page.waitForURL("**/plain");
      const post = sent.find((request) => request.method() === "POST");
      assert.ok(post, "a POST");
      assert.equal(new URL(post.url()).pathname, "/sign-out");
      await context.close();
    });

    test(`a sidebar link in the proof app is fetched ahead only when it asks (${engine})`, async () => {
      const context = await browser.newContext();
      const page = await context.newPage();
      await page.goto(`${PROOF}/deals/new`);
      await page.waitForLoadState("networkidle");
      const asked = [];
      page.on("request", (request) => asked.push(new URL(request.url()).pathname));
      await page.hover('[data-slot=sidebar-item][href="/plain"]');
      await sleep(250);
      assert.deepEqual(asked.filter((path) => path === "/plain"), [], "an item that didn't ask");
      const seen = page.waitForRequest((request) => new URL(request.url()).pathname === "/gallery");
      await page.hover('[data-slot=sidebar-item][href="/gallery"]');
      const request = await seen;
      assert.equal(request.headers()["x-navree-prefetch"], "1");
      await context.close();
    });

    test(`signing out works with JavaScript off (${engine})`, async () => {
      const context = await browser.newContext({ javaScriptEnabled: false });
      const page = await context.newPage();
      await page.goto(`${PROOF}/deals/new`);
      await page.click("[data-slot=sidebar-user]");
      await page.getByRole("button", { name: "Sign out" }).click();
      await page.waitForURL("**/plain");
      await context.close();
    });

    test(`a cross-site form can't post to the proof app (${engine})`, async () => {
      const response = await fetch(`${PROOF}/deals`, { method: "POST", headers: { "content-type": "application/x-www-form-urlencoded", origin: "https://evil.example", "sec-fetch-site": "cross-site" }, body: "title=x", redirect: "manual" });
      assert.equal(response.status, 403);
    });
  });
}
