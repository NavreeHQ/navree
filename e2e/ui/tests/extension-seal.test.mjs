// The seal on a plugin component's owned body and on a plugin island's body, in Chromium,
// Firefox and WebKit, under the page's exact content security policy:
// - a plugin's island never runs unsealed on first load, after a boosted navigation, after
//   an ops reply, when the extensions module fails to load, or on a page that starts clean;
// - a form written into a sealed body owns no field of the page;
// - a frame, an object or an embed written into a sealed body is taken out and loads nothing;
// - refresh and navigation hooks written into a sealed body do not leave the page;
// - a morph that replaces a sealed body leaves the new body sealed.
//
//   UI_BROWSERS=chromium,firefox,webkit node --test --test-concurrency=1 tests/extension-seal.test.mjs

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { after, afterEach, before, describe, test } from "node:test";
import { fileURLToPath } from "node:url";
import { chromium, firefox, webkit } from "playwright";
import { BUILD, page as doc, start } from "./support/fixture.mjs";
import { launch } from "./support/launch.mjs";

const DIST = join(dirname(fileURLToPath(import.meta.url)), "../../../crates/navree-ui/dist");
const EXT_JS = readFileSync(join(DIST, "extensions.js"));
const ISL_JS = readFileSync(join(DIST, "islands.js"));

const ENGINES = { chromium, firefox, webkit };
const NAMES = (process.env.UI_BROWSERS || "chromium,firefox,webkit").split(/[ ,]+/).filter((n) => ENGINES[n]);

/** Browser network status lines for expected non-2xx module checks, distinct from script errors. */
const NOISE = /Failed to load resource|ERR_|NS_ERROR|Load failed|Loading failed for the|Importing a module script failed|disallowed MIME type|status of \d{3}/i;

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

let seq = 0;
const key = (prefix) => `${prefix}-${++seq}`;
const hex = () => String(++seq).padStart(12, "0");

async function until(done, what, ms = 8000) {
  for (const end = Date.now() + ms; Date.now() < end; ) {
    if (await done()) return;
    await sleep(25);
  }
  assert.fail(`timed out waiting for ${what}`);
}

const pluginIslandSource = () => `
  const bodies = [...document.querySelectorAll('[data-ui-island*="notices.banner"] > [data-slot=island-body]')].map((b) => ({
    stored: b.hasAttribute("data-ui-stored"),
    hooks: b.querySelectorAll("[data-ui-click],[data-ui-form]").length,
  }));
  (window.__pluginIsland ||= []).push({ phase: "import", bodies });
  export function mount(body, props, island) {
    const sealedAtMount =
      body.hasAttribute("data-ui-stored") &&
      body.querySelectorAll("[data-ui-click],[data-ui-form]").length === 0 &&
      document.getElementById("ui-sealed")?.tagName === "TEMPLATE";
    body.innerHTML =
      '<p data-banner-city></p>' +
      '<button data-banner-cmd type="button" data-ui-click=\\'[["toast",{"tone":"info","text":"unsealed"}]]\\'>Notify</button>' +
      '<input data-banner-field name="banner" value="Amritsar">';
    body.querySelector("[data-banner-city]").textContent = props?.city || "Ludhiana";
    (window.__pluginIsland ||= []).push({ phase: "mount", id: island.id, sealedAtMount, city: props?.city || "Ludhiana" });
  }
`;

const appIslandSource = `
  export function mount(body, props, island) {
    body.textContent = "Depot: " + (props?.city || "Jalandhar");
    (window.__appIsland ||= []).push(island.id);
  }
`;

const bannerMarkup = (hash, city) =>
  `<div id="banner" data-slot="island" data-ui-island="/_navree/ui/island/notices.banner-${hash}.js" data-ui-props='${JSON.stringify({ city })}'>` +
  `<div id="banner-body" data-slot="island-body" data-ui-keep>` +
  `<button type="button" data-fallback-cmd data-ui-click='[["toast",{"tone":"info","text":"fallback"}]]'>Old</button>` +
  `</div></div>`;

for (const name of NAMES) {
  describe(name, { concurrency: false }, () => {
    let browser, f;
    const contexts = [];

    before(async () => {
      browser = await launch(ENGINES[name], name === "firefox" ? { firefoxUserPrefs: { "network.http.throttle.enable": false } } : {});
      f = await start();
    });

    after(async () => {
      await browser.close();
      f.close();
    });

    afterEach(async () => {
      for (const context of contexts.splice(0)) await context.close().catch(() => {});
    });

    const route = (path, body, headers) =>
      f.routes.set(path, (_req, res) => f.html(res, typeof body === "function" ? body() : body, headers));

    const js = (path, source, delayMs = 0) =>
      f.routes.set(path, (_req, res) => {
        const send = () => {
          res.writeHead(200, { "content-type": "text/javascript", "cache-control": "no-store" });
          res.end(source);
        };
        if (delayMs > 0) setTimeout(send, delayMs);
        else send();
      });

    const hits = (path) => f.log.filter((entry) => entry.path.split("?")[0] === path);

    async function open(path, { allowCsp, allowConsole } = {}) {
      const context = await browser.newContext({ viewport: { width: 1280, height: 800 } });
      contexts.push(context);
      const page = await context.newPage();
      const problems = [];
      page.on("console", (m) => {
        if (m.type() !== "error") return;
        const text = m.text();
        if (NOISE.test(text) || allowConsole?.test(text) || (name === "webkit" && /Refused to apply a stylesheet/.test(text))) return;
        problems.push(text);
      });
      page.on("pageerror", (e) => problems.push("pageerror: " + e.message));
      await page.addInitScript(() => {
        document.addEventListener("securitypolicyviolation", (e) => (window.__csp ||= []).push(`${e.violatedDirective} ${e.blockedURI}`));
      });
      await page.goto(f.origin + path, { waitUntil: "load" });
      await sleep(40);
      const close = async () => {
        let csp = await page.evaluate(() => window.__csp || []);
        if (name === "webkit") csp = csp.filter((line) => !/^style-src-(elem|attr) inline$/.test(line));
        if (allowCsp) csp = csp.filter((line) => !allowCsp.test(line));
        const idx = contexts.indexOf(context);
        if (idx !== -1) contexts.splice(idx, 1);
        await context.close();
        assert.deepEqual(csp, [], "no unexpected content security policy report");
        assert.deepEqual(problems, []);
      };
      return { page, close };
    }

    async function assertBannerSealed(page, expectedCity) {
      await page.locator("#banner[data-ui-mounted]").waitFor({ state: "attached" });
      const state = await page.evaluate(() => {
        const body = document.getElementById("banner-body");
        const cmd = body.querySelector("[data-banner-cmd]");
        const field = body.querySelector("[data-banner-field]");
        return {
          log: window.__pluginIsland,
          failed: document.getElementById("banner").hasAttribute("data-ui-failed"),
          stored: body.hasAttribute("data-ui-stored"),
          city: body.querySelector("[data-banner-city]")?.textContent,
          cmdClick: cmd.hasAttribute("data-ui-click"),
          cmdFormAttr: cmd.getAttribute("form"),
          cmdForm: cmd.form ? cmd.form.id || cmd.form.tagName : null,
          fieldFormAttr: field.getAttribute("form"),
          fieldForm: field.form ? field.form.id || field.form.tagName : null,
        };
      });
      assert.equal(state.log.length, 2);
      assert.deepEqual(state.log[0], { phase: "import", bodies: [{ stored: true, hooks: 0 }] });
      assert.deepEqual(state.log[1], { phase: "mount", id: "banner", sealedAtMount: true, city: expectedCity });
      assert.equal(state.failed, false);
      assert.equal(state.stored, true);
      assert.equal(state.city, expectedCity);
      assert.equal(state.cmdClick, false);
      assert.equal(state.cmdFormAttr, "ui-sealed");
      assert.equal(state.cmdForm, null);
      assert.equal(state.fieldFormAttr, "ui-sealed");
      assert.equal(state.fieldForm, null);
      await page.click("#banner [data-banner-cmd]");
      await sleep(80);
      assert.equal(await page.locator("[data-slot=toast]").count(), 0);
    }

    test("a_plugins_island_never_runs_unsealed_on_first_load_after_a_navigation_and_after_an_ops_reply", async () => {
      // 1. First load: the head lists extensions ahead of islands.
      const firstHash = hex();
      const firstPath = key("first");
      js(`/_navree/ui/island/notices.banner-${firstHash}.js`, pluginIslandSource());
      route(
        `/${firstPath}`,
        doc(bannerMarkup(firstHash, "Ludhiana"), {
          head: `<script type="module" src="/_navree/ui/extensions.js"></script><script type="module" src="/_navree/ui/islands.js"></script>`,
        }),
      );
      const firstPage = await open(`/${firstPath}`);
      await assertBannerSealed(firstPage.page, "Ludhiana");
      await firstPage.close();

      // 2. Boosted navigation bringing both modules, with islands arriving ahead of extensions.
      const navHash = hex();
      const navPath = key("nav");
      js(`/_navree/ui/island/notices.banner-${navHash}.js`, pluginIslandSource());
      js(`/_navree/ui/extensions-${navHash}.js`, EXT_JS, 120);
      js(`/_navree/ui/islands-${navHash}.js`, ISL_JS, 0);
      route(`/${navPath}`, doc(`<a id="to-notices" href="/${navPath}-dest">Notices</a>`));
      route(
        `/${navPath}-dest`,
        doc(bannerMarkup(navHash, "Amritsar"), {
          head: `<script type="module" src="/_navree/ui/extensions-${navHash}.js"></script><script type="module" src="/_navree/ui/islands-${navHash}.js"></script>`,
        }),
      );
      const navPage = await open(`/${navPath}`);
      await navPage.page.click("#to-notices");
      await assertBannerSealed(navPage.page, "Amritsar");
      await navPage.close();

      // 3. An ops reply bringing both modules (islands ahead of extensions) and replacing in the island.
      const opsHash = hex();
      const opsPath = key("ops");
      js(`/_navree/ui/island/notices.banner-${opsHash}.js`, pluginIslandSource());
      js(`/_navree/ui/extensions-${opsHash}.js`, EXT_JS, 120);
      js(`/_navree/ui/islands-${opsHash}.js`, ISL_JS, 0);
      route(
        `/${opsPath}`,
        doc(`<div id="slot">empty</div><form id="load-form" data-ui-form method="post" action="/${opsPath}-post"><button id="load-go" type="submit">Load</button></form>`),
      );
      f.routes.set(`/${opsPath}-post`, (_req, res) =>
        f.ops(res, {
          modules: [`/_navree/ui/islands-${opsHash}.js`, `/_navree/ui/extensions-${opsHash}.js`],
          ops: [{ op: "replace", target: "slot", html: bannerMarkup(opsHash, "Patiala") }],
        }),
      );
      const opsPage = await open(`/${opsPath}`);
      await opsPage.page.click("#load-go");
      await assertBannerSealed(opsPage.page, "Patiala");
      await opsPage.close();

      // 4. The extensions module fails to load: the app's island mounts; the plugin's fails without importing its script.
      const blockedHash = hex();
      const blockedPath = key("blocked");
      js(`/_navree/ui/island/notices.banner-${blockedHash}.js`, pluginIslandSource());
      js(`/_navree/ui/island/depot-${blockedHash}.js`, appIslandSource);
      route(
        `/${blockedPath}`,
        doc(
          bannerMarkup(blockedHash, "Bathinda") +
            `<div id="depot" data-slot="island" data-ui-island="/_navree/ui/island/depot-${blockedHash}.js" data-ui-props='{"city":"Jalandhar"}'><div data-slot="island-body" data-ui-keep>Waiting</div></div>`,
          { head: `<script type="module" src="/_navree/ui/extensions-${blockedHash}.js"></script><script type="module" src="/_navree/ui/islands.js"></script>` },
        ),
      );
      const blockedPage = await open(`/${blockedPath}`);
      await blockedPage.page.locator("#depot[data-ui-mounted]").waitFor({ state: "attached" });
      await blockedPage.page.locator("#banner[data-ui-failed]").waitFor({ state: "attached" });
      await sleep(120);
      assert.equal(await blockedPage.page.textContent("#depot [data-slot=island-body]"), "Depot: Jalandhar");
      assert.equal(await blockedPage.page.getAttribute("#banner", "data-ui-mounted"), null);
      assert.equal(hits(`/_navree/ui/island/notices.banner-${blockedHash}.js`).length, 0);
      assert.equal(await blockedPage.page.evaluate(() => window.__pluginIsland), undefined);
      await blockedPage.close();

      // 5. A page that starts clean: the app's island mounts; the plugin's script is never asked for.
      const freshHash = hex();
      const freshPath = key("fresh");
      js(`/_navree/ui/island/notices.banner-${freshHash}.js`, pluginIslandSource());
      js(`/_navree/ui/island/depot-${freshHash}.js`, appIslandSource);
      const freshHtml =
        `<!doctype html><html lang="en" data-build="${BUILD}" data-ui-fresh-page><head><meta charset="utf-8"><title>t</title>` +
        `<script type="module" src="/_navree/ui/core.js"></script><script type="module" src="/_navree/ui/extensions.js"></script><script type="module" src="/_navree/ui/islands.js"></script>` +
        `</head><body data-ui-boost>${bannerMarkup(freshHash, "Ludhiana")}` +
        `<div id="depot" data-slot="island" data-ui-island="/_navree/ui/island/depot-${freshHash}.js" data-ui-props='{"city":"Jalandhar"}'><div data-slot="island-body" data-ui-keep>Waiting</div></div>` +
        `</body></html>`;
      route(`/${freshPath}`, freshHtml, { "x-navree-fresh": "1" });
      const freshPage = await open(`/${freshPath}`);
      await freshPage.page.locator("#depot[data-ui-mounted]").waitFor({ state: "attached" });
      await sleep(120);
      assert.equal(await freshPage.page.textContent("#depot [data-slot=island-body]"), "Depot: Jalandhar");
      assert.equal(await freshPage.page.getAttribute("#banner", "data-ui-mounted"), null);
      assert.equal(hits(`/_navree/ui/island/notices.banner-${freshHash}.js`).length, 0);
      assert.equal(await freshPage.page.evaluate(() => window.__pluginIsland), undefined);
      await freshPage.close();
    });

    test("a_form_written_into_a_sealed_body_owns_no_field_of_the_page", async () => {
      const h = hex();
      const p = key("forms");
      js(
        `/_navree/ui/ext/khata.pad-${h}.js`,
        `
        export const contract = 1;
        export function mount(el) {
          const body = el.querySelector("[data-ui-keep]");
          const planted = document.createElement("form");
          planted.id = "ui-sealed";
          planted.method = "get";
          planted.action = "/${p}-planted";
          planted.innerHTML =
            '<input id="written-q" name="q" value="Ludhiana">' +
            '<select id="written-city" name="city"><option value="Patiala" selected>Patiala</option></select>' +
            '<textarea id="written-note" name="note">Leave at gate</textarea>' +
            '<fieldset id="written-group" name="group"><input id="group-item" name="item" value="1"></fieldset>' +
            '<output id="written-out" name="out">99</output>' +
            '<button id="written-submit" type="submit" name="via" value="sealed">Search</button>';
          body.append(planted);
          body.insertAdjacentHTML(
            "beforeend",
            '<input id="written-charge" name="charge" value="0">' +
            '<input id="repointed-charge" name="charge" value="999" form="delivery">'
          );
        }
        `,
      );
      route(
        `/${p}`,
        doc(
          `<input id="stray-field" name="stray" value="Bathinda" form="ui-sealed">` +
            `<form id="delivery" data-ui-form method="post" action="/${p}-save">` +
            `<input id="real-recipient" name="recipient" value="Harpreet Sandhu">` +
            `<input id="real-charge" name="charge" value="40">` +
            `<div id="pad" data-slot="khata:pad" data-ui-ext="/_navree/ui/ext/khata.pad-${h}.js">` +
            `<div id="pad-body" data-slot="khata:pad-body" data-ui-keep><input id="initial-charge" name="charge" value="0"></div>` +
            `</div>` +
            `<button id="real-save" type="submit">Save delivery</button>` +
            `</form>`,
          { head: `<script type="module" src="/_navree/ui/extensions.js"></script>` },
        ),
      );
      f.routes.set(`/${p}-save`, (_req, res) => f.ops(res, { ops: [] }));

      const { page, close } = await open(`/${p}`);
      await page.locator("#pad[data-ui-mounted]").waitFor({ state: "attached" });
      await page.evaluate(() => {
        document.getElementById("repointed-charge").setAttribute("form", "delivery");
      });

      const state = await page.evaluate(() => {
        const ids = [
          "initial-charge",
          "written-q",
          "written-city",
          "written-note",
          "written-group",
          "group-item",
          "written-out",
          "written-submit",
          "written-charge",
          "repointed-charge",
        ];
        const writtenForm = document.querySelector("#pad-body form#ui-sealed");
        return {
          headFirstTag: document.getElementById("ui-sealed")?.tagName,
          strayOwner: document.getElementById("stray-field").form?.tagName ?? null,
          sealedFields: ids.map((id) => {
            const el = document.getElementById(id);
            return { id, attr: el.getAttribute("form"), owner: el.form ? el.form.id || el.form.tagName : null };
          }),
          writtenElements: writtenForm.elements.length,
          writtenData: [...new FormData(writtenForm).entries()],
        };
      });

      assert.equal(state.headFirstTag, "TEMPLATE");
      assert.equal(state.strayOwner, null);
      for (const item of state.sealedFields) {
        assert.equal(item.attr, "ui-sealed", `${item.id} form attribute`);
        assert.equal(item.owner, null, `${item.id} form owner`);
      }
      assert.equal(state.writtenElements, 0);
      assert.deepEqual(state.writtenData, []);

      await page.click("#written-submit");
      await sleep(100);
      assert.equal(page.url(), f.origin + `/${p}`);
      assert.equal(hits(`/${p}-planted`).length, 0);
      assert.equal(hits(`/${p}-save`).length, 0);

      await page.click("#real-save");
      await until(() => hits(`/${p}-save`).length === 1, "delivery form save");
      assert.deepEqual([...new URLSearchParams(hits(`/${p}-save`)[0].body).entries()], [
        ["recipient", "Harpreet Sandhu"],
        ["charge", "40"],
      ]);
      await close();
    });

    test("a_frame_with_its_own_content_written_into_a_sealed_body_does_not_load", async () => {
      const h = hex();
      const p = key("frames");
      js(`/_navree/ui/frame-probe-${h}.js`, `parent.document.documentElement.setAttribute("data-frame-ran", "1");`);
      js(
        `/_navree/ui/ext/khata.frame-${h}.js`,
        `
        export const contract = 1;
        export function mount(el) {
          const body = el.querySelector("[data-ui-keep]");
          body.innerHTML =
            '<iframe id="in-srcdoc" srcdoc="<script type=&quot;module&quot; src=&quot;/_navree/ui/core.js&quot;></script><script type=&quot;module&quot; src=&quot;/_navree/ui/frame-probe-${h}.js&quot;></script><p>Ludhiana</p>"></iframe>' +
            '<iframe id="in-src" src="/${p}-frame"></iframe>' +
            '<object id="in-object" data="/${p}-object"></object>' +
            '<embed id="in-embed" src="/${p}-embed">';
          const other = document.implementation.createHTMLDocument("");
          const foreign = other.createElement("iframe");
          foreign.setAttribute("srcdoc", '<script type="module" src="/_navree/ui/frame-probe-${h}.js"></script>');
          body.append(foreign);
        }
        `,
      );
      js(`/_navree/ui/ext/khata.plain-${h}.js`, `export const contract = 1; export function mount() {}`);
      route(`/${p}-frame`, `<p>Framed</p>`);
      route(`/${p}-object`, `<p>Object</p>`);
      route(`/${p}-embed`, `<p>Embed</p>`);
      route(
        `/${p}`,
        doc(
          `<div id="pad" data-slot="khata:pad" data-ui-ext="/_navree/ui/ext/khata.frame-${h}.js"><div id="pad-body" data-slot="khata:pad-body" data-ui-keep></div></div>` +
            `<div id="slot"></div>` +
            `<form id="add-second" data-ui-form method="post" action="/${p}-add"><button id="add-go" type="submit">Add</button></form>`,
          { head: `<script type="module" src="/_navree/ui/extensions.js"></script>` },
        ),
      );
      f.routes.set(`/${p}-add`, (_req, res) =>
        f.ops(res, {
          ops: [
            {
              op: "replace",
              target: "slot",
              html:
                `<div id="second-pad" data-slot="khata:pad" data-ui-ext="/_navree/ui/ext/khata.plain-${h}.js">` +
                `<div id="second-body" data-slot="khata:pad-body" data-ui-keep>` +
                `<iframe srcdoc="<script type=&quot;module&quot; src=&quot;/_navree/ui/frame-probe-${h}.js&quot;></script>"></iframe>` +
                `<object data="/${p}-object"></object><embed src="/${p}-embed">` +
                `</div></div>`,
            },
          ],
        }),
      );

      const { page, close } = await open(`/${p}`, {
        allowCsp: /^(default|frame|child|object)-src\b/,
        allowConsole: /Content[- ]Security[- ]Policy/i,
      });
      await page.locator("#pad[data-ui-mounted]").waitFor({ state: "attached" });
      await page.click("#add-go");
      await page.locator("#second-pad[data-ui-mounted]").waitFor({ state: "attached" });
      await sleep(200);

      assert.equal(await page.evaluate(() => document.querySelectorAll("iframe,frame,object,embed").length), 0);
      assert.equal(await page.getAttribute("html", "data-frame-ran"), null);
      assert.equal(hits(`/_navree/ui/frame-probe-${h}.js`).length, 0);
      assert.equal(hits(`/${p}-frame`).length, 0);
      assert.equal(hits(`/${p}-object`).length, 0);
      assert.equal(hits(`/${p}-embed`).length, 0);
      await close();
    });

    test("a_refresh_written_into_a_sealed_body_does_not_leave_the_page", async () => {
      const h = hex();
      const p = key("refresh");
      js(
        `/_navree/ui/ext/khata.refresh-${h}.js`,
        `
        export const contract = 1;
        export function mount(el) {
          const body = el.querySelector("[data-ui-keep]");
          body.innerHTML =
            '<meta http-equiv="refresh" content="0;url=javascript:document.documentElement.setAttribute(\\'data-refresh-ran\\',\\'1\\')">' +
            '<div id="in-poll" data-ui-region data-ui-src="/${p}-away" data-ui-lazy data-ui-poll="100">' +
              '<button id="in-refresh" type="button" data-ui-refresh>Again</button>' +
            '</div>' +
            '<form id="in-form" data-ui-form method="post" action="/${p}-away">' +
              '<button id="in-send" type="submit">Send</button>' +
            '</form>' +
            '<button id="in-nav" type="button" data-ui-click=\\'[["navigate","/${p}-away"]]\\'>Go</button>';
        }
        `,
      );
      js(`/_navree/ui/ext/khata.plain-${h}.js`, `export const contract = 1; export function mount() {}`);
      route(`/${p}-away`, doc(`<h1 id="left">Left</h1>`));
      route(
        `/${p}`,
        doc(
          `<div id="pad" data-slot="khata:pad" data-ui-ext="/_navree/ui/ext/khata.refresh-${h}.js"><div id="pad-body" data-slot="khata:pad-body" data-ui-keep></div></div>` +
            `<div id="second-pad" data-slot="khata:pad" data-ui-ext="/_navree/ui/ext/khata.plain-${h}.js"><div id="second-body" data-slot="khata:pad-body" data-ui-keep>Ready</div></div>` +
            `<form id="redraw-second" data-ui-form method="post" action="/${p}-morph"><button id="morph-go" type="submit">Redraw</button></form>`,
          { head: `<script type="module" src="/_navree/ui/lazy.js"></script><script type="module" src="/_navree/ui/extensions.js"></script>` },
        ),
      );
      f.routes.set(`/${p}-morph`, (_req, res) =>
        f.ops(res, {
          ops: [
            {
              op: "morph",
              target: "second-pad",
              html:
                `<div id="second-pad" data-slot="khata:pad" data-ui-ext="/_navree/ui/ext/khata.plain-${h}.js">` +
                `<div data-slot="khata:pad-body" data-ui-keep>` +
                `<meta http-equiv="refresh" content="0;url=/${p}-away">` +
                `<button id="morphed-nav" type="button" data-ui-click='[["navigate","/${p}-away"]]'>Go</button>` +
                `</div></div>`,
            },
          ],
        }),
      );

      const { page, close } = await open(`/${p}`, { allowConsole: /Refused to refresh .* to a javascript: URL/i });
      await page.locator("#pad[data-ui-mounted]").waitFor({ state: "attached" });
      await page.locator("#second-pad[data-ui-mounted]").waitFor({ state: "attached" });
      await page.evaluate(() => {
        document.getElementById("second-body").id = "second-drawn";
      });
      await page.click("#morph-go");
      await page.locator("#morphed-nav").waitFor();

      await page.click("#in-refresh");
      await page.click("#in-send");
      await page.click("#in-nav");
      await page.click("#morphed-nav");
      await sleep(300);

      assert.equal(page.url(), f.origin + `/${p}`);
      assert.equal(await page.evaluate(() => document.querySelectorAll("meta[http-equiv]").length), 0);
      assert.equal(
        await page.evaluate(
          () =>
            document.querySelectorAll(
              "#pad-body [data-ui-region], #pad-body [data-ui-src], #pad-body [data-ui-lazy], #pad-body [data-ui-poll], #pad-body [data-ui-refresh], #pad-body [data-ui-form], #pad-body [data-ui-click], #second-pad [data-ui-click]",
            ).length,
        ),
        0,
      );
      assert.equal(await page.getAttribute("html", "data-refresh-ran"), null);
      assert.equal(hits(`/${p}-away`).length, 0);
      await close();
    });

    test("a_morph_that_replaces_a_sealed_body_leaves_it_sealed", async () => {
      const h = hex();
      const p = key("morph");
      js(
        `/_navree/ui/ext/khata.pad-${h}.js`,
        `
        export const contract = 1;
        export function mount(el, _props, ui) {
          const body = el.querySelector("[data-ui-keep]");
          body.id = "pad-drawn-1";
          window.__morphPad = { mounts: (window.__morphPad?.mounts || 0) + 1, updates: 0, firstBody: body, ui };
          return {
            update() {
              window.__morphPad.updates++;
            },
          };
        }
        `,
      );
      js(
        `/_navree/ui/island/notices.card-${h}.js`,
        `
        export function mount(body) {
          body.id = "notice-drawn-1";
          window.__morphNotice = { mounts: (window.__morphNotice?.mounts || 0) + 1, firstBody: body };
        }
        `,
      );
      js(
        `/_navree/ui/island/depot-${h}.js`,
        `
        export function mount(body) {
          body.id = "depot-drawn-1";
          body.innerHTML = '<input id="depot-initial" name="depot_city" value="Jalandhar">';
          window.__morphDepot = { mounts: (window.__morphDepot?.mounts || 0) + 1, firstBody: body };
        }
        `,
      );

      const orderMarkup = () =>
        `<form id="order" data-ui-form method="post" action="/${p}-redraw">` +
        `<input name="customer" value="Simran Kaur">` +
        `<div id="pad" data-slot="khata:pad" data-ui-ext="/_navree/ui/ext/khata.pad-${h}.js" data-ui-props='{"step":1}'>` +
        `<div data-slot="khata:pad-body" data-ui-keep>` +
        `<button type="button" data-pad-fallback data-ui-click='[["toast",{"tone":"info","text":"pad-fallback"}]]'>Old pad</button>` +
        `</div></div>` +
        `<div id="notice" data-slot="island" data-ui-island="/_navree/ui/island/notices.card-${h}.js" data-ui-props='{"city":"Ludhiana"}'>` +
        `<div data-slot="island-body" data-ui-keep>` +
        `<button type="button" data-notice-fallback data-ui-click='[["toast",{"tone":"info","text":"notice-fallback"}]]'>Old notice</button>` +
        `</div></div>` +
        `<div id="depot" data-slot="island" data-ui-island="/_navree/ui/island/depot-${h}.js" data-ui-props='{"city":"Jalandhar"}'>` +
        `<div data-slot="island-body" data-ui-keep>` +
        `<input id="depot-fresh" name="depot_city" value="Patiala">` +
        `<button id="depot-toast" type="button" data-ui-click='[["toast",{"tone":"info","text":"app-island-works"}]]'>Ping</button>` +
        `</div></div>` +
        `<button id="redraw" type="submit">Redraw</button>` +
        `<button id="save-order" type="submit" formaction="/${p}-save">Save</button>` +
        `</form>`;

      route(
        `/${p}`,
        doc(orderMarkup(), {
          head: `<script type="module" src="/_navree/ui/extensions.js"></script><script type="module" src="/_navree/ui/islands.js"></script>`,
        }),
      );
      f.routes.set(`/${p}-redraw`, (_req, res) => f.ops(res, { ops: [{ op: "morph", target: "order", html: orderMarkup() }] }));
      f.routes.set(`/${p}-save`, (_req, res) => f.ops(res, { ops: [] }));

      const { page, close } = await open(`/${p}`);
      await page.locator("#pad[data-ui-mounted]").waitFor({ state: "attached" });
      await page.locator("#notice[data-ui-mounted]").waitFor({ state: "attached" });
      await page.locator("#depot[data-ui-mounted]").waitFor({ state: "attached" });

      await page.click("#redraw");
      await until(() => hits(`/${p}-redraw`).length === 1, "order morph post");
      await page.waitForFunction(() => window.__morphPad?.updates === 1);

      const afterMorph = await page.evaluate(() => {
        const padBody = document.querySelector("#pad > [data-ui-keep]");
        const noticeBody = document.querySelector("#notice > [data-slot=island-body]");
        const depotBody = document.querySelector("#depot > [data-slot=island-body]");
        const padFallback = padBody.querySelector("[data-pad-fallback]");
        const noticeFallback = noticeBody.querySelector("[data-notice-fallback]");

        padBody.insertAdjacentHTML(
          "beforeend",
          '<button id="pad-after" type="button" data-ui-click=\'[["toast",{"tone":"info","text":"pad-after"}]]\'>After</button>' +
            '<input id="pad-field" name="extra" value="1">',
        );
        noticeBody.insertAdjacentHTML(
          "beforeend",
          '<button id="notice-after" type="button" data-ui-click=\'[["toast",{"tone":"info","text":"notice-after"}]]\'>After</button>' +
            '<input id="notice-field" name="extra" value="2">',
        );

        return {
          mounts: [window.__morphPad.mounts, window.__morphNotice.mounts, window.__morphDepot.mounts],
          replaced: [
            padBody !== window.__morphPad.firstBody && !window.__morphPad.firstBody.isConnected,
            noticeBody !== window.__morphNotice.firstBody && !window.__morphNotice.firstBody.isConnected,
            depotBody !== window.__morphDepot.firstBody && !window.__morphDepot.firstBody.isConnected,
          ],
          stored: [padBody.hasAttribute("data-ui-stored"), noticeBody.hasAttribute("data-ui-stored"), depotBody.hasAttribute("data-ui-stored")],
          fallbackClicks: [padFallback.hasAttribute("data-ui-click"), noticeFallback.hasAttribute("data-ui-click")],
          fallbackForms: [padFallback.getAttribute("form"), noticeFallback.getAttribute("form")],
          foundInNewBody: window.__morphPad.ui.find("[data-pad-fallback]") === padFallback,
        };
      });

      assert.deepEqual(afterMorph.mounts, [1, 1, 1]);
      assert.deepEqual(afterMorph.replaced, [true, true, true]);
      assert.deepEqual(afterMorph.stored, [true, true, false]);
      assert.deepEqual(afterMorph.fallbackClicks, [false, false]);
      assert.deepEqual(afterMorph.fallbackForms, ["ui-sealed", "ui-sealed"]);
      assert.equal(afterMorph.foundInNewBody, true);

      const afterWrite = await page.evaluate(() => ({
        clicks: [document.getElementById("pad-after").hasAttribute("data-ui-click"), document.getElementById("notice-after").hasAttribute("data-ui-click")],
        forms: [
          document.getElementById("pad-after").getAttribute("form"),
          document.getElementById("notice-after").getAttribute("form"),
          document.getElementById("pad-field").getAttribute("form"),
          document.getElementById("notice-field").getAttribute("form"),
        ],
        owners: [
          document.getElementById("pad-field").form?.tagName ?? null,
          document.getElementById("notice-field").form?.tagName ?? null,
          document.getElementById("depot-fresh").form?.id ?? null,
        ],
      }));
      assert.deepEqual(afterWrite.clicks, [false, false]);
      assert.deepEqual(afterWrite.forms, ["ui-sealed", "ui-sealed", "ui-sealed", "ui-sealed"]);
      assert.deepEqual(afterWrite.owners, [null, null, "order"]);

      await page.click("[data-pad-fallback]");
      await page.click("[data-notice-fallback]");
      await page.click("#pad-after");
      await page.click("#notice-after");
      await sleep(80);
      assert.equal(await page.locator("[data-slot=toast]").count(), 0);

      await page.click("#depot-toast");
      await page.locator("[data-slot=toast]").filter({ hasText: "app-island-works" }).first().waitFor();

      await page.click("#save-order");
      await until(() => hits(`/${p}-save`).length === 1, "order save post");
      assert.deepEqual([...new URLSearchParams(hits(`/${p}-save`)[0].body).entries()], [
        ["customer", "Simran Kaur"],
        ["depot_city", "Patiala"],
      ]);
      await close();
    });
  });
}
