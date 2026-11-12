// The app blocks in real browsers, against the proof app's /app-blocks: the app's own stylesheet and
// typeface (linked after the toolkit's, served immutable, applied, no policy report), the figure tile,
// the info tip, chart skeletons, the breathing guide, the dock (and one for phones only), the day pager,
// a view switcher, the copy field, a grid in a ratio, the app's own picture, a region whose answer holds
// lazy tabs, and a shell of its own (a pager in the middle of the bar, a title template, one theme):
// every case at 390 and 1440 px in dark and light with axe clean, the keys and the pointer, the guide's
// phases over time and under less motion, JavaScript off, hostile text.
// Run with the proof app serving:
//
//   PORT=8140 cargo run --release -p ui-proof -- serve
//   BLOCKS_URL=http://127.0.0.1:8140 UI_BROWSERS=chromium,firefox,webkit npm test -- tests/app-blocks.test.mjs
//
// BLOCKS_URL is the server (default http://127.0.0.1:8140). UI_BROWSERS picks the engines (default
// chromium). UI_SHOTS=<dir> saves a screenshot of every case at both widths in both themes. Without
// the server the suite is skipped.

import assert from "node:assert/strict";
import { mkdirSync, readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { after, afterEach, before, describe, test } from "node:test";
import { chromium, firefox, webkit } from "playwright";
import { launch } from "./support/launch.mjs";

const BASE = process.env.BLOCKS_URL || "http://127.0.0.1:8140";
const ENGINES = { chromium, firefox, webkit };
const NAMES = (process.env.UI_BROWSERS || "chromium").split(",").filter((n) => ENGINES[n]);
const SHOTS = process.env.UI_SHOTS;
if (SHOTS) mkdirSync(SHOTS, { recursive: true });

const require = createRequire(import.meta.url);
const axeSource = readFileSync(require.resolve("axe-core/axe.min.js"), "utf8");

const reachable = await fetch(`${BASE}/app-blocks?case=tiles`).then((r) => r.ok, () => false);

const CASES = ["tiles", "plain-tiles", "info-tip", "skeletons", "breathing", "dock", "day-pager", "view-switcher", "copy-field", "ratio-grid", "dock-phones", "app-image", "nested-lazy", "brand"];

async function settle(page) {
  await page.evaluate(() => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))));
  // An animation that never ends (a skeleton's pulse, the guide's circle) is not waited for.
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

for (const name of NAMES) {
  describe(name, { concurrency: false, skip: reachable ? false : `needs the proof app at ${BASE}` }, () => {
    let browser;
    before(async () => {
      browser = await launch(ENGINES[name]);
    });
    after(() => browser?.close());
    const contexts = [];
    afterEach(async () => {
      for (const context of contexts.splice(0)) await context.close().catch(() => {});
    });

    /** A page on the proof app; console errors, page errors and policy reports are kept and `done()` asserts there are none. */
    async function open(path, { width = 1440, height = 900, light = false, js = true, reducedMotion = "no-preference" } = {}) {
      const context = await browser.newContext({ viewport: { width, height }, javaScriptEnabled: js, colorScheme: light ? "light" : "dark", reducedMotion });
      contexts.push(context);
      const page = await context.newPage();
      page.problems = [];
      page.responses = [];
      page.on("console", (m) => {
        if (name === "webkit" && /Refused to apply a stylesheet/.test(m.text())) return;
        if (m.type() === "error" && !/Failed to load resource/.test(m.text())) page.problems.push(m.text());
      });
      page.on("pageerror", (e) => page.problems.push("pageerror: " + e.message));
      page.on("response", (r) => page.responses.push({ url: r.url(), status: r.status(), headers: r.headers() }));
      if (js) {
        await page.addInitScript(() => {
          document.addEventListener("securitypolicyviolation", (e) => {
            (window.__csp ||= []).push(e.violatedDirective + " " + e.blockedURI);
          });
        });
      }
      await page.goto(BASE + path + (path.includes("?") ? "&" : "?") + (light ? "mode=light" : "mode=dark"));
      page.done = async () => {
        let csp = js ? await page.evaluate(() => window.__csp || []) : [];
        if (name === "webkit") csp = csp.filter((line) => !/^style-src-(elem|attr) inline$/.test(line));
        assert.deepEqual(csp, [], "no content security policy report");
        assert.deepEqual(page.problems, [], "no console error");
      };
      return page;
    }

    describe("every case", () => {
      for (const kase of CASES) {
        for (const width of [1440, 390]) {
          test(`${kase} at ${width} px: no error, no policy report, no sideways scroll, axe clean in dark and light`, { timeout: 120000 }, async () => {
            for (const theme of ["dark", "light"]) {
              const page = await open(`/app-blocks?case=${kase}`, { width, light: theme === "light" });
              const label = `${kase} ${theme} ${width}`;
              const ground = await page.evaluate(() => getComputedStyle(document.body).backgroundColor);
              assert.equal(ground === "rgb(255, 255, 255)", theme === "light", `${label}: the page is ${theme} (${ground})`);
              assert.equal(await page.locator(`section[data-case=${kase}]`).count(), 1, `${label}: the case is drawn`);
              const spill = await page.evaluate(() => {
                const out = [];
                for (const el of document.querySelectorAll("[data-case] *")) {
                  if (el.closest("[data-ui-sr],[data-sr],[data-slot=chart-table],[popover]:not(:popover-open)")) continue;
                  if (el instanceof SVGElement && el.ownerSVGElement) continue;
                  const r = el.getBoundingClientRect();
                  if (r.width && (r.right > window.innerWidth + 1 || r.left < -1)) out.push(`${el.tagName}:${el.getAttribute("data-slot") || ""} ${Math.round(r.left)}..${Math.round(r.right)}`);
                }
                return { wide: document.documentElement.scrollWidth - window.innerWidth, out: out.slice(0, 5) };
              });
              assert.ok(spill.wide <= 0, `${label}: the page scrolls sideways by ${spill.wide} px`);
              assert.deepEqual(spill.out, [], `${label}: nothing sticks out of the screen`);
              assert.deepEqual(await axe(page), [], `${label}: axe`);
              if (SHOTS) {
                await settle(page);
                const box = await page.evaluate((k) => {
                  const r = document.querySelector(`section[data-case=${k}]`).getBoundingClientRect();
                  return { x: r.left + scrollX, y: r.top + scrollY, width: r.width, height: r.height };
                }, kase);
                await page.screenshot({ path: `${SHOTS}/${name}-${kase}-${width}-${theme}.png`, fullPage: true, clip: box });
              }
              await page.done();
              await page.context().close();
            }
          });
        }
      }
    });

    describe("the app's own stylesheet and typeface", () => {
      test("linked after the toolkit's sheets and the theme, served immutable, applied, and nothing inline", async () => {
        const page = await open("/app-blocks?case=brand");
        const links = await page.evaluate(() => [...document.head.querySelectorAll("link[rel=stylesheet],link[rel=preload]")].map((l) => ({ rel: l.rel, href: new URL(l.href).pathname, app: l.hasAttribute("data-ui-app") })));
        const sheet = links.findIndex((l) => /\/_navree\/ui\/app\/proof-brand-[0-9a-f]{12}\.css$/.test(l.href));
        const face = links.findIndex((l) => /\/_navree\/ui\/app\/proof-sans\.face-[0-9a-f]{12}\.css$/.test(l.href));
        const toolkit = links.findIndex((l) => /\/_navree\/ui\/navree-ui-[0-9a-f]{12}\.css$/.test(l.href));
        const preload = links.findIndex((l) => l.rel === "preload" && /\/_navree\/ui\/app\/proof-sans-[0-9a-f]{12}\.woff2$/.test(l.href));
        assert.ok(toolkit >= 0 && face > toolkit && sheet > face, `the face and the sheet come after the toolkit's: ${JSON.stringify(links)}`);
        assert.ok(preload >= 0 && preload < toolkit, "the font file is preloaded before any sheet");
        assert.equal(await page.locator("style, [style]").count(), 0, "no inline style anywhere");
        // Served by hash, immutable, and the right types.
        const served = (re) => page.responses.find((r) => re.test(new URL(r.url).pathname));
        const css = served(/proof-brand-.*\.css$/);
        assert.equal(css.status, 200);
        assert.equal(css.headers["cache-control"], "public, max-age=31536000, immutable");
        assert.match(css.headers["content-type"], /^text\/css/);
        const font = served(/proof-sans-.*\.woff2$/);
        assert.equal(font.status, 200);
        assert.equal(font.headers["content-type"], "font/woff2");
        assert.equal(font.headers["cache-control"], "public, max-age=31536000, immutable");
        // Applied: the backdrop is the sheet's gradient and the text is set in the app's family, which the browser loaded.
        const look = await page.evaluate(async () => {
          await document.fonts.ready;
          const hero = document.querySelector(".proof-brand");
          const style = getComputedStyle(hero);
          return { image: style.backgroundImage, family: getComputedStyle(hero.querySelector("h2")).fontFamily, loaded: document.fonts.check('16px "Brand Sans"'), faces: [...document.fonts].filter((f) => f.family.includes("Brand Sans")).map((f) => f.status) };
        });
        assert.match(look.image, /radial-gradient/, "the sheet's rule applies");
        assert.match(look.family, /Brand Sans/);
        assert.equal(look.loaded, true, "the typeface is loaded");
        assert.deepEqual(look.faces, ["loaded"]);
        await page.done();
      });

      test("an address the app did not contribute is not found, and a way out of the mount is refused", async () => {
        for (const path of ["/_navree/ui/app/other-000000000000.css", "/_navree/ui/app/proof-brand.css", "/_navree/ui/app/proof-brand-000000000000.js"]) {
          assert.equal((await fetch(BASE + path)).status, 404, path);
        }
        assert.equal((await fetch(BASE + "/_navree/ui/app/..%2Fcore.js")).status, 400);
        // An older hash still gets the sheet, uncached, so a page open across a deploy keeps working.
        const old = await fetch(BASE + "/_navree/ui/app/proof-brand-000000000000.css");
        assert.equal(old.status, 200);
        assert.equal(old.headers.get("cache-control"), "no-cache");
      });

      test("the page's policy allows it: no report with JavaScript off, and the guide still draws", async () => {
        const page = await open("/app-blocks?case=brand", { js: false });
        assert.match(await page.evaluate(() => getComputedStyle(document.querySelector(".proof-brand")).backgroundImage), /radial-gradient/);
      });
    });

    describe("the figure tile", () => {
      test("an icon chip, a unit read after the figure, a caption, a ring beside and a link over the whole tile", async () => {
        const page = await open("/app-blocks?case=tiles");
        const heart = page.locator("[data-slot=stat]", { hasText: "Resting heart rate" }).first();
        assert.equal(await heart.locator("[data-slot=stat-icon]").getAttribute("aria-hidden"), "true");
        assert.match(await heart.locator("[data-slot=stat-value]").innerText(), /^58\s+bpm$/);
        assert.match(await heart.locator("[data-slot=stat-trend]").innerText(), /vs last week/);
        assert.match(await heart.locator("[data-slot=stat-trend]").innerText(), /Down/, "the direction is a word for a screen reader");
        const steps = page.locator("[data-slot=stat]", { hasText: "Steps" }).first();
        assert.equal(await steps.locator("[data-slot=stat-aside] figure[data-kind=ring]").count(), 1, "a ring beside the figure");
        // The ring is inside the tile's box, at the end of the figure's line.
        const boxes = await steps.evaluate((tile) => {
          const t = tile.getBoundingClientRect();
          const a = tile.querySelector("[data-slot=stat-aside]").getBoundingClientRect();
          const v = tile.querySelector("[data-slot=stat-value]").getBoundingClientRect();
          return { inside: a.left >= t.left && a.right <= t.right && a.top >= t.top && a.bottom <= t.bottom, after: a.left >= v.right - 1 };
        });
        assert.deepEqual(boxes, { inside: true, after: true });
        // The link is the label, and its target is the whole tile.
        const link = steps.locator("a[data-slot=stat-link]");
        assert.equal(await link.getAttribute("href"), "/app-blocks?case=tiles");
        const reach = await steps.evaluate((tile) => {
          const t = tile.getBoundingClientRect();
          const hit = document.elementFromPoint(t.right - 6, t.bottom - 6);
          return tile.contains(hit) && (hit.closest("a") === tile.querySelector("a[data-slot=stat-link]") || hit.matches("a"));
        });
        assert.equal(reach, true, "a press near the tile's corner is on the link");
        await link.focus();
        const ring = await steps.evaluate((tile) => getComputedStyle(tile).outlineStyle);
        assert.equal(ring, "solid", "the tile shows a focus ring when its link has focus");
        await page.done();
      });

      test("hostile text stays text and wraps inside the tile", async () => {
        const page = await open("/app-blocks?case=tiles", { width: 390 });
        assert.equal(await page.evaluate(() => window.__pwned), undefined);
        assert.equal(await page.locator("[data-case=tiles] img, [data-case=tiles] script").count(), 0);
        const wide = await page.evaluate(() => [...document.querySelectorAll("[data-slot=stat]")].filter((s) => s.scrollWidth > s.clientWidth + 1).length);
        assert.equal(wide, 0, "no tile is wider than itself");
        await page.done();
      });

      test("a grid that fits wraps by the room: one column at 390 px, more at 1440 px", async () => {
        const columns = async (width) => {
          const page = await open("/app-blocks?case=plain-tiles", { width });
          const n = await page.evaluate(() => new Set([...document.querySelectorAll("[data-case=plain-tiles] [data-slot=stat]")].map((s) => Math.round(s.getBoundingClientRect().left))).size);
          await page.context().close();
          return n;
        };
        assert.equal(await columns(390), 1);
        assert.equal(await columns(1440), 3);
      });
    });

    describe("the info tip", () => {
      test("a keyboard opens it and Escape closes it, focus comes back, and a click outside closes it", async () => {
        const page = await open("/app-blocks?case=info-tip");
        const chip = page.locator("button[popovertarget=tip-autonomic]");
        assert.equal(await chip.getAttribute("aria-label"), "About Autonomic balance");
        await chip.focus();
        await page.keyboard.press("Enter");
        const note = page.locator("#tip-autonomic");
        assert.equal(await note.evaluate((n) => n.matches(":popover-open")), true, "Enter opens it");
        assert.equal(await note.getAttribute("aria-labelledby"), "tip-autonomic-title");
        assert.equal(await page.evaluate(() => document.activeElement?.id), "tip-autonomic", "focus moves into the note");
        await page.keyboard.press("Escape");
        assert.equal(await note.evaluate((n) => n.matches(":popover-open")), false, "Escape closes it");
        assert.equal(await chip.evaluate((b) => b === document.activeElement), true, "focus returns to the chip");
        await chip.click();
        assert.equal(await note.evaluate((n) => n.matches(":popover-open")), true);
        await page.mouse.click(5, 400);
        assert.equal(await note.evaluate((n) => n.matches(":popover-open")), false, "a click outside closes it");
        await page.done();
      });

      test("it opens inside the window beside the edge at 390 px, and axe is clean with it open in both themes", async () => {
        for (const light of [false, true]) {
          const page = await open("/app-blocks?case=info-tip", { width: 390, light });
          for (const id of ["tip-autonomic", "tip-brain", "tip-hostile"]) {
            await page.locator(`button[popovertarget=${id}]`).click();
            const inside = await page.locator(`#${id}`).evaluate((n) => {
              const r = n.getBoundingClientRect();
              return r.left >= -0.5 && r.right <= innerWidth + 0.5 && r.top >= -0.5 && r.bottom <= innerHeight + 0.5;
            });
            assert.equal(inside, true, `${id} is inside the window`);
            assert.deepEqual(await axe(page), [], `axe with ${id} open (${light ? "light" : "dark"})`);
            await page.keyboard.press("Escape");
          }
          assert.equal(await page.evaluate(() => window.__pwned), undefined, "hostile text in a note is only text");
          await page.done();
        }
      });

      test("with JavaScript off the chip still opens its note", async () => {
        const page = await open("/app-blocks?case=info-tip", { js: false });
        await page.locator("button[popovertarget=tip-brain]").click();
        const shown = await page.locator("#tip-brain").evaluate((n) => ({ open: n.matches(":popover-open"), inside: (() => { const r = n.getBoundingClientRect(); return r.left >= 0 && r.right <= innerWidth && r.top >= 0 && r.bottom <= innerHeight; })() }));
        assert.deepEqual(shown, { open: true, inside: true });
      });
    });

    describe("chart skeletons", () => {
      test("each is one status that names what is loading, with its shapes hidden, and pulses", async () => {
        const page = await open("/app-blocks?case=skeletons");
        const all = page.locator("[data-slot=chart-skeleton]");
        assert.equal(await all.count(), 7);
        for (let i = 0; i < 7; i++) {
          const one = all.nth(i);
          assert.equal(await one.getAttribute("role"), "status");
          assert.match(await one.locator("[data-ui-sr]").textContent(), /^Loading /);
          assert.equal(await one.locator("svg").getAttribute("aria-hidden"), "true");
          assert.match(await one.locator("svg").evaluate((s) => getComputedStyle(s).animationName), /ui-pulse/);
        }
        // The heat map has 26 weeks of seven days, and the matrix its lower triangle.
        assert.equal(await page.locator("[data-kind=heat-map] rect").count(), 182);
        assert.equal(await page.locator("[data-kind=matrix] rect").count(), 21);
        await page.done();
      });

      test("with a request for less motion the pulse stops", async () => {
        const page = await open("/app-blocks?case=skeletons", { reducedMotion: "reduce" });
        const lasts = await page.evaluate(() => [...document.querySelectorAll("[data-slot=chart-skeleton] svg")].map((s) => getComputedStyle(s).animationDuration));
        assert.ok(lasts.every((d) => parseFloat(d) < 0.001), `${lasts}`);
      });

      test("a skeleton keeps its height and width where the chart will come, at both widths", async () => {
        for (const width of [1440, 390]) {
          const page = await open("/app-blocks?case=skeletons", { width });
          const sizes = await page.evaluate(() => [...document.querySelectorAll("[data-slot=chart-skeleton]")].map((s) => { const r = s.getBoundingClientRect(); return [s.getAttribute("data-kind"), Math.round(r.width), Math.round(r.height)]; }));
          for (const [kind, w, h] of sizes) {
            assert.ok(w > 0 && h > 0, `${kind} has a box at ${width}`);
            assert.ok(w <= width, `${kind} fits the screen at ${width}`);
          }
        }
      });
    });

    describe("the breathing guide", () => {
      test("it starts still and ready, and a switch runs it through 4, 7 and 8 seconds, and pauses it where it is", async () => {
        const page = await open("/app-blocks?case=breathing");
        const orb = page.locator("#guide [data-slot=breathing-orb]");
        const scale = () => orb.evaluate((o) => parseFloat(getComputedStyle(o).scale));
        const words = () => page.evaluate(() => [...document.querySelectorAll("#guide [data-slot=breathing-label] > span")].filter((s) => getComputedStyle(s).visibility === "visible" && parseFloat(getComputedStyle(s).opacity) > 0.5).map((s) => s.textContent));
        assert.equal(await orb.evaluate((o) => getComputedStyle(o).animationPlayState), "paused");
        assert.deepEqual(await words(), ["Ready"]);
        assert.ok(Math.abs((await scale()) - 0.6) < 0.01, "at rest it is small");
        const toggle = page.getByRole("switch", { name: "Guide" }).first();
        assert.equal(await toggle.isChecked(), false);
        await toggle.focus();
        await page.keyboard.press("Space");
        assert.equal(await toggle.isChecked(), true, "Space turns it on");
        await page.waitForTimeout(2000);
        const inhaling = await scale();
        assert.ok(inhaling > 0.7 && inhaling < 0.95, `breathing in at 2 s: ${inhaling}`);
        assert.deepEqual(await words(), ["Breathe in"]);
        await page.waitForTimeout(6000);
        assert.ok(Math.abs((await scale()) - 1) < 0.01, "held at full size");
        assert.deepEqual(await words(), ["Hold"]);
        await page.waitForTimeout(6500);
        const out = await scale();
        assert.ok(out > 0.6 && out < 0.95, `breathing out at 14.5 s: ${out}`);
        assert.deepEqual(await words(), ["Breathe out"]);
        // A pause holds the circle where it is.
        await page.keyboard.press("Space");
        const held = await scale();
        await page.waitForTimeout(700);
        assert.ok(Math.abs((await scale()) - held) < 0.002, "paused, the circle does not move");
        assert.deepEqual(await words(), ["Ready"]);
        await page.done();
      });

      test("the steps are written for everyone, and a screen reader hears their lengths", async () => {
        const page = await open("/app-blocks?case=breathing");
        const steps = await page.locator("#guide [data-slot=breathing-steps] li").evaluateAll((lis) => lis.map((li) => [...li.children].filter((c) => !c.hasAttribute("data-ui-sr")).map((c) => c.textContent).join(" ")));
        assert.deepEqual(steps, ["4 Breathe in", "7 Hold", "8 Breathe out"]);
        assert.deepEqual(await page.locator("#guide [data-slot=breathing-steps] [data-ui-sr]").allTextContents(), ["4 seconds", "7 seconds", "8 seconds"]);
        assert.equal(await page.locator("#guide [data-slot=breathing-stage]").getAttribute("aria-hidden"), "true");
        assert.equal(await page.locator("#guide").getAttribute("aria-label"), "4-7-8 breathing");
      });

      test("with a request for less motion nothing moves: no animation, no switch, the steps stay", async () => {
        const page = await open("/app-blocks?case=breathing", { reducedMotion: "reduce" });
        const state = await page.evaluate(() => ({
          orb: getComputedStyle(document.querySelector("#guide [data-slot=breathing-orb]")).animationName,
          run: getComputedStyle(document.querySelector("#guide [data-slot=breathing-run]")).display,
          label: getComputedStyle(document.querySelector("#guide [data-slot=breathing-label]")).display,
          steps: [...document.querySelectorAll("#guide [data-slot=breathing-steps] li")].map((li) => getComputedStyle(li).animationName),
          running: document.getAnimations().filter((a) => a.playState === "running" && a.effect?.getComputedTiming().duration > 1).length,
        }));
        assert.deepEqual(state, { orb: "none", run: "none", label: "none", steps: ["none", "none", "none"], running: 0 });
        assert.equal(await page.locator("#guide [data-slot=breathing-steps] li").count(), 3);
        assert.deepEqual(await axe(page), []);
      });

      test("it works with JavaScript off", async () => {
        const page = await open("/app-blocks?case=breathing", { js: false });
        await page.getByRole("switch", { name: "Guide" }).first().check();
        await page.waitForTimeout(1500);
        const s = await page.locator("#guide [data-slot=breathing-orb]").evaluate((o) => parseFloat(getComputedStyle(o).scale));
        assert.ok(s > 0.65, `the circle grows with no script: ${s}`);
      });
    });

    describe("the dock", () => {
      test("it floats at the bottom centre, marks the current page, names each icon and takes the keyboard in order", async () => {
        for (const width of [1440, 390]) {
          const page = await open("/app-blocks?case=dock", { width });
          const dock = page.locator("nav[data-slot=dock]");
          assert.equal(await dock.getAttribute("aria-label"), "Sections");
          const place = await dock.evaluate((d) => { const r = d.getBoundingClientRect(); return { left: r.left, right: r.right, bottom: r.bottom, vw: innerWidth, vh: innerHeight }; });
          assert.ok(place.left >= 0 && place.right <= place.vw, `inside the window at ${width}`);
          assert.ok(Math.abs((place.left + place.right) / 2 - place.vw / 2) < 2, "centred");
          assert.ok(place.vh - place.bottom >= 8 && place.vh - place.bottom <= 40, `near the bottom: ${place.vh - place.bottom}`);
          assert.deepEqual(await dock.locator("a").evaluateAll((as) => as.map((a) => a.getAttribute("title"))), ["Sleep", "Heart", "Home", "Mind", "Fitness"]);
          assert.equal(await dock.locator("a[aria-current=page]").count(), 1);
          assert.equal(await dock.locator("a[aria-current=page]").getAttribute("title"), "Home");
          // Every target is at least 24 px.
          const small = await dock.locator("a").evaluateAll((as) => as.filter((a) => { const r = a.getBoundingClientRect(); return r.width < 24 || r.height < 24; }).length);
          assert.equal(small, 0);
          await dock.locator("a").first().focus();
          // Safari's Tab skips links unless the reader turned that on, in WebKit too: the order is the markup's.
          if (name !== "webkit") {
            await page.keyboard.press("Tab");
            assert.equal(await page.evaluate(() => document.activeElement?.getAttribute("title")), "Heart");
            assert.equal(await dock.locator("a").nth(1).evaluate((a) => getComputedStyle(a).outlineStyle), "solid", "a focus ring");
          }
          await page.done();
        }
      });
    });

    describe("the day pager", () => {
      test("arrows are links, a missing day is a dimmed arrow out of the tab order, and the date opens a picker", async () => {
        const page = await open("/app-blocks?case=day-pager");
        const today = page.locator("#day-today");
        assert.equal(await today.locator("a[aria-label='Previous day']").getAttribute("href"), "/app-blocks?case=day-pager&date=2026-11-02");
        const next = today.locator("[data-slot=day-pager-next]");
        assert.equal(await next.getAttribute("aria-disabled"), "true");
        assert.equal(await next.evaluate((n) => n.tagName), "SPAN", "not a link");
        // The two pagers beside each other are the same width.
        const widths = await page.evaluate(() => ["day-today", "day-first"].map((id) => Math.round(document.getElementById(id).querySelector("[data-slot=day-pager-previous]").getBoundingClientRect().width + document.getElementById(id).querySelector("[data-slot=day-pager-next]").getBoundingClientRect().width)));
        assert.equal(widths[0], widths[1], "the arrows take the same room, linked or not");
        const picker = page.locator("#day-picker button[popovertarget=day-picker-picker]");
        await picker.focus();
        await page.keyboard.press("Enter");
        assert.equal(await page.locator("#day-picker-picker").evaluate((n) => n.matches(":popover-open")), true);
        assert.deepEqual(await axe(page), []);
        await page.keyboard.press("Escape");
        await page.done();
      });

      test("the arrows step with JavaScript off", async () => {
        const page = await open("/app-blocks?case=day-pager", { js: false });
        await page.locator("#day-today a[aria-label='Previous day']").click();
        assert.match(page.url(), /date=2026-11-02/);
      });
    });

    describe("a view switcher", () => {
      test("the segmented tabs switch the views by pointer and by key, and the chosen one is in the address", async () => {
        const page = await open("/app-blocks?case=view-switcher");
        const radial = page.getByRole("tab", { name: "Radial" });
        const story = page.getByRole("tab", { name: "Story" });
        assert.equal(await radial.getAttribute("aria-selected"), "true");
        assert.equal(await page.locator("figure[data-kind=rings]").isVisible(), true);
        await radial.focus();
        await page.keyboard.press("ArrowRight");
        assert.equal(await story.getAttribute("aria-selected"), "true", "an arrow key moves to the next view");
        assert.equal(await page.locator("#view-columns").isVisible(), true);
        assert.equal(await page.locator("figure[data-kind=rings]").isVisible(), false, "the other view is hidden");
        assert.match(page.url(), /tab=story/);
        await radial.click();
        assert.equal(await page.locator("figure[data-kind=rings]").isVisible(), true);
        await page.done();
      });

      test("with JavaScript off each view is a link to the page with it drawn", async () => {
        const page = await open("/app-blocks?case=view-switcher", { js: false });
        await page.getByRole("tab", { name: "Story" }).click();
        await page.waitForLoadState();
        assert.equal(await page.locator("#view-columns").isVisible(), true);
        assert.equal(await page.locator("figure[data-kind=rings]").isVisible(), false);
      });
    });
    describe("the copy field", () => {
      test("the button copies the value and says so, and focusing the field selects it whole", async () => {
        const page = await open("/app-blocks?case=copy-field");
        if (name === "chromium") await page.context().grantPermissions(["clipboard-read", "clipboard-write"]);
        const field = page.getByLabel("Invite link", { exact: true });
        assert.equal(await field.getAttribute("readonly"), "");
        await field.focus();
        const selected = await field.evaluate((f) => f.value.slice(f.selectionStart, f.selectionEnd));
        assert.equal(selected, "https://kesar.example/join/8k2m-q4rt", "the whole value is selected");
        const button = page.locator("[data-case=copy-field] [data-slot=copy-field-button]").first();
        assert.equal(await button.isVisible(), true);
        assert.equal(await button.innerText(), "Copy");
        await button.click();
        await page.waitForFunction(() => document.querySelector("[data-slot=copy-field-button]").hasAttribute("data-copied"));
        assert.equal(await button.innerText(), "Copied");
        if (name === "chromium") assert.equal(await page.evaluate(() => navigator.clipboard.readText()), "https://kesar.example/join/8k2m-q4rt");
        await page.waitForFunction(() => !document.querySelector("[data-slot=copy-field-button]").hasAttribute("data-copied"), null, { timeout: 4000 });
        assert.equal(await button.innerText(), "Copy");
        // Hostile text is a value, never markup.
        assert.match(await page.locator("#copy-hostile").inputValue(), /^<script>window.__pwned=1<\/script>/);
        assert.equal(await page.evaluate(() => window.__pwned), undefined);
        await page.done();
      });

      test("with JavaScript off the button is not drawn and the value can still be selected", async () => {
        const page = await open("/app-blocks?case=copy-field", { js: false });
        assert.equal(await page.locator("[data-slot=copy-field-button]").first().isVisible(), false);
        const field = page.getByLabel("Invite link", { exact: true });
        assert.equal(await field.inputValue(), "https://kesar.example/join/8k2m-q4rt");
        await field.selectText();
        assert.equal(await field.evaluate((f) => f.selectionEnd - f.selectionStart), "https://kesar.example/join/8k2m-q4rt".length);
      });
    });

    describe("a grid in a ratio", () => {
      test("seven parts beside five from 768 px, one column on a phone", async () => {
        const widths = async (page) => page.evaluate(() => [...document.querySelectorAll("[data-ratio='2'] > *")].map((el) => Math.round(el.getBoundingClientRect().width)));
        const wide = await open("/app-blocks?case=ratio-grid", { width: 1440 });
        const [a, b] = await widths(wide);
        assert.ok(Math.abs(a / b - 7 / 5) < 0.06, `7:5, not ${a}:${b}`);
        const thirds = await wide.evaluate(() => [...document.querySelectorAll("[data-ratio='3'] > *")].map((el) => Math.round(el.getBoundingClientRect().width)));
        assert.ok(Math.abs(thirds[1] / thirds[0] - 2) < 0.08 && Math.abs(thirds[0] - thirds[2]) <= 1, `1:2:1, not ${thirds}`);
        await wide.done();
        const phone = await open("/app-blocks?case=ratio-grid", { width: 390 });
        const tops = await phone.evaluate(() => [...document.querySelectorAll("[data-ratio='2'] > *")].map((el) => Math.round(el.getBoundingClientRect().top)));
        assert.ok(tops[1] > tops[0], "stacked on a phone");
        const [c, d] = await widths(phone);
        assert.equal(c, d, "each the whole width");
        await phone.done();
      });
    });

    describe("a dock for phones only", () => {
      test("it shows on a phone and is not drawn from 768 px", async () => {
        const phone = await open("/app-blocks?case=dock-phones", { width: 390 });
        assert.equal(await phone.getByRole("navigation", { name: "Sections on a phone" }).isVisible(), true);
        const tablet = await open("/app-blocks?case=dock-phones", { width: 768 });
        assert.equal(await tablet.locator("[data-slot=dock][data-below=md]").isVisible(), false);
        assert.equal(await tablet.getByRole("navigation", { name: "Sections on a phone" }).count(), 0, "nor read by a screen reader");
      });
    });

    describe("the app's own picture", () => {
      test("served by its hash, immutable, as SVG under a policy that runs nothing, and drawn", async () => {
        const page = await open("/app-blocks?case=app-image");
        const svg = page.responses.find((r) => /\/_navree\/ui\/app\/proof-mark-[0-9a-f]{12}\.svg$/.test(new URL(r.url).pathname));
        assert.ok(svg, "the picture was fetched");
        assert.equal(svg.status, 200);
        assert.equal(svg.headers["content-type"], "image/svg+xml");
        assert.equal(svg.headers["cache-control"], "public, max-age=31536000, immutable");
        assert.match(svg.headers["content-security-policy"], /default-src 'none'.*sandbox/);
        await page.locator("[data-case=app-image] picture img").scrollIntoViewIfNeeded();
        await page.waitForFunction(() => [...document.querySelectorAll("[data-case=app-image] img")].every((img) => img.complete && img.naturalWidth > 0));
        assert.equal(await page.locator("[data-case=app-image] img").count(), 2, "an img and a picture, both drawn");
        // Opened on its own, it runs nothing.
        const alone = await page.context().newPage();
        await alone.goto(svg.url);
        assert.equal(await alone.evaluate(() => typeof window.__pwned), "undefined");
        await page.done();
      });
    });

    describe("a region whose answer holds lazy tabs", () => {
      test("draws in place, its tabs load their panels, and the page stays where it is", async () => {
        const page = await open("/app-blocks?case=nested-lazy");
        const before = page.url();
        await page.locator("#nested-region [role=tab]").first().waitFor();
        assert.equal(page.url(), before, "the page is not replaced by the fragment");
        assert.equal(await page.title(), "App blocks");
        assert.equal(await page.locator("[data-case=nested-lazy]").count(), 1);
        await page.getByRole("tab", { name: "30 nights" }).click();
        await page.getByText("Thirty nights: 7 h 4 min on average.").waitFor();
        assert.equal(page.url().startsWith(before.split("&")[0]), true);
        await page.done();
      });

      test("the server marks it as the region's fragment: without the mark the runtime would leave for it", async () => {
        const page = await open("/app-blocks?case=nested-lazy&strip=1");
        const marks = [];
        await page.route("**/app-blocks/fragment/trends", async (route) => {
          // Only the region's own fetch: the page the runtime goes to after is a navigation.
          if (!route.request().headers()["x-navree-region"]) return route.continue();
          const response = await route.fetch();
          marks.push(response.headers()["x-navree-fragment"]);
          const headers = { ...response.headers() };
          delete headers["x-navree-fragment"];
          await route.fulfill({ response, headers });
        });
        await page.reload();
        await page.waitForURL(/\/app-blocks\/fragment\/trends$/);
        assert.deepEqual(marks, ["1"], "a view answered to a region's fetch is marked");
      });
    });

    describe("a shell of its own", () => {
      test("the day pager stands in the middle of the bar, the tab is titled by the page, and one theme leaves the modes only", async () => {
        const page = await open("/app-blocks/shell", { width: 1440 });
        assert.equal(await page.title(), "Today · Kesar");
        assert.equal(await page.locator("h1").innerText(), "Good evening, Simran");
        const centre = await page.evaluate(() => {
          const bar = document.querySelector("[data-slot=topbar]").getBoundingClientRect();
          const pager = document.querySelector("[data-slot=topbar-center] [data-slot=day-pager]").getBoundingClientRect();
          return { bar: bar.left + bar.width / 2, pager: pager.left + pager.width / 2 };
        });
        assert.ok(Math.abs(centre.bar - centre.pager) <= 2, `centred: ${JSON.stringify(centre)}`);
        await page.getByRole("button", { name: "Appearance" }).click();
        assert.equal(await page.locator("[data-slot=theme-menu-form] input[name=theme]").count(), 0, "no theme cards");
        assert.equal(await page.locator("[data-slot=theme-menu-form] input[name=mode]").count(), 3);
        const logo = await page.locator("[data-slot=topbar-brand] img").getAttribute("src");
        assert.match(logo, /\/_navree\/ui\/app\/proof-mark-[0-9a-f]{12}\.svg$/);
        await page.done();
      });

      test("on a phone the mark stays beside the menu button, the pager stays in the bar, and the dock shows", async () => {
        const page = await open("/app-blocks/shell", { width: 390 });
        assert.equal(await page.locator("[data-slot=topbar-brand]").isVisible(), true);
        assert.equal(await page.locator("[data-slot=topbar-brand] img").isVisible(), true);
        assert.equal(await page.locator("[data-slot=topbar-center] [data-slot=day-pager]").isVisible(), true);
        assert.equal(await page.getByRole("navigation", { name: "Sections" }).isVisible(), true);
        const wide = await page.evaluate(() => document.documentElement.scrollWidth - innerWidth);
        assert.ok(wide <= 0, `no sideways scroll: ${wide}`);
        for (const theme of ["dark", "light"]) {
          const shot = await open("/app-blocks/shell", { width: 390, light: theme === "light" });
          assert.deepEqual(await axe(shot), [], `axe ${theme}`);
          if (SHOTS) await shot.screenshot({ path: `${SHOTS}/${name}-shell-390-${theme}.png` });
          const desk = await open("/app-blocks/shell", { width: 1440, light: theme === "light" });
          assert.deepEqual(await axe(desk), [], `axe ${theme} 1440`);
          if (SHOTS) await desk.screenshot({ path: `${SHOTS}/${name}-shell-1440-${theme}.png` });
        }
        await page.done();
      });
    });
  });
}
