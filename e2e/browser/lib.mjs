// Shared helpers of the screenshot runners: sign-in, URL templates, the
// per-page measurements (load time, console, failed requests, overflow,
// charts, style rules) and the screenshot itself.

import { createHash } from 'node:crypto';
import path from 'node:path';

export const BASE_URL = (process.env.BASE_URL || 'http://localhost:47080').replace(/\/$/, '');
export const EMAIL = process.env.EMAIL || 'operator@evidence.test';
export const PASSWORD = process.env.PASSWORD || 'evidence-password';
export const SETTLE_MS = Number(process.env.SETTLE_MS || 700);

/** The browser context options of a width (phones get touch and a mobile viewport). */
export function contextOptions(width) {
  return {
    viewport: { width, height: width >= 1000 ? 900 : 844 },
    deviceScaleFactor: 1,
    isMobile: width < 600,
    hasTouch: width < 600,
    ignoreHTTPSErrors: true,
    // The tools are dark; Logbook and the references follow the system.
    colorScheme: process.env.COLOR_SCHEME || 'dark',
  };
}

const UUID = /[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}/gi;

/** `/admin/resources/tasks/0196...` as `/admin/resources/tasks/{id}`. */
export function templateOf(pathname) {
  return pathname
    .replace(UUID, '{id}')
    .replace(/\/[0-9a-f]{16,64}(?=\/|$)/gi, '/{id}')
    .replace(/\/\d+(?=\/|$)/g, '/{id}');
}

/** `#jobs/abc` as `#jobs/{id}`. */
export function hashTemplate(hash) {
  if (!hash) return '';
  const [view, ...rest] = hash.replace(/^#/, '').split('/');
  return rest.length ? `#${view}/{id}` : `#${view}`;
}

export function slug(text, max = 70) {
  const s = text.replace(/^https?:\/\/[^/]+/, '').replace(/^\/+/, '').replace(/[^a-zA-Z0-9]+/g, '-').replace(/^-+|-+$/g, '');
  return (s || 'root').slice(0, max);
}

export function fileFor(url, width, prefix = '') {
  const h = createHash('sha1').update(url).digest('hex').slice(0, 6);
  return `${prefix}${slug(url)}-${h}--${width}.png`;
}

/**
 * Signs in when the page shows a password field: the first email or text
 * field gets `email`, the password field `password`, and the form is
 * submitted. Returns whether a sign-in screen was handled.
 */
export async function signInIfAsked(page, email = EMAIL, password = PASSWORD) {
  const field = page.locator('input[type="password"]').first();
  if (!(await field.isVisible().catch(() => false))) return false;
  const who = page.locator('input[type="email"], input[name="email"], input[type="text"]').first();
  await who.fill(email);
  await field.fill(password);
  await Promise.all([
    page.waitForLoadState('load').catch(() => {}),
    field.press('Enter'),
  ]);
  await page.waitForLoadState('networkidle', { timeout: 4000 }).catch(() => {});
  await page.waitForTimeout(500);
  return true;
}

/** Attaches the collectors for console output and failing requests to `page`. */
export function collect(page) {
  const bag = { errors: [], warnings: [], failed: [] };
  page.on('console', (msg) => {
    const text = msg.text().slice(0, 400);
    if (msg.type() === 'error') bag.errors.push(text);
    else if (msg.type() === 'warning') bag.warnings.push(text);
  });
  page.on('pageerror', (err) => bag.errors.push(`pageerror: ${String(err).slice(0, 400)}`));
  page.on('requestfailed', (req) => {
    const failure = req.failure()?.errorText || 'failed';
    if (failure.includes('ERR_ABORTED')) return;
    bag.failed.push({ url: req.url().replace(BASE_URL, ''), failure });
  });
  page.on('response', (res) => {
    if (res.status() >= 400 && res.request().resourceType() !== 'document') {
      bag.failed.push({ url: res.url().replace(BASE_URL, ''), status: res.status() });
    }
  });
  return bag;
}

export function resetCollected(bag) {
  bag.errors.length = 0;
  bag.warnings.length = 0;
  bag.failed.length = 0;
}

/** What the page looks like, measured in the page. */
async function measure(page) {
  return page.evaluate(() => {
    const vw = window.innerWidth;
    const sel = (el) => {
      const id = el.id ? `#${el.id}` : '';
      const cls = typeof el.className === 'string' && el.className.trim() ? `.${el.className.trim().split(/\s+/).slice(0, 2).join('.')}` : '';
      return `${el.tagName.toLowerCase()}${id}${cls}`.slice(0, 80);
    };
    const visible = (el) => {
      const r = el.getBoundingClientRect();
      const cs = getComputedStyle(el);
      return r.width > 0 && r.height > 0 && cs.visibility !== 'hidden' && cs.display !== 'none';
    };
    const all = Array.from(document.body.querySelectorAll('*')).filter(visible).slice(0, 4000);

    const offenders = [];
    for (const el of all) {
      const r = el.getBoundingClientRect();
      if (r.right > vw + 1 && !el.closest('[data-scroll], .table-wrap, .scroll, .overflow-x, pre, code, table')) {
        offenders.push({ el: sel(el), right: Math.round(r.right), text: (el.innerText || '').trim().slice(0, 40) });
      }
    }

    const radii = new Map();
    const pills = [];
    const glows = [];
    const tiles = [];
    const elevation = [];
    for (const el of all) {
      if (el.closest('svg') || ['IMG', 'SVG', 'CANVAS', 'VIDEO', 'PATH'].includes(el.tagName.toUpperCase())) continue;
      const cs = getComputedStyle(el);
      const r = el.getBoundingClientRect();
      const radius = parseFloat(cs.borderTopLeftRadius) || 0;
      const percent = cs.borderTopLeftRadius.includes('%');
      const hasPaint = cs.backgroundColor !== 'rgba(0, 0, 0, 0)' || parseFloat(cs.borderTopWidth) > 0;
      if (!percent && !radius) {
        const text0 = (el.innerText || '').trim();
        if (/^[A-Za-z]{1,3}$/.test(text0) && r.width >= 18 && r.width <= 56 && Math.abs(r.width - r.height) < 4 && cs.backgroundColor !== 'rgba(0, 0, 0, 0)') {
          tiles.push({ el: sel(el), text: text0, w: Math.round(r.width) });
        }
      }
      if (hasPaint && (radius > 0 || percent)) {
        const key = percent ? cs.borderTopLeftRadius : `${Math.round(radius)}px`;
        radii.set(key, (radii.get(key) || 0) + 1);
        const text = (el.innerText || '').trim();
        const fullRound = percent ? parseFloat(cs.borderTopLeftRadius) >= 50 : radius >= Math.min(r.height, r.width) / 2 - 0.5 && radius > 6;
        if (fullRound && text && r.height <= 48 && r.width > r.height * 1.3) {
          pills.push({ el: sel(el), text: text.slice(0, 30), radius: cs.borderTopLeftRadius });
        }
        if (/^[A-Za-z]{1,3}$/.test(text) && r.width >= 18 && r.width <= 56 && Math.abs(r.width - r.height) < 4 && cs.backgroundColor !== 'rgba(0, 0, 0, 0)') {
          tiles.push({ el: sel(el), text, w: Math.round(r.width) });
        }
      }
      const shadow = cs.boxShadow;
      if (shadow && shadow !== 'none') {
        const m = shadow.match(/(-?\d+(?:\.\d+)?)px\s+(-?\d+(?:\.\d+)?)px\s+(\d+(?:\.\d+)?)px/);
        const color = shadow.match(/rgba?\((\d+),\s*(\d+),\s*(\d+)/);
        const coloured = color && Math.max(+color[1], +color[2], +color[3]) - Math.min(+color[1], +color[2], +color[3]) > 40 && Math.max(+color[1], +color[2], +color[3]) > 90;
        // A glow is a coloured soft shadow; a dark drop shadow is elevation.
        if (m && parseFloat(m[3]) >= 6 && !/inset/.test(shadow) && coloured) glows.push({ el: sel(el), shadow: shadow.slice(0, 80) });
        else if (m && parseFloat(m[3]) >= 12 && !/inset/.test(shadow)) elevation.push({ el: sel(el), shadow: shadow.slice(0, 80) });
      }
      if (cs.textShadow && cs.textShadow !== 'none') glows.push({ el: sel(el), shadow: `text ${cs.textShadow.slice(0, 60)}` });
    }
    const over4 = [];
    for (const [key, count] of radii) {
      const n = parseFloat(key);
      if (key.includes('%') || n > 4) over4.push({ radius: key, count });
    }

    const charts = [];
    for (const svg of document.querySelectorAll('svg')) {
      const r = svg.getBoundingClientRect();
      if (r.width < 120 || r.height < 40 || !visible(svg)) continue;
      const shapes = svg.querySelectorAll('path,rect,circle,line,polyline,polygon').length;
      charts.push({ el: sel(svg), w: Math.round(r.width), h: Math.round(r.height), shapes });
    }
    for (const c of document.querySelectorAll('canvas')) {
      const r = c.getBoundingClientRect();
      if (r.width >= 120) charts.push({ el: sel(c), w: Math.round(r.width), h: Math.round(r.height), shapes: null });
    }

    // Tables wider than the box that holds them: they scroll sideways (or are cut).
    const wideTables = [];
    for (const t of document.querySelectorAll('table')) {
      if (!visible(t)) continue;
      let box = t.parentElement;
      while (box && box !== document.body && box.clientWidth === t.offsetWidth) box = box.parentElement;
      const holder = t.parentElement;
      const hw = holder ? holder.clientWidth : 0;
      if (t.scrollWidth > hw + 2 || t.offsetWidth > hw + 2) {
        const cs = holder ? getComputedStyle(holder) : null;
        wideTables.push({ el: sel(t), table: t.offsetWidth, box: hw, overflowX: cs ? cs.overflowX : '' });
      }
    }
    const brokenImages = Array.from(document.images).filter((i) => i.complete && i.naturalWidth === 0 && visible(i)).map((i) => i.src.slice(0, 100));
    const nav = performance.getEntriesByType('navigation')[0];
    const fetches = performance.getEntriesByType('resource').filter((e) => e.initiatorType === 'fetch' || e.initiatorType === 'xmlhttprequest');
    const slowest = fetches.sort((a, b) => b.duration - a.duration)[0];
    return {
      fetches: { count: fetches.length, slowest_ms: slowest ? Math.round(slowest.duration) : 0, slowest_url: slowest ? slowest.name.replace(location.origin, '').slice(0, 100) : '' },
      title: document.title,
      h1: (document.querySelector('h1')?.innerText || '').trim().slice(0, 100),
      text: (document.body.innerText || '').replace(/\s+/g, ' ').trim().slice(0, 4000),
      textLength: (document.body.innerText || '').length,
      scrollWidth: document.documentElement.scrollWidth,
      scrollHeight: document.documentElement.scrollHeight,
      innerWidth: vw,
      horizontalOverflow: document.documentElement.scrollWidth > vw + 1,
      offenders: offenders.slice(0, 6),
      stylesheets: document.styleSheets.length,
      bodyBackground: getComputedStyle(document.body).backgroundColor,
      fontFamily: getComputedStyle(document.body).fontFamily.slice(0, 60),
      radiiOver4: over4.slice(0, 8),
      pills: pills.slice(0, 6),
      glows: glows.slice(0, 6),
      elevation: elevation.slice(0, 4),
      letterTiles: tiles.slice(0, 6),
      charts: charts.slice(0, 12),
      wideTables: wideTables.slice(0, 4),
      brokenImages: brokenImages.slice(0, 5),
      timing: nav
        ? {
            ttfb: Math.round(nav.responseStart),
            dcl: Math.round(nav.domContentLoadedEventEnd),
            load: Math.round(nav.loadEventEnd),
            transfer: Math.round(nav.transferSize || 0),
          }
        : null,
    };
  });
}

/**
 * Goes to `url` (already signed in), waits for it to settle, measures it
 * and writes the screenshot. Returns the record of the page.
 */
export async function shoot(page, bag, { url, file, outDir, relDir, expect, label, settle = SETTLE_MS, navigate = true, expectStatus = null, noLogin = false }) {
  resetCollected(bag);
  const rec = { url: url.replace(BASE_URL, ''), file: `${relDir}/${file}`, label: label || null, ok: false };
  const t0 = Date.now();
  try {
    let res = null;
    if (navigate) {
      res = await page.goto(url, { waitUntil: 'load', timeout: 30000 });
      if (!noLogin && (await signInIfAsked(page))) {
        rec.signed_in = true;
        res = await page.goto(url, { waitUntil: 'load', timeout: 30000 });
      }
    }
    rec.wall_ms = Date.now() - t0;
    await page.waitForLoadState('networkidle', { timeout: 3500 }).catch(() => {});
    await page.waitForTimeout(settle);
    // A change of the hash is the same document: no response of its own.
    rec.status = res ? res.status() : 200;
    if (!res) rec.same_document = true;
    const m = await measure(page);
    Object.assign(rec, m);
    rec.expect_found = expect ? m.text.includes(expect) : true;
    rec.still_signing_in = noLogin ? false : await page.locator('input[type="password"]').first().isVisible().catch(() => false);
    // A shell with a sticky sidebar or an inner scroller is only as tall as
    // the window in a plain full-page capture: make the window as tall as
    // the content (up to 14000 px) and capture that.
    const height = await page.evaluate(() => {
      let h = document.documentElement.scrollHeight;
      for (const el of document.querySelectorAll('body *')) {
        const cs = getComputedStyle(el);
        if ((cs.overflowY === 'auto' || cs.overflowY === 'scroll') && el.scrollHeight > el.clientHeight + 20) {
          h = Math.max(h, Math.round(el.getBoundingClientRect().top + window.scrollY + el.scrollHeight));
        }
      }
      return h;
    });
    const view = page.viewportSize();
    const clipHeight = Math.min(Math.max(height, view.height), 14000);
    rec.content_height = height;
    rec.truncated = height > 14000;
    if (clipHeight !== view.height) {
      await page.setViewportSize({ width: view.width, height: clipHeight });
      await page.waitForTimeout(250);
    }
    await page.screenshot({ path: path.join(outDir, file), fullPage: false });
    if (clipHeight !== view.height) await page.setViewportSize(view);
    rec.ok = rec.status !== null && (expectStatus ? rec.status === expectStatus : rec.status < 400) && rec.expect_found && !rec.still_signing_in;
  } catch (err) {
    rec.error = String(err).slice(0, 400);
  }
  // The browser logs the document's own 4xx/5xx as a console error: that is
  // the status, already recorded, not a script error.
  const own = `Failed to load resource: the server responded with a status of ${rec.status}`;
  let dropped = false;
  rec.console_errors = bag.errors.filter((e) => {
    if (!dropped && rec.status >= 400 && e.startsWith(own)) {
      dropped = true;
      return false;
    }
    return true;
  }).slice(0, 15);
  rec.console_warnings = bag.warnings.slice(0, 8);
  rec.failed_requests = bag.failed.slice(0, 15);
  return rec;
}
