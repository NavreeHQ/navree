// Components of the app's own and of a plugin, in real browsers, against the proof app's /extensions pages:
// a script that mounts once, is given new props in place and is let go of when its element leaves; the body
// it owns, kept through a morph and sealed before the script runs, so what a library writes there drives
// nothing and posts with nothing; calls that reach only the routes the server named; a script that asks for
// a file of another site and fails with its fallback; a stylesheet that reaches only its own components;
// pages that start clean, entered by a full load and never drawn into another page; the map plugin's drop
// point edited in place; and the page's content security policy, which no source widens.
// Run with the proof app serving:
//
//   cargo run --release -p ui-proof -- serve          # UI_PROOF_URL, default http://127.0.0.1:8090
//   UI_BROWSERS=chromium,firefox,webkit npm test -- tests/extensions.test.mjs
//
// The last test prints the seal's measured cost, one line for each engine, starting `seal:`.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { after, afterEach, before, beforeEach, describe, test } from "node:test";
import { chromium, firefox, webkit } from "playwright";
import { launch } from "./support/launch.mjs";

const BASE = process.env.UI_PROOF_URL || "http://127.0.0.1:8090";
const ENGINES = { chromium, firefox, webkit };
const NAMES = (process.env.UI_BROWSERS || "chromium,firefox").split(",").filter((n) => ENGINES[n]);

const require = createRequire(import.meta.url);
const axeSource = readFileSync(require.resolve("axe-core/axe.min.js"), "utf8");

/** What a browser logs for an answer that is not a success: the page's own business, not a script error. */
const NOISE = /Failed to load resource|ERR_|NS_ERROR|Load failed|status of \d{3}/i;

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const round = (n) => Math.round(n * 100) / 100;

/** What the proof app counted: the two routes a library must not reach, and the fields of the last delivery post. */
const hits = async () => (await fetch(BASE + "/extensions/hits")).json();
/** Puts the counts and the drop points back as the server began. */
const reset = () => fetch(BASE + "/extensions/reset", { method: "POST" });

/** Waits, polling from here, until `check` gives something that is not false. */
async function eventually(check, what, ms = 8000) {
  const end = Date.now() + ms;
  for (;;) {
    const got = await check();
    if (got) return got;
    if (Date.now() > end) assert.fail(`timed out waiting for ${what}`);
    await sleep(50);
  }
}

/** Waits until `check`, run in the page with `arg`, holds. */
async function until(page, check, arg, what, timeout = 8000) {
  try {
    await page.waitForFunction(check, arg, { timeout });
  } catch {
    assert.fail(`waited ${timeout} ms for ${what}`);
  }
}

async function axe(page) {
  await page.evaluate(axeSource);
  return page.evaluate(async () => {
    const out = await window.axe.run(document, { runOnly: ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa", "best-practice"] });
    return out.violations.map((v) => `${v.id}: ${v.nodes.slice(0, 3).map((n) => n.target.join(" ")).join(" | ")}`);
  });
}

/** Whether the pad drawn by the element with this id has mounted. */
const mounted = (page, id) => page.locator(`#${id}[data-ui-mounted]`).waitFor();
/** What the pad on the element with this id has done: its mounts, updates and leaves. */
const seen = (page, id) =>
  page.evaluate((id) => {
    const mine = window.__pad?.by?.[id];
    return mine ? { mounts: mine.mounts, updates: mine.updates, leaves: mine.leaves } : null;
  }, id);
/** The lines the pad on the element with this id drew into its body. */
const lines = (page, id) => page.locator(`#${id} [data-slot="app:pad-line"]`).allTextContents();
/** The toasts that say `text`. */
const toasts = (page, text) => page.locator("[data-slot=toast]").filter({ hasText: text });
/** How many times the page asked for the edit of the first delivery's drop point. */
const drops = (requests) => requests.filter((r) => r === "POST /extensions/deliveries/1/drop").length;

/**
 * Runs in the page before its scripts: times each callback of the observers that watch an owned body (the
 * seal's), and counts what they were given, in `window.__seal.observer`.
 */
function timeTheSeal() {
  const Native = window.MutationObserver;
  const cost = { observers: 0, callbacks: 0, records: 0, ms: 0, longest: 0 };
  (window.__seal ||= {}).observer = cost;
  window.MutationObserver = class extends Native {
    constructor(callback) {
      super((records, observer) => {
        if (!observer.sealing) return callback.call(observer, records, observer);
        const start = performance.now();
        try {
          return callback.call(observer, records, observer);
        } finally {
          const took = performance.now() - start;
          cost.callbacks++;
          cost.records += records.length;
          cost.ms += took;
          cost.longest = Math.max(cost.longest, took);
        }
      });
    }
    observe(target, options) {
      if (target instanceof Element && target.hasAttribute("data-ui-keep") && target.hasAttribute("data-ui-stored")) {
        this.sealing = true;
        cost.observers++;
      }
      return super.observe(target, options);
    }
  };
}

for (const name of NAMES) {
  describe(name, { concurrency: false }, () => {
    let browser;
    before(async () => {
      browser = await launch(ENGINES[name]);
    });
    after(() => browser.close());
    beforeEach(async () => {
      await reset();
    });
    const contexts = [];
    afterEach(async () => {
      for (const context of contexts.splice(0)) await context.close().catch(() => {});
    });

    /**
     * A page on the proof app. `signedIn` signs the context in as Harpreet Sandhu, which the plugin's routes
     * ask for. Console errors, page errors and content security policy reports are kept, and `close()` asserts
     * there are none besides the ones the test says to expect: `errors` is a list of patterns for console
     * lines that are expected (they are returned in `expected`), and `csp` a pattern every report must match,
     * of which there must be at least one. `prepare` runs on the page before it loads.
     */
    async function open(path, options = {}) {
      const { signedIn = false, errors: allowedErrors = [], csp: allowedCsp, prepare, ...rest } = options;
      const context = await browser.newContext({ viewport: { width: 1280, height: 900 }, ...rest });
      contexts.push(context);
      if (signedIn) await context.addCookies([{ name: "proof_user", value: "harpreet", url: BASE }]);
      const page = await context.newPage();
      const problems = [];
      const expected = [];
      const requests = [];
      page.on("console", (m) => {
        if (m.type() !== "error") return;
        const text = m.text();
        if (NOISE.test(text) || (name === "webkit" && /Refused to apply a stylesheet/.test(text))) return;
        (allowedErrors.some((pattern) => pattern.test(text)) ? expected : problems).push(text);
      });
      page.on("pageerror", (e) => problems.push("pageerror: " + e.message));
      page.on("request", (r) => requests.push(r.method() + " " + r.url().replace(BASE, "")));
      await page.addInitScript(() => {
        document.addEventListener("securitypolicyviolation", (e) => (window.__csp ||= []).push(`${e.violatedDirective} ${e.blockedURI}`));
      });
      if (prepare) await prepare(page);
      await page.goto(BASE + path);
      const close = async () => {
        let csp = await page.evaluate(() => window.__csp || []);
        if (name === "webkit") csp = csp.filter((line) => !/^style-src-(elem|attr) inline$/.test(line));
        await context.close();
        if (allowedCsp) {
          assert.ok(csp.length > 0, "a content security policy report was expected");
          assert.ok(csp.every((line) => allowedCsp.test(line)), `content security policy reports: ${csp.join(", ")}`);
        } else {
          assert.deepEqual(csp, [], "no content security policy report");
        }
        assert.deepEqual(problems, []);
      };
      return { page, requests, expected, close };
    }

    test("an extension mounts, keeps its body through a boosted visit and a region swap, and leaves", async () => {
      const { page, close } = await open("/extensions");
      await mounted(page, "pad");
      await mounted(page, "vendor-pad");
      assert.deepEqual(await seen(page, "pad"), { mounts: 1, updates: 0, leaves: 0 });
      assert.deepEqual(await lines(page, "pad"), ["Ludhiana, step 1"]);
      assert.equal(await page.getAttribute("#pad", "data-count"), "1", "the script's mark on its element");

      // A link in the region: the server draws the pad for the next step, the pad's body is as it was left and
      // the pad is given its new props in place, once.
      await page.click("#step-2");
      await until(page, () => window.__pad.by.pad.updates === 1, null, "the pad's update");
      assert.deepEqual(await seen(page, "pad"), { mounts: 1, updates: 1, leaves: 0 });
      assert.deepEqual(await lines(page, "pad"), ["Ludhiana, step 1", "Ludhiana, step 2"], "the owned body was kept by the swap");
      assert.equal(await page.getAttribute("#pad", "data-count"), "1", "the attribute the extension keeps was kept, though the server's pad has none");
      assert.match(page.url(), /\/extensions\?step=2$/);
      assert.deepEqual(await seen(page, "vendor-pad"), { mounts: 1, updates: 0, leaves: 0 }, "a pad outside the region was left alone");

      // A boosted link to a page with a pad of its own: the first pad leaves and the new one mounts.
      await page.click("#to-other");
      await mounted(page, "other-pad");
      await until(page, () => window.__pad.by.pad.leaves === 1, null, "the first pad to leave");
      assert.deepEqual(await seen(page, "pad"), { mounts: 1, updates: 1, leaves: 1 });
      assert.deepEqual(await seen(page, "other-pad"), { mounts: 1, updates: 0, leaves: 0 });
      assert.equal(await page.locator("#pad").count(), 0);
      assert.equal(await page.locator("#stored").count(), 0);
      // The stylesheet of a component that left is taken out of the head; the pad's, which the new page names, stays.
      const sheets = await page.locator("head link[data-ui-ext-link]").evaluateAll((links) => links.map((link) => link.getAttribute("href")));
      assert.ok(sheets.some((href) => /\/app\.pad-[0-9a-f]{12}\.css$/.test(href)), `the pad's sheet is linked: ${sheets}`);
      assert.ok(!sheets.some((href) => /\/app\.styled-[0-9a-f]{12}\.css$/.test(href)), `the sheet no element names is gone: ${sheets}`);
      await close();
    });

    test("new props from Reply::morph reach update", async () => {
      const { page, close } = await open("/extensions");
      await mounted(page, "pad");
      await page.click("#call-save");
      await until(page, () => window.__pad.by.pad.updates === 1, null, "the pad's update");
      const mine = await page.evaluate(() => window.__pad.by.pad);
      assert.equal(mine.props.step, 2, "update was given the step the server drew");
      assert.equal(mine.mounts, 1, "the pad was not mounted again");
      assert.equal(JSON.parse(await page.getAttribute("#pad", "data-ui-props")).step, 2);
      await toasts(page, "Saved for Harpreet Sandhu").first().waitFor();
      await close();
    });

    test("ui.call applies the reply's ops and a failure is toasted", async () => {
      const { page, requests, close } = await open("/extensions");
      await mounted(page, "pad");
      const calls = () => page.evaluate(() => window.__pad.calls);

      // A route the server named: its ops are applied, the pad is drawn for the next step and the toast shows.
      await page.click("#call-save");
      await until(page, () => window.__pad.calls.length === 1, null, "the save's answer");
      assert.deepEqual((await calls())[0], { url: "/extensions/pad/save", ok: true, status: 200 });
      assert.equal(JSON.parse(await page.getAttribute("#pad", "data-ui-props")).step, 2, "the reply's morph reached the pad");
      await toasts(page, "Saved for Harpreet Sandhu").first().waitFor();

      // The same route failing: the runtime says so.
      await page.click("#call-fail");
      await until(page, () => window.__pad.calls.length === 2, null, "the failed call's answer");
      assert.deepEqual((await calls())[1], { url: "/extensions/pad/fail", ok: false, status: 500 });
      await toasts(page, "That didn't work. Try again.").first().waitFor();

      // An address the props hold and the server did not name: nothing is sent.
      const sent = requests.length;
      await page.click("#call-planted");
      await until(page, () => window.__pad.calls.length === 3, null, "the refused call's answer");
      assert.deepEqual((await calls())[2], { url: "/account/email", ok: false, status: 0 });
      assert.deepEqual(
        requests.slice(sent).filter((r) => r.includes("/account/email")),
        [],
        "no request left for an address the server did not name",
      );
      assert.equal((await hits()).email, 0);
      await close();
    });

    test("an extension inside stored rich text is never mounted", async () => {
      const { page, close } = await open("/extensions");
      await mounted(page, "pad");
      await mounted(page, "vendor-pad");
      const twin = page.locator('#stored [data-slot="app:pad"]');
      assert.equal(await twin.count(), 1, "the stored copy is on the page");
      assert.equal(await twin.getAttribute("data-ui-mounted"), null, "the stored copy was not mounted");
      const body = twin.locator('[data-slot="app:pad-body"]');
      assert.equal(await body.getAttribute("data-ui-stored"), null, "the module sealed nothing in the stored copy");
      assert.equal(await body.locator("*").count(), 0, "the script drew nothing in the stored copy");
      assert.equal(await page.evaluate(() => window.__pad.by["stored-pad"] ?? null), null, "the script never ran for it");
      await close();
    });

    test("a script that imports another origin fails and keeps its fallback", async () => {
      // What leaves the page for the other site would pass through this route. A browser can announce a
      // request its own policy then stops (Chromium does), so the page's request events do not say it.
      const sent = [];
      const { page, expected, close } = await open("/extensions/outside", {
        errors: [/could not be loaded/, /Refused to load|Content[- ]Security[- ]Policy|script-src/i],
        csp: /script-src.*cdn\.example/,
        prepare: (page) =>
          page.route("https://cdn.example/**", (route) => {
            sent.push(route.request().url());
            return route.abort();
          }),
      });
      await page.locator("#outside[data-ui-failed]").waitFor();
      assert.equal((await page.textContent("#outside")).trim(), "Map unavailable", "the server's fallback stays");
      assert.equal(await page.getAttribute("#outside", "data-ui-mounted"), null);
      assert.deepEqual(sent, [], "nothing was sent to the other site");
      assert.ok(
        expected.some((line) => /navree-ui: \/_navree\/ui\/ext\/app\.outside-[0-9a-f]{12}\.js could not be loaded/.test(line)),
        `the runtime says which script could not be loaded: ${expected.join(" | ")}`,
      );
      await close();
    });

    test("a form a vendor wrote into a mounted extension does not post with the token", async () => {
      const { page, requests, close } = await open("/extensions");
      await mounted(page, "vendor-pad");
      // The library's own writes: its markup at once, and a field of its own and a change to another later on.
      await page.locator("#vendor-late").waitFor({ state: "attached" });
      await page.evaluate(() => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))));

      const owners = await page.evaluate(() =>
        [...document.querySelectorAll('#vendor-pad [data-slot="app:pad-body"] :is(input, button)')].map((field) => `${field.id || field.name}=${field.getAttribute("form")}`),
      );
      assert.equal(owners.length, 5, `the fields the library wrote: ${owners}`);
      assert.ok(owners.every((owner) => owner.endsWith("=ui-sealed")), `every field the library wrote belongs to no form: ${owners}`);
      assert.deepEqual(
        await page.evaluate(() => [...new FormData(document.getElementById("new-delivery"))].map(([field, value]) => [field, String(value)])),
        [
          ["recipient", "Harpreet Sandhu"],
          ["charge", "40"],
        ],
        "the page's form holds its own fields only",
      );
      assert.equal(await page.locator('#vendor-pad [data-ui-click], #vendor-pad [data-ui-own], #vendor-pad [data-ui-form]').count(), 0, "no hook is left in what the library wrote");

      // The button the library wrote posts nothing.
      await page.click("#vendor-send");
      await sleep(400);
      assert.deepEqual(await hits(), { email: 0, deliveries: 0, fields: [] });

      // The page's own button posts the page's own fields, once.
      await page.click("#add-delivery");
      await eventually(async () => (await hits()).deliveries === 1, "the delivery post");
      await sleep(300);
      assert.deepEqual(await hits(), {
        email: 0,
        deliveries: 1,
        fields: [
          ["recipient", "Harpreet Sandhu"],
          ["charge", "40"],
        ],
      });
      assert.deepEqual(
        requests.filter((r) => r.includes("/account/email")),
        [],
      );
      await close();
    });

    test("a plugin rule does not reach a form outside its components", async () => {
      const { page, close } = await open("/extensions");
      await mounted(page, "pad");
      const style = (selector) =>
        page.evaluate((selector) => {
          const found = getComputedStyle(document.querySelector(selector));
          return { display: found.display, opacity: found.opacity };
        }, selector);
      // The sheet is there and its rules apply inside the component that carries it.
      assert.equal((await style("#styled-sign-out")).display, "none", "the rule applies inside the component");
      assert.equal((await style("#styled-dialog")).opacity, "0", "the rule applies inside the component");
      // And nowhere else on the page.
      const outer = await style("#sign-out");
      assert.notEqual(outer.display, "none", "the page's sign-out form");
      assert.equal(await page.locator("#sign-out").isVisible(), true);
      const dialog = await style("#dialog");
      assert.equal(dialog.opacity, "1", "the page's dialog");
      assert.equal(await page.locator("#dialog").isVisible(), true);
      const everywhere = await page.evaluate(() =>
        [...document.querySelectorAll('form[action$="/sign-out"]')].filter((form) => !form.closest("#styled")).map((form) => getComputedStyle(form).display),
      );
      assert.ok(everywhere.length >= 2, "the page's own form and the shell's");
      assert.ok(everywhere.every((display) => display !== "none"), `every sign-out form outside the component: ${everywhere}`);
      await close();
    });

    test("entering_a_fresh_page_from_a_page_with_an_extension_loads_a_new_document", async () => {
      const { page, close } = await open("/extensions");
      await mounted(page, "pad");
      await page.evaluate(() => {
        window.__marker = "the document that was here before";
      });
      // A boosted link, which would draw the page in place, to a page that must start clean.
      await page.click("#to-fresh");
      await page.waitForURL(/\/extensions\/fresh$/);
      await page.waitForLoadState("load");
      assert.equal(await page.evaluate(() => window.__marker), undefined, "the page was loaded as a new document, not drawn into the old one");
      assert.equal(await page.getAttribute("html", "data-ui-fresh-page"), "", "the page says it starts clean");
      assert.equal(await page.locator("input[type=password]").count(), 1);
      assert.equal(await page.evaluate(() => window.__pad), undefined, "no extension ran on the page that starts clean");
      await close();
    });

    test("a lazy tab whose source is a fresh page draws no field into a page with an extension", async () => {
      const { page, close } = await open("/extensions/security");
      await mounted(page, "security-pad");
      // The region draws nothing when it is refused, so it is in the page and has no size.
      await page.locator('#security[data-ui-failed="403"]').waitFor({ state: "attached" });
      assert.equal(await page.locator("input[type=password]").count(), 0, "no password field was drawn into the page");
      assert.ok(!(await page.locator("body").innerText()).includes("Confirm it is you"), "nothing of the fresh page was drawn");
      await close();
    });

    test("the inline editor's styles are there on first load and its own keys do not post the cell", async () => {
      const { page, requests, close } = await open("/extensions/deliveries", { signedIn: true });

      // The server's own answer names the map's stylesheet in its head, before any script runs, though the
      // controls that need it are inside the rows of a table.
      const html = await (await fetch(BASE + "/extensions/deliveries", { headers: { cookie: "proof_user=harpreet" } })).text();
      assert.match(html.slice(0, html.indexOf("</head>")), /<link rel="stylesheet" href="\/_navree\/ui\/ext\/geo\.map-[0-9a-f]{12}\.css" data-ui-ext-link>/);
      assert.equal(await page.locator('head link[data-ui-ext-link][href^="/_navree/ui/ext/geo.map-"]').count(), 1, "the sheet is linked once, though three maps use it");

      const maps = page.locator('[data-slot="geo:map-body"].leaflet-container');
      await maps.first().waitFor();
      assert.equal(await page.evaluate(() => getComputedStyle(document.querySelector('[data-slot="geo:map-body"]')).isolation), "isolate", "the sheet applies");
      const input = page.locator("input[name=value]").first();
      assert.equal(await input.inputValue(), "30.8890,75.8340");

      // Enter in the field is the control's own key: it posts nothing.
      await input.focus();
      await page.keyboard.press("Enter");
      await sleep(500);
      assert.equal(drops(requests), 0, "Enter in the drop point field sent the cell");

      // A pick inside Ludhiana, on the map of the first row: the cell posts once, and the server keeps it.
      const box = await maps.first().boundingBox();
      await page.mouse.click(box.x + box.width / 2 + 60, box.y + box.height / 2);
      await eventually(() => drops(requests) >= 1, "the cell's post");
      await sleep(500);
      assert.equal(drops(requests), 1, "a pick posts the cell once");
      const picked = await input.inputValue();
      assert.match(picked, /^30\.\d{4},75\.\d{4}$/);
      assert.notEqual(picked, "30.8890,75.8340", "the pick moved the point");
      const again = await (await fetch(BASE + "/extensions/deliveries", { headers: { cookie: "proof_user=harpreet" } })).text();
      assert.ok(again.includes(`value="${picked}"`), `the server kept ${picked}`);
      await close();
    });

    test("with no source allowed the page has no policy violation", async () => {
      // The map's tiles are this app's own route, so nothing is allowed for any other site.
      const extensions = await open("/extensions");
      await mounted(extensions.page, "pad");
      await mounted(extensions.page, "vendor-pad");
      await extensions.page.locator("#vendor-late").waitFor({ state: "attached" });
      await extensions.close();

      const map = await open("/extensions/map", { signedIn: true });
      await map.page.locator('[data-slot="geo:map-body"].leaflet-container').waitFor();
      await until(map.page, () => [...document.querySelectorAll("img.leaflet-tile")].some((tile) => tile.complete && tile.naturalWidth > 0), null, "a tile of the map");
      await mounted(map.page, "map-pad");
      await map.close();
    });

    test("the seal keeps up with a panned map and two thousand cells", async () => {
      const { page, close } = await open("/extensions/map", { signedIn: true, prepare: (page) => page.addInitScript(timeTheSeal) });
      const map = page.locator('[data-slot="geo:map-body"].leaflet-container');
      await map.waitFor();
      await mounted(page, "map-pad");
      await until(page, () => [...document.querySelectorAll("img.leaflet-tile")].some((tile) => tile.complete && tile.naturalWidth > 0), null, "a tile of the map");
      assert.equal(await page.evaluate(() => window.__seal.observer.observers), 2, "the seal watches the map's body and the pad's");

      // Two seconds of dragging the map about, with the button down, from a place beside the pin.
      const box = await map.boundingBox();
      const x = box.x + box.width / 2 + 100;
      const y = box.y + box.height / 2 + 40;
      await page.mouse.move(x, y);
      await page.mouse.down();
      const began = Date.now();
      for (let step = 0; Date.now() - began < 2000; step++) {
        await page.mouse.move(x + Math.sin(step / 12) * 140, y + Math.cos(step / 17) * 90);
      }
      await page.mouse.up();
      const panMs = Date.now() - began;
      const panned = await page.evaluate(() => ({ ...window.__seal.observer }));
      assert.ok(panned.records > 0, "the seal saw the map move");
      assert.ok(panned.ms < 1000, `the seal's observer spent ${round(panned.ms)} ms of two seconds of panning`);
      await sleep(600);
      const quiet = await page.evaluate(() => ({ ...window.__seal.observer }));
      assert.deepEqual(
        await page.evaluate(() => {
          const body = document.querySelector('[data-slot="geo:map-body"]');
          const hooked = [body, ...body.querySelectorAll("*")].filter((found) => found.getAttributeNames().some((attribute) => attribute.startsWith("data-ui-") && !(found === body && (attribute === "data-ui-keep" || attribute === "data-ui-stored"))));
          return { marked: body.hasAttribute("data-ui-stored"), hooked: hooked.length };
        }),
        { marked: true, hooked: 0 },
        "after the pan the map's body is still sealed and holds no hook",
      );

      // Two thousand cells that each carry a hook, written in one go into the pad's body.
      await page.click("#add-cells");
      await until(page, () => window.__seal.cells?.done === true, null, "the cells to be written");
      const after = await page.evaluate(() => ({ observer: { ...window.__seal.observer }, cells: { ...window.__seal.cells } }));
      assert.equal(await page.locator('#map-pad [data-slot="app:cell"]').count(), 2000);
      assert.ok(after.cells.turns < 100, "the seal reached the last cell");
      assert.equal(after.cells.leftAtFrame, 0, "every cell was scrubbed before the next animation frame ended");
      assert.equal(await page.locator('#map-pad [data-slot="app:cell"][data-ui-click]').count(), 0);

      const figures = {
        pan: {
          ms: panMs,
          callbacks: panned.callbacks,
          records: panned.records,
          observerMs: round(panned.ms),
          longestMs: round(panned.longest),
        },
        cells: {
          count: after.cells.count,
          callbacks: after.observer.callbacks - quiet.callbacks,
          records: after.observer.records - quiet.records,
          observerMs: round(after.observer.ms - quiet.ms),
          writeMs: round(after.cells.writeMs),
          scrubMs: round(after.cells.scrubMs),
          turns: after.cells.turns,
          leftAtFrame: after.cells.leftAtFrame,
        },
      };
      console.log(`seal: ${name} ${JSON.stringify(figures)}`);
      await close();
    });

    test("axe finds nothing on the extensions proof page", async () => {
      const { page, close } = await open("/extensions");
      await mounted(page, "pad");
      await mounted(page, "vendor-pad");
      await page.locator("#vendor-late").waitFor({ state: "attached" });
      assert.deepEqual(await axe(page), []);
      await close();
    });
  });
}
