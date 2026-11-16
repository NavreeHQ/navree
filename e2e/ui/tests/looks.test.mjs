// The looks a component takes only when a page asks for one, in real browsers, against the
// proof app's `/looks` (a theme with a shape and colour roles) and `/looks/plain` (the
// built-in theme): what each looks like as the browser computes it (colours, corners, sizes,
// shadows and the contrast of its text) in light and dark, at 1440 and 390 px, outside and
// inside the inverse surface; how it is named and reached by keyboard; that it works with
// JavaScript off; and axe. Run with the proof app serving:
//
//   cargo run --release -p ui-proof -- serve          # UI_PROOF_URL, default http://127.0.0.1:8090
//   UI_BROWSERS=chromium,firefox,webkit node --test --test-concurrency=1 tests/looks.test.mjs
//
// UI_SHOTS=<dir> also saves a screenshot of every state.

import assert from "node:assert/strict";
import { mkdirSync, readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { join } from "node:path";
import { after, afterEach, before, describe, test } from "node:test";
import { chromium, firefox, webkit } from "playwright";
import { launch } from "./support/launch.mjs";

const BASE = process.env.UI_PROOF_URL || "http://127.0.0.1:8090";
const require = createRequire(import.meta.url);
const axeSource = readFileSync(require.resolve("axe-core/axe.min.js"), "utf8");
const ENGINES = { chromium, firefox, webkit };
const NAMES = (process.env.UI_BROWSERS || "chromium,firefox,webkit").split(",").filter((n) => ENGINES[n]);
const SHOTS = process.env.UI_SHOTS;
if (SHOTS) mkdirSync(SHOTS, { recursive: true });

/** The two pages, each in both modes and at both widths. */
const PAGES = ["/looks", "/looks/plain"];
const MODES = ["light", "dark"];
const WIDTHS = [1440, 390];

async function axe(page) {
  await page.evaluate(axeSource);
  return page.evaluate(async () => {
    const out = await window.axe.run(document, { runOnly: ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa", "best-practice"] });
    return out.violations.map((v) => `${v.id}: ${v.nodes.slice(0, 3).map((n) => n.target.join(" ")).join(" | ")}`);
  });
}

/** WCAG's ratio of two colours given as `[r, g, b]`. */
const contrastOf = (a, b) => {
  const lum = ([r, g, bl]) => {
    const c = (v) => ((v /= 255) <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4);
    return 0.2126 * c(r) + 0.7152 * c(g) + 0.0722 * c(bl);
  };
  const [x, y] = [lum(a), lum(b)].sort((p, q) => q - p);
  return (x + 0.05) / (y + 0.05);
};

for (const name of NAMES) {
  describe(name, { concurrency: false }, () => {
    let browser;
    before(async () => {
      browser = await launch(ENGINES[name]);
      // The first page of a browser that has just started can take long on a busy machine.
      const warm = await browser.newContext();
      await (await warm.newPage()).goto(BASE + "/looks", { timeout: 120000 });
      await warm.close();
    });
    after(() => browser.close());
    const contexts = [];
    afterEach(async () => {
      for (const context of contexts.splice(0)) await context.close().catch(() => {});
    });

    /**
     * A page on the proof app. Console errors, page errors and content security policy
     * reports are kept in `page.problems`; `page.done()` asserts there are none.
     */
    async function open(path, options = {}) {
      const { width = 1440, ...rest } = options;
      const context = await browser.newContext({ viewport: { width, height: 900 }, reducedMotion: "reduce", ...rest });
      contexts.push(context);
      const page = await context.newPage();
      page.problems = [];
      page.on("console", (m) => {
        if (name === "webkit" && /Refused to apply a stylesheet/.test(m.text())) return;
        if (m.type() === "error" && !/Failed to load resource|404/.test(m.text())) page.problems.push(m.text());
      });
      page.on("pageerror", (e) => page.problems.push("pageerror: " + e.message));
      page.scripted = rest.javaScriptEnabled !== false;
      if (page.scripted) {
        await page.addInitScript(() => {
          document.addEventListener("securitypolicyviolation", (e) => {
            (window.__csp ||= []).push(e.violatedDirective + " " + e.blockedURI);
          });
        });
      }
      page.done = async () => {
        if (page.scripted) assert.deepEqual(await page.evaluate(() => window.__csp || []), [], "no content security policy report");
        assert.deepEqual(page.problems, [], "no console or page error");
      };
      await page.goto(BASE + path, { waitUntil: "load" });
      return page;
    }

    const shot = async (page, file) => {
      if (!SHOTS) return;
      const before = page.scripted ? await page.evaluate(() => (window.__csp || []).length) : null;
      await page.screenshot({ path: join(SHOTS, `${name}-looks-${file}.png`), fullPage: true });
      // A screenshot in Playwright's WebKit adds a <style> of the tool's own, which the policy
      // reports: reports raised by the screenshot itself are dropped, and only those.
      if (before !== null) {
        await page.evaluate(() => new Promise((done) => requestAnimationFrame(() => setTimeout(done, 0))));
        await page.evaluate((n) => { if (window.__csp) window.__csp.length = n; }, before);
      }
    };

    /**
     * Tab, as a keyboard user presses it. WebKit's Tab leaves links out unless the reader turns that
     * on, so there the page's own order is read and focus is moved along it after a key press.
     */
    const tab = async (page) => {
      if (name !== "webkit") return page.keyboard.press("Tab");
      await page.keyboard.press("Shift");
      await page.evaluate(() => {
        const list = [...document.querySelectorAll("a[href],button:not([disabled]),input:not([disabled]):not([type=hidden]),select,textarea,summary,[tabindex]:not([tabindex='-1'])")].filter((el) => el.getClientRects().length && getComputedStyle(el).visibility !== "hidden");
        const at = list.indexOf(document.activeElement);
        list[at === -1 ? 0 : at + 1]?.focus();
      });
    };

    const noSidewaysScroll = (page) => page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth + 1);

    /**
     * The computed style of the first element `selector` finds, with each colour the browser
     * resolved read back as `[r, g, b]` through a canvas, so a mix or a wide-gamut form reads too.
     */
    const styles = (page, selector, props) =>
      page.evaluate(
        ([selector, props]) => {
          const found = document.querySelector(selector);
          if (!found) throw new Error(`no ${selector}`);
          const style = getComputedStyle(found);
          const canvas = document.createElement("canvas");
          canvas.width = canvas.height = 1;
          const ctx = canvas.getContext("2d", { willReadFrequently: true });
          const rgb = (value) => {
            ctx.clearRect(0, 0, 1, 1);
            ctx.fillStyle = "#000";
            ctx.fillStyle = value;
            ctx.fillRect(0, 0, 1, 1);
            const [r, g, b, a] = ctx.getImageData(0, 0, 1, 1).data;
            return a === 0 ? null : [r, g, b];
          };
          const out = {};
          for (const prop of props) {
            const value = style.getPropertyValue(prop);
            out[prop] = /color$/.test(prop) || prop === "background-color" ? rgb(value) : value;
          }
          return out;
        },
        [selector, props],
      );

    // ---- Segmented links -------------------------------------------

    describe("segmented links", () => {
      test("a named group of real links, the current one marked, each a tab stop that Enter follows", async () => {
        const page = await open("/looks?mode=light");
        const group = page.getByRole("group", { name: "Mode", exact: true });
        assert.equal(await group.count(), 1);
        const links = group.getByRole("link");
        assert.deepEqual(await links.allTextContents(), ["Mine", "Team"]);
        assert.deepEqual(
          await links.evaluateAll((all) => all.map((a) => [a.tagName, a.getAttribute("aria-current"), new URL(a.href).search])),
          [
            ["A", "page", "?part=mine&mode=light"],
            ["A", null, "?part=team&mode=light"],
          ],
        );
        assert.match(await group.ariaSnapshot(), /group "Mode":[\s\S]*- link "Mine"[\s\S]*- link "Team"/);
        // Each link is a tab stop, in order; Enter follows the focused one.
        await links.first().focus();
        await tab(page);
        assert.equal(await page.evaluate(() => document.activeElement.textContent), "Team");
        await Promise.all([page.waitForURL("**/looks?part=team&mode=light"), page.keyboard.press("Enter")]);
        assert.deepEqual(
          await page.getByRole("group", { name: "Mode", exact: true }).getByRole("link").evaluateAll((all) => all.map((a) => a.getAttribute("aria-current"))),
          [null, "page"],
          "the page it leads to marks its own link",
        );
        await page.done();
      });

      test("with JavaScript off the links work the same and nothing is missing", async () => {
        const page = await open("/looks?mode=dark", { javaScriptEnabled: false });
        const group = page.getByRole("group", { name: "Mode", exact: true });
        await Promise.all([page.waitForURL("**/looks?part=team&mode=dark"), group.getByRole("link", { name: "Team" }).click()]);
        assert.equal(await page.getByRole("group", { name: "Mode", exact: true }).getByRole("link", { name: "Team" }).getAttribute("aria-current"), "page");
        await page.done();
      });

      test("the track, its items and the current one: the theme's colours, corners, heights and shadow, light and dark, 1440 and 390 px, outside and inside the inverse surface", async () => {
        const PROPS = ["background-color", "color", "padding-top", "column-gap", "border-top-left-radius", "height", "box-shadow", "border-top-color"];
        for (const path of PAGES) {
          for (const mode of MODES) {
            for (const width of WIDTHS) {
              const page = await open(`${path}?mode=${mode}`, { width });
              const where = `${path} ${mode} ${width}`;
              const read = async (root) => ({
                track: await styles(page, `${root} [data-variant=segmented]`, PROPS),
                current: await styles(page, `${root} [data-variant=segmented] > [aria-current]`, PROPS),
                other: await styles(page, `${root} [data-variant=segmented] > :not([aria-current])`, PROPS),
              });
              const out = await read("[data-proof=segmented]");
              const inside = await read("[data-proof=inverse-segmented]");
              const small = await styles(page, "[data-proof=segmented-small] [data-variant=segmented] > [data-slot=button]", PROPS);
              for (const [seen, at] of [
                [out, "on the page"],
                [inside, "on the inverse surface"],
              ]) {
                // A 3 px track with 2 px between its items.
                assert.equal(seen.track["padding-top"], "3px", `${where} ${at}: the track's padding`);
                assert.equal(seen.track["column-gap"], "2px", `${where} ${at}: the gap`);
                // The other item's text on the track, and the current one's on its own fill, read.
                assert.ok(contrastOf(seen.other.color, seen.track["background-color"]) >= 4.5, `${where} ${at}: the other item's text`);
                assert.ok(contrastOf(seen.current.color, seen.current["background-color"]) >= 4.5, `${where} ${at}: the current item's text`);
                assert.equal(seen.other["background-color"], null, `${where} ${at}: the other item shows the track`);
              }
              if (path === "/looks") {
                // The theme's pill toggle corner, controls of 40 and 32 px less the track's 6, a soft shadow.
                assert.equal(out.track["border-top-left-radius"], "999px", `${where}: the track's corner`);
                assert.equal(out.current["border-top-left-radius"], "999px", `${where}: an item's corner`);
                assert.equal(out.current.height, "34px", `${where}: an item's height`);
                assert.equal(small.height, "26px", `${where}: a small item's height`);
                assert.notEqual(out.current["box-shadow"], "none", `${where}: the current item is raised`);
                assert.equal(out.other["box-shadow"], "none", `${where}: the other item is flat`);
                // The segment colours of the theme, and the inverse surface's own.
                if (mode === "light") {
                  assert.deepEqual(out.track["background-color"], [239, 239, 236], `${where}: the track`);
                  assert.deepEqual(out.current["background-color"], [255, 255, 255], `${where}: the current item`);
                  assert.deepEqual(inside.track["background-color"], [22, 22, 21], `${where}: the track on the inverse surface`);
                } else {
                  assert.deepEqual(out.track["background-color"], [21, 21, 21], `${where}: the track`);
                  assert.deepEqual(out.current["background-color"], [29, 29, 29], `${where}: the current item`);
                  assert.deepEqual(inside.track["background-color"], [242, 242, 240], `${where}: the track on the inverse surface`);
                }
              } else {
                // The built-in theme: its own corners and heights, no shadow, and its tokens.
                assert.equal(out.track["border-top-left-radius"], "6px", `${where}: the track's corner`);
                assert.equal(out.current["border-top-left-radius"], "4px", `${where}: an item's corner`);
                assert.equal(out.current.height, "30px", `${where}: an item's height`);
                assert.equal(out.current["box-shadow"], "none", `${where}: a flat theme draws no shadow`);
                assert.notDeepEqual(out.current["background-color"], out.track["background-color"], `${where}: the current item stands out`);
                assert.notDeepEqual(inside.track["background-color"], out.track["background-color"], `${where}: the inverse surface's own track`);
              }
              // A group of buttons drawn as before keeps its look.
              const plain = await styles(page, "[data-proof=plain-group] [data-slot=button-group]", ["padding-top", "background-color"]);
              assert.deepEqual(plain, { "padding-top": "0px", "background-color": null }, `${where}: a plain group`);
              assert.ok(await noSidewaysScroll(page), `${where}: no sideways scroll`);
              assert.deepEqual(await axe(page), [], `${where}: axe`);
              await shot(page, `segmented-${path.replaceAll("/", "_")}-${mode}-${width}`);
              await page.done();
            }
          }
        }
      });
    });
  });
}
