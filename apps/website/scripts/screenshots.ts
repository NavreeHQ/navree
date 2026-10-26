/**
 * Visual check: full-page screenshots of the key pages at desktop (1440 px)
 * and phone (390 px) widths, plus the search dialog and the mobile docs
 * menu. Saved to `screenshots/` (git-ignored).
 *
 *   pnpm build && pnpm serve           # in one terminal
 *   pnpm screenshots                   # in another
 *   BASE_URL=http://127.0.0.1:3210 pnpm screenshots home docs-stub
 */
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { chromium, type Page } from "playwright";

const BASE = process.env.BASE_URL ?? "http://127.0.0.1:3211";
const DIR = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "screenshots");

const pages: [string, string][] = [
  ["home", "/"],
  ["docs-index", "/docs/"],
  ["docs-stub", "/docs/basics/validation/"],
  ["styleguide", "/styleguide/"],
  ["benchmarks", "/benchmarks/"],
];

const viewports = [
  { name: "1440", width: 1440, height: 900 },
  { name: "390", width: 390, height: 844 },
];

const only = process.argv.slice(2);

async function settle(page: Page) {
  await page.evaluate(() => document.fonts.ready);
  await page.waitForTimeout(400);
}

async function main() {
  fs.mkdirSync(DIR, { recursive: true });
  const browser = await chromium.launch();
  const errors: string[] = [];
  for (const viewport of viewports) {
    const context = await browser.newContext({
      viewport: { width: viewport.width, height: viewport.height },
      deviceScaleFactor: viewport.width < 600 ? 2 : 1,
      reducedMotion: "reduce",
    });
    const page = await context.newPage();
    page.on("pageerror", (error) => errors.push(`${page.url()}: ${error.message}`));
    page.on("console", (message) => {
      if (message.type() === "error") errors.push(`${page.url()}: ${message.text()}`);
    });

    for (const [name, url] of pages) {
      if (only.length && !only.includes(name)) continue;
      await page.goto(`${BASE}${url}`, { waitUntil: "networkidle" });
      await settle(page);
      const overflow = await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth);
      if (overflow > 0) errors.push(`${url} at ${viewport.name}px overflows horizontally by ${overflow}px`);
      await page.screenshot({ path: path.join(DIR, `${name}-${viewport.name}.png`), fullPage: true });
      console.log(`saved ${name}-${viewport.name}.png`);
    }

    if (!only.length || only.includes("search")) {
      await page.goto(`${BASE}/docs/basics/validation/`, { waitUntil: "networkidle" });
      await settle(page);
      await page.keyboard.press("Control+KeyK");
      await page.locator("dialog.search-dialog input").fill("valid");
      await page.locator("dialog.search-dialog [role=option]").first().waitFor();
      await page.waitForTimeout(200);
      await page.screenshot({ path: path.join(DIR, `search-${viewport.name}.png`) });
      console.log(`saved search-${viewport.name}.png`);
      await page.keyboard.press("Escape");
    }

    if (viewport.width < 1024 && (!only.length || only.includes("menu"))) {
      await page.goto(`${BASE}/docs/basics/validation/`, { waitUntil: "networkidle" });
      await settle(page);
      await page.getByRole("button", { name: /documentation menu/i }).click();
      await page.waitForTimeout(300);
      await page.screenshot({ path: path.join(DIR, `docs-menu-${viewport.name}.png`) });
      console.log(`saved docs-menu-${viewport.name}.png`);
    }
    await context.close();
  }
  await browser.close();
  if (errors.length) {
    console.error(`\n${errors.length} problem(s):\n- ${errors.join("\n- ")}`);
    process.exitCode = 1;
  }
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
