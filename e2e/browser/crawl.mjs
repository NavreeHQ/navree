// The screenshot crawler: starts from each tool's roots (tools.json),
// signs in as the operator, follows every same-origin link inside the
// tool's scope and photographs each page at every width.
//
// A page is found, never listed: the roots and a few variants come from
// tools.json, everything else from the links on the pages already seen.
// To keep the walk finite, links are grouped by their route template
// (`/admin/resources/tasks/{id}`) and their query keys; a template that
// holds a record (`{id}`) is opened once per list that links to it, up to
// `per_template` (default 4) pages.
//
// Widths: the first width discovers the pages, the others replay that list.
//
//   BASE_URL=http://localhost:8080 OUT_DIR=./shots PHASE=after node crawl.mjs
//
// Environment: BASE_URL, OUT_DIR, PHASE (a label; folder under OUT_DIR),
// MODE (`crawl`: follow links; `roots`: roots and variants only),
// TOOLS (comma list of tool names), WIDTHS (default 1440,390), SERVICE
// (only tools served by this compose service), EMAIL, PASSWORD, SETTLE_MS.

import { chromium } from 'playwright';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import {
  BASE_URL, contextOptions, collect, fileFor, hashTemplate, shoot, signInIfAsked, templateOf,
} from './lib.mjs';
import { runStates } from './states.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const OUT_DIR = process.env.OUT_DIR || './shots';
const PHASE = process.env.PHASE ?? 'after';
const MODE = process.env.MODE || 'crawl';
const WIDTHS = (process.env.WIDTHS || '1440,390').split(',').map(Number);
const ONLY = process.env.TOOLS ? new Set(process.env.TOOLS.split(',')) : null;
const SERVICE = process.env.SERVICE || null;
const STATES = process.env.STATES ? process.env.STATES !== '0' : MODE !== 'roots';

const config = JSON.parse(await readFile(path.join(here, 'tools.json'), 'utf8'));
const tools = config.tools.filter(
  (tool) => (!ONLY || ONLY.has(tool.name)) && (!SERVICE || (tool.service || 'web') === SERVICE),
);

const phaseDir = path.join(OUT_DIR, PHASE);
// One results file per service, so the processes that walk web and web-dev
// at the same time do not overwrite each other.
const RESULTS = PHASE ? `results-${SERVICE || 'all'}.json` : 'results.json';
await mkdir(phaseDir, { recursive: true });

const SKIP_EXT = /\.(css|js|mjs|map|png|jpe?g|gif|svg|ico|woff2?|ttf|json|txt|xml|pdf|zip)(\?|$)/i;

function insideScope(tool, pathname) {
  return tool.scope.some((s) => pathname === s || pathname.startsWith(s.endsWith('/') ? s : `${s}/`));
}

/** A page that is a sign-in screen by design (a gallery's example): never filled in. */
function isSignInPage(tool, url) {
  return (tool.sign_in_pages || []).some((p) => url.includes(p));
}

function avoided(tool, href) {
  return (tool.avoid || []).some((re) => new RegExp(re).test(href));
}

/** `path?key&key=value#view/{id}` for grouping links; values only for `keep_values`. */
function signatureOf(tool, url) {
  const keep = new Set(tool.keep_values || []);
  const keys = [...url.searchParams.keys()].sort();
  const query = keys.map((k) => (keep.has(k) ? `${k}=${url.searchParams.get(k)}` : k)).join('&');
  return { template: templateOf(url.pathname), query, hash: hashTemplate(url.hash), full: `${templateOf(url.pathname)}?${query}${hashTemplate(url.hash)}` };
}

const isRecord = (sig) => sig.template.includes('{id}') || sig.hash.includes('{id}') || /endpoint|trace/.test(sig.query);

/** The page's links; those whose text matches `avoidText` (records the load run deletes again) are left out. */
async function linksOf(page, avoidText = []) {
  const patterns = avoidText;
  return page.evaluate((avoid) =>
    Array.from(document.querySelectorAll('a[href]'))
      .filter((a) => {
        const r = a.getBoundingClientRect();
        return r.width > 0 || r.height > 0 || a.closest('nav, aside');
      })
      .filter((a) => !avoid.some((re) => new RegExp(re).test((a.innerText || '').trim())))
      .map((a) => a.getAttribute('href'))
      .filter((h) => h && !h.startsWith('mailto:') && !h.startsWith('javascript:')),
  patterns);
}

async function openTool(context, tool) {
  const page = await context.newPage();
  const bag = collect(page);
  return { page, bag };
}

/** The discovery walk of one tool at the first width. */
async function discover(context, tool, width, dir, rel) {
  const records = [];
  const { page, bag } = await openTool(context, tool);
  const spa = !!tool.spa;
  const queue = [];
  const seen = new Set();
  const perTemplate = new Map();
  const perReferrerTemplate = new Map();
  const queryVariants = new Map();
  const cap = Number(process.env.MAX_PAGES || tool.max_pages || 90);
  const limit = (tool.per_template && typeof tool.per_template === 'object') ? tool.per_template : {};
  const defaultPer = typeof tool.per_template === 'number' ? tool.per_template : 4;
  // How many records of one template a single list page may lead to.
  const perList = tool.per_list || {};

  const enqueue = (href, base, referrer, force = false) => {
    let url;
    try {
      url = new URL(href, base);
    } catch {
      return;
    }
    if (url.origin !== new URL(BASE_URL).origin) return;
    if (SKIP_EXT.test(url.pathname) && !tool.allow_ext) return;
    if (!insideScope(tool, url.pathname)) return;
    if (avoided(tool, url.pathname + url.search + url.hash)) return;
    url.hash = spa ? url.hash : '';
    const sig = signatureOf(tool, url);
    const key = url.pathname + url.search + url.hash;
    if (seen.has(key)) return;
    if (force) {
      // A page the tool's config names: always taken, and the links that
      // look like it are not taken again.
      const variants = queryVariants.get(sig.template + sig.hash) || new Set();
      variants.add(sig.query);
      queryVariants.set(sig.template + sig.hash, variants);
    } else if (isRecord(sig)) {
      const listKey = `${sig.template}${sig.hash}@${referrer?.full || ''}`;
      const n = perTemplate.get(sig.template + sig.hash) || 0;
      const allowed = limit[sig.template] ?? defaultPer;
      const fromList = perReferrerTemplate.get(listKey) || 0;
      if (fromList >= (perList[sig.template] ?? 1) || n >= allowed) return;
      perReferrerTemplate.set(listKey, fromList + 1);
      perTemplate.set(sig.template + sig.hash, n + 1);
    } else {
      const variants = queryVariants.get(sig.template + sig.hash) || new Set();
      if (!variants.has(sig.query) && variants.size >= (tool.max_query_variants ?? 4)) return;
      if (variants.has(sig.query) && sig.query !== '' ) {
        // the same keys and kept values: one page is enough
        return;
      }
      variants.add(sig.query);
      queryVariants.set(sig.template + sig.hash, variants);
    }
    seen.add(key);
    queue.push({ url: url.toString(), sig });
  };

  for (const root of tool.roots) enqueue(root, BASE_URL, null, true);
  if (MODE !== 'roots') for (const variant of tool.variants || []) enqueue(variant, BASE_URL, null, true);

  // Sign in once (the tool's own screen), then walk.
  await page.goto(BASE_URL + tool.roots[0], { waitUntil: 'load', timeout: 30000 }).catch(() => {});
  await signInIfAsked(page);

  let n = 0;
  while (queue.length && n < cap) {
    const item = queue.shift();
    const file = fileFor(new URL(item.url).pathname + new URL(item.url).search + new URL(item.url).hash, width, `${tool.name}--`);
    const rec = await shoot(page, bag, {
      url: item.url, file, outDir: dir, relDir: rel, expect: n === 0 ? tool.expect : null, navigate: true, settle: tool.settle_ms, expectStatus: tool.expect_status, noLogin: isSignInPage(tool, item.url),
    });
    rec.tool = tool.name;
    rec.phase = PHASE;
    rec.width = width;
    rec.template = item.sig.full;
    records.push(rec);
    n += 1;
    console.log(`${rec.ok ? 'ok  ' : 'FAIL'} ${width} ${tool.name} ${rec.url} ${rec.status ?? ''} ${rec.wall_ms ?? ''}ms ${rec.error ?? ''}`);
    if (MODE === 'roots') continue;
    if (rec.status && rec.status < 400) {
      const hrefs = await linksOf(page, tool.avoid_text || []).catch(() => []);
      const here = new URL(page.url());
      for (const href of hrefs) enqueue(href, here, item.sig);
      if (tool.click_rows) {
        const urls = await clickRows(page, tool, rec.url);
        for (const u of urls) enqueue(u, here, item.sig);
      }
    }
  }
  await page.close();
  return records;
}

/** For hash-routed screens: the pages that clicking the first table rows leads to. */
async function clickRows(page, tool, from) {
  const found = [];
  const rows = page.locator('table tbody tr');
  const count = Math.min(await rows.count().catch(() => 0), 1);
  for (let i = 0; i < count; i++) {
    const before = page.url();
    await rows.nth(i).click({ timeout: 2000 }).catch(() => {});
    await page.waitForTimeout(400);
    if (page.url() !== before) found.push(page.url());
    await page.goto(BASE_URL + from, { waitUntil: 'load' }).catch(() => {});
    await page.waitForTimeout(300);
  }
  return found;
}

/** Replays a list of URLs at another width. */
async function replay(context, tool, width, urls, dir, rel) {
  const records = [];
  const { page, bag } = await openTool(context, tool);
  await page.goto(BASE_URL + tool.roots[0], { waitUntil: 'load', timeout: 30000 }).catch(() => {});
  await signInIfAsked(page);
  let n = 0;
  for (const u of urls) {
    const parsed = new URL(BASE_URL + u);
    const file = fileFor(parsed.pathname + parsed.search + parsed.hash, width, `${tool.name}--`);
    const rec = await shoot(page, bag, { url: BASE_URL + u, file, outDir: dir, relDir: rel, expect: n === 0 ? tool.expect : null, settle: tool.settle_ms, expectStatus: tool.expect_status, noLogin: isSignInPage(tool, u) });
    rec.tool = tool.name;
    rec.phase = PHASE;
    rec.width = width;
    records.push(rec);
    n += 1;
    console.log(`${rec.ok ? 'ok  ' : 'FAIL'} ${width} ${tool.name} ${rec.url} ${rec.status ?? ''} ${rec.wall_ms ?? ''}ms ${rec.error ?? ''}`);
  }
  await page.close();
  return records;
}

const browser = await chromium.launch();
const all = [];
for (const tool of tools) {
  const rel = PHASE ? `${PHASE}/${tool.name}` : tool.name;
  const dir = path.join(OUT_DIR, rel);
  await mkdir(dir, { recursive: true });
  let urls = [];
  for (let i = 0; i < WIDTHS.length; i++) {
    const width = WIDTHS[i];
    const context = await browser.newContext(contextOptions(width));
    try {
      const records = i === 0 ? await discover(context, tool, width, dir, rel) : await replay(context, tool, width, urls, dir, rel);
      if (i === 0) urls = records.map((r) => r.url);
      all.push(...records);
    } catch (err) {
      console.log(`FAIL ${width} ${tool.name} ${String(err).slice(0, 300)}`);
      all.push({ tool: tool.name, phase: PHASE, width, url: tool.roots[0], ok: false, error: String(err).slice(0, 400) });
    }
    await context.close();
  }
  if (STATES) {
    for (const width of WIDTHS) {
      const states = await runStates(browser, tool, { width, outDir: dir, relDir: rel, phase: PHASE });
      all.push(...states);
    }
  }
  await writeFile(path.join(phaseDir, RESULTS), JSON.stringify(all, null, 2));
}
await browser.close();
await writeFile(path.join(phaseDir, RESULTS), JSON.stringify(all, null, 2));
const failed = all.filter((r) => !r.ok && r.kind !== 'state').length;
const perTool = {};
for (const r of all) perTool[r.tool] = (perTool[r.tool] || 0) + 1;
console.log(`${PHASE || 'screenshots'}: ${all.length} screenshots, ${failed} pages not ok`, perTool);
process.exit(failed ? 1 : 0);
