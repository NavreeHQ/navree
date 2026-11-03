// What the dates suites share: the proof app's address, axe, the light theme's values,
// the cases both readers are held to, and a browser per describe.

import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { createRequire } from "node:module";
import { after, before, describe } from "node:test";
import * as playwright from "playwright";

export const BASE = process.env.DATES_URL || "http://127.0.0.1:8290";
const require = createRequire(import.meta.url);
const axeSource = await readFile(require.resolve("axe-core/axe.min.js"), "utf8");
export const cases = JSON.parse(await readFile(new URL("../../../../crates/navree-ui/tests/fixtures/dates-parse.json", import.meta.url), "utf8"));

// The light values of `src/tokens.rs`: the toolkit ships dark only so far, so light
// is proved by putting these on the page.
export const LIGHT = ":root{color-scheme:light;--ink-0:#ffffff;--ink-1:#f7f8f7;--ink-2:#eff1f0;--ink-3:#e5e8e6;--line:#dfe3e0;--line-2:#c8cecb;--line-3:#aeb6b2;--fg:#0a0e0c;--fg-1:#1c2420;--fg-2:#3f4a44;--fg-3:#55615b;--accent:#0b7a3e;--accent-ink:#ffffff;--amber:#8a5a00;--danger:#b3261e;--bar:#8a968f}";

/** Runs axe. Violations come back as short lines. */
export async function axe(page) {
  await page.evaluate(axeSource);
  return page.evaluate(async () => {
    const out = await window.axe.run(document, { runOnly: ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa", "best-practice"] });
    return out.violations.map((v) => `${v.id}: ${v.nodes.slice(0, 3).map((n) => n.target.join(" ")).join(" | ")}`);
  });
}

export const browsers = (process.env.DATES_BROWSERS || "chromium firefox").split(" ");

/** One `describe` per browser, with `open`, which ends by asserting nothing was reported. */
export function inBrowsers(body) {
  for (const browserName of browsers) {
    describe(browserName, () => {
      let browser;
      before(async () => {
        browser = await playwright[browserName].launch();
      });
      after(() => browser?.close());

      async function open(path, options = {}) {
        const context = await browser.newContext({
          viewport: options.viewport || { width: 1440, height: 900 },
          javaScriptEnabled: options.js !== false,
          bypassCSP: !!options.light,
          reducedMotion: "reduce",
          locale: options.locale,
        });
        const page = await context.newPage();
        if (options.asIfScriptingOff) await asIfScriptingOff(page);
        const problems = [];
        page.on("console", (message) => {
          if (browserName === "webkit" && /Refused to apply a stylesheet/.test(message.text())) return;
          if (message.type() === "error" && !/Failed to load resource/.test(message.text())) problems.push(message.text());
        });
        page.on("pageerror", (error) => problems.push(String(error)));
        await page.goto(BASE + path);
        if (options.light) await page.addStyleTag({ content: LIGHT });
        return {
          page,
          context,
          problems,
          close: async () => {
            await context.close();
            assert.deepEqual(problems, [], "nothing in the console, no script error, no policy report");
          },
        };
      }
      body({ open, browserName });
    });
  }
}

/**
 * Makes a page, loaded with its scripts blocked, look as it does with JavaScript off: the
 * stylesheet's `scripting` queries are turned the way a browser with no scripting would take
 * them. Axe needs scripting to run, so this is how the JavaScript-off page is checked by it.
 */
export async function asIfScriptingOff(page) {
  await page.route("**/*.js", (route) => route.abort());
  await page.route("**/navree-ui-*.css", async (route) => {
    const response = await route.fetch();
    const text = (await response.text()).replaceAll("(scripting:none)", "all").replaceAll("(scripting:enabled)", "not all");
    await route.fulfill({ response, body: text });
  });
}

/** Every `input` and `change` the page's own listeners would hear. */
export const record = (page) =>
  page.evaluate(() => {
    window.__events = [];
    for (const type of ["input", "change"]) {
      document.addEventListener(type, (e) => window.__events.push(`${type}:${e.target.name || e.target.id || e.target.tagName}`));
    }
  });
export const events = (page) => page.evaluate(() => window.__events);
export const field = (page, name) => page.locator(`[name="${name}"]`).first();
export const val = (page, name) => field(page, name).inputValue();
export const day = (page, root, iso) => page.locator(`${root} button[data-date="${iso}"]`);
export const isOpen = (page, id) => page.locator(`#${id}`).evaluate((e) => e.matches(":popover-open"));
export const focusedDate = (page) => page.evaluate(() => document.activeElement?.getAttribute("data-date"));
export const label = (page, root) => page.locator(`${root} [data-ui-label]`).innerText();
