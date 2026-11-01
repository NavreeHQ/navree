// Photographs what the website says each tool looks like: the product mock
// of each tool on /ecosystem (and the tool's whole block, with its list of
// features), at 1440 and 390 pixels. The mocks cycle through a few states,
// so each is taken twice, a few seconds apart.
//
//   SITE_URL=https://navree.com OUT_DIR=./shots node website.mjs

import { chromium } from 'playwright';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { contextOptions } from './lib.mjs';

const SITE_URL = (process.env.SITE_URL || 'https://navree.com').replace(/\/$/, '');
const OUT_DIR = process.env.OUT_DIR || './shots';
const WIDTHS = (process.env.WIDTHS || '1440,390').split(',').map(Number);
const ALL = (process.env.TOOLS || 'lighthouse,fleet,helm,logbook,harbor,navigator,crm').split(',');
const TOOLS = ALL.filter((t) => t !== 'crm');
// The screens the CRM page of the website shows: static images of crm-web.
const CRM_IMAGES = ['dashboard-1440', 'pipeline-1440', 'deal-1440', 'contacts-1440', 'companies-1440', 'palette-1440', 'phone-dashboard-390', 'phone-deal-390'];

const dir = path.join(OUT_DIR, 'website');
await mkdir(dir, { recursive: true });
const browser = await chromium.launch();
let out = { site: SITE_URL, taken_at: new Date().toISOString(), tools: {} };
// A run of a few tools adds to what the file already holds.
try {
  const before = JSON.parse(await readFile(path.join(OUT_DIR, 'website.json'), 'utf8'));
  if (before.tools) out = { ...before, taken_at: out.taken_at };
} catch {
  // no earlier file
}

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
if (ALL.includes('crm')) {
  const context = await browser.newContext();
  const entry = (out.tools.crm = { shots: [] });
  for (const name of CRM_IMAGES) {
    const res = await context.request.get(`${SITE_URL}/images/crm/next/${name}.webp`);
    if (!res.ok()) {
      console.log(`FAIL crm ${name} ${res.status()}`);
      continue;
    }
    const file = `crm--${name}.webp`;
    await writeFile(path.join(dir, file), await res.body());
    entry.shots.push({ kind: 'crm-image', name, file: `website/${file}` });
    console.log(`ok   crm ${name}`);
  }
  await context.close();
}
await browser.close();
await writeFile(path.join(OUT_DIR, 'website.json'), JSON.stringify(out, null, 2));
