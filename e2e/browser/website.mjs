// Photographs what the website says each tool looks like: the product mock
// of each tool on /ecosystem (and the tool's whole block, with its list of
// features), at 1440 and 390 pixels. The mocks cycle through a few states,
// so each is taken twice, a few seconds apart.
//
//   SITE_URL=https://navree.com OUT_DIR=./shots node website.mjs

import { chromium } from 'playwright';
import { mkdir, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { contextOptions } from './lib.mjs';

const SITE_URL = (process.env.SITE_URL || 'https://navree.com').replace(/\/$/, '');
const OUT_DIR = process.env.OUT_DIR || './shots';
const WIDTHS = (process.env.WIDTHS || '1440,390').split(',').map(Number);
const TOOLS = ['lighthouse', 'fleet', 'helm', 'logbook', 'harbor', 'navigator'];

const dir = path.join(OUT_DIR, 'website');
await mkdir(dir, { recursive: true });
const browser = await chromium.launch();
const out = { site: SITE_URL, taken_at: new Date().toISOString(), tools: {} };

for (const width of WIDTHS) {
  const context = await browser.newContext({ ...contextOptions(width), reducedMotion: 'no-preference' });
  const page = await context.newPage();
  await page.goto(`${SITE_URL}/ecosystem/`, { waitUntil: 'load', timeout: 45000 });
  await page.waitForTimeout(1500);
  for (const tool of TOOLS) {
    const block = page.locator(`#${tool}`).first();
    const entry = (out.tools[tool] ||= { shots: [] });
    try {
      await block.scrollIntoViewIfNeeded({ timeout: 8000 });
      await page.waitForTimeout(1200);
      if (width === WIDTHS[0]) {
        entry.title = (await page.locator(`#${tool}-title`).first().innerText().catch(() => '')).trim();
        entry.text = (await block.innerText().catch(() => '')).replace(/\n{2,}/g, '\n').slice(0, 3000);
        entry.mock_label = (await block.locator('figcaption').first().textContent().catch(() => '') || '').trim();
      }
      const mock = block.locator('figure.bezel').first();
      // The site's sticky header and category bar overlap the mock when it is scrolled to the top: hide what is fixed or sticky.
      await page.evaluate(() => {
        for (const el of document.querySelectorAll('body *')) {
          const position = getComputedStyle(el).position;
          if ((position === 'fixed' || position === 'sticky') && !el.closest('figure.bezel') && !el.querySelector('figure.bezel')) el.style.visibility = 'hidden';
        }
      });
      for (const [n, wait] of [[1, 0], [2, 5500]]) {
        await page.waitForTimeout(wait);
        const mockFile = `${tool}--mock-${n}--${width}.png`;
        if (await mock.count()) {
          await mock.screenshot({ path: path.join(dir, mockFile), timeout: 15000 });
          entry.shots.push({ kind: 'mock', n, width, file: `website/${mockFile}` });
        }
      }
      const blockFile = `${tool}--block--${width}.png`;
      await block.screenshot({ path: path.join(dir, blockFile), timeout: 20000 });
      entry.shots.push({ kind: 'block', width, file: `website/${blockFile}` });
      console.log(`ok   ${width} ${tool}`);
    } catch (err) {
      entry.error = String(err).slice(0, 300);
      console.log(`FAIL ${width} ${tool} ${entry.error}`);
    }
  }
  await context.close();
}
await browser.close();
await writeFile(path.join(OUT_DIR, 'website.json'), JSON.stringify(out, null, 2));
