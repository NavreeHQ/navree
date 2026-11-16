// A form's own parts in real browsers, on the proof app's `/forms/parts`: a submit button of the
// app's own and its look, a field in the mono face, and fields and a select at the theme's large
// step, in light and dark, a refused form, with JavaScript off, axe clean. Run with the proof
// app serving:
//
//   cargo run --release -p ui-proof -- serve          # UI_PROOF_URL, default http://127.0.0.1:8090
//   UI_BROWSERS=chromium,firefox node --test tests/form-parts.test.mjs
//
// UI_SHOTS=<dir> also saves a screenshot of each state.

import assert from "node:assert/strict";
import { mkdirSync, readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { join } from "node:path";
import { after, before, describe, test } from "node:test";
import { chromium, firefox, webkit } from "playwright";
import { launch } from "./support/launch.mjs";

const BASE = process.env.UI_PROOF_URL || "http://127.0.0.1:8090";
const require = createRequire(import.meta.url);
const axeSource = readFileSync(require.resolve("axe-core/axe.min.js"), "utf8");
const ENGINES = { chromium, firefox, webkit };
const NAMES = (process.env.UI_BROWSERS || "chromium").split(",").filter((n) => ENGINES[n]);
const SHOTS = process.env.UI_SHOTS;
if (SHOTS) mkdirSync(SHOTS, { recursive: true });

async function axe(page) {
  await page.evaluate(axeSource);
  return page.evaluate(async () => {
    const out = await window.axe.run(document, { runOnly: ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa", "best-practice"] });
    return out.violations.map((v) => `${v.id}: ${v.nodes.slice(0, 3).map((n) => n.target.join(" ")).join(" | ")}`);
  });
}

for (const name of NAMES) {
  describe(name, () => {
    let browser;
    before(async () => {
      browser = await launch(ENGINES[name]);
    });
    after(() => browser?.close());

    /** A page of `path` in `colorScheme`, recording what the console says. */
    async function open(path, { colorScheme = "light", js = true } = {}) {
      const context = await browser.newContext({ viewport: { width: 1280, height: 900 }, colorScheme, javaScriptEnabled: js });
      const page = await context.newPage();
      const problems = [];
      page.on("console", (message) => {
        if (name === "webkit" && /Refused to apply a stylesheet/.test(message.text())) return;
        if (message.type() === "error" && !/Failed to load resource/.test(message.text())) problems.push(message.text());
      });
      page.on("pageerror", (error) => problems.push(String(error)));
      await page.goto(BASE + path);
      return { page, problems, close: () => context.close() };
    }

    const shot = async (page, file) => {
      if (SHOTS) await page.screenshot({ path: join(SHOTS, `${name}-${file}.png`), fullPage: true });
    };

    test("the submit button's look, a mono field and the large field step on inputs and a select, light and dark, axe clean", async () => {
      for (const colorScheme of ["light", "dark"]) {
        const { page, problems, close } = await open("/forms/parts", { colorScheme });
        const seen = await page.evaluate(() => {
          const style = (selector) => {
            const found = document.querySelector(selector);
            if (!found) throw new Error(`no ${selector}`);
            return getComputedStyle(found);
          };
          const probe = (property, value) => {
            const span = document.createElement("span");
            span.style.setProperty(property, value);
            document.querySelector("main").append(span);
            const out = getComputedStyle(span).getPropertyValue(property);
            span.remove();
            return out;
          };
          const start = document.getElementById("start");
          const startButton = start.querySelector("[data-slot=form-actions] [data-slot=button]");
          const pay = document.querySelector("#checkout [data-slot=form-actions] [data-slot=button]");
          return {
            startButton: {
              type: startButton.getAttribute("type"),
              tag: startButton.tagName,
              variant: startButton.getAttribute("data-variant"),
              height: getComputedStyle(startButton).height,
              radius: getComputedStyle(startButton).borderTopLeftRadius,
              background: getComputedStyle(startButton).backgroundColor,
              width: startButton.getBoundingClientRect().width,
              formWidth: start.getBoundingClientRect().width,
            },
            pay: { variant: pay.getAttribute("data-variant"), background: getComputedStyle(pay).backgroundColor, accent: probe("color", "var(--accent)") },
            heights: ["#start [name=email]", "#checkout [name=plan]", "#checkout [name=company]", "#checkout [name=gstin]"].map((selector) => style(selector).height),
            mono: style("#checkout [name=gstin]").fontFamily,
            text: style("#checkout [name=company]").fontFamily,
            monoFace: probe("font-family", "var(--font-mono)"),
          };
        });
        assert.deepEqual(
          [seen.startButton.tag, seen.startButton.type, seen.startButton.variant],
          ["BUTTON", "submit", "outline"],
          `${colorScheme}: the app's own button submits`
        );
        assert.equal(seen.startButton.height, "48px", `${colorScheme}: the large control step`);
        assert.equal(seen.startButton.radius, "999px", `${colorScheme}: the theme's pill`);
        assert.equal(seen.startButton.background, "rgba(0, 0, 0, 0)", `${colorScheme}: an outline button`);
        assert.ok(Math.abs(seen.startButton.width - seen.startButton.formWidth) < 1, `${colorScheme}: the width of the form`);
        assert.equal(seen.pay.variant, "primary");
        assert.equal(seen.pay.background, seen.pay.accent, `${colorScheme}: the primary look`);
        assert.deepEqual(seen.heights, ["48px", "48px", "48px", "48px"], `${colorScheme}: every field and the select at the large step`);
        assert.equal(seen.mono, seen.monoFace, `${colorScheme}: the tax number in the mono face`);
        assert.notEqual(seen.text, seen.monoFace, `${colorScheme}: the company in the text face`);
        assert.deepEqual(await axe(page), [], `${colorScheme}: axe`);
        assert.deepEqual(problems, []);
        await shot(page, `form-parts-${colorScheme}`);
        await close();
      }
    });

    test("a refused form shows the field's error and keeps what was typed and chosen, at the large step", async () => {
      const { page, problems, close } = await open("/forms/parts");
      const checkout = page.locator("#checkout");
      await checkout.locator("select[name=plan]").selectOption("scale");
      await checkout.locator("[name=company]").fill("Sutlej Supply");
      await checkout.locator("[name=gstin]").fill("22AAAA");
      const answer = page.waitForResponse((response) => response.url().endsWith("/forms/parts/checkout"));
      await checkout.getByRole("button", { name: "Pay and start" }).click();
      assert.equal((await answer).status(), 422);
      await page.waitForSelector("#checkout-gstin-error");
      assert.match(await page.locator("#checkout-gstin-error").innerText(), /15 letters and digits/);
      assert.equal(await checkout.locator("select[name=plan]").inputValue(), "scale", "the choice stays");
      assert.equal(await checkout.locator("[name=gstin]").getAttribute("aria-invalid"), "true");
      const after = await page.evaluate(() =>
        ["#checkout [name=gstin]", "#checkout [name=plan]"].map((selector) => getComputedStyle(document.querySelector(selector)).height)
      );
      assert.deepEqual(after, ["48px", "48px"], "still at the large step");
      await shot(page, "form-parts-refused");
      assert.deepEqual(await axe(page), []);
      assert.deepEqual(problems, []);
      await close();
    });

    test("with JavaScript off the form posts and comes back with what was typed and chosen and its errors", async () => {
      const { page, close } = await open("/forms/parts", { js: false });
      const checkout = page.locator("#checkout");
      await checkout.locator("select[name=plan]").selectOption("starter");
      await checkout.locator("[name=company]").fill("Sutlej Supply");
      await checkout.locator("[name=gstin]").fill("22-AAAA");
      await Promise.all([page.waitForURL(/\/forms\/parts\/checkout$/), checkout.getByRole("button", { name: "Pay and start" }).click()]);
      assert.equal(await page.locator("#checkout select[name=plan]").inputValue(), "starter", "the posted plan is chosen again");
      assert.match(await page.locator("#checkout-gstin-error").innerText(), /15 letters and digits/);
      assert.equal(await page.locator("#checkout [name=company]").inputValue(), "Sutlej Supply");
      await close();
    });
  });
}
