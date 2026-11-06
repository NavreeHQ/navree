// What the flow suite shares: the proof app's address, axe, the light theme's values, and
// a browser per describe whose pages report any console error or policy violation.

import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { createRequire } from "node:module";
import { after, before, describe } from "node:test";
import * as playwright from "playwright";
import { launch } from "./launch.mjs";
import { LIGHT } from "./light.mjs";

export const BASE = process.env.UI_PROOF_URL || "http://127.0.0.1:8090";
const require = createRequire(import.meta.url);
const axeSource = await readFile(require.resolve("axe-core/axe.min.js"), "utf8");
export const NAMES = (process.env.UI_BROWSERS || "chromium,firefox").split(/[ ,]+/).filter((n) => playwright[n]);
/** A folder to keep a screenshot of every state in, or none. */
export const SHOTS = process.env.UI_SHOTS || "";

// The page in light, from the tokens.
export { LIGHT };

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

/**
 * Moves focus on, as Tab does. WebKit's Tab leaves links out unless the reader turns that on,
 * so there focus is moved along the page's order after a key press, as the pages suite does.
 */
export async function tab(page, browserName) {
  if (browserName !== "webkit") return page.keyboard.press("Tab");
  await page.keyboard.press("Shift");
  await page.evaluate(() => {
    const list = [...document.querySelectorAll("a[href],button:not([disabled]),input:not([disabled]):not([type=hidden]),select,textarea,summary,[tabindex]:not([tabindex='-1'])")].filter((el) => el.getClientRects().length && getComputedStyle(el).visibility !== "hidden");
    list[list.indexOf(document.activeElement) + 1]?.focus();
  });
}

/** One `describe` per browser, with `open`, whose result ends by asserting nothing was reported. */
export function inBrowsers(body) {
  for (const browserName of NAMES) {
    describe(browserName, { concurrency: false }, () => {
      let browser;
      before(async () => {
        browser = await launch(playwright[browserName]);
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
  page.evaluate(
    (s) => [...document.querySelectorAll(s)].filter((e) => e.getAttribute("tabindex") === "0" && !e.disabled && !e.hidden && !e.closest("[popover]")).length,
    selector,
  );

/**
 * Runs `take`, which takes a screenshot of `page`, and returns what it returns. WebKit's screenshot puts a <style> in
 * the page to hide the caret, and the policy reports it: that report belongs to the screenshot, not to the page, so it
 * is taken out again.
 */
export async function screenshotting(page, take) {
  const webkit = page.context().browser()?.browserType().name() === "webkit";
  const seen = webkit ? await page.evaluate(() => (window.__csp || []).length).catch(() => 0) : 0;
  try {
    return await take();
  } finally {
    if (webkit) {
      await page
        .evaluate((n) => {
          if (window.__csp) window.__csp = window.__csp.slice(0, n).concat(window.__csp.slice(n).filter((r) => r !== "style-src-elem inline"));
        }, seen)
        .catch(() => {});
    }
  }
}

export async function shot(page, name, selector) {
  if (!SHOTS) return;
  await screenshotting(page, async () => {
    if (selector) {
      // One part of the page, its whole height. A bar that sticks to the top would be drawn over it, so it is out of sight meanwhile.
      await page.evaluate((s) => {
        const part = document.querySelector(s);
        window.__out = [...document.querySelectorAll("body *")].filter((el) => {
          const position = getComputedStyle(el).position;
          return (position === "sticky" || position === "fixed") && !part.contains(el) && !el.contains(part);
        });
        for (const el of window.__out) el.style.visibility = "hidden";
      }, selector);
      await page.locator(selector).screenshot({ path: `${SHOTS}/${name}.png` });
      await page.evaluate(() => { for (const el of window.__out || []) el.style.visibility = ""; });
    } else {
      await page.screenshot({ path: `${SHOTS}/${name}.png`, fullPage: false });
    }
  });
}
