// Screenshots of every UI tool in tools.json at each width, signed in as
// the operator. Writes <OUT_DIR>/<tool>--<page>--<width>.png and
// <OUT_DIR>/results.json: per screenshot the HTTP status, the console
// errors, whether the page's `expect` text was found, and ok.
//
// A tool's sign-in screen is filled the same way for every tool: when the
// page shows a password field, the first email (or text) field gets
// EMAIL, the password field PASSWORD, and the form is submitted.
//
//   BASE_URL=http://localhost:47080 OUT_DIR=./shots node shoot.mjs

import { chromium } from 'playwright';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const BASE_URL = (process.env.BASE_URL || 'http://localhost:47080').replace(/\/$/, '');
const OUT_DIR = process.env.OUT_DIR || './screenshots';
const EMAIL = process.env.EMAIL || 'operator@evidence.test';
const PASSWORD = process.env.PASSWORD || 'evidence-password';
const WIDTHS = (process.env.WIDTHS || '1440,390').split(',').map(Number);
const ONLY = process.env.TOOLS ? new Set(process.env.TOOLS.split(',')) : null;

const tools = JSON.parse(await readFile(path.join(here, 'tools.json'), 'utf8')).filter(
  (tool) => !ONLY || ONLY.has(tool.name),
);
await mkdir(OUT_DIR, { recursive: true });

const slug = (p) => p.replace(/^\/+/, '').replace(/[^a-zA-Z0-9]+/g, '-').replace(/-+$/, '') || 'root';

async function signInIfAsked(page) {
  const password = page.locator('input[type="password"]').first();
  if (!(await password.isVisible().catch(() => false))) return false;
  const email = page.locator('input[type="email"], input[name="email"], input[type="text"]').first();
  await email.fill(EMAIL);
  await password.fill(PASSWORD);
  await Promise.all([
    page.waitForLoadState('networkidle').catch(() => {}),
    password.press('Enter'),
  ]);
  await page.waitForTimeout(500);
  return true;
}

const browser = await chromium.launch();
const results = [];
for (const width of WIDTHS) {
  const height = width >= 1000 ? 900 : 844;
  const context = await browser.newContext({
    viewport: { width, height },
    deviceScaleFactor: 1,
    isMobile: width < 600,
    hasTouch: width < 600,
    ignoreHTTPSErrors: true,
  });
  for (const tool of tools) {
    for (const pagePath of tool.pages) {
      const page = await context.newPage();
      const errors = [];
      page.on('console', (msg) => {
        if (msg.type() === 'error') errors.push(msg.text());
      });
      page.on('pageerror', (err) => errors.push(String(err)));
      const file = `${tool.name}--${slug(pagePath)}--${width}.png`;
      const result = { tool: tool.name, path: pagePath, width, file, ok: false };
      try {
        let res = await page.goto(BASE_URL + pagePath, { waitUntil: 'networkidle', timeout: 30000 });
        if (await signInIfAsked(page)) {
          result.signed_in = true;
          res = await page.goto(BASE_URL + pagePath, { waitUntil: 'networkidle', timeout: 30000 });
        }
        await page.waitForTimeout(Number(tool.settle_ms || 750));
        result.status = res ? res.status() : null;
        const text = await page.locator('body').innerText().catch(() => '');
        result.expect_found = tool.expect ? text.includes(tool.expect) : true;
        result.still_signing_in = await page.locator('input[type="password"]').first().isVisible().catch(() => false);
        result.horizontal_overflow = await page.evaluate(
          () => document.documentElement.scrollWidth > window.innerWidth + 1,
        );
        await page.screenshot({ path: path.join(OUT_DIR, file), fullPage: true });
        result.ok = result.status !== null && result.status < 400 && result.expect_found && !result.still_signing_in;
      } catch (err) {
        result.error = String(err).slice(0, 500);
      }
      result.console_errors = errors.slice(0, 20);
      results.push(result);
      console.log(`${result.ok ? 'ok  ' : 'FAIL'} ${width} ${tool.name} ${pagePath} ${result.status ?? ''} ${result.error ?? ''}`);
      await page.close();
    }
  }
  await context.close();
}
await browser.close();
await writeFile(path.join(OUT_DIR, 'results.json'), JSON.stringify(results, null, 2));
const failed = results.filter((r) => !r.ok).length;
console.log(`${results.length - failed}/${results.length} screenshots ok`);
process.exit(failed ? 1 : 0);
