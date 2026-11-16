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

/** `top`, which may be thin (`[r, g, b, alpha]`) or missing, laid over the opaque `base`. */
const onto = (top, base) => {
  if (!top) return base;
  if (top.length === 3) return top;
  const a = top[3];
  return [0, 1, 2].map((i) => Math.round(top[i] * a + base[i] * (1 - a)));
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
            if (a === 0) return null;
            return a === 255 ? [r, g, b] : [r, g, b, a / 255];
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

    // ---- A card, a stat and a code block on the inverse surface ----------

    describe("the inverse surface", () => {
      test("a card, a stat and a code block on the other palette, everything in them reading, light and dark, 1440 and 390 px", async () => {
        const PROPS = ["background-color", "color", "border-top-color"];
        for (const path of PAGES) {
          for (const mode of MODES) {
            for (const width of WIDTHS) {
              const page = await open(`${path}?mode=${mode}`, { width });
              const where = `${path} ${mode} ${width}`;
              const pageBg = (await styles(page, "main", ["background-color"]))["background-color"] || (await styles(page, "body", ["background-color"]))["background-color"];
              const card = await styles(page, "[data-proof=inverse-card] [data-slot=card]", PROPS);
              const title = await styles(page, "[data-proof=inverse-card] [data-slot=card-title]", PROPS);
              const description = await styles(page, "[data-proof=inverse-card] [data-slot=card-description]", PROPS);
              const text = await styles(page, "[data-proof=inverse-card] [data-slot=card-content] [data-slot=text]", PROPS);
              const badge = await styles(page, "[data-proof=inverse-card] [data-slot=badge]", PROPS);
              const primary = await styles(page, "[data-proof=inverse-card] [data-slot=button][data-variant=primary]", PROPS);
              const outline = await styles(page, "[data-proof=inverse-card] [data-slot=button][data-variant=outline]", PROPS);
              const plain = await styles(page, "[data-proof=plain-card] [data-slot=card]", PROPS);
              const stat = await styles(page, "[data-proof=inverse-stat] [data-slot=stat]", PROPS);
              const label = await styles(page, "[data-proof=inverse-stat] [data-slot=stat-label]", PROPS);
              const figure = await styles(page, "[data-proof=inverse-stat] [data-slot=stat-value]", PROPS);
              const caption = await styles(page, "[data-proof=inverse-stat] [data-slot=stat-caption]", PROPS);
              const code = await styles(page, "[data-proof=inverse-code] [data-slot=code-block]", PROPS);
              const pre = await styles(page, "[data-proof=inverse-code] pre", PROPS);
              const copy = await styles(page, "[data-proof=inverse-code] [data-slot=button]", PROPS);
              // The surface is the other palette's, far from the page's own; a card drawn as before keeps the page's.
              assert.ok(contrastOf(card["background-color"], pageBg) >= 3, `${where}: the inverse card stands apart from the page`);
              assert.deepEqual(stat["background-color"], card["background-color"], `${where}: the stat on the same surface`);
              assert.deepEqual(code["background-color"], card["background-color"], `${where}: the code panel on the same surface`);
              assert.ok(contrastOf(plain["background-color"], pageBg) < 3, `${where}: a card drawn as before is on the page's palette`);
              for (const [what, fg, bg] of [
                ["the title", title.color, card["background-color"]],
                ["the description", description.color, card["background-color"]],
                ["the text", text.color, card["background-color"]],
                ["a badge", badge.color, onto(badge["background-color"], card["background-color"])],
                ["the primary button", primary.color, primary["background-color"]],
                ["the outline button", outline.color, card["background-color"]],
                ["the stat's label", label.color, stat["background-color"]],
                ["the stat's figure", figure.color, stat["background-color"]],
                ["the stat's caption", caption.color, stat["background-color"]],
                ["the code", pre.color, code["background-color"]],
              ]) {
                assert.ok(contrastOf(fg, bg) >= 4.5, `${where}: ${what} reads: ${contrastOf(fg, bg).toFixed(2)}`);
              }
              assert.ok(contrastOf(copy.color, code["background-color"]) >= 3, `${where}: the copy button's icon`);
              // Every colour of code reads on its panel.
              const kinds = await page.evaluate(() => {
                const canvas = document.createElement("canvas");
                canvas.width = canvas.height = 1;
                const ctx = canvas.getContext("2d", { willReadFrequently: true });
                const rgb = (value) => {
                  ctx.clearRect(0, 0, 1, 1);
                  ctx.fillStyle = value;
                  ctx.fillRect(0, 0, 1, 1);
                  return [...ctx.getImageData(0, 0, 1, 1).data.slice(0, 3)];
                };
                const panel = rgb(getComputedStyle(document.querySelector("[data-proof=inverse-code] [data-slot=code-block]")).backgroundColor);
                return [...document.querySelectorAll("[data-proof=inverse-code] [data-t]")].map((t) => [t.getAttribute("data-t"), rgb(getComputedStyle(t).color), panel]);
              });
              assert.ok(new Set(kinds.map(([kind]) => kind)).size >= 4, `${where}: the code is coloured: ${kinds.map(([k]) => k)}`);
              for (const [kind, fg, bg] of kinds) assert.ok(contrastOf(fg, bg) >= 4.5, `${where}: code of kind ${kind}: ${contrastOf(fg, bg).toFixed(2)}`);
              if (path === "/looks") {
                // The theme's own inverse colours: near-black with white text on a light page, the reverse on a dark one.
                const [surface, ink] = mode === "light" ? [[10, 10, 10], [255, 255, 255]] : [[255, 255, 255], [10, 10, 10]];
                assert.deepEqual(card["background-color"], surface, `${where}: the surface`);
                assert.deepEqual(title.color, ink, `${where}: its text`);
                assert.deepEqual(primary["background-color"], ink, `${where}: the primary button in the text's colour`);
                assert.deepEqual(primary.color, surface, `${where}: and its text in the surface's`);
              }
              assert.ok(await noSidewaysScroll(page), `${where}: no sideways scroll`);
              assert.deepEqual(await axe(page), [], `${where}: axe`);
              await shot(page, `inverse-${path.replaceAll("/", "_")}-${mode}-${width}`);
              await page.done();
            }
          }
        }
      });

      test("the copy button on the inverse code block is named and reached by keyboard", async () => {
        const page = await open("/looks?mode=light");
        const copy = page.locator("[data-proof=inverse-code]").getByRole("button", { name: "Copy code" });
        assert.equal(await copy.count(), 1);
        await copy.focus();
        assert.equal(await page.evaluate(() => document.activeElement.getAttribute("aria-label")), "Copy code");
        assert.equal(await page.locator("[data-proof=inverse-card]").getByRole("heading", { name: "Security, in every mode" }).count(), 1);
        await page.done();
      });
    });

    // ---- A destructive outline button, and buttons on the inverse surface

    describe("button variants", () => {
      test("a destructive outline: the danger colour, a faint line of it and no fill, a thin tint on hover, light and dark, 1440 and 390 px, outside and inside the inverse surface", async () => {
        const PROPS = ["background-color", "color", "border-top-color", "border-top-left-radius", "height"];
        for (const path of PAGES) {
          for (const mode of MODES) {
            for (const width of WIDTHS) {
              const page = await open(`${path}?mode=${mode}`, { width });
              const where = `${path} ${mode} ${width}`;
              const pageBg = (await styles(page, "body", ["background-color"]))["background-color"];
              const outline = await styles(page, "[data-proof=destructive-outline] [data-slot=button]", PROPS);
              const link = await styles(page, "[data-proof=destructive-outline-link] [data-slot=button]", PROPS);
              const solidTint = await styles(page, "[data-proof=destructive] [data-slot=button]", PROPS);
              const plainOutline = await styles(page, "[data-proof=outline] [data-slot=button]", PROPS);
              const inside = await styles(page, "[data-proof=inverse-destructive] [data-slot=button]", PROPS);
              const card = await styles(page, "[data-proof=inverse-card] [data-slot=card]", ["background-color"]);
              // No fill, a thin line of the danger colour, and the danger colour as its text.
              assert.equal(outline["background-color"], null, `${where}: no fill`);
              assert.equal(outline["border-top-color"].length, 4, `${where}: the line is the danger colour laid thin`);
              assert.ok(Math.abs(outline["border-top-color"][3] - 0.3) < 0.02, `${where}: at 30%: ${outline["border-top-color"]}`);
              assert.deepEqual(outline.color, (await styles(page, "[data-proof=destructive] [data-slot=button]", ["color"])).color, `${where}: the destructive button's text colour`);
              assert.ok(contrastOf(outline.color, pageBg) >= 4.5, `${where}: its text on the page`);
              assert.ok(contrastOf(inside.color, card["background-color"]) >= 4.5, `${where}: its text on the inverse surface`);
              assert.deepEqual([link["background-color"], link.color], [null, outline.color], `${where}: the same look as a small link`);
              // The theme's button corner and control heights.
              assert.equal(outline["border-top-left-radius"], plainOutline["border-top-left-radius"], `${where}: the button's corner`);
              assert.equal(outline.height, plainOutline.height, `${where}: a control's height`);
              // On hover: the destructive button's own tint at rest, and the text still reads.
              await page.locator("[data-proof=destructive-outline] [data-slot=button]").hover();
              await page.waitForFunction(() => getComputedStyle(document.querySelector("[data-proof=destructive-outline] [data-slot=button]")).backgroundColor !== "rgba(0, 0, 0, 0)");
              const hovered = await styles(page, "[data-proof=destructive-outline] [data-slot=button]", PROPS);
              assert.ok(Math.abs(hovered["background-color"][3] - 0.1) < 0.02, `${where}: a 10% tint on hover: ${hovered["background-color"]}`);
              assert.deepEqual(hovered["background-color"], solidTint["background-color"], `${where}: the destructive button's tint`);
              assert.ok(contrastOf(hovered.color, onto(hovered["background-color"], pageBg)) >= 4.5, `${where}: its text on its hover`);
              // The variants drawn as before keep their look.
              assert.equal(plainOutline["background-color"], null, `${where}: an outline button`);
              assert.equal(solidTint["background-color"].length, 4, `${where}: the destructive button's tint`);
              await page.mouse.move(0, 0);
              // On the inverse surface, the primary button's hover still reads.
              await page.locator("[data-proof=inverse-card] [data-slot=button][data-variant=primary]").hover();
              const primary = await styles(page, "[data-proof=inverse-card] [data-slot=button][data-variant=primary]", PROPS);
              assert.ok(contrastOf(primary.color, onto(primary["background-color"], card["background-color"])) >= 4.5, `${where}: the inverse primary button on hover`);
              assert.deepEqual(await axe(page), [], `${where}: axe`);
              await shot(page, `buttons-${path.replaceAll("/", "_")}-${mode}-${width}`);
              await page.done();
            }
          }
        }
      });

      test("a destructive outline is a button, or a link when it has somewhere to go, named by its words", async () => {
        const page = await open("/looks?mode=light");
        assert.equal(await page.locator("#buttons").getByRole("button", { name: "Delete deal" }).count(), 1);
        assert.equal(await page.locator("#buttons").getByRole("link", { name: "Refund" }).count(), 1);
        await page.done();
      });
    });
  });
}
