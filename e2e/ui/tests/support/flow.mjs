// What the flow suite shares: the proof app's address, axe, the light theme's values, and
// a browser per describe whose pages report any console error or policy violation.

import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { createRequire } from "node:module";
import { after, before, describe } from "node:test";
import * as playwright from "playwright";

export const BASE = process.env.UI_PROOF_URL || "http://127.0.0.1:8090";
const require = createRequire(import.meta.url);
const axeSource = await readFile(require.resolve("axe-core/axe.min.js"), "utf8");
export const NAMES = (process.env.UI_BROWSERS || "chromium,firefox").split(/[ ,]+/).filter((n) => playwright[n]);
/** A folder to keep a screenshot of every state in, or none. */
export const SHOTS = process.env.UI_SHOTS || "";

// The light values of the tokens: the toolkit ships dark only so far, so light is proved by putting these on the page.
export const LIGHT = ":root{color-scheme:light;--ink-0:#ffffff;--ink-1:#f7f8f7;--ink-2:#eff1f0;--ink-3:#e5e8e6;--line:#dfe3e0;--line-2:#c8cecb;--line-3:#aeb6b2;--fg:#0a0e0c;--fg-1:#1c2420;--fg-2:#3f4a44;--fg-3:#55615b;--accent:#0b7a3e;--accent-ink:#ffffff;--amber:#8a5a00;--danger:#b3261e;--bar:#8a968f}";

const lightFromTheStart = (context) =>
  context.addInitScript((css) => {
    const style = document.createElement("style");
    style.textContent = css;
    const add = () => {
      const host = document.head || document.documentElement;
      if (host) host.append(style);
      return !!host;
    };
    if (!add()) {
      const watch = new MutationObserver(() => add() && watch.disconnect());
      watch.observe(document, { childList: true, subtree: true });
    }
  }, LIGHT.replaceAll(";", " !important;").replace(/}$/, " !important}"));

/** Runs axe over the page, or over one part of it. Violations come back as short lines. */
export async function axe(page, selector) {
  await page.evaluate(axeSource);
  return page.evaluate(async (scope) => {
    const out = await window.axe.run(scope ? document.querySelector(scope) : document, {
      runOnly: ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa", "best-practice"],
    });
    return out.violations.map((v) => `${v.id}: ${v.nodes.slice(0, 3).map((n) => n.target.join(" ")).join(" | ")}`);
  }, selector || null);
}

/** One `describe` per browser, with `open`, whose result ends by asserting nothing was reported. */
export function inBrowsers(body) {
  for (const browserName of NAMES) {
    describe(browserName, { concurrency: false }, () => {
      let browser;
      before(async () => {
        browser = await playwright[browserName].launch();
        // The first page of a browser that has just started can take long on a busy machine: it is not what a test measures.
        const warm = await browser.newContext();
        await (await warm.newPage()).goto(BASE + "/flow", { timeout: 120000 });
        await warm.close();
      });
      after(() => browser?.close());

      async function open(path, options = {}) {
        const context = await browser.newContext({
          viewport: options.viewport || { width: 1440, height: 900 },
          javaScriptEnabled: options.js !== false,
          bypassCSP: !!options.light,
          reducedMotion: "reduce",
          locale: options.locale,
          colorScheme: options.light ? "light" : "dark",
        });
        if (options.light) await lightFromTheStart(context);
        const page = await context.newPage();
        const problems = [];
        page.on("console", (message) => {
          if (browserName === "webkit" && /Refused to apply a stylesheet/.test(message.text())) return;
          if (message.type() === "error" && !/Failed to load resource|ERR_|net::/.test(message.text())) problems.push(message.text());
        });
        page.on("pageerror", (error) => problems.push(String(error)));
        if (options.js !== false) {
          await page.addInitScript(() => {
            document.addEventListener("securitypolicyviolation", (e) => {
              (window.__csp ||= []).push(e.violatedDirective + " " + e.blockedURI);
            });
          });
        }
        if (options.before) await options.before(page);
        await page.goto(BASE + path);
        return {
          page,
          context,
          problems,
          close: async () => {
            const csp = options.js === false ? [] : await page.evaluate(() => window.__csp || []).catch(() => []);
            await context.close();
            assert.deepEqual(csp, [], "no content security policy report");
            assert.deepEqual(problems, [], "nothing in the console and no script error");
          },
        };
      }
      body({ open, browserName, get browser() { return browser; } });
    });
  }
}

/** Waits for a request to the proof app's counter, and returns how many times each parent was asked for. */
export const hits = (page) => page.evaluate(async () => (await fetch("/flow/hits")).json());

/** The tab stops inside `selector`: elements a Tab key can land on. */
export const stops = (page, selector) =>
  page.evaluate((s) => [...document.querySelectorAll(s)].filter((e) => e.tabIndex === 0 && !e.disabled && !e.hidden).length, selector);

export async function shot(page, name) {
  if (SHOTS) await page.screenshot({ path: `${SHOTS}/${name}.png`, fullPage: false });
}
