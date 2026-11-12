// The sortable grid and server-drawn chart pictures in real browsers, against the proof app's
// /widgets, a home-screen widget builder: tiles moved by the keys (with every announcement), by
// the mouse and by touch, taken away, and saved as one whole order a moment after the last change
// (quick changes sent once, a failed save leaving the page as it is and the next change saving
// the whole order); the picture drawn again after each save, in dark and light, answered 304 for
// the tag a browser holds; less motion; JavaScript off, where each tile's own buttons post; and
// axe clean at 390 and 1440 px in dark and light.
// Run with the proof app serving:
//
//   PORT=8150 cargo run --release -p ui-proof -- serve
//   WIDGETS_URL=http://127.0.0.1:8150 UI_BROWSERS=chromium,firefox,webkit npm test -- tests/widgets.test.mjs
//
// WIDGETS_URL is the server (default http://127.0.0.1:8150). UI_BROWSERS picks the engines (default
// chromium). UI_SHOTS=<dir> saves screenshots of the builder and of the pictures. Without the server
// the suite is skipped. The suite resets the server's widget before each test, so run one engine at
// a time.

import assert from "node:assert/strict";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { after, afterEach, before, beforeEach, describe, test } from "node:test";
import { chromium, firefox, webkit } from "playwright";
import { launch } from "./support/launch.mjs";

const BASE = process.env.WIDGETS_URL || "http://127.0.0.1:8150";
const ENGINES = { chromium, firefox, webkit };
const NAMES = (process.env.UI_BROWSERS || "chromium").split(",").filter((n) => ENGINES[n]);
const SHOTS = process.env.UI_SHOTS;
if (SHOTS) mkdirSync(SHOTS, { recursive: true });

const require = createRequire(import.meta.url);
const axeSource = readFileSync(require.resolve("axe-core/axe.min.js"), "utf8");

const reachable = await fetch(`${BASE}/widgets/state`).then((r) => r.ok, () => false);
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

/** What the server holds. */
const state = () => fetch(`${BASE}/widgets/state`).then((r) => r.json());
const reset = () => fetch(`${BASE}/widgets/reset`, { method: "POST" });

/** Waits until `check` (run in the page) is true. */
async function until(page, check, arg, what, timeout = 5000) {
  try {
    await page.waitForFunction(check, arg, { timeout });
  } catch {
    assert.fail(`waited ${timeout} ms for ${what}`);
  }
}

/** Waits until the server holds `n` saves, then a little more for a save that shouldn't come. */
async function saves(n, quiet = 0) {
  const start = Date.now();
  let now = await state();
  while (now.saves < n && Date.now() - start < 5000) {
    await sleep(50);
    now = await state();
  }
  if (quiet) {
    await sleep(quiet);
    now = await state();
  }
  return now;
}

async function settle(page) {
  await page.evaluate(() => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))));
  await page.evaluate(() => Promise.all(document.getAnimations().map((a) => a.finished.catch(() => {}))));
}

async function axe(page) {
  await settle(page);
  await page.evaluate(axeSource);
  return page.evaluate(async () => {
    const out = await window.axe.run(document, { runOnly: ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa", "best-practice"] });
    return out.violations.map((v) => `${v.id}: ${v.nodes.slice(0, 3).map((n) => n.target.join(" ")).join(" | ")}`);
  });
}

/** The keys of the tiles in the order the page shows them. */
const keys = (page) => page.$$eval("#tiles [data-slot=sortable-grid-item]", (items) => items.map((i) => i.dataset.key));
/** What the grid's live region says. */
const said = async (page) => (await page.locator("#tiles [data-slot=sortable-grid-status]").textContent()).trim();
const handle = (page, key) => page.locator(`#tiles [data-key="${key}"] [data-slot=sortable-grid-handle]`);

for (const name of NAMES) {
  describe(name, { concurrency: false, skip: reachable ? false : `needs the proof app at ${BASE}` }, () => {
    let browser;
    before(async () => {
      browser = await launch(ENGINES[name]);
    });
    after(() => browser?.close());
    beforeEach(reset);
    const contexts = [];
    afterEach(async () => {
      for (const context of contexts.splice(0)) await context.close().catch(() => {});
    });

    /** The builder; console errors, page errors and policy reports are kept and `done()` asserts there are none. */
    async function open({ width = 1440, height = 1000, light = false, js = true, reducedMotion = "no-preference", hasTouch = false } = {}) {
      const context = await browser.newContext({ viewport: { width, height }, javaScriptEnabled: js, colorScheme: light ? "light" : "dark", reducedMotion, hasTouch });
      contexts.push(context);
      const page = await context.newPage();
      page.problems = [];
      page.on("console", (m) => {
        if (name === "webkit" && /Refused to apply a stylesheet/.test(m.text())) return;
        if (m.type() === "error" && !/Failed to load resource/.test(m.text())) page.problems.push(m.text());
      });
      page.on("pageerror", (e) => page.problems.push("pageerror: " + e.message));
      if (js) {
        await page.addInitScript(() => {
          document.addEventListener("securitypolicyviolation", (e) => {
            (window.__csp ||= []).push(e.violatedDirective + " " + e.blockedURI);
          });
        });
      }
      await page.goto(`${BASE}/widgets?mode=${light ? "light" : "dark"}`);
      page.done = async () => {
        let csp = js ? await page.evaluate(() => window.__csp || []) : [];
        if (name === "webkit") csp = csp.filter((line) => !/^style-src-(elem|attr) inline$/.test(line));
        assert.deepEqual(csp, [], "no content security policy report");
        assert.deepEqual(page.problems, [], "no console error");
      };
      return page;
    }

    test("the builder at 1440 and 390 px in dark and light: axe clean, no sideways scroll, the grid's own module and sheet", { timeout: 120000 }, async () => {
      for (const width of [1440, 390]) {
        for (const theme of ["dark", "light"]) {
          const page = await open({ width, light: theme === "light" });
          const label = `${theme} ${width}`;
          const scripts = await page.$$eval("script[src]", (s) => s.map((e) => e.getAttribute("src")));
          assert.ok(scripts.some((s) => /\/sortable-grid-[0-9a-f]+\.js$/.test(s)), `${label}: the grid's module loads: ${scripts}`);
          assert.ok(!scripts.some((s) => /\/sortable-[0-9a-f]+\.js$/.test(s)), `${label}: not the lists' module`);
          const sheets = await page.$$eval("link[rel=stylesheet]", (s) => s.map((e) => e.getAttribute("href")));
          assert.ok(sheets.some((s) => /sortable-grid-styles-/.test(s)), `${label}: ${sheets}`);
          const wide = await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth);
          assert.ok(wide <= 0, `${label}: the page scrolls sideways by ${wide} px`);
          // The buttons a page with no script moves tiles with are hidden; the handles show.
          assert.equal(await page.locator("#tiles [data-slot=sortable-grid-steps]").first().isVisible(), false);
          assert.equal(await handle(page, "recovery").isVisible(), true);
          assert.deepEqual(await axe(page), [], `${label}: axe`);
          if (SHOTS) await page.screenshot({ path: `${SHOTS}/builder-${theme}-${width}.png`, fullPage: true });
          await page.done();
        }
      }
    });

    test("keys: pick up, move, drop and cancel, every step said, focus kept, the order saved once", async () => {
      const page = await open();
      assert.deepEqual(await keys(page), ["health", "recovery", "sleep"]);
      await handle(page, "recovery").focus();
      await page.keyboard.press("Space");
      assert.equal(await said(page), "Picked up Recovery. Position 2 of 3.");
      assert.equal(await page.locator('#tiles [data-key="recovery"]').getAttribute("data-lifted"), "");
      if (SHOTS) await page.locator("#tiles").screenshot({ path: `${SHOTS}/grid-lifted-dark.png` });
      await page.keyboard.press("ArrowRight");
      assert.equal(await said(page), "Position 3 of 3.");
      assert.deepEqual(await keys(page), ["health", "sleep", "recovery"]);
      // Focus stays on the handle of the tile that moved.
      assert.equal(await page.evaluate(() => document.activeElement?.closest("[data-key]")?.dataset.key), "recovery");
      await page.keyboard.press("ArrowRight");
      assert.equal(await said(page), "Position 3 of 3.", "the end is said again");
      await page.keyboard.press("Enter");
      assert.equal(await said(page), "Dropped Recovery at position 3 of 3.");
      const saved = await saves(1, 700);
      assert.deepEqual(saved.tiles, ["health", "sleep", "recovery"]);
      assert.equal(saved.saves, 1, "one save");

      // Up and down move by a row: two columns.
      await handle(page, "recovery").focus();
      await page.keyboard.press("Space");
      await page.keyboard.press("ArrowUp");
      assert.deepEqual(await keys(page), ["recovery", "health", "sleep"]);
      assert.equal(await said(page), "Position 1 of 3.");
      // Escape puts it back, and nothing is sent.
      await page.keyboard.press("Escape");
      assert.equal(await said(page), "Move cancelled. Recovery is back at position 3 of 3.");
      assert.deepEqual(await keys(page), ["health", "sleep", "recovery"]);
      // Tab away while held puts it back too.
      await page.keyboard.press("Space");
      await page.keyboard.press("Home");
      assert.deepEqual(await keys(page), ["recovery", "health", "sleep"]);
      await page.keyboard.press("Tab");
      await until(page, () => [...document.querySelectorAll("#tiles [data-key]")].map((i) => i.dataset.key).join() === "health,sleep,recovery", null, "the tile put back");
      assert.equal((await saves(1, 700)).saves, 1, "a cancelled move sends nothing");
      await page.done();
    });

    test("quick changes are one save of the last order", async () => {
      const page = await open();
      await handle(page, "sleep").focus();
      for (const key of ["Space", "ArrowLeft", "Space", "Space", "ArrowLeft", "Space"]) await page.keyboard.press(key);
      assert.deepEqual(await keys(page), ["sleep", "health", "recovery"]);
      const saved = await saves(1, 800);
      assert.equal(saved.saves, 1);
      assert.deepEqual(saved.tiles, ["sleep", "health", "recovery"]);
      await page.done();
    });

    test("mouse: a tile dragged by its body lands where it is dropped, and the picture is drawn again", async () => {
      const page = await open();
      const before = await page.locator('#widget-preview img[data-mode="dark"]').getAttribute("src");
      const from = await page.locator('#tiles [data-key="sleep"] [data-slot=sortable-grid-body]').boundingBox();
      const onto = await page.locator('#tiles [data-key="health"]').boundingBox();
      await page.mouse.move(from.x + 20, from.y + 10);
      await page.mouse.down();
      await page.mouse.move(from.x + 30, from.y + 14, { steps: 3 });
      await until(page, () => !!document.querySelector("[data-ghost]"), null, "a drag");
      if (SHOTS) await page.screenshot({ path: `${SHOTS}/grid-dragging-dark.png` });
      await page.mouse.move(onto.x + 20, onto.y + 20, { steps: 12 });
      await page.mouse.up();
      assert.equal(await page.evaluate(() => !!document.querySelector("[data-ghost]")), false, "the copy is gone");
      assert.deepEqual(await keys(page), ["sleep", "health", "recovery"]);
      assert.match(await said(page), /^Dropped Sleep at position 1 of 3\.$/);
      const saved = await saves(1);
      assert.deepEqual(saved.tiles, ["sleep", "health", "recovery"]);
      // The answer's ops draw the picture again: a new address, from a new tag.
      await until(page, (src) => document.querySelector('#widget-preview img[data-mode="dark"]').getAttribute("src") !== src, before, "a new picture");
      await page.done();
    });

    if (name === "chromium") {
      test("touch: the handle drags, a held press drags, and a swipe scrolls", async () => {
        const page = await open({ hasTouch: true, height: 700 });
        const cdp = await page.context().newCDPSession(page);
        const touch = (type, x, y) => cdp.send("Input.dispatchTouchEvent", { type, touchPoints: type === "touchEnd" ? [] : [{ x, y }] });
        const glide = async (x0, y0, x1, y1, steps = 12) => {
          for (let i = 1; i <= steps; i++) await touch("touchMove", x0 + ((x1 - x0) * i) / steps, y0 + ((y1 - y0) * i) / steps);
        };
        const grip = await handle(page, "sleep").boundingBox();
        const onto = await page.locator('#tiles [data-key="health"]').boundingBox();
        await touch("touchStart", grip.x + 8, grip.y + 8);
        await glide(grip.x + 8, grip.y + 8, onto.x + 30, onto.y + 30);
        await until(page, () => !!document.querySelector("[data-ghost]"), null, "a touch drag");
        await touch("touchEnd");
        assert.deepEqual(await keys(page), ["sleep", "health", "recovery"]);
        // A press held still on the tile's body, then moved.
        const body = await page.locator('#tiles [data-key="recovery"] [data-slot=sortable-grid-body]').boundingBox();
        const first = await page.locator('#tiles [data-key="sleep"]').boundingBox();
        await touch("touchStart", body.x + 20, body.y + 20);
        await sleep(420);
        await glide(body.x + 20, body.y + 20, first.x + 20, first.y + 20, 8);
        await touch("touchEnd");
        assert.deepEqual(await keys(page), ["recovery", "sleep", "health"]);
        // A quick swipe on a tile is a scroll, not a drag.
        await touch("touchStart", body.x + 20, body.y + 40);
        await glide(body.x + 20, body.y + 40, body.x + 20, body.y - 120, 4);
        assert.equal(await page.evaluate(() => !!document.querySelector("[data-ghost]")), false);
        await touch("touchEnd");
        const saved = await saves(2);
        assert.deepEqual(saved.tiles, ["recovery", "sleep", "health"]);
        await page.done();
      });
    }

    test("remove: the tile goes at once, focus moves on, the server takes it away and the add buttons open", async () => {
      const page = await open();
      // A tile the widget always holds has no remove button.
      assert.equal(await page.locator('#tiles [data-key="health"] [data-slot=sortable-grid-remove]').count(), 0);
      // Fill the widget: Medium fits 4.
      await page.getByRole("button", { name: "+ Movement" }).click();
      await until(page, () => document.querySelectorAll("#tiles [data-key]").length === 4, null, "a fourth tile");
      assert.equal(await page.getByRole("button", { name: "+ HRV" }).isDisabled(), true, "full: nothing more to add");
      await page.getByRole("button", { name: "Remove Sleep" }).click();
      assert.deepEqual(await keys(page), ["health", "recovery", "movement"]);
      assert.equal(await said(page), "Removed Sleep. 3 left.");
      assert.equal(await page.evaluate(() => document.activeElement?.closest("[data-key]")?.dataset.key), "movement", "focus moves to the next tile");
      const saved = await saves(1);
      assert.deepEqual(saved.tiles, ["health", "recovery", "movement"]);
      await until(page, () => !document.querySelector("#widget-adds button[value=hrv]")?.disabled, null, "room again");
      // The server keeps the fixed tile whatever is posted.
      const forged = await fetch(`${BASE}/widgets/tiles`, { method: "POST", redirect: "manual", headers: { "content-type": "application/x-www-form-urlencoded" }, body: "order=health&order=recovery&order=movement&remove=health" });
      assert.equal(forged.status, 303);
      assert.deepEqual((await state()).tiles, ["health", "recovery", "movement"]);
      await page.done();
    });

    test("a failed save leaves the page as it is, says so, and the next change saves the whole order", async () => {
      const page = await open();
      await fetch(`${BASE}/widgets/fail`, { method: "POST" });
      await handle(page, "sleep").focus();
      for (const key of ["Space", "ArrowLeft", "Space"]) await page.keyboard.press(key);
      await until(page, () => /wasn't saved/.test(document.body.textContent), null, "a toast");
      assert.deepEqual(await keys(page), ["health", "sleep", "recovery"]);
      assert.equal((await state()).saves, 0);
      for (const key of ["Space", "ArrowLeft", "Space"]) await page.keyboard.press(key);
      const saved = await saves(1);
      assert.deepEqual(saved.tiles, ["sleep", "health", "recovery"]);
      await page.done();
    });

    test("less motion: tiles jump to their places", async () => {
      // With motion, a move slides the tiles that make room.
      const moving = await open();
      await handle(moving, "sleep").focus();
      await moving.keyboard.press("Space");
      await moving.keyboard.press("ArrowLeft");
      assert.ok(await moving.evaluate(() => document.getAnimations().filter((a) => !(a instanceof CSSTransition) && a.effect?.target?.closest?.("#tiles")).length) > 0, "tiles slide");
      await moving.keyboard.press("Escape");
      const page = await open({ reducedMotion: "reduce" });
      await handle(page, "sleep").focus();
      await page.keyboard.press("Space");
      await page.keyboard.press("ArrowLeft");
      assert.equal(await page.evaluate(() => document.getAnimations().filter((a) => !(a instanceof CSSTransition) && a.effect?.target?.closest?.("#tiles")).length), 0);
      await page.keyboard.press("Space");
      await page.done();
    });

    test("JavaScript off: each tile's buttons move it and take it away, and the picture follows", async () => {
      const page = await open({ js: false });
      assert.equal(await handle(page, "recovery").isVisible(), false, "no handle without a script");
      // A press posts the form; the server answers with the page again, read once it has settled.
      const press = (label) => Promise.all([page.waitForResponse((r) => r.request().method() === "GET" && new URL(r.url()).pathname === "/widgets"), page.getByRole("button", { name: label }).click()]);
      const order = async (expected) => {
        let now = [];
        for (let i = 0; i < 50; i++) {
          now = await keys(page).catch(() => []);
          if (JSON.stringify(now) === JSON.stringify(expected)) return;
          await sleep(100);
        }
        assert.deepEqual(now, expected);
      };
      await press("Move Recovery later");
      await order(["health", "sleep", "recovery"]);
      assert.equal(await page.getByRole("button", { name: "Move Recovery later" }).isDisabled(), true, "the last tile moves no later");
      await press("Move Sleep earlier");
      await order(["sleep", "health", "recovery"]);
      await press("Remove Recovery");
      await order(["sleep", "health"]);
      assert.deepEqual((await state()).tiles, ["sleep", "health"]);
      if (SHOTS) await page.screenshot({ path: `${SHOTS}/builder-no-script-dark.png`, fullPage: true });
      await page.done();
    });

    test("the picture: a PNG at the widget's size, tagged by what draws it, 304 for the tag held, dark and light apart", async () => {
      const get = (query, headers = {}) => fetch(`${BASE}/widgets/image.png?${query}`, { headers });
      const dark = await get("size=medium&mode=dark");
      assert.equal(dark.status, 200);
      assert.equal(dark.headers.get("content-type"), "image/png");
      assert.equal(dark.headers.get("cache-control"), "private, no-cache");
      const tag = dark.headers.get("etag");
      assert.match(tag, /^"[0-9a-f]{32}"$/);
      const bytes = Buffer.from(await dark.arrayBuffer());
      assert.equal(bytes.readUInt32BE(16), 1014);
      assert.equal(bytes.readUInt32BE(20), 474);
      // The same picture asked again: the same bytes and tag, and 304 when the tag is held.
      const again = await get("size=medium&mode=dark");
      assert.equal(again.headers.get("etag"), tag);
      assert.ok(Buffer.from(await again.arrayBuffer()).equals(bytes));
      const held = await get("size=medium&mode=dark", { "if-none-match": tag });
      assert.equal(held.status, 304);
      const light = await get("size=medium&mode=light");
      assert.notEqual(light.headers.get("etag"), tag);
      for (const [size, w, h] of [["small", 474, 474], ["large", 1014, 1014]]) {
        const other = Buffer.from(await (await get(`size=${size}&mode=dark`)).arrayBuffer());
        assert.equal(other.readUInt32BE(16), w);
        assert.equal(other.readUInt32BE(20), h);
      }
      // A change of the widget is a new tag.
      await fetch(`${BASE}/widgets/accent`, { method: "POST", redirect: "manual", headers: { "content-type": "application/x-www-form-urlencoded" }, body: "accent=purple" });
      const purple = await get("size=medium&mode=dark", { "if-none-match": tag });
      assert.equal(purple.status, 200);
      if (SHOTS) {
        for (const size of ["small", "medium", "large"]) {
          for (const mode of ["dark", "light"]) {
            writeFileSync(`${SHOTS}/picture-${size}-${mode}.png`, Buffer.from(await (await get(`size=${size}&mode=${mode}`)).arrayBuffer()));
          }
        }
      }
      // The page shows them, loaded, at the widget's display size.
      const page = await open();
      await until(page, () => [...document.querySelectorAll("#widget-preview img")].every((i) => i.complete && i.naturalWidth === 1014), null, "the pictures");
      await page.done();
    });
  });
}
