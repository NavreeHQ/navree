// The page of a sign-in form in real browsers, against the proof app's /auth-layouts: the panel
// first or after the form, flush or inset from the window's edges in the theme's panel corner and
// on the theme's inverse surface; the app's brand at the top of the form's column and a line at its
// foot, at 1440 px and at 390 px where the panel is hidden; a form with no card around it and its
// links drawn as ghost buttons. Every case in light and dark, axe clean, no sideways scroll, no
// policy report. Run with the proof app serving:
//
//   cargo run --release -p ui-proof -- serve          # UI_PROOF_URL, default http://127.0.0.1:8090
//   UI_BROWSERS=chromium,firefox,webkit npm test -- tests/auth-layout.test.mjs
//
// UI_BROWSERS picks the engines (default chromium). UI_SHOTS=<dir> saves a screenshot of every case
// at both widths in both modes. Without the server the suite is skipped.

import assert from "node:assert/strict";
import { mkdirSync, readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { after, afterEach, before, describe, test } from "node:test";
import { chromium, firefox, webkit } from "playwright";
import { launch } from "./support/launch.mjs";

const BASE = process.env.UI_PROOF_URL || "http://127.0.0.1:8090";
const ENGINES = { chromium, firefox, webkit };
const NAMES = (process.env.UI_BROWSERS || "chromium").split(",").filter((n) => ENGINES[n]);
const SHOTS = process.env.UI_SHOTS;
if (SHOTS) mkdirSync(SHOTS, { recursive: true });

const require = createRequire(import.meta.url);
const axeSource = readFileSync(require.resolve("axe-core/axe.min.js"), "utf8");

const reachable = await fetch(`${BASE}/auth-layouts?case=plain`).then((r) => r.ok, () => false);

const CASES = ["plain", "flush-end", "start", "end"];

async function axe(page) {
  await page.evaluate(axeSource);
  return page.evaluate(async () => {
    const out = await window.axe.run(document, { runOnly: ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa", "best-practice"] });
    return out.violations.map((v) => `${v.id}: ${v.nodes.slice(0, 3).map((n) => n.target.join(" ")).join(" | ")}`);
  });
}

/** The box of the first element `selector` finds, rounded, and whether it shows. */
const box = (page, selector) =>
  page.evaluate((selector) => {
    const found = document.querySelector(selector);
    if (!found) return null;
    const r = found.getBoundingClientRect();
    const style = getComputedStyle(found);
    return {
      x: Math.round(r.left),
      y: Math.round(r.top),
      right: Math.round(r.right),
      bottom: Math.round(r.bottom),
      width: Math.round(r.width),
      height: Math.round(r.height),
      shown: style.display !== "none" && r.width > 0 && r.height > 0,
    };
  }, selector);

/** Computed styles of the first element `selector` finds. */
const styles = (page, selector, names) =>
  page.evaluate(
    ({ selector, names }) => {
      const style = getComputedStyle(document.querySelector(selector));
      return Object.fromEntries(names.map((name) => [name, style.getPropertyValue(name)]));
    },
    { selector, names },
  );

/** The names of the toolkit's stylesheets the page links, in order. */
const sheets = (page) =>
  page.evaluate(() =>
    [...document.head.querySelectorAll("link[rel=stylesheet]")]
      .map((l) => new URL(l.href).pathname.match(/^\/_navree\/ui\/([a-z-]+)-[0-9a-f]{12}\.css$/)?.[1])
      .filter(Boolean),
  );

for (const name of NAMES) {
  describe(name, { concurrency: false, skip: reachable ? false : `needs the proof app at ${BASE}` }, () => {
    let browser;
    before(async () => {
      browser = await launch(ENGINES[name]);
    });
    after(() => browser?.close());
    const contexts = [];
    afterEach(async () => {
      for (const context of contexts.splice(0)) await context.close().catch(() => {});
    });

    /** A case on the proof app in `mode`; console errors, page errors and policy reports are kept and `done()` asserts there are none. */
    async function open(kase, { width = 1440, height = 980, mode = "light" } = {}) {
      const context = await browser.newContext({ viewport: { width, height }, colorScheme: mode });
      contexts.push(context);
      const page = await context.newPage();
      page.problems = [];
      page.on("console", (m) => {
        if (name === "webkit" && /Refused to apply a stylesheet/.test(m.text())) return;
        if (m.type() === "error" && !/Failed to load resource/.test(m.text())) page.problems.push(m.text());
      });
      page.on("pageerror", (e) => page.problems.push("pageerror: " + e.message));
      await page.addInitScript(() => {
        document.addEventListener("securitypolicyviolation", (e) => {
          (window.__csp ||= []).push(e.violatedDirective + " " + e.blockedURI);
        });
      });
      await page.goto(`${BASE}/auth-layouts?case=${kase}&mode=${mode}`, { waitUntil: "load" });
      await page.evaluate(() => document.fonts.ready);
      page.done = async () => {
        let csp = await page.evaluate(() => window.__csp || []);
        if (name === "webkit") csp = csp.filter((line) => !/^style-src-(elem|attr) inline$/.test(line));
        assert.deepEqual(csp, [], "no content security policy report");
        assert.deepEqual(page.problems, [], "no console error");
      };
      return page;
    }

    describe("every case", () => {
      for (const kase of CASES) {
        for (const width of [1440, 390]) {
          test(`${kase} at ${width} px: the form and brand show, no sideways scroll, axe clean in light and dark`, { timeout: 120000 }, async () => {
            for (const mode of ["light", "dark"]) {
              const page = await open(kase, { width, mode });
              const label = `${kase} ${mode} ${width}`;
              // A page in the toolkit's default theme and mode, dark, says neither.
              assert.equal(await page.evaluate(() => document.documentElement.getAttribute("data-mode") ?? "dark"), mode, `${label}: the mode`);
              assert.ok((await box(page, "[data-slot=auth-body]")).shown, `${label}: the form shows`);
              const aside = await box(page, "[data-slot=auth-aside]");
              assert.equal(aside.shown, width === 1440, `${label}: the panel shows on a wide screen only`);
              if (kase === "start" || kase === "end") {
                assert.ok((await box(page, "[data-slot=auth-brand]")).shown, `${label}: the brand shows at every width`);
                assert.ok((await box(page, "[data-slot=auth-footer]")).shown, `${label}: the footer shows at every width`);
              }
              const wide = await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth);
              assert.ok(wide <= 0, `${label}: the page scrolls sideways by ${wide} px`);
              assert.deepEqual(await axe(page), [], `${label}: axe`);
              if (SHOTS) await page.screenshot({ path: `${SHOTS}/${name}-auth-${kase}-${width}-${mode}.png`, fullPage: true });
              await page.done();
              await page.context().close();
            }
          });
        }
      }
    });

    test("a plain layout is as it was: the panel first and flush, the form in a card, the app blocks' sheet alone", async () => {
      const page = await open("plain");
      assert.deepEqual(await sheets(page), ["navree-ui", "app-blocks"]);
      const aside = await box(page, "[data-slot=auth-aside]");
      const main = await box(page, "[data-slot=auth-main]");
      assert.equal(aside.x, 0, "the panel is first, at the window's edge");
      assert.equal(main.x, aside.right, "the form's column follows it");
      assert.equal(aside.height, 980, "the panel is as high as the window");
      const look = await styles(page, "[data-slot=auth-aside]", ["border-top-left-radius", "margin-top", "border-right-width"]);
      assert.deepEqual(look, { "border-top-left-radius": "0px", "margin-top": "0px", "border-right-width": "1px" });
      assert.equal(await page.locator("[data-slot=auth-main] [data-slot=card]").count(), 1, "the form is in its card");
      await page.done();
    });

    test("the panel after the form, flush: the form's column first and the panel's line facing it", async () => {
      const page = await open("flush-end");
      assert.deepEqual(await sheets(page), ["navree-ui", "app-blocks", "auth-layout"]);
      const main = await box(page, "[data-slot=auth-main]");
      const aside = await box(page, "[data-slot=auth-aside]");
      assert.equal(main.x, 0, "the form's column is first");
      assert.equal(aside.x, main.right, "the panel follows it");
      assert.equal(aside.right, 1440, "to the window's edge");
      assert.ok(main.width > aside.width, "the form's column is the wider, as the panel is when first");
      const look = await styles(page, "[data-slot=auth-aside]", ["border-left-width", "border-right-width"]);
      assert.deepEqual(look, { "border-left-width": "1px", "border-right-width": "0px" });
      // After the form in reading order too.
      const order = await page.evaluate(() => [...document.querySelector("[data-slot=auth-layout]").children].map((c) => c.getAttribute("data-slot")));
      assert.deepEqual(order, ["auth-main", "auth-aside"]);
      await page.done();
    });

    for (const [kase, side] of [["start", "first"], ["end", "after the form"]]) {
      test(`the panel ${side}, inset and rounded in the theme's panel corner, on the inverse surface, light and dark`, async () => {
        for (const mode of ["light", "dark"]) {
          const page = await open(kase, { mode });
          const label = `${kase} ${mode}`;
          const aside = await box(page, "[data-slot=auth-aside]");
          const main = await box(page, "[data-slot=auth-main]");
          // 12 px off the window's edges, as high as the window less both.
          assert.equal(aside.y, 12, `${label}: off the top`);
          assert.equal(aside.height, 980 - 24, `${label}: off the bottom`);
          if (kase === "start") {
            assert.equal(aside.x, 12, `${label}: off the start`);
            assert.equal(main.x, aside.right + 12, `${label}: the form's column after it`);
            assert.equal(main.right, 1440, `${label}: to the window's edge`);
          } else {
            assert.equal(main.x, 0, `${label}: the form's column first`);
            assert.equal(aside.x, main.right + 12, `${label}: the panel after it`);
            assert.equal(aside.right, 1440 - 12, `${label}: off the end`);
          }
          // The form's column grows from 35rem and the panel from 30rem by equal parts.
          assert.equal(main.width - (aside.width + 24), 560 - 504, `${label}: the room left is shared equally`);
          const look = await styles(page, "[data-slot=auth-aside]", [
            "border-top-left-radius",
            "border-bottom-right-radius",
            "background-color",
            "padding-top",
            "padding-left",
            "row-gap",
            "border-left-width",
            "border-right-width",
            "color-scheme",
          ]);
          assert.equal(look["border-top-left-radius"], "12px", `${label}: the theme's panel corner`);
          assert.equal(look["border-bottom-right-radius"], "12px", `${label}: every corner`);
          assert.equal(look["padding-top"], "56px", `${label}: padded`);
          assert.equal(look["padding-left"], "56px", `${label}: padded`);
          assert.equal(look["row-gap"], "40px", `${label}: its parts apart`);
          assert.deepEqual([look["border-left-width"], look["border-right-width"]], ["0px", "0px"], `${label}: no line beside it`);
          // The theme's inverse surface: near black on a light page, white on a dark one, its
          // scheme the other mode's.
          assert.equal(look["background-color"], mode === "light" ? "rgb(10, 10, 10)" : "rgb(255, 255, 255)", `${label}: the inverse surface`);
          assert.equal(look["color-scheme"], mode === "light" ? "dark" : "light", `${label}: the flipped scheme`);
          const text = await styles(page, "[data-slot=auth-aside] h2", ["color"]);
          assert.equal(text.color, mode === "light" ? "rgb(255, 255, 255)" : "rgb(10, 10, 10)", `${label}: the inverse text`);
          const page_ = await styles(page, "[data-slot=auth-layout]", ["background-color"]);
          assert.notEqual(page_["background-color"], look["background-color"], `${label}: the panel stands out from the page`);
          await page.done();
        }
      });
    }

    for (const width of [1440, 390]) {
      test(`the brand at the top of the form's column and the footer at its foot at ${width} px, light and dark`, async () => {
        for (const mode of ["light", "dark"]) {
          const page = await open("end", { width, mode });
          const label = `${mode} ${width}`;
          const brand = await box(page, "[data-slot=auth-brand]");
          const mark = await box(page, "[data-slot=auth-brand] img");
          const body = await box(page, "[data-slot=auth-body]");
          const footer = await box(page, "[data-slot=auth-footer]");
          const main = await box(page, "[data-slot=auth-main]");
          // 32 px from the top, 40 px from the start on a wide screen and 16 px on a phone.
          assert.equal(brand.y, 32, `${label}: the brand's top`);
          assert.equal(mark.x, width === 1440 ? 40 : 16, `${label}: the mark's start`);
          assert.deepEqual([mark.width, mark.height], [24, 24], `${label}: the mark's size`);
          assert.ok(body.y >= brand.bottom + 48, `${label}: the form below the brand`);
          assert.ok(footer.y >= body.bottom, `${label}: the footer below the form`);
          assert.equal(main.bottom - footer.bottom, 32, `${label}: the footer at the column's foot`);
          // The form's column 400 px wide, centred in its column, or the room there is.
          if (width === 1440) {
            assert.equal(body.width, 400, `${label}: the form's width`);
            assert.ok(Math.abs(body.x - main.x - (main.width - 400) / 2) <= 1, `${label}: centred`);
          } else {
            assert.equal(body.width, 390 - 32, `${label}: the room there is`);
          }
          // The brand's name in the type scale's brand style, in the display face.
          const name = await styles(page, "[data-slot=auth-brand] [data-slot=site-brand-name]", ["font-size", "font-weight", "font-family"]);
          assert.equal(name["font-size"], "18px", `${label}: the brand's size`);
          assert.equal(name["font-weight"], "600", `${label}: the brand's weight`);
          assert.match(name["font-family"], /Proof Display/, `${label}: the display face`);
          const link = await styles(page, "[data-slot=auth-brand] a", ["text-decoration-line", "column-gap"]);
          assert.deepEqual(link, { "text-decoration-line": "none", "column-gap": "10px" }, `${label}: the brand's link`);
          const small = await styles(page, "[data-slot=auth-footer]", ["font-size"]);
          assert.equal(small["font-size"], "12px", `${label}: the footer's size`);
          await page.done();
        }
      });
    }

    test("a form with no card around it and its links drawn as ghost buttons, light and dark", async () => {
      for (const mode of ["light", "dark"]) {
        const page = await open("end", { mode });
        assert.equal(await page.locator("[data-slot=auth-body] [data-slot=card]").count(), 0, `${mode}: no card`);
        assert.equal(await page.locator("[data-slot=auth-body] form").count(), 1, `${mode}: the form`);
        const links = await page.evaluate(() =>
          [...document.querySelectorAll("[data-slot=auth-body] a")].map((a) => ({
            text: a.textContent,
            href: a.getAttribute("href"),
            slot: a.getAttribute("data-slot"),
            variant: a.getAttribute("data-variant"),
            width: Math.round(a.getBoundingClientRect().width),
            background: getComputedStyle(a).backgroundColor,
          })),
        );
        assert.equal(links.length, 2, `${mode}: two links`);
        for (const link of links) {
          assert.deepEqual([link.slot, link.variant], ["button", "ghost"], `${mode}: ${link.text} is a ghost button`);
          assert.equal(link.href, "/auth-layouts?case=end", `${mode}: ${link.text} keeps its address`);
          assert.equal(link.width, 400, `${mode}: ${link.text} fills the form's width`);
        }
        // A ghost button has no fill until hovered.
        assert.equal(links[0].background, "rgba(0, 0, 0, 0)", `${mode}: no fill`);
        await page.locator("[data-slot=auth-body] a").first().hover();
        // The fill comes in with a short transition.
        const hovered = await page.evaluate(async () => {
          const link = document.querySelector("[data-slot=auth-body] a");
          await Promise.all(link.getAnimations().map((a) => a.finished.catch(() => {})));
          return getComputedStyle(link).backgroundColor;
        });
        assert.notEqual(hovered, "rgba(0, 0, 0, 0)", `${mode}: a fill when hovered`);
        await page.done();
      }
    });
  });
}
