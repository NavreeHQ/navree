// Themes, the theme menu, the shell's layouts and the theme builder in real browsers:
// every built-in theme in light and dark at 390 and 1440 px with axe clean and every
// contrast pair measured in the page, the first paint in the chosen theme even when its
// stylesheet is slow, a theme change that never paints between two themes, system mode
// following the device live, the menu by keyboard and with JavaScript off, hostile
// cookies, URLs, posts and stylesheet names, each layout and how it folds, the builder
// end to end, and the app, workspace, user and URL levels. Run with the proof app serving:
//
//   cargo run --release -p ui-proof -- serve          # UI_PROOF_URL, default http://127.0.0.1:8090
//   UI_BROWSERS=chromium,firefox,webkit npm test -- tests/theme.test.mjs
//
// UI_SHOTS=<dir> also saves a screenshot of every state.

import assert from "node:assert/strict";
import { mkdirSync, readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import { after, afterEach, before, describe, test } from "node:test";
import { fileURLToPath } from "node:url";
import { chromium, firefox, webkit } from "playwright";

const BASE = process.env.UI_PROOF_URL || "http://127.0.0.1:8090";
const require = createRequire(import.meta.url);
const axeSource = readFileSync(require.resolve("axe-core/axe.min.js"), "utf8");
const ENGINES = { chromium, firefox, webkit };
const NAMES = (process.env.UI_BROWSERS || "chromium,firefox,webkit").split(",").filter((n) => ENGINES[n]);
const SHOTS = process.env.UI_SHOTS;
if (SHOTS) mkdirSync(SHOTS, { recursive: true });

const here = dirname(fileURLToPath(import.meta.url));
/**
 * Every pair the components draw, written from the toolkit's one list (which a
 * test keeps in step with the stylesheet): each colour as CSS, tints and mixes
 * included, text at 4.5:1 and graphics at 3:1.
 */
const PAIRS = JSON.parse(readFileSync(join(here, "support/pairs.json"), "utf8"));
assert.ok(PAIRS.length >= 100, "the pairs were read");
const BUILT_IN = ["navree", "neutral", "ocean", "violet", "sunset", "rose", "contrast"];

async function axe(page) {
  await page.evaluate(axeSource);
  return page.evaluate(async () => {
    const out = await window.axe.run(document, { runOnly: ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa", "best-practice"] });
    return out.violations.map((v) => `${v.id}: ${v.nodes.slice(0, 3).map((n) => n.target.join(" ")).join(" | ")}`);
  });
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/** The `data-theme` a page in `theme` and `mode` carries: none for the default, dark Navree. */
const keyOf = (theme, mode) => (theme === "navree" && mode === "dark" ? null : theme);

for (const name of NAMES) {
  describe(name, { concurrency: false }, () => {
    let browser;
    before(async () => {
      browser = await ENGINES[name].launch();
      await fetch(BASE + "/themes/scopes/reset", { method: "POST" });
    });
    after(async () => {
      await fetch(BASE + "/themes/scopes/reset", { method: "POST" });
      await browser.close();
    });
    const contexts = [];
    afterEach(async () => {
      for (const context of contexts.splice(0)) await context.close().catch(() => {});
    });

    /**
     * A page on the proof app. Console errors, page errors and content security policy
     * reports are kept in `page.problems`; `page.done()` asserts there are none.
     */
    async function open(path, options = {}) {
      const { slowSheets, ...rest } = options;
      const context = await browser.newContext({ viewport: { width: 1440, height: 900 }, colorScheme: "dark", ...rest });
      contexts.push(context);
      if (slowSheets) {
        await context.route("**/_navree/ui/theme/**", async (route) => {
          await sleep(slowSheets);
          await route.continue();
        });
      }
      const page = await context.newPage();
      page.waitFor = async (fn, arg, ms = 15000) => {
        const end = Date.now() + ms;
        for (;;) {
          const got = await page.evaluate(fn, arg).catch(() => null);
          if (got) return got;
          if (Date.now() > end) throw new Error(`timed out waiting for ${String(fn).slice(0, 160)}`);
          await sleep(50);
        }
      };
      page.problems = [];
      page.on("console", (m) => {
        // Playwright's WebKit on Windows reports this on every page, a plain 404 too.
        if (name === "webkit" && /Refused to apply a stylesheet/.test(m.text())) return;
        if (m.type() === "error" && !/Failed to load resource|404|422|403/.test(m.text())) page.problems.push(m.text());
      });
      page.on("pageerror", (e) => page.problems.push("pageerror: " + e.message));
      if (rest.javaScriptEnabled !== false) {
        await page.addInitScript((engine) => {
          document.addEventListener("securitypolicyviolation", (e) => {
            // Playwright's WebKit reports style-src-attr for every native <select> under a policy
            // without 'unsafe-inline', on a blank page with no stylesheet too: its own control, not
            // a style attribute in the page. Only that case is passed over.
            const target = /** @type {Element} */ (e.target);
            if (engine === "webkit" && e.violatedDirective === "style-src-attr" && target?.localName === "select" && !target.hasAttribute("style")) return;
            (window.__csp ||= []).push(e.violatedDirective + " " + e.blockedURI);
          });
        }, name);
      }
      page.scripted = rest.javaScriptEnabled !== false;
      page.done = async () => {
        if (rest.javaScriptEnabled !== false) {
          const csp = await page.evaluate(() => window.__csp || []);
          assert.deepEqual(csp, [], "no content security policy report");
        }
        assert.deepEqual(page.problems, [], "no console or page error");
      };
      if (path) await page.goto(BASE + path, { waitUntil: "load" });
      return page;
    }

    const shot = async (page, file) => {
      if (!SHOTS) return;
      // With scripting off nothing reports, and no frame callback would run.
      const before = page.scripted ? await page.evaluate(() => (window.__csp || []).length).catch(() => null) : null;
      await page.screenshot({ path: join(SHOTS, `${name}-${file}.png`), caret: "initial" });
      // A screenshot in Playwright's WebKit adds a <style> of the tool's own, which the policy
      // reports (on a page with nothing of this module too): reports raised by the screenshot
      // itself are dropped, and only those.
      if (before !== null) {
        await page.evaluate(() => new Promise((done) => requestAnimationFrame(() => setTimeout(done, 0))));
        await page.evaluate((n) => { if (window.__csp) window.__csp.length = n; }, before);
      }
    };

    /** The page's look: its `<html>` attributes and the colours its tokens resolve to there. */
    const look = (page) =>
      page.evaluate(() => {
        const probe = document.createElement("span");
        document.body.append(probe);
        const colour = (token) => {
          probe.style.color = `var(--${token})`;
          return getComputedStyle(probe).color;
        };
        const out = {
          theme: document.documentElement.getAttribute("data-theme"),
          mode: document.documentElement.getAttribute("data-mode"),
          accent: colour("accent"),
          page: colour("ink-0"),
          scheme: getComputedStyle(document.documentElement).colorScheme,
        };
        probe.remove();
        return out;
      });

    /**
     * Every pair's contrast below its minimum, measured from the colours the
     * browser itself resolves and mixes: each paint is set on a probe, and the
     * computed colour is drawn to a canvas to read it back as sRGB.
     */
    const shortfalls = (page) =>
      page.evaluate((pairs) => {
        const probe = document.createElement("span");
        document.body.append(probe);
        const canvas = document.createElement("canvas");
        canvas.width = canvas.height = 1;
        const ctx = canvas.getContext("2d", { willReadFrequently: true });
        const rgb = (paint) => {
          probe.style.color = paint;
          ctx.clearRect(0, 0, 1, 1);
          ctx.fillStyle = "#000";
          ctx.fillStyle = getComputedStyle(probe).color;
          ctx.fillRect(0, 0, 1, 1);
          const [r, g, b] = ctx.getImageData(0, 0, 1, 1).data;
          return [r, g, b];
        };
        const lum = ([r, g, b]) => {
          const c = (v) => ((v /= 255) <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4);
          return 0.2126 * c(r) + 0.7152 * c(g) + 0.0722 * c(b);
        };
        const ratio = (a, b) => {
          const [x, y] = [lum(rgb(a)), lum(rgb(b))].sort((p, q) => q - p);
          return (x + 0.05) / (y + 0.05);
        };
        const out = pairs.filter((p) => ratio(p.text, p.on) < p.min - 0.01).map((p) => `${p.name}: ${ratio(p.text, p.on).toFixed(2)}`);
        probe.remove();
        return out;
      }, PAIRS);

    const noSidewaysScroll = (page) => page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth + 1);

    for (const width of [1440, 390]) {
      test(`${width}px: every built-in theme, light and dark, axe clean with every pair at its ratio and no sideways scroll`, async () => {
        const page = await open(null, { viewport: { width, height: 900 } });
        for (const theme of BUILT_IN) {
          for (const mode of ["light", "dark"]) {
            await page.goto(`${BASE}/themes/layouts/sidebar?theme=${theme}&mode=${mode}`, { waitUntil: "load" });
            const seen = await look(page);
            assert.equal(seen.theme, keyOf(theme, mode), `${theme} ${mode}`);
            assert.equal(seen.scheme, mode, `${theme} ${mode}: the page's color-scheme`);
            assert.deepEqual(await shortfalls(page), [], `${theme} ${mode}`);
            assert.ok(await noSidewaysScroll(page), `${theme} ${mode}: no sideways scroll`);
            assert.deepEqual(await axe(page), [], `${theme} ${mode}`);
            await shot(page, `${width}-${theme}-${mode}`);
          }
        }
        await page.done();
      });
    }

    test("1440px: the component gallery is axe clean in every built-in theme, light and dark", async () => {
      const page = await open(null);
      for (const theme of BUILT_IN) {
        for (const mode of ["light", "dark"]) {
          await page.goto(`${BASE}/gallery?theme=${theme}&mode=${mode}`, { waitUntil: "load" });
          assert.equal((await look(page)).theme, keyOf(theme, mode));
          assert.deepEqual(await axe(page), [], `gallery in ${theme} ${mode}`);
        }
      }
      await page.done();
    });

    /** The short form of the proof app's own theme, from its card in the menu. */
    async function acmeForm() {
      const html = await (await fetch(BASE + "/themes")).text();
      const forms = [...html.matchAll(/name="theme" value="(v1-[0-9a-z-]+)"/g)].map((m) => m[1]);
      assert.equal(forms.length, 1, "one theme of the app's own is offered");
      return forms[0];
    }

    test("the first paint is already in the chosen theme, a built-in or the app's own, even with its stylesheet 1.5 s late", async () => {
      const form = await acmeForm();
      for (const [value, expectTheme] of [["light.ocean", "ocean"], [`dark.${form}`, null]]) {
        const page = await open(null, { slowSheets: 1500 });
        await page.context().addCookies([{ name: "navree_ui_theme", value, url: BASE }]);
        // The colour of the accent at every frame from the first one.
        await page.addInitScript(() => {
          window.__frames = [];
          const probe = () => {
            const span = document.createElement("span");
            document.documentElement.append(span);
            span.style.color = "var(--accent)";
            const colour = getComputedStyle(span).color;
            span.remove();
            return colour;
          };
          const tick = () => {
            window.__frames.push(probe());
            if (window.__frames.length < 40) requestAnimationFrame(tick);
          };
          requestAnimationFrame(tick);
        });
        await page.goto(BASE + "/themes/layouts/sidebar", { waitUntil: "load" });
        await page.waitFor(() => window.__frames.length >= 40);
        const frames = await page.evaluate(() => window.__frames);
        const final = (await look(page)).accent;
        assert.deepEqual([...new Set(frames)], [final], `${value}: every frame from the first is in the chosen theme`);
        if (expectTheme) assert.equal((await look(page)).theme, expectTheme);
        else assert.match((await look(page)).theme, /^c[0-9a-f]{24}$/);
        await page.done();
      }
    });

    test("a theme chosen in the menu stays chosen on a page reached with ?theme= and ?mode=", async () => {
      const page = await open("/themes?theme=ocean&mode=dark");
      assert.equal((await look(page)).theme, "ocean");
      await page.click("[popovertarget=ui-theme-menu]");
      await page.waitFor(() => document.getElementById("ui-theme-menu")?.matches(":popover-open"));
      // Picked in the page: the driver's own checks before a click stall Firefox for seconds at times.
      await page.evaluate(() => [...document.querySelectorAll("#ui-theme-menu [data-slot=theme-card]")].find((card) => card.textContent.includes("Violet")).click());
      await page.waitFor(() => document.documentElement.getAttribute("data-theme") === "violet");
      assert.equal(new URL(page.url()).search, "", "the address it came back to doesn't undo the choice");
      const cookie = (await page.context().cookies()).find((c) => c.name === "navree_ui_theme");
      assert.equal(cookie?.value, "dark.violet");
      await page.done();
    });

    test("with JavaScript off, the chosen theme is in the page as served, with no script", async () => {
      const form = await acmeForm();
      const page = await open(null, { javaScriptEnabled: false });
      await page.context().addCookies([{ name: "navree_ui_theme", value: `light.${form}`, url: BASE }]);
      await page.goto(BASE + "/themes/layouts/inset", { waitUntil: "load" });
      const seen = await look(page);
      assert.match(seen.theme, /^c[0-9a-f]{24}$/);
      assert.equal(seen.mode, "light");
      assert.equal(seen.accent, "rgb(124, 92, 255)", "the app's own violet accent");
      await shot(page, "nojs-acme-light");
      await page.done();
    });

    test("a theme changed from the menu on a boosted page never paints in a third theme while its stylesheet is late", async () => {
      const page = await open("/themes", { slowSheets: 800 });
      const before = (await look(page)).accent;
      await page.click("[popovertarget=ui-theme-menu]");
      await page.waitFor(() => document.getElementById("ui-theme-menu")?.matches(":popover-open"));
      await page.evaluate(() => {
        window.__frames = [];
        const tick = () => {
          const span = document.createElement("span");
          document.documentElement.append(span);
          span.style.color = "var(--accent)";
          window.__frames.push(getComputedStyle(span).color);
          span.remove();
          if (!window.__stop) requestAnimationFrame(tick);
        };
        requestAnimationFrame(tick);
      });
      // The app's own theme is the last card.
      await page.locator("#ui-theme-menu [data-slot=theme-card]").last().click();
      await page.waitFor(() => /^c[0-9a-f]{24}$/.test(document.documentElement.getAttribute("data-theme") || ""));
      await page.waitFor(() => window.__frames.length > 10);
      const after = (await look(page)).accent;
      await page.evaluate(() => { window.__stop = true; });
      const seen = [...new Set(await page.evaluate(() => window.__frames))];
      assert.deepEqual(seen, [before, after], "only the old theme, then the new");
      assert.ok(await page.evaluate(() => [...document.querySelectorAll("link[data-ui-theme]")].some((l) => l.sheet && l.sheet.cssRules.length > 0)), "the new theme's stylesheet stays in the page");
      const cookie = (await page.context().cookies()).find((c) => c.name === "navree_ui_theme");
      assert.match(cookie.value, /^dark\.v1-/, "kept in the cookie");
      assert.ok(cookie.httpOnly, "the cookie is HttpOnly");
      await page.done();
    });

    test("system mode follows the device, and changes live when it does", async () => {
      const page = await open("/themes?mode=system", { colorScheme: "dark" });
      const dark = await look(page);
      assert.equal(dark.mode, "system");
      assert.equal(dark.page, "rgb(4, 6, 5)");
      await page.emulateMedia({ colorScheme: "light" });
      await page.waitFor(() => {
        const span = document.createElement("span");
        document.body.append(span);
        span.style.color = "var(--ink-0)";
        const colour = getComputedStyle(span).color;
        span.remove();
        return colour === "rgb(255, 255, 255)";
      });
      assert.equal(await page.evaluate(() => document.querySelector("meta[name=color-scheme]").content), "light dark");
      await page.done();
    });

    test("the theme menu by keyboard alone: open it, move through the themes and modes, close it", async () => {
      const page = await open("/themes");
      // Tab to the menu's button.
      for (let i = 0; i < 80; i++) {
        await page.keyboard.press("Tab");
        if (await page.evaluate(() => document.activeElement?.getAttribute("popovertarget") === "ui-theme-menu")) break;
      }
      assert.ok(await page.evaluate(() => document.activeElement?.getAttribute("popovertarget") === "ui-theme-menu"), "the menu's button takes focus");
      await page.keyboard.press("Enter");
      await page.waitFor(() => document.getElementById("ui-theme-menu")?.matches(":popover-open"));
      await page.waitFor(() => document.getElementById("ui-theme-menu").getAnimations().every((a) => a.playState === "finished"));
      await shot(page, "menu-open");
      assert.deepEqual(await axe(page), [], "axe with the menu open");
      // Into the theme cards: the chosen one takes focus.
      await page.keyboard.press("Tab");
      assert.equal(await page.evaluate(() => document.activeElement?.value), "navree");
      await page.keyboard.press("ArrowDown");
      await page.waitFor(() => document.documentElement.getAttribute("data-theme") === "neutral");
      assert.ok(await page.evaluate(() => document.getElementById("ui-theme-menu").contains(document.activeElement)), "focus stays in the menu");
      // On to the modes.
      await page.keyboard.press("Tab");
      assert.equal(await page.evaluate(() => document.activeElement?.name), "mode");
      await page.keyboard.press("ArrowLeft");
      await page.waitFor(() => document.documentElement.getAttribute("data-mode") === "light");
      assert.equal((await look(page)).page, "rgb(255, 255, 255)");
      await page.keyboard.press("Escape");
      await page.waitFor(() => !document.getElementById("ui-theme-menu").matches(":popover-open"));
      const cookie = (await page.context().cookies()).find((c) => c.name === "navree_ui_theme");
      assert.equal(cookie.value, "light.neutral");
      await page.done();
    });

    test("with JavaScript off, the menu posts with Apply and the next page is in the new theme", async () => {
      const page = await open("/themes", { javaScriptEnabled: false });
      await page.click("[popovertarget=ui-theme-menu]");
      await page.locator("#ui-theme-menu [data-slot=theme-card]:has(input[value=sunset])").click();
      await page.locator("#ui-theme-menu [data-slot=theme-mode]:has(input[value=light])").click();
      // The address stays /themes: wait for the page the redirect brings.
      await page.click("#ui-theme-menu [data-theme-apply]");
      await page.waitFor(() => document.documentElement.getAttribute("data-theme") === "sunset");
      const seen = await look(page);
      assert.equal(seen.theme, "sunset");
      assert.equal(seen.mode, "light");
      await page.done();
    });

    test("hostile cookies, URLs, posts and stylesheet names change nothing", async () => {
      const page = await open(null);
      for (const value of ["dark.ocean%22%3E%3Cscript%3E", "dark.v1-0406-d-g-zz", "__proto__", "light.custom", "light.v1-0406-d-g-" + "f".repeat(168) + "-" + "0".repeat(168)]) {
        await page.context().clearCookies();
        await page.context().addCookies([{ name: "navree_ui_theme", value, url: BASE }]);
        await page.goto(BASE + "/themes/layouts/sidebar", { waitUntil: "load" });
        const seen = await look(page);
        assert.equal(seen.theme, null, `${value} is no theme`);
      }
      await page.context().clearCookies();
      await page.goto(BASE + "/themes/layouts/sidebar?theme=%3C%2Fstyle%3E%3Cscript%3Ealert(1)%3C%2Fscript%3E&mode=%22%3E", { waitUntil: "load" });
      assert.equal((await look(page)).theme, null);
      assert.equal(await page.evaluate(() => document.querySelectorAll("script:not([type=module])").length), 0);
      // A post from another site is refused; a bad value is refused, not stored.
      const post = (body, headers) => fetch(BASE + "/_navree/ui/appearance", { method: "POST", redirect: "manual", headers: { "content-type": "application/x-www-form-urlencoded", ...headers }, body });
      assert.equal((await post("theme=ocean", { "sec-fetch-site": "cross-site" })).status, 403);
      assert.equal((await post("theme=ocean", { origin: "http://evil.example" })).status, 403);
      assert.equal((await post("theme=%3Cscript%3E", {})).status, 400);
      assert.equal((await post("theme=nope&mode=light", {})).status, 400);
      assert.equal((await post("mode=dim", {})).status, 400);
      const ok = await post("theme=ocean&mode=light&back=https%3A%2F%2Fevil.example%2F", {});
      assert.equal(ok.status, 303);
      assert.equal(ok.headers.get("location"), "/", "never off the site");
      // The stylesheet route serves a theme's short form and nothing else.
      const form = await acmeForm();
      const sheet = await fetch(`${BASE}/_navree/ui/theme/${form}.css`);
      assert.equal(sheet.status, 200);
      assert.match(sheet.headers.get("cache-control"), /immutable/);
      assert.match(await sheet.text(), /^:where\(\[data-theme=c[0-9a-f]{24}\]\)\{--ink-0:/);
      for (const bad of ["navree.css", `${form.toUpperCase()}.css`, `${form.slice(0, -1)}g.css`, "v1-0406-d-g-%3B%7D.css"]) {
        assert.equal((await fetch(`${BASE}/_navree/ui/theme/${bad}`)).status, 404, bad);
      }
      // The router refuses an encoded path before the route sees it.
      assert.ok([400, 404].includes((await fetch(`${BASE}/_navree/ui/theme/..%2F..%2Fsecret.css`)).status));
      await page.done();
    });

    for (const layout of ["sidebar", "inset", "floating", "header"]) {
      for (const width of [1440, 390]) {
        test(`${layout} layout at ${width}px, light and dark: axe clean, no sideways scroll, and the navigation by keyboard`, async () => {
          const page = await open(null, { viewport: { width, height: 900 } });
          for (const mode of ["dark", "light"]) {
            await page.goto(`${BASE}/themes/layouts/${layout}?mode=${mode}`, { waitUntil: "load" });
            assert.ok(await noSidewaysScroll(page), `${mode}: no sideways scroll`);
            assert.deepEqual(await axe(page), [], `${layout} ${mode}`);
            await shot(page, `layout-${layout}-${width}-${mode}`);
          }
          const wrapper = "[data-slot=sidebar-wrapper]";
          if (width === 390) {
            // Every layout puts the links in a sheet on a phone.
            await page.click("[data-ui-sidebar-trigger]");
            await page.waitFor((w) => document.querySelector(w).hasAttribute("data-mobile-open"), wrapper);
            await page.locator("[data-slot=sidebar] [data-slot=sidebar-item]").first().waitFor({ state: "visible" });
            await shot(page, `layout-${layout}-390-sheet`);
            await page.keyboard.press("Escape");
            await page.waitFor((w) => !document.querySelector(w).hasAttribute("data-mobile-open"), wrapper);
          } else if (layout === "header") {
            assert.equal(await page.locator("[data-slot=sidebar]").isVisible(), false, "no sidebar on wide screens");
            await page.keyboard.press("Control+b");
            assert.equal(await page.evaluate((w) => document.querySelector(w).hasAttribute("data-collapsed"), wrapper), false, "Ctrl+B folds nothing");
            // A labelled group is a menu in the top bar: open it by keyboard and reach its links.
            const sales = page.locator("[data-slot=topnav] button[data-slot=topnav-item]").first();
            await sales.focus();
            await page.keyboard.press("Enter");
            const id = await sales.getAttribute("popovertarget");
            await page.waitFor((id) => document.getElementById(id)?.matches(":popover-open"), id);
            await page.waitFor((id) => document.getElementById(id).getAnimations().every((a) => a.playState === "finished"), id);
            if (name === "webkit") {
              // WebKit moves Tab to links only when the reader asks it to (Safari's "Press Tab to
              // highlight each item"): the links are there and take focus.
              await page.evaluate((id) => document.getElementById(id).querySelector("a").focus(), id);
            } else {
              await page.keyboard.press("Tab");
            }
            assert.ok(await page.evaluate((id) => document.getElementById(id).contains(document.activeElement), id), "focus moves into the menu's links");
            assert.deepEqual(await axe(page), [], "axe with a top bar menu open");
            await shot(page, "layout-header-menu-open");
          } else {
            await page.keyboard.press("Control+b");
            await page.waitFor((w) => document.querySelector(w).hasAttribute("data-collapsed"), wrapper);
            await shot(page, `layout-${layout}-collapsed`);
            await page.reload({ waitUntil: "load" });
            assert.ok(await page.evaluate((w) => document.querySelector(w).hasAttribute("data-collapsed"), wrapper), "still folded after a reload");
            assert.deepEqual(await axe(page), [], `${layout} folded`);
            await page.keyboard.press("Control+b");
            await page.waitFor((w) => !document.querySelector(w).hasAttribute("data-collapsed"), wrapper);
          }
          await page.done();
        });
      }
    }

    test("a sidebar that folds away hides entirely, and one that never folds has no button on wide screens", async () => {
      const page = await open("/themes/layouts/sidebar?collapse=offcanvas");
      await page.keyboard.press("Control+b");
      await page.waitFor(() => document.querySelector("[data-slot=sidebar-wrapper]").hasAttribute("data-collapsed"));
      await page.waitFor(() => document.querySelector("[data-slot=sidebar]").getBoundingClientRect().width === 0);
      await shot(page, "offcanvas-folded");
      await page.keyboard.press("Control+b");
      await page.goto(BASE + "/themes/layouts/inset?collapse=never", { waitUntil: "load" });
      assert.equal(await page.locator("[data-ui-sidebar-trigger]").isVisible(), false);
      await page.keyboard.press("Control+b");
      await sleep(0);
      assert.equal(await page.evaluate(() => document.querySelector("[data-slot=sidebar-wrapper]").hasAttribute("data-collapsed")), false);
      await page.done();
    });

    test("the builder by keyboard: an accent changes the page and its preview without painting between two themes", async () => {
      const page = await open("/themes/builder?mode=dark", { slowSheets: 600 });
      assert.deepEqual(await axe(page), [], "axe on the builder");
      const before = (await look(page)).accent;
      await page.evaluate(() => {
        window.__frames = [];
        const tick = () => {
          const span = document.createElement("span");
          document.documentElement.append(span);
          span.style.color = "var(--accent)";
          window.__frames.push(getComputedStyle(span).color);
          span.remove();
          if (!window.__stop) requestAnimationFrame(tick);
        };
        requestAnimationFrame(tick);
      });
      await page.locator("input[name=accent][value=base]").focus();
      await page.keyboard.press("ArrowRight");
      await page.keyboard.press("ArrowRight");
      // The address changes first; the page follows once the new theme's stylesheet is in.
      await page.waitFor(() => new URL(location.href).searchParams.get("accent") === "blue");
      await page.waitFor(() => window.__frames.at(-1) === "rgb(90, 162, 255)");
      assert.match((await look(page)).theme, /^c[0-9a-f]{24}$/);
      await page.evaluate(() => { window.__stop = true; });
      const after = (await look(page)).accent;
      assert.notEqual(after, before);
      assert.deepEqual([...new Set(await page.evaluate(() => window.__frames))], [before, after], "the old theme, then the new");
      assert.equal(await page.evaluate(() => document.activeElement?.value), "blue", "focus stays on the radio");
      assert.deepEqual(await shortfalls(page), [], "the derived colours read");
      assert.deepEqual(await axe(page), [], "axe after the change");
      await shot(page, "builder-blue");
      await page.done();
    });

    test("the builder refuses what doesn't parse and what doesn't read, in the page and on the server", async () => {
      const page = await open("/themes/builder?mode=light");
      const theme = (await look(page)).theme;
      await page.click("[data-slot=theme-tokens-summary]");
      await page.fill("input[name='d.fg-3']", "#12345g");
      await page.waitFor(() => document.querySelector("input[name='d.fg-3']")?.getAttribute("aria-invalid") === "true");
      assert.match(await page.locator("#theme-d-fg-3-error").innerText(), /hex colour/);
      assert.equal((await look(page)).theme, theme, "a colour that doesn't parse changes nothing");
      await page.fill("input[name='d.fg-3']", "");
      await page.fill("input[name='l.fg-3']", "#dddddd");
      await page.waitFor(() => document.querySelector("[data-slot=theme-contrast-summary]")?.getAttribute("data-state") === "fails");
      assert.ok((await page.locator("tr[data-fails=text]").count()) >= 3, "the failing pairs are listed");
      assert.equal(await page.locator("[data-slot=theme-save]").count(), 0, "no save while text fails");
      assert.equal((await look(page)).theme, "navree", "the page stays in its base while the draft fails");
      assert.deepEqual(await axe(page), [], "axe with failures listed");
      assert.ok(await page.evaluate(() => document.querySelector("[data-slot=theme-preview-pair]").inert), "the preview is a picture");
      await shot(page, "builder-fails");
      // The server checks again.
      const refused = await fetch(BASE + "/themes/builder/save", { method: "POST", redirect: "manual", headers: { "content-type": "application/x-www-form-urlencoded" }, body: "draft=" + encodeURIComponent("l.fg-3=#dddddd&id=pale") });
      assert.equal(refused.status, 422);
      const saved = await fetch(BASE + "/themes/builder/save", { method: "POST", redirect: "manual", headers: { "content-type": "application/x-www-form-urlencoded" }, body: "draft=" + encodeURIComponent("base=violet&id=acme-two&name=Acme two") });
      assert.equal(saved.status, 303);
      await page.done();
    });

    test("the builder's export is Rust and a stored theme, and a stored theme imports back; a hostile one is refused field by field", async () => {
      const page = await open("/themes/builder?base=sunset&accent=custom&accent_hex=%23c2410c&id=acme-three&name=Acme%20three&mode=dark");
      const [rust, json] = await page.locator("pre[data-variant=export]").allInnerTexts();
      assert.match(rust, /pub fn acme_three_theme\(\) -> Theme \{/);
      assert.match(json, /"schema": "navree-theme\/1"/);
      // Copy works by the button, with no request.
      if (name === "chromium") {
        await page.context().grantPermissions(["clipboard-read", "clipboard-write"], { origin: BASE });
        await page.locator("[data-slot=theme-export-block] [data-slot=button]").first().click();
        await page.waitFor(() => navigator.clipboard.readText().then((text) => text.includes("acme_three_theme")));
      }
      // Import it back under another id.
      await page.fill("textarea[name=json]", json.replace('"acme-three"', '"acme-four"'));
      await Promise.all([page.waitForURL(/\/themes\/builder\?/), page.click("[data-slot=theme-import] [data-slot=button]")]);
      await page.waitFor(() => document.querySelector("input[name=id]")?.value === "acme-four");
      const imported = await look(page);
      assert.match(imported.theme, /^c[0-9a-f]{24}$/);
      // A hostile file: refused with what is wrong, and nothing of it in the page as markup or style.
      const hostile = JSON.stringify({ schema: "navree-theme/1", id: "x", name: "<img src=x onerror=alert(1)>", radius: { control: "4px;}", surface: "6px" }, density: "default", font: "geist", dark: { "ink-0": "red;}body{display:none" }, light: {} });
      await page.fill("textarea[name=json]", hostile);
      await page.click("[data-slot=theme-import] [data-slot=button]");
      await page.waitFor(() => document.querySelector("#theme-import-json-error"));
      const why = await page.locator("#theme-import-json-error").innerText();
      assert.match(why, /dark\.ink-0/);
      assert.match(why, /radius/);
      assert.equal(await page.locator("img[onerror]").count(), 0);
      assert.equal(await page.evaluate(() => getComputedStyle(document.body).display), "block");
      await page.done();
    });

    test("hostile values in the builder's URL reach the page only as escaped text", async () => {
      const page = await open(
        "/themes/builder?d.ink-0=%3C%2Fstyle%3E%3Cscript%3Ewindow.__x%3D1%3C%2Fscript%3E&l.fg=red%3B%7Dbody%7Bdisplay%3Anone&name=%3Cimg%20src%3Dx%20onerror%3Dwindow.__x%3D1%3E&id=%22%3E%3Cb%3E&accent=custom&accent_hex=url(javascript:alert(1))&radius=99px&font=Comic",
      );
      assert.equal(await page.evaluate(() => window.__x), undefined);
      assert.equal(await page.locator("img[onerror], b").count(), 0);
      assert.equal(await page.evaluate(() => getComputedStyle(document.body).display), "block");
      assert.equal(await page.locator("input[name='d.ink-0']").inputValue(), "</style><script>window.__x=1</sc", "kept as typed, cut to 32 characters");
      assert.equal(await page.locator("input[name='d.ink-0']").getAttribute("aria-invalid"), "true");
      assert.equal((await look(page)).theme, null, "nothing hostile made a theme");
      const html = await (await fetch(page.url())).text();
      assert.ok(!/ style=/.test(html), "no style attribute in the page");
      assert.ok(!/<style/.test(html), "no style element in the page");
      await page.done();
    });

    test("with JavaScript off, the builder previews with its button and the page is the draft", async () => {
      const page = await open("/themes/builder?mode=light", { javaScriptEnabled: false });
      assert.ok(await page.locator("#theme-builder-form [data-theme-apply]").isVisible(), "the Preview button shows");
      await page.locator("[data-slot=theme-choice]:has(input[name=accent][value=rose])").click();
      await Promise.all([page.waitForURL(/accent=rose/), page.click("#theme-builder-form [data-theme-apply]")]);
      const seen = await look(page);
      assert.match(seen.theme, /^c[0-9a-f]{24}$/);
      assert.equal(seen.mode, "light");
      await page.done();
    });

    test("the app's, a workspace's (locked or not), the user's and the URL's choices lay over each other", async () => {
      const post = (path, body) => fetch(BASE + path, { method: "POST", redirect: "manual", headers: { "content-type": "application/x-www-form-urlencoded" }, body });
      await post("/themes/scopes/reset", "");
      const page = await open("/themes/scopes");
      assert.equal((await look(page)).theme, null, "the app's default");
      await post("/themes/scopes/workspace", "value=dark.ocean");
      await page.reload();
      assert.equal((await look(page)).theme, "ocean", "the workspace's");
      await post("/themes/scopes/user", "value=light.rose");
      await page.reload();
      let seen = await look(page);
      assert.deepEqual([seen.theme, seen.mode], ["rose", "light"], "the user's over the workspace's");
      await page.goto(BASE + "/themes/scopes?theme=violet");
      assert.equal((await look(page)).theme, "violet", "the URL's over the user's");
      await post("/themes/scopes/workspace", "value=dark.ocean&locked=on");
      await page.goto(BASE + "/themes/scopes?theme=violet");
      seen = await look(page);
      assert.deepEqual([seen.theme, seen.mode], ["ocean", "light"], "locked: the workspace's theme, the user's mode");
      assert.equal(await page.locator("#ui-theme-menu [data-slot=theme-card]").count(), 0, "locked: no theme to pick");
      assert.equal(await page.locator("#ui-theme-menu input[name=mode]").count(), 3, "the mode is still the reader's");
      assert.deepEqual(await axe(page), [], "axe on the scopes page");
      await shot(page, "scopes-locked");
      await post("/themes/scopes/reset", "");
      await page.done();
    });
  });
}
