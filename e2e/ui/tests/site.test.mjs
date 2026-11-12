// A public website of marketing blocks, in real browsers: the proof app's /site pages in dark
// and light at 390, 768 and 1440 px with axe clean; their heads read by the rules a search
// engine and an answer engine apply; the mobile menu, the pricing switch, the questions and
// the theme toggle by pointer, by keyboard and with JavaScript off; the skip link; the
// responsive picture, its light and dark versions by the page's mode and the device's, below the
// phone's camera; labels in the text face; no layout shift; and the sign-up form checked on the server.
// Run with the proof app serving:
//
//   PORT=8147 cargo run --release -p ui-proof -- serve
//   SITE_URL=http://127.0.0.1:8147 UI_BROWSERS=chromium,firefox,webkit npm test -- tests/site.test.mjs
//
// SITE_URL is the server (default http://127.0.0.1:8147). UI_BROWSERS picks the engines
// (default all three). UI_SHOTS=<dir> saves a screenshot of every state. Without the server the
// suite is skipped.

import assert from "node:assert/strict";
import { mkdirSync, readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { after, afterEach, before, describe, test } from "node:test";
import { chromium, firefox, webkit } from "playwright";
import { launch } from "./support/launch.mjs";

const BASE = process.env.SITE_URL || "http://127.0.0.1:8147";
const ENGINES = { chromium, firefox, webkit };
const NAMES = (process.env.UI_BROWSERS || "chromium,firefox,webkit").split(",").filter((n) => ENGINES[n]);
const SHOTS = process.env.UI_SHOTS;
if (SHOTS) mkdirSync(SHOTS, { recursive: true });
const require = createRequire(import.meta.url);
const axeSource = readFileSync(require.resolve("axe-core/axe.min.js"), "utf8");

const reachable = await fetch(`${BASE}/site`).then((r) => r.ok, () => false);

/** Where the proof site says it lives, and its pages. */
const SITE = "https://kesar.example";
const PAGES = ["/site", "/site/pricing", "/site/privacy", "/site/contact"];
/** The AI crawlers the site welcomes by name. */
const CRAWLERS = ["GPTBot", "OAI-SearchBot", "ChatGPT-User", "ClaudeBot", "Claude-SearchBot", "Claude-User", "PerplexityBot", "Perplexity-User", "Google-Extended", "Applebot-Extended", "CCBot"];

async function settle(page) {
  // With JavaScript off the page runs no frame callbacks: wait out the longest transition instead.
  if (page.js === false) return page.waitForTimeout(350);
  await page.evaluate(() => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))));
  await page.evaluate(() => Promise.all(document.getAnimations().filter((a) => Number.isFinite(a.effect?.getComputedTiming().endTime)).map((a) => a.finished.catch(() => {}))));
}

async function axe(page) {
  await settle(page);
  await page.evaluate(axeSource);
  return page.evaluate(async () => {
    const out = await window.axe.run(document, { runOnly: ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa", "best-practice"] });
    return out.violations.map((v) => `${v.id}: ${v.nodes.slice(0, 3).map((n) => n.target.join(" ")).join(" | ")}`);
  });
}

/** What a page's head and body tell a search engine, read in the page. */
const readHead = (page) =>
  page.evaluate(() => {
    const meta = (key) => document.head.querySelector(`meta[name="${key}"],meta[property="${key}"]`)?.getAttribute("content") ?? "";
    const blocks = [...document.querySelectorAll('script[type="application/ld+json"]')].map((s) => {
      try {
        return { data: JSON.parse(s.textContent) };
      } catch (e) {
        return { error: String(e) };
      }
    });
    const headings = [...document.body.querySelectorAll("h1,h2,h3,h4,h5,h6")].map((h) => ({ level: Number(h.tagName[1]), text: h.textContent.trim() }));
    return {
      title: document.title,
      lang: document.documentElement.lang,
      description: meta("description"),
      robots: meta("robots"),
      viewport: !!document.head.querySelector('meta[name="viewport"]'),
      canonicals: [...document.head.querySelectorAll('link[rel="canonical"]')].map((l) => l.getAttribute("href")),
      og: Object.fromEntries(["og:title", "og:description", "og:url", "og:image", "og:image:alt", "og:image:width", "og:image:height", "og:site_name", "og:type", "og:locale", "twitter:card", "twitter:title", "twitter:description", "twitter:image", "twitter:image:alt"].map((k) => [k, meta(k)])),
      blocks,
      headings,
      images: [...document.body.querySelectorAll("img")].map((i) => ({ alt: i.getAttribute("alt"), width: i.getAttribute("width"), height: i.getAttribute("height") })),
      internal: [...document.body.querySelectorAll('a[href^="/"]')].filter((a) => !a.getAttribute("href").startsWith("//")).length,
      text: document.body.textContent.replace(/\s+/g, ""),
      h3s: [...document.body.querySelectorAll("h3")].map((h) => h.textContent.replace(/\s+/g, " ").trim()),
    };
  });

/** Every node of a page's structured data, nested typed nodes included. */
function nodes(blocks) {
  const out = [];
  const walk = (v) => {
    if (Array.isArray(v)) v.forEach(walk);
    else if (v && typeof v === "object") {
      if (v["@type"]) out.push(v);
      Object.values(v).forEach(walk);
    }
  };
  for (const b of blocks) walk(b.data?.["@graph"] ?? b.data);
  return out;
}

for (const name of NAMES) {
  describe(`site (${name})`, { skip: !reachable && `no proof app at ${BASE}` }, () => {
    let browser;
    before(async () => {
      browser = await launch(ENGINES[name]);
    });
    after(() => browser?.close());
    const contexts = [];
    afterEach(async () => {
      for (const context of contexts.splice(0)) await context.close().catch(() => {});
    });

    /** A page of the site; `page.done()` asserts no console error and no policy report. */
    async function open(path, { width = 1440, height = 900, mode = "dark", js = true } = {}) {
      const context = await browser.newContext({ viewport: { width, height }, javaScriptEnabled: js, colorScheme: mode === "light" ? "light" : "dark" });
      contexts.push(context);
      const page = await context.newPage();
      page.js = js;
      page.problems = [];
      page.on("console", (m) => {
        if (name === "webkit" && /Refused to apply a stylesheet/.test(m.text())) return;
        if (m.type() === "error" && !/Failed to load resource/.test(m.text())) page.problems.push(m.text());
      });
      page.on("pageerror", (e) => page.problems.push("pageerror: " + e.message));
      if (js) {
        await page.addInitScript(() => {
          document.addEventListener("securitypolicyviolation", (e) => (window.__csp ||= []).push(e.violatedDirective + " " + e.blockedURI));
          // Every layout shift that no input caused, as Cumulative Layout Shift counts them.
          window.__shift = 0;
          new PerformanceObserver((list) => {
            for (const entry of list.getEntries()) if (!entry.hadRecentInput) window.__shift += entry.value;
          }).observe({ type: "layout-shift", buffered: true });
        });
      }
      const sep = path.includes("?") ? "&" : "?";
      await page.goto(`${BASE}${path}${mode ? `${sep}mode=${mode}` : ""}`, { waitUntil: "networkidle" });
      page.done = async () => {
        let csp = js ? await page.evaluate(() => window.__csp || []) : [];
        if (name === "webkit") csp = csp.filter((line) => !/^style-src-(elem|attr) inline$/.test(line));
        assert.deepEqual(csp, [], "no content security policy report");
        assert.deepEqual(page.problems, [], "no console error");
      };
      return page;
    }

    /**
     * Tab, as a keyboard user presses it. WebKit's Tab leaves links out unless the reader turns that
     * on, so there the page's own order is read and focus is moved along it after a key press.
     */
    const tab = async (page, shift = false) => {
      if (name !== "webkit") return page.keyboard.press(shift ? "Shift+Tab" : "Tab");
      await page.keyboard.press("Shift");
      await page.evaluate((step) => {
        const list = [...document.querySelectorAll("a[href],button:not([disabled]),input:not([disabled]):not([type=hidden]),select,textarea,summary,[tabindex]:not([tabindex='-1'])")].filter((el) => el.getClientRects().length && getComputedStyle(el).visibility !== "hidden");
        const at = list.indexOf(document.activeElement);
        list[at === -1 ? 0 : at + step]?.focus();
      }, shift ? -1 : 1);
    };

    const shot = async (page, label) => {
      if (SHOTS) await page.screenshot({ path: `${SHOTS}/${name}-${label}.png`, fullPage: false });
    };

    // ---- Every page: clean, fitting and axe clean in both modes at three widths ----------------
    for (const path of PAGES) {
      for (const width of [390, 768, 1440]) {
        test(`${path} at ${width} px: no error, no policy report, no sideways scroll, axe clean in dark and light`, { timeout: 120000 }, async () => {
          for (const mode of ["dark", "light"]) {
            const page = await open(path, { width, mode });
            const wide = await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth);
            assert.ok(wide <= 0, `${mode}: ${wide} px of sideways scroll`);
            assert.equal(await page.evaluate(() => document.documentElement.dataset.mode), mode);
            assert.deepEqual(await axe(page), [], `${mode}: axe`);
            await page.done();
          }
        });
      }
    }

    // ---- Search and answer engines -------------------------------------------------------------
    test("every page passes the site's search rules: title, description, canonical, cards, headings, alt text, links, structured data", async () => {
      const titles = new Set();
      const descriptions = new Set();
      for (const path of PAGES) {
        const page = await open(path, { mode: null });
        const head = await readHead(page);
        const where = (m) => `${path}: ${m}`;
        assert.ok(head.title.length >= 30 && head.title.length <= 60, where(`title of ${head.title.length}: ${head.title}`));
        assert.ok(head.description.length >= 70 && head.description.length <= 160, where(`description of ${head.description.length}`));
        assert.ok(!titles.has(head.title) && !descriptions.has(head.description), where("a title or description used twice"));
        titles.add(head.title);
        descriptions.add(head.description);
        assert.deepEqual(head.canonicals, [`${SITE}${path}`], where("one canonical, the page's own"));
        for (const [key, value] of Object.entries(head.og)) assert.ok(value, where(`no ${key}`));
        assert.equal(head.og["og:url"], `${SITE}${path}`, where("og:url"));
        assert.ok(head.og["og:image"].startsWith("https://"), where("og:image absolute"));
        assert.equal(head.og["twitter:card"], "summary_large_image", where("twitter:card"));
        assert.equal(head.lang, "en", where("lang"));
        assert.ok(head.viewport, where("viewport"));
        assert.match(head.robots, /max-image-preview:large/, where("robots"));
        assert.equal(head.headings.filter((h) => h.level === 1).length, 1, where("one h1"));
        for (let i = 1; i < head.headings.length; i++) {
          assert.ok(head.headings[i].level - head.headings[i - 1].level <= 1, where(`h${head.headings[i - 1].level} then h${head.headings[i].level} (${head.headings[i].text})`));
        }
        for (const img of head.images) assert.ok(img.alt !== null && img.width && img.height, where(`an image without alt or size`));
        assert.ok(head.internal >= 3, where(`${head.internal} internal links`));
        // Structured data: parses, schema.org, the site graph, the page and its breadcrumb.
        assert.ok(head.blocks.length >= 1 && head.blocks.every((b) => b.data && b.data["@context"] === "https://schema.org"), where("JSON-LD parses"));
        const all = nodes(head.blocks);
        const types = new Set(all.map((n) => n["@type"]));
        for (const need of ["Organization", "WebSite", "BreadcrumbList"]) assert.ok(types.has(need), where(`no ${need}`));
        assert.ok(["WebPage", "ItemPage", "ContactPage", "AboutPage", "CollectionPage"].some((t) => types.has(t)), where("no page node"));
        const ids = new Set(all.filter((n) => n["@id"]).map((n) => n["@id"]));
        const refs = [];
        JSON.stringify(head.blocks, (k, v) => (v && typeof v === "object" && !Array.isArray(v) && Object.keys(v).length === 1 && v["@id"] ? (refs.push(v["@id"]), v) : v));
        for (const ref of refs) assert.ok(ids.has(ref), where(`a reference to ${ref} that no node is`));
        const crumbs = all.find((n) => n["@type"] === "BreadcrumbList").itemListElement;
        crumbs.forEach((step, i) => assert.equal(step.position, i + 1, where("breadcrumb positions")));
        assert.equal(crumbs.at(-1).item, `${SITE}${path}`, where("the breadcrumb ends at the page"));
        assert.equal(crumbs[0].item, `${SITE}/site`, where("the breadcrumb starts at home"));
        await page.done();
      }
    });

    test("every question is an h3 and its visible answer is the structured data's, 40 to 80 words", async () => {
      for (const path of ["/site", "/site/pricing"]) {
        const page = await open(path, { mode: null });
        const head = await readHead(page);
        const faq = nodes(head.blocks).filter((n) => n["@type"] === "FAQPage");
        assert.equal(faq.length, 1, `${path}: one FAQPage`);
        for (const item of faq[0].mainEntity) {
          const words = item.acceptedAnswer.text.split(/\s+/).filter(Boolean).length;
          assert.ok(words >= 40 && words <= 80, `${item.name}: ${words} words`);
          assert.ok(item.name.endsWith("?"), item.name);
          assert.ok(head.h3s.includes(item.name), `${item.name} is an h3`);
          assert.ok(head.text.includes(item.acceptedAnswer.text.replace(/\s+/g, "")), `${item.name}: the page says the same`);
        }
        await page.done();
      }
    });

    test("the sitemap lists every page with a date, and robots.txt points at it and welcomes AI crawlers", async () => {
      const sitemap = await fetch(`${BASE}/site/sitemap.xml`);
      assert.match(sitemap.headers.get("content-type"), /application\/xml/);
      const xml = await sitemap.text();
      const entries = [...xml.matchAll(/<url><loc>([^<]+)<\/loc><lastmod>([^<]+)<\/lastmod>/g)].map((m) => [m[1], m[2]]);
      assert.deepEqual(entries.map((e) => e[0]).sort(), PAGES.map((p) => SITE + p).sort());
      for (const [, date] of entries) assert.ok(Date.parse(date) <= Date.now() + 86400000, date);
      const robots = await (await fetch(`${BASE}/site/robots.txt`)).text();
      assert.match(robots, /^User-agent: \*\nAllow: \/\n/);
      assert.ok(robots.includes(`Sitemap: ${SITE}/site/sitemap.xml`));
      for (const crawler of CRAWLERS) assert.ok(robots.includes(`User-agent: ${crawler}\nAllow: /\n`), crawler);
    });

    test("a page of blocks loads no script: only the toolkit's sheet, the marketing sheet and the form's runtime", async () => {
      const html = await (await fetch(`${BASE}/site/privacy`)).text();
      assert.ok(!html.includes('<script type="module"'), "the privacy page has no script");
      assert.match(html, /\/_navree\/ui\/marketing-[0-9a-f]{12}\.css/);
      const res = await fetch(`${BASE}/site/privacy`);
      assert.match(res.headers.get("content-security-policy"), /script-src 'self'/);
      assert.ok(!/unsafe|nonce/.test(res.headers.get("content-security-policy")));
    });

    // ---- The skip link and landmarks -----------------------------------------------------------
    for (const js of [true, false]) {
      test(`the skip link is the first stop and moves to the content${js ? "" : " (JavaScript off)"}`, async () => {
        const page = await open("/site", { width: 390, js });
        await tab(page);
        const skip = page.locator("[data-slot=skip-link]");
        assert.ok(await skip.evaluate((el) => el === document.activeElement), "the skip link has focus");
        await settle(page);
        const box = await skip.boundingBox();
        assert.ok(box && box.y >= 0 && box.y < 100, `the skip link shows itself: ${JSON.stringify(box)}`);
        await shot(page, `skip-${js ? "js" : "nojs"}`);
        await page.keyboard.press("Enter");
        await page.waitForFunction(() => location.hash === "#main");
        assert.equal(await page.evaluate(() => document.activeElement?.id), "main");
        assert.equal(await page.locator("header").count() >= 1 && (await page.locator("main").count()), 1);
        if (js) await page.done();
      });
    }

    // ---- The mobile menu -----------------------------------------------------------------------
    for (const js of [true, false]) {
      test(`the phone menu opens and closes by touch and keys, and its links work${js ? "" : " (JavaScript off)"}`, async () => {
        const page = await open("/site", { width: 390, height: 844, js });
        const menu = page.locator("[data-slot=site-menu]");
        const button = page.locator("[data-slot=site-menu-button]");
        const links = page.locator("[data-slot=site-menu-nav] a");
        assert.ok(!(await page.locator("[data-slot=site-nav]").isVisible()), "the wide nav is folded away");
        assert.ok(await button.isVisible());
        assert.equal(await button.evaluate((el) => el.textContent.trim()), "Menu", "the button is named Menu");
        assert.ok(!(await links.first().isVisible()), "closed at first");
        await button.click();
        assert.ok(await menu.evaluate((d) => d.open));
        assert.ok(await links.first().isVisible(), "open after a tap");
        await shot(page, `menu-open-${js ? "js" : "nojs"}`);
        // No sideways scroll and nothing off screen while open.
        const panel = await page.locator("[data-slot=site-menu-panel]").boundingBox();
        assert.ok(panel.x >= 0 && panel.x + panel.width <= 390.5, JSON.stringify(panel));
        assert.ok(await page.locator("[data-slot=site-menu-actions] a").last().isVisible(), "the calls to action are in the menu");
        await button.click();
        assert.ok(!(await links.first().isVisible()), "closed after a second tap");
        // By keys: the button is a tab stop, Enter opens it, Tab reaches its first link.
        await button.focus();
        await page.keyboard.press("Enter");
        assert.ok(await links.first().isVisible(), "open after Enter");
        await tab(page);
        assert.equal(await page.locator(":focus").textContent(), "Features");
        await tab(page, true);
        await page.keyboard.press(" ");
        assert.ok(!(await links.first().isVisible()), "closed after Space");
        // A link leaves for its page.
        await button.click();
        await page.locator("[data-slot=site-menu-nav] a", { hasText: "Pricing" }).click();
        await page.waitForURL(/\/site\/pricing/);
        assert.equal(await page.locator("[data-slot=site-menu-nav] a[aria-current=page]").textContent(), "Pricing");
        if (js) await page.done();
      });
    }

    test("at 1440 px the links are in the bar and the menu button is gone", async () => {
      const page = await open("/site", { width: 1440 });
      assert.ok(await page.locator("[data-slot=site-nav]").isVisible());
      assert.ok(!(await page.locator("[data-slot=site-menu-button]").isVisible()));
      // The header stays at the top as the page scrolls.
      await page.evaluate(() => scrollTo({ top: 2400, behavior: "instant" }));
      await page.waitForFunction(() => scrollY > 1000);
      const top = await page.locator("[data-slot=site-header]").evaluate((h) => h.getBoundingClientRect().top);
      assert.equal(top, 0);
      await page.done();
    });

    // ---- The pricing switch --------------------------------------------------------------------
    for (const js of [true, false]) {
      test(`the pricing switch shows monthly or yearly prices by pointer and by keys${js ? "" : " (JavaScript off)"}`, async () => {
        const page = await open("/site/pricing", { width: 390, height: 844, mode: "light", js });
        const shown = () =>
          page.evaluate(() =>
            [...document.querySelectorAll("[data-slot=plan-price]")]
              .filter((p) => p.checkVisibility())
              .map((p) => p.querySelector("[data-slot=plan-figure]").textContent),
          );
        assert.deepEqual(await shown(), ["₹0", "₹299", "₹499"]);
        const toggle = page.locator("[data-slot=pricing-cycle]");
        const before = await toggle.boundingBox();
        await page.locator("[data-slot=pricing-choice]", { hasText: "Yearly" }).click();
        assert.deepEqual(await shown(), ["₹0", "₹2,990", "₹4,990"]);
        assert.ok(await page.getByText("₹249 a month, billed once a year").isVisible());
        // Each plan's call to action follows the period.
        const plus = page.locator("[data-slot=plan]").filter({ has: page.locator("h3", { hasText: /^Plus$/ }) }).locator("[data-slot=plan-action]:visible a");
        assert.match(await plus.getAttribute("href"), /billing=yearly/);
        const after = await toggle.boundingBox();
        assert.deepEqual(after, before, "the switch does not move");
        await shot(page, `pricing-yearly-${js ? "js" : "nojs"}`);
        // By keys: the arrows move between the two choices.
        await page.locator("input[value=yearly]").focus();
        await page.keyboard.press("ArrowLeft");
        assert.ok(await page.locator("input[value=monthly]").isChecked());
        assert.deepEqual(await shown(), ["₹0", "₹299", "₹499"]);
        // The focused choice shows a ring. WebKit does not match a radio focused by its arrow keys as
        // `:focus-visible`, so it is checked in the other two.
        if (name !== "webkit") {
          const ring = await page.locator("[data-slot=pricing-choice]").first().evaluate((l) => getComputedStyle(l).outlineStyle);
          assert.equal(ring, "solid");
        }
        if (js) await page.done();
      });
    }

    test("a link that asks for yearly billing draws the yearly prices first, and a hidden price is not read out", async () => {
      const page = await open("/site/pricing?billing=yearly", { width: 1440 });
      assert.ok(await page.locator("input[value=yearly]").isChecked());
      assert.ok(await page.getByText("₹2,990").isVisible());
      const snapshot = await page.locator("[data-slot=pricing-plans]").ariaSnapshot();
      assert.ok(snapshot.includes("₹2,990") && !snapshot.includes("₹299 "), snapshot);
      await page.done();
    });

    // ---- The questions -------------------------------------------------------------------------
    for (const js of [true, false]) {
      test(`a question opens to its answer, one at a time, by pointer and by keys${js ? "" : " (JavaScript off)"}`, async () => {
        const page = await open("/site", { width: 390, height: 844, js });
        const items = page.locator("#questions [data-slot=faq-item]");
        const answer = (i) => items.nth(i).locator("[data-slot=faq-answer]");
        assert.equal(await items.count(), 6);
        assert.ok(!(await answer(0).isVisible()));
        await items.nth(0).locator("summary").click();
        await settle(page);
        assert.ok(await answer(0).isVisible(), "open after a tap");
        assert.match(await answer(0).textContent(), /Health Connect/);
        await shot(page, `faq-open-${js ? "js" : "nojs"}`);
        await items.nth(1).locator("summary").focus();
        await page.keyboard.press("Enter");
        await settle(page);
        assert.ok(await answer(1).isVisible(), "open after Enter");
        assert.ok(!(await items.nth(0).evaluate((d) => d.open)), "the first closed: one at a time");
        await page.keyboard.press(" ");
        await settle(page);
        assert.ok(!(await items.nth(1).evaluate((d) => d.open)), "closed after Space");
        // The question is a heading inside its button, so a reader can jump to it.
        assert.equal(await items.nth(0).locator("summary h3").count(), 1);
        if (js) await page.done();
      });
    }

    // ---- The theme toggle ----------------------------------------------------------------------
    for (const js of [true, false]) {
      test(`the footer's toggle sets light, dark or the device's mode and comes back${js ? "" : " (JavaScript off)"}`, async () => {
        const page = await open("/site/privacy", { width: 768, js, mode: null });
        assert.equal(await page.locator("html").getAttribute("data-mode"), "system");
        await Promise.all([page.waitForNavigation(), page.locator("[data-slot=theme-toggle-button][value=dark]").click()]);
        assert.equal(await page.locator("html").getAttribute("data-mode"), "dark");
        assert.equal(await page.locator("html").getAttribute("data-theme"), "sunset", "the site keeps its own theme");
        assert.match(page.url(), /\/site\/privacy$/);
        assert.equal(await page.locator("[data-slot=theme-toggle-button][value=dark]").getAttribute("aria-pressed"), "true");
        const ink = await page.locator("[data-slot=site]").evaluate((el) => getComputedStyle(el).backgroundColor);
        await Promise.all([page.waitForNavigation(), page.locator("[data-slot=theme-toggle-button][value=light]").click()]);
        assert.equal(await page.locator("html").getAttribute("data-mode"), "light");
        const paper = await page.locator("[data-slot=site]").evaluate((el) => getComputedStyle(el).backgroundColor);
        assert.notEqual(ink, paper, "the page's background follows the mode");
        // The choice holds on the next page.
        await page.goto(`${BASE}/site/contact`);
        assert.equal(await page.locator("html").getAttribute("data-mode"), "light");
        if (js) await page.done();
      });
    }

    // ---- The picture and layout shift ----------------------------------------------------------
    test("the picture loads lazily in the best format the browser takes, keeps its room, and nothing shifts", async () => {
      for (const width of [390, 1440]) {
        const page = await open("/site", { width, height: 844 });
        const img = page.locator("[data-slot=phone-view] img");
        assert.equal(await img.getAttribute("loading"), "lazy");
        const room = await img.boundingBox();
        await img.scrollIntoViewIfNeeded();
        await page.waitForFunction(() => document.querySelector("[data-slot=phone-view] img").complete);
        const loaded = await img.evaluate((i) => ({ src: i.currentSrc, natural: i.naturalWidth }));
        assert.ok(loaded.natural > 0, "it loaded");
        assert.match(loaded.src, name === "chromium" || name === "firefox" ? /\.avif$/ : /\.(avif|webp|png)$/);
        const box = await img.boundingBox();
        assert.equal(Math.round(box.height), Math.round(room.height), "it kept the room it held before it loaded");
        await page.evaluate(async () => {
          for (let y = 0; y < document.body.scrollHeight; y += 500) {
            scrollTo(0, y);
            await new Promise((r) => setTimeout(r, 30));
          }
        });
        if (name === "chromium") {
          const shift = await page.evaluate(() => window.__shift);
          assert.ok(shift < 0.01, `${width} px: layout shift ${shift}`);
        }
        await page.done();
      }
    });

    // ---- The picture in light and dark, under the camera ----------------------------------------
    test("the phone's screenshot follows the page's mode, chosen or the device's, and starts below the camera", async () => {
      const loaded = async (page) => {
        const img = page.locator("[data-slot=phone-view] img");
        await img.scrollIntoViewIfNeeded();
        await page.waitForFunction(() => document.querySelector("[data-slot=phone-view] img").complete);
        return img.evaluate((i) => i.currentSrc);
      };
      // The reader chose a mode on the site: that version alone is in the page.
      for (const mode of ["light", "dark"]) {
        const page = await open("/site", { width: 1440, mode });
        const src = await loaded(page);
        assert.equal(/sleep-light-/.test(src), mode === "light", `${mode}: ${src}`);
        const other = mode === "light" ? "/site/img/sleep-780" : "/site/img/sleep-light-";
        assert.equal(await page.locator(`[data-slot=phone-view] [srcset*="${other}"]`).count(), 0, `${mode}: only one version is drawn`);
        await shot(page, `phone-picture-${mode}`);
        await page.done();
      }
      // The site's own mode is the device's: the device picks, with no script.
      for (const scheme of ["light", "dark"]) {
        const context = await browser.newContext({ viewport: { width: 1440, height: 900 }, colorScheme: scheme, javaScriptEnabled: false });
        contexts.push(context);
        const page = await context.newPage();
        await page.goto(`${BASE}/site`, { waitUntil: "networkidle" });
        assert.equal(await page.locator("html").getAttribute("data-mode"), "system");
        const src = await loaded(page);
        assert.equal(/sleep-light-/.test(src), scheme === "light", `system on a ${scheme} device: ${src}`);
      }
      // The camera stands over the strip above the screenshot, never over the screenshot.
      const page = await open("/site", { width: 1440, mode: "light" });
      await loaded(page);
      const { camera, picture } = await page.evaluate(() => {
        const img = document.querySelector("[data-slot=phone-view] img");
        const screen = img.closest("[data-slot=phone-screen]");
        return {
          camera: screen.querySelector("[data-slot=phone-camera]").getBoundingClientRect().bottom,
          picture: img.getBoundingClientRect().top,
        };
      });
      assert.ok(picture >= camera, `the picture starts at ${picture}, below the camera's ${camera}`);
      await page.done();
    });

    // ---- Labels in the text face ---------------------------------------------------------------
    test("an eyebrow and the legal page's date are set in the site's text face, not its code face", async () => {
      const page = await open("/site/privacy", { width: 1440 });
      const faces = await page.evaluate(() => ({
        text: getComputedStyle(document.body).fontFamily,
        eyebrow: getComputedStyle(document.querySelector("[data-slot=eyebrow]") || document.body).fontFamily,
        updated: getComputedStyle(document.querySelector("[data-slot=legal-updated]")).fontFamily,
        mono: getComputedStyle(document.documentElement).getPropertyValue("--font-mono").trim(),
      }));
      assert.equal(faces.updated, faces.text, JSON.stringify(faces));
      assert.equal(faces.eyebrow, faces.text, JSON.stringify(faces));
      assert.notEqual(faces.updated, faces.mono);
      await page.done();
    });

    // ---- The sign-up form ----------------------------------------------------------------------
    for (const js of [true, false]) {
      test(`the sign-up form is checked on the server and says what is wrong at the field${js ? "" : " (JavaScript off)"}`, async () => {
        const page = await open("/site", { width: 390, height: 844, js });
        const field = page.locator("#letter input[name=email]");
        // An empty address passes the browser's own check and is refused by the server's.
        await field.fill("");
        await page.locator("#letter button[type=submit]").click();
        await page.locator("#letter [data-slot=field-error]").waitFor();
        assert.match(await page.locator("#letter [data-slot=field-error]").textContent(), /Enter an email address/);
        await shot(page, `signup-error-${js ? "js" : "nojs"}`);
        assert.equal(await field.getAttribute("aria-invalid"), "true");
        await field.fill("gurpreet@example.in");
        await page.locator("#letter button[type=submit]").click();
        await page.waitForURL(/subscribed=1/);
        assert.ok(await page.getByText("Thank you. The next letter").isVisible());
        if (js) await page.done();
      });
    }
  });
}
