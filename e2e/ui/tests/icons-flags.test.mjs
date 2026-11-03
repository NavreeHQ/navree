// Icons and flags in real browsers, against the proof app's /icons-flags page:
// every icon and every flag drawn, nothing reporting a policy violation, axe
// clean in dark and light, a hidden copy of a flag never changing a visible one,
// names for assistive technology, and no flag bent at 390px or right to left.
//
//   cargo run --release -p ui-proof -- serve &
//   ICON_BROWSERS="chromium firefox webkit" npm test
//
// UI_PROOF_URL points at another server; ICON_SHOTS=dir keeps screenshots.

import assert from "node:assert/strict";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { createRequire } from "node:module";
import { join } from "node:path";
import { after, before, describe, test } from "node:test";
import * as playwright from "playwright";

import { colours, decodePng } from "./support/png.mjs";

const BASE = process.env.UI_PROOF_URL || "http://127.0.0.1:8090";
const SHOTS = process.env.ICON_SHOTS || "";
const require = createRequire(import.meta.url);
const axeSource = await readFile(require.resolve("axe-core/axe.min.js"), "utf8");

// The light values of `src/tokens.rs`: the toolkit ships dark only so far, so light
// is proved by putting these on the page.
const LIGHT = ":root{color-scheme:light;--ink-0:#ffffff;--ink-1:#f7f8f7;--ink-2:#eff1f0;--ink-3:#e5e8e6;--line:#dfe3e0;--line-2:#c8cecb;--line-3:#aeb6b2;--fg:#0a0e0c;--fg-1:#1c2420;--fg-2:#3f4a44;--fg-3:#55615b;--accent:#0b7a3e;--accent-ink:#ffffff;--amber:#8a5a00;--danger:#b3261e;--bar:#8a968f}";

const browsers = (process.env.ICON_BROWSERS || "chromium firefox").split(" ");

/** Runs axe on the page as it stands. Violations come back as short lines. */
async function axe(page) {
  await page.evaluate(axeSource);
  return page.evaluate(async () => {
    const out = await window.axe.run(document, { runOnly: ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa", "best-practice"] });
    return out.violations.map((v) => `${v.id}: ${v.nodes.slice(0, 3).map((n) => n.target.join(" ")).join(" | ")}`);
  });
}

async function shot(page, name) {
  if (!SHOTS) return;
  await mkdir(SHOTS, { recursive: true });
  await page.screenshot({ path: join(SHOTS, `${name}.png`) });
}

for (const browserName of browsers) {
  describe(browserName, () => {
    let browser;
    before(async () => {
      browser = await playwright[browserName].launch();
    });
    after(() => browser?.close());

    /** A page, with policy reports and console errors collected. */
    async function open(path, { light = false, viewport = { width: 1440, height: 900 } } = {}) {
      const context = await browser.newContext({ viewport, bypassCSP: light });
      const page = await context.newPage();
      const problems = [];
      await page.addInitScript(() => {
        window.__csp = [];
        document.addEventListener("securitypolicyviolation", (event) => window.__csp.push(`${event.violatedDirective} ${event.blockedURI}`));
      });
      page.on("console", (message) => {
        // Playwright's WebKit reports this on every page, with none of our markup.
        if (browserName === "webkit" && /Refused to apply a stylesheet/.test(message.text())) return;
        if (message.type() === "error" && !/Failed to load resource/.test(message.text())) problems.push(message.text());
      });
      page.on("pageerror", (error) => problems.push(String(error)));
      await page.goto(BASE + path);
      if (light) {
        await page.addStyleTag({ content: LIGHT });
        // Past the colour transitions, so that axe and the screenshots see the light colours.
        await page.waitForTimeout(500);
      }
      return { page, problems, close: () => context.close() };
    }

    test("every icon and every flag is on the page, and no policy is violated", async () => {
      const { page, problems, close } = await open("/icons-flags");
      assert.ok((await page.locator("[data-icons=all] svg[data-slot=icon]").count()) >= 2000);
      for (const grid of ["rect", "square", "round"]) {
        assert.equal(await page.locator(`[data-flags=${grid}] svg[data-slot=flag]`).count(), 270, grid);
      }
      assert.deepEqual(await page.evaluate(() => window.__csp), []);
      assert.deepEqual(problems, []);
      assert.match(await page.locator("[data-flag-count]").innerText(), /^270 flags, 250 countries$/);
      await close();
    });

    test("no icon and no flag carries a style attribute, a link or a script", async () => {
      const { page, close } = await open("/icons-flags");
      const found = await page.evaluate(() =>
        [...document.querySelectorAll("svg[data-slot=icon] *, svg[data-slot=flag] *")].filter((node) =>
          node.hasAttribute("style") || (node.hasAttribute("href") && !node.getAttribute("href").startsWith("#")) || node.hasAttribute("xlink:href") || /^(script|style|image|foreignobject|a)$/i.test(node.localName) || [...node.attributes].some((a) => /^on/i.test(a.name))
        ).length,
      );
      assert.equal(found, 0);
      await close();
    });

    for (const [theme, light] of [["dark", false], ["light", true]]) {
      test(`axe finds nothing in ${theme}`, async () => {
        const { page, close } = await open("/icons-flags", { light });
        assert.deepEqual(await axe(page), []);
        await shot(page, `${browserName}-${theme}`);
        await close();
      });

      test(`icons follow the text colour in ${theme}; the placeholder follows it too`, async () => {
        const { page, close } = await open("/icons-flags", { light });
        const result = await page.evaluate(() => {
          const icon = document.querySelector("[data-icons=all] [data-icon-name=mail-open] svg");
          const style = getComputedStyle(icon);
          const group = icon.parentElement;
          const unknown = [...document.querySelectorAll("svg[data-state=unknown]")][0];
          const rect = unknown.querySelector("rect");
          return {
            text: getComputedStyle(group).color,
            stroke: style.stroke,
            fill: style.fill,
            placeholder: getComputedStyle(rect).fill,
            placeholderText: getComputedStyle(unknown).color,
          };
        });
        assert.equal(result.stroke, result.text);
        assert.equal(result.fill, "none");
        assert.ok(result.placeholder === result.placeholderText || /^rgb/.test(result.placeholder), JSON.stringify(result));
        // The text colour changes with the theme, so the icon's does.
        assert.equal(result.text, light ? "rgb(28, 36, 32)" : "rgb(207, 216, 211)");
        await close();
      });
    }

    test("every flag draws something: its box holds more than one colour", async () => {
      const { page, close } = await open("/icons-flags");
      const grid = page.locator("[data-flags=rect]");
      await grid.scrollIntoViewIfNeeded();
      const boxes = await page.locator("[data-flags=rect] svg[data-slot=flag]").evaluateAll((nodes) => {
        const origin = nodes[0].closest("[data-flags]").getBoundingClientRect();
        return nodes.map((node) => {
          const box = node.getBoundingClientRect();
          return { code: node.closest("[data-flag-code]").dataset.flagCode, x: box.x - origin.x, y: box.y - origin.y, width: box.width, height: box.height };
        });
      });
      const image = decodePng(await grid.screenshot());
      const scale = image.width / (await grid.evaluate((node) => node.getBoundingClientRect().width));
      assert.equal(boxes.length, 270);
      const blank = [];
      for (const box of boxes) {
        assert.equal(Math.round(box.width), 32, box.code);
        assert.equal(Math.round(box.height), 24, box.code);
        // The inside of the flag, clear of the outline.
        const n = colours(image, (box.x + 3) * scale, (box.y + 3) * scale, (box.width - 6) * scale, (box.height - 6) * scale);
        if (n < 2) blank.push(box.code);
      }
      assert.deepEqual(blank, []);
      await close();
    });

    test("round and square flags are as tall as they are wide", async () => {
      const { page, close } = await open("/icons-flags");
      for (const grid of ["square", "round"]) {
        const sizes = await page.locator(`[data-flags=${grid}] svg[data-slot=flag]`).evaluateAll((nodes) => [...new Set(nodes.map((n) => `${Math.round(n.getBoundingClientRect().width)}x${Math.round(n.getBoundingClientRect().height)}`))]);
        assert.deepEqual(sizes, ["32x32"], grid);
      }
      const radius = await page.locator("[data-flags=round] svg[data-slot=flag]").first().evaluate((n) => getComputedStyle(n).borderRadius);
      assert.match(radius, /50%|16px/);
      await close();
    });

    test("a copy of a flag that is not drawn, and comes first, never changes the drawn ones", async () => {
      const plain = await open("/icons-flags");
      const first = await open("/icons-flags/hidden-first");
      const pairs = [];
      for (const grid of ["rect", "square", "round"]) {
        const a = await plain.page.locator(`[data-flags=${grid}]`).screenshot();
        const b = await first.page.locator(`[data-flags=${grid}]`).screenshot();
        pairs.push([grid, a, b]);
      }
      for (const [grid, a, b] of pairs) {
        assert.ok(a.equals(b), `the ${grid} flags drew differently with hidden copies before them`);
      }
      // And the hidden copies are really there, before the drawn ones.
      assert.equal(await first.page.locator("[data-hidden] svg[data-slot=flag]").count(), 270 * 3);
      assert.equal(await first.page.locator("[data-hidden] svg[data-slot=flag]:visible").count(), 0);
      await plain.close();
      await first.close();
    });

    test("a flag has the country's name, unless its name is beside it", async () => {
      const { page, close } = await open("/icons-flags");
      // One per shape in the grids, and the standalone ones.
      assert.ok((await page.getByRole("img", { name: "United Arab Emirates" }).count()) >= 3);
      assert.equal(await page.getByRole("img", { name: "Brazil" }).count() >= 5, true);
      assert.equal(await page.getByRole("img", { name: "Unknown country" }).count(), 2);
      // Beside their names: hidden from assistive technology.
      assert.equal(await page.locator("[data-slot=badge] svg[data-slot=flag][aria-hidden=true]").count(), 2);
      assert.equal(await page.getByRole("img", { name: "Germany" }).count(), 3, "one in each grid, none in the badge");
      assert.equal(await page.getByRole("img", { name: "Delivered" }).count(), 1);
      await close();
    });

    test("own icons are drawn in the text colour, and a colour they keep stays", async () => {
      const { page, close } = await open("/icons-flags");
      const result = await page.evaluate(() => {
        const seal = document.querySelector("[data-own=seal] svg");
        const [outer, inner] = seal.querySelectorAll("path");
        return {
          text: getComputedStyle(seal).color,
          outer: getComputedStyle(outer).fill,
          outerStroke: getComputedStyle(outer).stroke,
          inner: getComputedStyle(inner).stroke,
          box: seal.getAttribute("viewBox"),
          width: seal.getBoundingClientRect().width,
        };
      });
      assert.equal(result.outer, result.text);
      assert.equal(result.outerStroke, "none");
      assert.equal(result.inner, "rgb(255, 255, 255)");
      assert.equal(result.box, "0 0 32 32");
      assert.equal(result.width, 16);
      await close();
    });

    test("at 390px nothing scrolls sideways and a flag in a table cell keeps its shape", async () => {
      const { page, close } = await open("/icons-flags", { viewport: { width: 390, height: 800 } });
      const widths = await page.evaluate(() => ({ scroll: document.documentElement.scrollWidth, inner: innerWidth }));
      assert.ok(widths.scroll <= widths.inner, JSON.stringify(widths));
      const cell = await page.locator("td svg[data-slot=flag]").first().evaluate((n) => {
        const box = n.getBoundingClientRect();
        return [Math.round(box.width), Math.round(box.height)];
      });
      assert.deepEqual(cell, [24, 18]);
      await shot(page, `${browserName}-390`);
      await close();
    });

    test("right to left: a flag keeps its size and its shape", async () => {
      const { page, close } = await open("/icons-flags");
      await page.evaluate(() => document.documentElement.setAttribute("dir", "rtl"));
      const sizes = await page.locator("td svg[data-slot=flag]").evaluateAll((nodes) => nodes.map((n) => `${Math.round(n.getBoundingClientRect().width)}x${Math.round(n.getBoundingClientRect().height)}`));
      assert.deepEqual([...new Set(sizes)], ["24x18"]);
      const transform = await page.locator("td svg[data-slot=flag]").first().evaluate((n) => getComputedStyle(n).transform);
      assert.equal(transform, "none");
      await close();
    });

    test("a table of flags next to long text does not squeeze them", async () => {
      const { page, close } = await open("/icons-flags", { viewport: { width: 390, height: 800 } });
      await page.evaluate(() => {
        for (const cell of document.querySelectorAll("td:first-child [data-slot=row]")) cell.append(" " + "very long country name ".repeat(12));
      });
      const sizes = await page.locator("td svg[data-slot=flag]").evaluateAll((nodes) => nodes.map((n) => `${Math.round(n.getBoundingClientRect().width)}x${Math.round(n.getBoundingClientRect().height)}`));
      assert.deepEqual([...new Set(sizes)], ["24x18"]);
      await close();
    });

    test("the page of 2,000 icons and 810 flags is ready within a few seconds", async () => {
      const { page, close } = await open("/icons-flags");
      const timing = await page.evaluate(() => {
        const [nav] = performance.getEntriesByType("navigation");
        return { dom: Math.round(nav.domContentLoadedEventEnd), load: Math.round(nav.loadEventEnd), bytes: nav.transferSize };
      });
      assert.ok(timing.load < 5000, JSON.stringify(timing));
      if (SHOTS) await writeFile(join(SHOTS, `${browserName}-timing.json`), JSON.stringify(timing));
      await close();
    });
  });
}
