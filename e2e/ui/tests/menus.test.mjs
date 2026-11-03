// Tabs, toggle groups, dropdown menus and toasts in real browsers: the keys of each
// pattern, the address bar, requests, JavaScript off, placement, the live region, the
// content security policy and axe in every state. Run with the proof app serving:
//
//   cargo run -p ui-proof -- serve          # UI_PROOF_URL, default http://127.0.0.1:8090
//   UI_BROWSERS=chromium,firefox,webkit npm test -- tests/menus.test.mjs

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { readFile } from "node:fs/promises";
import { createRequire } from "node:module";
import { after, before, describe, test } from "node:test";
import { chromium, firefox, webkit } from "playwright";
import { page as doc, start } from "./support/fixture.mjs";

const BASE = process.env.UI_PROOF_URL || "http://127.0.0.1:8090";
const require = createRequire(import.meta.url);
const axeSource = await readFile(require.resolve("axe-core/axe.min.js"), "utf8");
const ENGINES = { chromium, firefox, webkit };
const NAMES = (process.env.UI_BROWSERS || "chromium,firefox").split(",").filter((n) => ENGINES[n]);

/** The light values of the tokens, for a page that otherwise ships only dark. */
const LIGHT = ":root{color-scheme:light;--ink-0:#ffffff;--ink-1:#f7f8f7;--ink-2:#eff1f0;--ink-3:#e5e8e6;--line:#dfe3e0;--line-2:#c8cecb;--line-3:#aeb6b2;--fg:#0a0e0c;--fg-1:#1c2420;--fg-2:#3f4a44;--fg-3:#55615b;--accent:#0b7a3e;--accent-ink:#ffffff;--amber:#8a5a00;--danger:#b3261e}body{background:var(--ink-0)}";

async function axe(page) {
  await page.evaluate(axeSource);
  return page.evaluate(async () => {
    const out = await window.axe.run(document, { runOnly: ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa", "best-practice"] });
    return out.violations.map((v) => `${v.id}: ${v.nodes.slice(0, 3).map((n) => n.target.join(" ")).join(" | ")}`);
  });
}

for (const name of NAMES) {
  describe(name, { concurrency: false }, () => {
    let browser;
    before(async () => {
      browser = await ENGINES[name].launch();
      // The first page of a browser that has just started can take long on a busy machine: it is not what a test measures.
      const warm = await browser.newContext();
      await (await warm.newPage()).goto(BASE + "/menus", { timeout: 120000 });
      await warm.close();
    });
    after(() => browser.close());

    /**
     * A page on the proof app. Console errors and content security policy reports are
     * kept in `page.problems`, and each test ends by asserting there are none.
     */
    async function open(path, options = {}) {
      const { light, before, ...rest } = options;
      const context = await browser.newContext({ viewport: { width: 1280, height: 800 }, bypassCSP: !!light, ...rest });
      const page = await context.newPage();
      page.problems = [];
      page.on("console", (m) => {
        // A refused or dropped request on purpose shows up as a failed resource.
        if (m.type() === "error" && !/422|500|ERR_FAILED|ERR_ABORTED/.test(m.text())) page.problems.push(m.text());
      });
      page.on("pageerror", (e) => page.problems.push("pageerror: " + e.message));
      await page.addInitScript(() => {
        document.addEventListener("securitypolicyviolation", (e) => {
          (window.__csp ||= []).push(e.violatedDirective + " " + e.blockedURI);
        });
      });
      if (before) await before(page);
      await page.goto(BASE + path);
      if (light) {
        await page.addStyleTag({ content: LIGHT });
        // The page's colours ease from dark to light; axe reads them once they have.
        await page.waitForTimeout(300);
      }
      page.done = async () => {
        const csp = rest.javaScriptEnabled === false ? [] : await page.evaluate(() => window.__csp || []);
        await context.close();
        assert.deepEqual(csp, [], "no content security policy report");
        assert.deepEqual(page.problems, []);
      };
      return page;
    }

    const selected = (page, root) =>
      page.evaluate((r) => [...document.querySelectorAll(`#${r} > [role=tablist] > [role=tab]`)].find((t) => t.getAttribute("aria-selected") === "true")?.textContent.trim(), root);
    const stops = (page, selector) => page.evaluate((s) => [...document.querySelectorAll(s)].filter((e) => e.tabIndex === 0 && !e.disabled).length, selector);
    const focused = (page) => page.evaluate(() => {
      const a = document.activeElement;
      return a?.closest("label")?.textContent?.trim() || a?.textContent?.trim() || a?.getAttribute("aria-label") || a?.id;
    });
    const focusIs = (page, text) => page.waitForFunction((t) => {
      const a = document.activeElement;
      return (a?.closest("label")?.textContent?.trim() || a?.textContent?.trim() || a?.getAttribute("aria-label") || a?.id) === t;
    }, text);
    const toasts = (page) => page.locator("[data-slot=toaster] [data-slot=toast]");
    // The overlay module places a popup when the browser reports it open, a task after it showed.
    const placed = (page, selector) => page.waitForFunction((sel) => document.querySelector(sel).style.top !== "", selector);
    const requests = (page) => {
      const list = [];
      page.on("request", (r) => list.push(r.url().replace(BASE, "")));
      return list;
    };

    // ---- Tabs ------------------------------------------------------------

    describe("tabs", () => {
      test("T1 click, arrows, Home, End and wrap skip the disabled tab; one tab stop", async () => {
        const page = await open("/menus");
        assert.equal(await selected(page, "t-line"), "Overview");
        assert.equal(await stops(page, "#t-line > [role=tablist] > [role=tab]"), 1);
        await page.locator("#t-line-tab-overview").focus();
        await page.keyboard.press("ArrowRight");
        assert.equal(await selected(page, "t-line"), "Activity12", "arrows open the tab (automatic)");
        await page.keyboard.press("ArrowRight");
        assert.equal(await selected(page, "t-line"), "Settings", "the disabled Files tab is skipped");
        await page.keyboard.press("ArrowRight");
        assert.equal(await selected(page, "t-line"), "Overview", "wraps");
        await page.keyboard.press("End");
        assert.equal(await selected(page, "t-line"), "Settings");
        await page.keyboard.press("Home");
        assert.equal(await selected(page, "t-line"), "Overview");
        await page.keyboard.press("ArrowLeft");
        assert.equal(await selected(page, "t-line"), "Settings");
        assert.equal(await stops(page, "#t-line > [role=tablist] > [role=tab]"), 1);
        // One Tab leaves the list for the panel, which is itself a tab stop.
        await page.keyboard.press("Tab");
        assert.equal(await page.evaluate(() => document.activeElement.id), "t-line-panel-settings");
        await page.done();
      });

      test("T1 manual activation: arrows move focus, Enter and Space open", async () => {
        const page = await open("/menus");
        await page.locator("#t-seg-tab-day").focus();
        await page.keyboard.press("ArrowRight");
        assert.equal(await selected(page, "t-seg"), "Day", "still Day");
        assert.equal(await focused(page), "Week");
        await page.keyboard.press("Enter");
        assert.equal(await selected(page, "t-seg"), "Week");
        await page.keyboard.press("ArrowRight");
        await page.keyboard.press(" ");
        assert.equal(await selected(page, "t-seg"), "Month3");
        assert.equal(await page.locator("#t-seg-panel-month").isVisible(), true);
        assert.equal(await page.locator("#t-seg-panel-week").isHidden(), true);
        // Leaving the list puts its tab stop back on the open tab.
        await page.keyboard.press("ArrowLeft");
        await page.evaluate(() => document.activeElement.blur());
        assert.equal(await page.evaluate(() => document.querySelector("#t-seg-tab-month").tabIndex), 0);
        await page.done();
      });

      test("T1 vertical tabs use Up and Down; right to left swaps Left and Right", async () => {
        const page = await open("/menus");
        await page.locator("#t-vert-tab-profile").focus();
        await page.keyboard.press("ArrowRight");
        assert.equal(await selected(page, "t-vert"), "Profile", "Right does nothing in a vertical list");
        await page.keyboard.press("ArrowDown");
        assert.equal(await selected(page, "t-vert"), "Billing");
        await page.keyboard.press("ArrowUp");
        await page.keyboard.press("ArrowUp");
        assert.equal(await selected(page, "t-vert"), "Team", "wraps up");
        await page.evaluate(() => { document.querySelector("#t-line").dir = "rtl"; });
        await page.locator("#t-line-tab-overview").focus();
        await page.keyboard.press("ArrowLeft");
        assert.equal(await selected(page, "t-line"), "Activity12", "Left is next in a right-to-left list");
        await page.done();
      });

      test("T2 the address follows: back, forward and reload, with no request for a client tab", async () => {
        const page = await open("/menus");
        const seen = requests(page);
        await page.locator("#t-line-tab-activity").click();
        assert.match(page.url(), /[?&]tab=activity/);
        await page.locator("#t-line-tab-settings").click();
        assert.match(page.url(), /tab=settings/);
        await page.goBack();
        assert.equal(await selected(page, "t-line"), "Activity12");
        assert.match(page.url(), /tab=activity/);
        await page.goBack();
        assert.equal(await selected(page, "t-line"), "Overview");
        await page.goForward();
        assert.equal(await selected(page, "t-line"), "Activity12");
        assert.deepEqual(seen.filter((u) => u.startsWith("/menus") && !u.startsWith("/menus/tab/")), [], "tabs, back and forward made no request");
        await page.reload();
        assert.equal(await selected(page, "t-line"), "Activity12", "reload restores it");
        assert.equal(await page.locator("#t-line-panel-activity").isVisible(), true);
        await page.done();
      });

      test("T2 arrow keys replace the address; clicks add to history; other parameters stay", async () => {
        const page = await open("/menus?keep=1");
        await page.locator("#t-line-tab-overview").focus();
        await page.keyboard.press("ArrowRight");
        assert.match(page.url(), /keep=1/);
        assert.match(page.url(), /tab=activity/);
        const before = await page.evaluate(() => history.length);
        await page.keyboard.press("ArrowRight");
        assert.equal(await page.evaluate(() => history.length), before, "arrows don't add history entries");
        await page.locator("#t-line-tab-overview").click();
        assert.equal(await page.evaluate(() => history.length), before + 1, "a click does");
        await page.done();
      });

      test("T3 with JavaScript off a tab is a link and the page shows that tab", async () => {
        const page = await open("/menus", { javaScriptEnabled: false });
        assert.equal(await selected(page, "t-line"), "Overview");
        assert.equal(await page.locator("#t-line-tab-settings").getAttribute("href"), "?tab=settings");
        await page.locator("#t-line-tab-settings").click();
        await page.waitForURL(/tab=settings/);
        assert.equal(await selected(page, "t-line"), "Settings");
        assert.equal(await page.locator("#t-line-panel-settings").isVisible(), true);
        assert.equal(await page.locator("#t-line-panel-overview").isHidden(), true);
        assert.equal(await page.locator("#t-line-tab-files").getAttribute("href"), null, "a disabled tab has no link");
        await page.done();
      });

      test("T4 a lazy tab loads when first shown, once, and a fresh one each time", async () => {
        const page = await open("/menus?lz=fast");
        await page.locator("#t-lazy-panel-fast").getByText("The fast tab, loaded").waitFor();
        const hits = async () => (await (await page.request.get(BASE + "/menus/hits")).json());
        const base = await hits();
        await page.locator("#t-lazy-tab-slow").click();
        await page.locator("#t-lazy-panel-slow").getByText("The slow tab, loaded").waitFor();
        await page.locator("#t-lazy-tab-fast").click();
        await page.locator("#t-lazy-tab-slow").click();
        await page.locator("#t-lazy-tab-fast").click();
        assert.equal((await hits()).slow, (base.slow || 0) + 1, "the slow tab was asked for once");
        assert.equal((await hits()).fast, base.fast, "the fast tab, already loaded, was not asked again");
        const fresh0 = (await hits()).fresh || 0;
        await page.locator("#t-lazy-tab-fresh").click();
        await page.locator("#t-lazy-panel-fresh").getByText("The fresh tab, loaded").waitFor();
        await page.locator("#t-lazy-tab-fast").click();
        await page.locator("#t-lazy-tab-fresh").click();
        await page.waitForFunction(() => document.querySelector("#t-lazy-panel-fresh").getAttribute("aria-busy") !== "true");
        assert.equal((await hits()).fresh, fresh0 + 2, "a fresh tab is asked for each time");
        // A tab given its content with the page is loaded already.
        await page.locator("#t-lazy-tab-given").click();
        assert.equal(await page.locator("#t-lazy-panel-given").innerText(), "Drawn with the page.");
        assert.equal((await hits()).given, undefined);
        await page.done();
      });

      test("T4 a slow tab shows its skeleton and is busy until it arrives", async () => {
        const page = await open("/menus");
        await page.route("**/menus/tab/slow", async (route) => {
          await new Promise((r) => setTimeout(r, 700));
          await route.continue();
        });
        await page.locator("#t-lazy-tab-slow").click();
        assert.equal(await page.locator("#t-lazy-panel-slow").getAttribute("aria-busy"), "true");
        assert.equal(await page.locator("#t-lazy-panel-slow [data-slot=skeleton]").count() > 0, true);
        await page.locator("#t-lazy-panel-slow").getByText("The slow tab, loaded").waitFor();
        assert.equal(await page.locator("#t-lazy-panel-slow").getAttribute("aria-busy"), null);
        await page.done();
      });

      test("T4 a failed load says so, Retry works, and showing the tab again tries again", async () => {
        const page = await open("/menus");
        let fail = true;
        await page.route("**/menus/tab/slow", (route) => (fail ? route.fulfill({ status: 500, body: "no" }) : route.continue()));
        await page.locator("#t-lazy-tab-slow").click();
        const alert = page.locator("#t-lazy-panel-slow [data-slot=tab-alert]");
        await alert.waitFor();
        assert.match(await alert.innerText(), /This tab didn't load/);
        assert.equal(await page.locator("#t-lazy-panel-slow").getAttribute("aria-busy"), null);
        fail = false;
        await alert.getByRole("button", { name: "Retry" }).click();
        await page.locator("#t-lazy-panel-slow").getByText("The slow tab, loaded").waitFor();
        assert.equal(await alert.count(), 0);
        // A network failure too, and showing the tab again retries it.
        const second = await open("/menus");
        let down = true;
        await second.route("**/menus/tab/slow", (route) => (down ? route.abort() : route.continue()));
        await second.locator("#t-lazy-tab-slow").click();
        await second.locator("#t-lazy-panel-slow [data-slot=tab-alert]").waitFor();
        down = false;
        await second.locator("#t-lazy-tab-fresh").click();
        await second.locator("#t-lazy-tab-slow").click();
        await second.locator("#t-lazy-panel-slow").getByText("The slow tab, loaded").waitFor();
        await second.done();
        page.problems.length = 0;
        await page.done();
      });

      test("T4 an older answer never replaces a newer one", async () => {
        const page = await open("/menus");
        let first = true;
        await page.route("**/menus/tab/fresh", async (route) => {
          if (first) {
            first = false;
            await new Promise((r) => setTimeout(r, 900));
          }
          await route.continue();
        });
        await page.locator("#t-lazy-tab-fresh").click();
        await page.locator("#t-lazy-tab-fast").click();
        await page.locator("#t-lazy-tab-fresh").click();
        await page.waitForTimeout(1500);
        const text = await page.locator("#t-lazy-panel-fresh").innerText();
        const hits = (await (await page.request.get(BASE + "/menus/hits")).json()).fresh;
        // The newest request reached the server first and said so; the late reply said hits.
        assert.match(text, new RegExp(`loaded ${hits - 1} time`), text);
        await page.done();
      });

      test("T5 at 390 px the list scrolls and the page doesn't; the open tab stays in view; the edge fades", async () => {
        const page = await open("/menus", { viewport: { width: 390, height: 800 } });
        const list = page.locator("#t-many > [role=tablist]");
        const m = await list.evaluate((e) => ({ scroll: e.scrollWidth, client: e.clientWidth }));
        assert.ok(m.scroll > m.client * 2, JSON.stringify(m));
        assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true, "no sideways scroll on the page");
        assert.equal(await list.getAttribute("data-fade-r"), "");
        assert.equal(await list.getAttribute("data-fade-l"), null);
        await page.locator("#t-many-tab-t1").focus();
        await page.keyboard.press("End");
        const visible = await page.evaluate(() => {
          const l = document.querySelector("#t-many > [role=tablist]").getBoundingClientRect();
          const t = document.querySelector("#t-many-tab-t24").getBoundingClientRect();
          return t.left >= l.left - 1 && t.right <= l.right + 1;
        });
        assert.equal(visible, true, "the last tab was scrolled into view");
        assert.equal(await list.getAttribute("data-fade-l"), "");
        assert.equal(await list.getAttribute("data-fade-r"), null);
        await page.done();
      });

      test("T6 an unknown or disabled key opens the first tab", async () => {
        for (const q of ["tab=nope", "tab=files", "tab="]) {
          const page = await open("/menus?" + q);
          assert.equal(await selected(page, "t-line"), "Overview", q);
          assert.equal(await page.locator("#t-line-panel-files").isHidden(), true);
          await page.done();
        }
      });

      test("T7 a link to another tab of the page morphs in and the tab stops are restored", async () => {
        const page = await open("/menus");
        await page.evaluate(() => {
          const link = document.createElement("a");
          link.id = "to-settings";
          link.href = "/menus?tab=settings";
          link.textContent = "Go to settings";
          document.querySelector("main").prepend(link);
        });
        await page.locator("#to-settings").click();
        await page.waitForURL(/tab=settings/);
        await page.waitForFunction(() => document.querySelector("#t-line-tab-settings").getAttribute("aria-selected") === "true");
        assert.equal(await stops(page, "#t-line > [role=tablist] > [role=tab]"), 1);
        assert.equal(await page.evaluate(() => document.querySelector("#t-line-tab-settings").tabIndex), 0);
        await page.goBack();
        await page.waitForFunction(() => document.querySelector("#t-line-tab-overview").getAttribute("aria-selected") === "true");
        await page.done();
      });

      test("T8 a find that matches a hidden panel opens its tab", async () => {
        const page = await open("/menus");
        assert.equal(await page.locator("#t-find-panel-b").getAttribute("hidden"), "until-found");
        await page.locator("#t-find-panel-b").evaluate((e) => e.dispatchEvent(new Event("beforematch", { bubbles: true })));
        assert.equal(await selected(page, "t-find"), "Second");
        assert.equal(await page.locator("#t-find-panel-b").getAttribute("hidden"), null);
        await page.done();
      });

      test("a tab with a very long label, one tab, and no panel don't break the layout", async () => {
        const page = await open("/menus", { viewport: { width: 390, height: 800 } });
        assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true);
        await page.done();
      });
    });

    // ---- Toggle groups ---------------------------------------------------

    describe("toggle groups", () => {
      test("G1 a single group posts one value and filters a region from a GET form", async () => {
        const page = await open("/menus");
        await page.getByRole("radio", { name: "Grid", exact: true }).check({ force: true });
        await page.locator("#g-result").getByText("Showing as grid.").waitFor();
        assert.match(page.url(), /view=grid/);
        assert.equal(await page.getByRole("radio", { name: "Chart" }).isChecked(), false);
        await page.goBack();
        await page.locator("#g-result").getByText("Showing as list.").waitFor();
        await page.done();
      });

      test("G1 with JavaScript off: a single group chooses with the radio and Apply", async () => {
        const page = await open("/menus", { javaScriptEnabled: false });
        await page.getByRole("radio", { name: "Chart" }).check({ force: true });
        await page.getByRole("button", { name: "Apply" }).click();
        await page.waitForURL(/view=chart/);
        assert.equal(await page.getByText("Showing as chart.").isVisible(), true);
        assert.equal(await page.getByRole("radio", { name: "Chart" }).isChecked(), true);
        await page.done();
      });

      test("G1 a multiple group posts every item that is on, and not a disabled one", async () => {
        const page = await open("/menus");
        await page.getByRole("checkbox", { name: "Mon" }).uncheck({ force: true });
        await page.getByRole("checkbox", { name: "Fri" }).check({ force: true });
        await page.getByRole("button", { name: "Save days" }).click();
        await toasts(page).filter({ hasText: "Posted" }).waitFor();
        const text = await toasts(page).first().innerText();
        assert.match(text, /days=wed, days=fri/);
        assert.doesNotMatch(text, /thu|mon/);
        await page.done();
      });

      test("G1 with JavaScript off a plain form posts both groups and the flash is a toast", async () => {
        const page = await open("/menus", { javaScriptEnabled: false });
        await page.locator("#g-plain-form").getByRole("checkbox", { name: "B" }).check({ force: true });
        await page.locator("#g-plain-form").getByRole("radio", { name: "S" }).check({ force: true });
        await page.getByRole("button", { name: "Post", exact: true }).click();
        await page.waitForURL("**/menus");
        const toast = page.locator("[data-slot=toaster] [data-slot=toast]");
        await toast.waitFor();
        assert.match(await toast.innerText(), /plain=b, size=s/);
        assert.equal(await toast.getAttribute("role"), "status");
        await page.done();
      });

      test("G2 a group of checkboxes has one tab stop; arrows, Home, End skip the disabled item; Space and Enter toggle", async () => {
        const page = await open("/menus");
        assert.equal(await stops(page, "#g-days-form input"), 1);
        await page.getByRole("checkbox", { name: "Mon" }).focus();
        await page.keyboard.press("ArrowRight");
        assert.equal(await page.evaluate(() => document.activeElement.value), "tue");
        await page.keyboard.press("ArrowRight");
        await page.keyboard.press("ArrowRight");
        assert.equal(await page.evaluate(() => document.activeElement.value), "fri", "Thu is disabled and skipped");
        await page.keyboard.press("ArrowRight");
        assert.equal(await page.evaluate(() => document.activeElement.value), "mon", "wraps");
        await page.keyboard.press("End");
        assert.equal(await page.evaluate(() => document.activeElement.value), "fri");
        await page.keyboard.press("Home");
        assert.equal(await stops(page, "#g-days-form input"), 1);
        await page.keyboard.press("ArrowRight");
        await page.keyboard.press(" ");
        assert.equal(await page.getByRole("checkbox", { name: "Tue" }).isChecked(), true);
        await page.keyboard.press("Enter");
        assert.equal(await page.getByRole("checkbox", { name: "Tue" }).isChecked(), false, "Enter toggles it too, and doesn't submit");
        await page.keyboard.press("Tab");
        assert.equal(await focused(page), "Save days", "one Tab leaves the group");
        await page.done();
      });

      test("G2 a single group: native arrows choose; a deselectable one turns off and on; a disabled group is inert", async () => {
        const page = await open("/menus");
        await page.getByRole("radio", { name: "List", exact: true }).first().focus();
        await page.keyboard.press("ArrowRight");
        assert.equal(await page.getByRole("radio", { name: "Grid", exact: true }).isChecked(), true);
        // Deselectable and vertical.
        const list = page.getByRole("radio", { name: "As a list" });
        assert.equal(await list.isChecked(), true);
        await list.focus();
        await page.keyboard.press("ArrowDown");
        assert.equal(await page.getByRole("radio", { name: "As a grid" }).isChecked(), true);
        await page.getByRole("radio", { name: "As a grid" }).evaluate((e) => e.parentElement.click());
        assert.equal(await page.getByRole("radio", { name: "As a grid" }).isChecked(), false, "a click on the chosen item turns it off");
        await page.getByRole("radio", { name: "As a grid" }).evaluate((e) => e.parentElement.click());
        assert.equal(await page.getByRole("radio", { name: "As a grid" }).isChecked(), true, "and on again");
        await page.keyboard.press("Home");
        assert.equal(await page.evaluate(() => document.activeElement.value), "list");
        assert.equal(await page.locator("input[name=all-off]").first().isDisabled(), true);
        await page.done();
      });

      test("G2 icon-only items have a name and a group has a label", async () => {
        const page = await open("/menus");
        for (const label of ["As a list", "As a grid", "Settings"]) assert.equal(await page.getByRole("radio", { name: label }).count(), 1, label);
        assert.equal(await page.getByRole("radiogroup", { name: "Layout" }).count(), 1);
        assert.equal(await page.getByRole("group", { name: "Working days" }).count(), 1);
        await page.done();
      });
    });

    // ---- Dropdown menus --------------------------------------------------

    describe("dropdown menus", () => {
      const menuItems = (page, id) => page.locator(`#${id} [role^=menuitem]`);

      test("M1 Enter, Space and the arrows open it; Esc returns focus; Tab closes; arrows wrap", async () => {
        const page = await open("/menus");
        const trigger = page.locator("#m-basic-trigger");
        const menu = page.locator("#m-basic");
        assert.equal(await trigger.getAttribute("aria-expanded"), "false");
        await trigger.focus();
        await page.keyboard.press("Enter");
        await menu.waitFor();
        await focusIs(page, "EditE");
        assert.equal(await trigger.getAttribute("aria-expanded"), "true");
        await page.keyboard.press("Escape");
        await menu.waitFor({ state: "hidden" });
        assert.equal(await page.evaluate(() => document.activeElement.id), "m-basic-trigger");
        assert.equal(await trigger.getAttribute("aria-expanded"), "false");
        await page.keyboard.press(" ");
        await menu.waitFor();
        await focusIs(page, "EditE");
        await page.keyboard.press("Escape");
        await page.keyboard.press("ArrowUp");
        await menu.waitFor();
        await focusIs(page, "Delete");
        await page.keyboard.press("ArrowDown");
        assert.equal(await focused(page), "EditE", "wraps down");
        await page.keyboard.press("ArrowUp");
        assert.equal(await focused(page), "Delete", "wraps up");
        await page.keyboard.press("Home");
        assert.equal(await focused(page), "EditE");
        await page.keyboard.press("End");
        assert.equal(await focused(page), "Delete");
        await page.keyboard.press("Tab");
        await menu.waitFor({ state: "hidden" });
        assert.notEqual(await page.evaluate(() => document.activeElement.closest("#m-basic")), true);
        await page.keyboard.press("Escape");
        await trigger.focus();
        await page.keyboard.press("ArrowDown");
        await menu.waitFor();
        assert.equal(await focused(page), "EditE");
        await page.done();
      });

      test("M1 a press opens it with the menu focused, and the arrows then start from the ends", async () => {
        const page = await open("/menus");
        await page.locator("#m-basic-trigger").click();
        await page.locator("#m-basic").waitFor();
        await page.waitForFunction(() => document.activeElement.id === "m-basic");
        await page.keyboard.press("ArrowDown");
        assert.equal(await focused(page), "EditE");
        await page.done();
      });

      test("M2 typeahead finds items, repeated letters cycle, and a disabled item is reached but does nothing", async () => {
        const page = await open("/menus");
        await page.locator("#m-basic-trigger").focus();
        await page.keyboard.press("ArrowDown");
        await page.keyboard.type("du");
        assert.equal(await focused(page), "DuplicateCtrl+D");
        await page.waitForTimeout(600);
        await page.keyboard.type("e");
        assert.equal(await focused(page), "Email", "the next item that starts with e, after this one");
        await page.waitForTimeout(600);
        await page.keyboard.type("e");
        assert.equal(await focused(page), "EditE", "and again, round to the first");
        await page.waitForTimeout(600);
        await page.keyboard.type("ee");
        assert.equal(await focused(page), "EditE", "a repeated letter cycles one item per press");
        await page.waitForTimeout(600);
        await page.keyboard.type("r");
        assert.equal(await focused(page), "Rename");
        await page.waitForTimeout(600);
        await page.keyboard.type("c");
        assert.equal(await focused(page), "Copy link");
        // Archive is disabled: the arrows reach it, Enter does nothing and the menu stays.
        await page.keyboard.press("Home");
        await page.keyboard.press("ArrowDown");
        await page.keyboard.press("ArrowDown");
        assert.equal(await focused(page), "Archive");
        assert.equal(await page.locator("#m-basic [data-disabled]").getAttribute("aria-disabled"), "true");
        await page.keyboard.press("Enter");
        assert.equal(await page.locator("#m-basic").isVisible(), true);
        await page.done();
      });

      test("M3 checkbox and radio items toggle, close the menu unless told not to, and post with the form", async () => {
        const page = await open("/menus");
        const trigger = page.locator("#m-view-trigger");
        await trigger.focus();
        await page.keyboard.press("ArrowDown");
        assert.equal(await focused(page), "Show archived");
        assert.equal(await page.getByRole("menuitemcheckbox", { name: "Show archived" }).isChecked(), true);
        await page.keyboard.press(" ");
        await page.locator("#m-view").waitFor({ state: "hidden" });
        assert.equal(await page.evaluate(() => document.activeElement.id), "m-view-trigger");
        await trigger.focus();
        await page.keyboard.press("ArrowDown");
        assert.equal(await page.getByRole("menuitemcheckbox", { name: "Show archived" }).isChecked(), false, "it was turned off");
        await page.keyboard.press("ArrowDown");
        assert.equal(await focused(page), "Show owner");
        await page.keyboard.press("Enter");
        assert.equal(await page.locator("#m-view").isVisible(), true, "keep_open leaves it open");
        assert.equal(await page.getByRole("menuitemcheckbox", { name: "Show owner" }).isChecked(), true);
        await page.keyboard.press("ArrowDown");
        await page.keyboard.press("ArrowDown");
        assert.equal(await focused(page), "Date");
        await page.keyboard.press("Enter");
        await page.locator("#m-view").waitFor({ state: "hidden" });
        await page.getByRole("button", { name: "Save view" }).click();
        const toast = toasts(page).filter({ hasText: "Posted" });
        await toast.waitFor();
        const text = await toast.innerText();
        assert.match(text, /owner=on/);
        assert.match(text, /sort=date/);
        assert.doesNotMatch(text, /archived/, "an unchecked box posts nothing");
        await page.done();
      });

      test("M4 a submenu opens with the keys, closes one level at a time, and its item closes everything", async () => {
        const page = await open("/menus");
        await page.locator("#m-sub-trigger").focus();
        await page.keyboard.press("ArrowDown");
        await page.keyboard.press("ArrowDown");
        assert.equal(await focused(page), "Move to");
        assert.equal(await page.locator("#m-sub-s1-trigger").getAttribute("aria-expanded"), "false");
        await page.keyboard.press("ArrowRight");
        await page.locator("#m-sub-s1").waitFor();
        assert.equal(await focused(page), "Inbox");
        assert.equal(await page.locator("#m-sub-s1-trigger").getAttribute("aria-expanded"), "true");
        assert.equal(await page.locator("#m-sub").isVisible(), true, "the parent stays open");
        await page.keyboard.press("ArrowDown");
        await page.keyboard.press("ArrowDown");
        assert.equal(await focused(page), "Label as");
        await page.keyboard.press("Enter");
        await page.locator("#m-sub-s2").waitFor();
        assert.equal(await focused(page), "Urgent");
        await page.keyboard.press("ArrowLeft");
        await page.locator("#m-sub-s2").waitFor({ state: "hidden" });
        assert.equal(await focused(page), "Label as", "Left closes one level and returns to its item");
        await page.keyboard.press("Escape");
        await page.locator("#m-sub-s1").waitFor({ state: "hidden" });
        assert.equal(await focused(page), "Move to", "Esc closes the open submenu first");
        await page.keyboard.press("ArrowRight");
        await page.keyboard.press("ArrowRight");
        await page.keyboard.press("ArrowDown");
        await page.keyboard.press("ArrowDown");
        await page.keyboard.press("Enter");
        await page.locator("#m-sub-s2").waitFor();
        await page.keyboard.press("Enter");
        await toasts(page).filter({ hasText: "Labelled Urgent" }).waitFor();
        assert.equal(await page.locator("#m-sub").isHidden(), true, "choosing closes the whole menu");
        assert.equal(await page.evaluate(() => document.activeElement.id), "m-sub-trigger");
        await page.done();
      });

      test("M4 a submenu opens on hover and stays while the pointer crosses to it; leaving for another item closes it", async () => {
        const page = await open("/menus");
        await page.locator("#m-sub-trigger").click();
        const moveTo = await page.locator("#m-sub-s1-trigger").boundingBox();
        await page.mouse.move(moveTo.x + 20, moveTo.y + moveTo.height / 2);
        await page.locator("#m-sub-s1").waitFor();
        await placed(page, "#m-sub-s1");
        const sub = await page.locator("#m-sub-s1").boundingBox();
        // A slanting path to the submenu, passing over the item below the trigger.
        await page.mouse.move(sub.x + 10, sub.y + sub.height / 2, { steps: 6 });
        await page.waitForTimeout(450);
        assert.equal(await page.locator("#m-sub-s1").isVisible(), true, "reached it, so it stays");
        await page.getByRole("menuitem", { name: "Inbox" }).click();
        await toasts(page).filter({ hasText: "Moved to Inbox" }).waitFor();
        assert.equal(await page.locator("#m-sub").isHidden(), true);
        // Resting on another item in the parent closes the submenu.
        await page.locator("#m-sub-trigger").click();
        const t = await page.locator("#m-sub-s1-trigger").boundingBox();
        await page.mouse.move(t.x + 20, t.y + t.height / 2);
        await page.locator("#m-sub-s1").waitFor();
        const rename = await page.getByRole("menuitem", { name: "Rename" }).last().boundingBox();
        await page.mouse.move(rename.x + 20, rename.y + rename.height / 2, { steps: 3 });
        await page.locator("#m-sub-s1").waitFor({ state: "hidden", timeout: 2000 });
        assert.equal(await page.locator("#m-sub").isVisible(), true);
        await page.done();
      });

      test("M4 a disabled submenu does not open", async () => {
        const page = await open("/menus");
        await page.locator("#m-sub-trigger").focus();
        await page.keyboard.press("ArrowDown");
        await page.keyboard.press("ArrowDown");
        await page.keyboard.press("ArrowDown");
        assert.equal(await focused(page), "Disabled submenu");
        await page.keyboard.press("ArrowRight");
        await page.keyboard.press("Enter");
        assert.equal(await page.locator("#m-sub-s3").count(), 1);
        assert.equal(await page.locator("#m-sub-s3").isHidden(), true);
        await page.done();
      });

      test("M5 it flips above a trigger at the bottom edge and shifts in from the sides", async () => {
        const page = await open("/menus", { viewport: { width: 1280, height: 520 } });
        await page.locator("#m-basic-trigger").evaluate((e) => e.scrollIntoView({ block: "end" }));
        await page.locator("#m-basic-trigger").click();
        await page.locator("#m-basic").waitFor();
        await placed(page, "#m-basic");
        const box = await page.locator("#m-basic").boundingBox();
        const trigger = await page.locator("#m-basic-trigger").boundingBox();
        assert.ok(box.y >= 0 && box.y + box.height <= 520, JSON.stringify(box));
        assert.ok(box.y + box.height <= trigger.y + 1 || box.y >= trigger.y + trigger.height, "it doesn't cover its button");
        await page.keyboard.press("Escape");
        // A row menu at the end of a table lines up with the end of its button and stays inside.
        await page.locator("#m-row-1-trigger").scrollIntoViewIfNeeded();
        await page.locator("#m-row-1-trigger").click();
        await placed(page, "#m-row-1");
        const row = await page.locator("#m-row-1").boundingBox();
        const rt = await page.locator("#m-row-1-trigger").boundingBox();
        assert.ok(row.x >= 8 && row.x + row.width <= 1280 - 8, JSON.stringify(row));
        assert.ok(Math.abs(row.x + row.width - (rt.x + rt.width)) < 2 || row.x + row.width <= 1280 - 8, "end aligned or shifted in");
        await page.done();
      });

      test("M5 at 390 px it stays inside the screen; a menu taller than the window scrolls inside and keeps its place on resize", async () => {
        const page = await open("/menus", { viewport: { width: 390, height: 700 } });
        await page.locator("#m-right-trigger").scrollIntoViewIfNeeded();
        await page.locator("#m-right-trigger").click();
        await placed(page, "#m-right");
        const small = await page.locator("#m-right").boundingBox();
        assert.ok(small.x >= 7 && small.x + small.width <= 390 - 7, JSON.stringify(small));
        await page.keyboard.press("Escape");
        await page.setViewportSize({ width: 1280, height: 420 });
        await page.locator("#m-long-trigger").scrollIntoViewIfNeeded();
        await page.locator("#m-long-trigger").click();
        await page.locator("#m-long").waitFor();
        await placed(page, "#m-long");
        const tall = await page.locator("#m-long").evaluate((e) => ({ h: e.getBoundingClientRect().height, scroll: e.scrollHeight, client: e.clientHeight }));
        assert.ok(tall.h <= 420 - 8, JSON.stringify(tall));
        assert.ok(tall.scroll > tall.client, "it scrolls inside");
        await page.keyboard.press("End");
        assert.equal(await focused(page), "Item 60");
        // A browser may scroll to a focused item a frame later.
        await page.waitForFunction(() => {
          const m = document.querySelector("#m-long").getBoundingClientRect();
          const i = document.activeElement.getBoundingClientRect();
          return i.top >= m.top - 1 && i.bottom <= m.bottom + 1;
        }, null, { timeout: 3000 });
        await page.setViewportSize({ width: 1280, height: 600 });
        await page.waitForTimeout(250);
        const after = await page.locator("#m-long").boundingBox();
        assert.ok(after.y >= 0 && after.y + after.height <= 600, "placed again after a resize: " + JSON.stringify(after));
        await page.done();
      });

      test("M5 in a right-to-left page the menu lines up with the end of its button and a submenu opens toward the end", async () => {
        const page = await open("/menus");
        await page.evaluate(() => { document.documentElement.dir = "rtl"; });
        await page.locator("#m-row-1-trigger").scrollIntoViewIfNeeded();
        await page.locator("#m-row-1-trigger").click();
        await placed(page, "#m-row-1");
        const menu = await page.locator("#m-row-1").boundingBox();
        const trigger = await page.locator("#m-row-1-trigger").boundingBox();
        assert.ok(Math.abs(menu.x - trigger.x) < 2 || menu.x >= 8, "end is the left edge in right-to-left: " + JSON.stringify({ menu, trigger }));
        assert.ok(Math.abs(menu.x - trigger.x) < 2, "aligned with the button's left edge: " + JSON.stringify({ menu, trigger }));
        await page.keyboard.press("Escape");
        await page.locator("#m-sub-trigger").scrollIntoViewIfNeeded();
        await page.locator("#m-sub-trigger").click();
        const parent = await page.locator("#m-sub").boundingBox();
        await page.getByRole("menuitem", { name: "Move to" }).hover();
        await page.locator("#m-sub-s1").waitFor();
        await placed(page, "#m-sub-s1");
        const sub = await page.locator("#m-sub-s1").boundingBox();
        assert.ok(sub.x + sub.width <= parent.x + 8, "the submenu is on the left of its parent: " + JSON.stringify({ parent, sub }));
        // The keys swap too: Left opens, Right closes.
        await page.keyboard.press("Escape");
        await page.locator("#m-sub-trigger").focus();
        await page.keyboard.press("ArrowDown");
        await page.keyboard.press("ArrowDown");
        await page.keyboard.press("ArrowLeft");
        await page.locator("#m-sub-s1").waitFor();
        await page.keyboard.press("ArrowRight");
        await page.locator("#m-sub-s1").waitFor({ state: "hidden" });
        await page.done();
      });

      test("M6 link, action and post items: each does its work and closes the menu", async () => {
        const page = await open("/menus");
        await page.locator("#m-basic-trigger").click();
        await page.getByRole("menuitem", { name: "Duplicate" }).click();
        await toasts(page).filter({ hasText: "Duplicated." }).waitFor();
        assert.equal(await page.locator("#m-basic").isHidden(), true);
        await page.locator("#m-basic-trigger").click();
        await page.getByRole("menuitem", { name: "Delete" }).click();
        await toasts(page).filter({ hasText: "Posted: " }).waitFor();
        assert.doesNotMatch(await toasts(page).filter({ hasText: "Posted: " }).innerText(), /_csrf/);
        await page.locator("#m-basic-trigger").click();
        await page.getByRole("menuitem", { name: "Edit" }).click();
        await page.waitForURL(/edited=1/);
        assert.equal(await page.locator("#m-basic").isHidden(), true);
        await page.done();
      });

      test("M6 with JavaScript off the menu still opens and its links and posts work", async () => {
        const page = await open("/menus", { javaScriptEnabled: false });
        await page.locator("#m-basic-trigger").click();
        assert.equal(await page.locator("#m-basic").isVisible(), true);
        await page.getByRole("menuitem", { name: "Delete" }).click();
        await page.waitForURL("**/menus");
        assert.match(await page.locator("[data-slot=toaster] [data-slot=toast]").innerText(), /Posted:/);
        await page.locator("#m-basic-trigger").click();
        await page.getByRole("menuitem", { name: "Edit" }).click();
        await page.waitForURL(/edited=1/);
        await page.done();
      });

      test("M7 a menu in a table row, in a dialog and in a sheet", async () => {
        const page = await open("/menus");
        await page.locator("#m-row-2-trigger").click();
        await page.getByRole("menuitem", { name: "Open" }).last().click();
        await page.waitForURL(/open=2/);
        // In a dialog: opened by a menu item, with its own menu inside.
        await page.locator("#m-basic-trigger").click();
        await page.getByRole("menuitem", { name: "Rename" }).click();
        await page.locator("#m-dialog-box[open]").waitFor();
        await page.locator("#m-in-dialog-trigger").click();
        await page.locator("#m-in-dialog").waitFor();
        const m = await page.locator("#m-in-dialog").boundingBox();
        const top = await page.evaluate(({ x, y }) => document.elementFromPoint(x, y)?.closest("#m-in-dialog") !== null, { x: m.x + 10, y: m.y + 10 });
        assert.equal(top, true, "the menu is above the dialog");
        await page.keyboard.press("Escape");
        await page.locator("#m-in-dialog").waitFor({ state: "hidden" });
        assert.equal(await page.locator("#m-dialog-box").evaluate((e) => e.open), true, "Esc closed the menu, not the dialog");
        await page.keyboard.press("Escape");
        assert.equal(await page.locator("#m-dialog-box").evaluate((e) => e.open), false);
        await page.getByRole("button", { name: "Open a sheet" }).click();
        await page.locator("#m-in-sheet-trigger").click();
        await page.locator("#m-in-sheet").waitFor();
        assert.equal(await page.locator("#m-in-sheet").isVisible(), true);
        await page.done();
      });

      test("M8 the roles in the accessibility tree", async () => {
        const page = await open("/menus");
        await page.locator("#m-view-trigger").click();
        const snapshot = await page.locator("#m-view").ariaSnapshot();
        assert.match(snapshot, /menu/);
        assert.match(snapshot, /menuitemcheckbox "Show archived" \[checked\]/);
        assert.match(snapshot, /group "Sort by"/);
        assert.match(snapshot, /menuitemradio "Name" \[checked\]/);
        await page.done();
      });
    });

    // ---- Toasts ----------------------------------------------------------

    describe("toasts", () => {
      test("N1 four tones from commands; an error is an alert, the rest a status; each has a close button", async () => {
        const page = await open("/menus");
        for (const [id, tone, role] of [["n-info", "info", "status"], ["n-success", "success", "status"], ["n-warning", "warning", "status"], ["n-danger", "danger", "alert"]]) {
          await page.locator("#" + id).click();
          const toast = toasts(page).last();
          await toast.waitFor();
          assert.equal(await toast.getAttribute("data-tone"), tone);
          assert.equal(await toast.getAttribute("role"), role);
          assert.equal(await toast.locator("[data-slot=toast-close]").getAttribute("aria-label"), "Dismiss");
        }
        const durations = await toasts(page).evaluateAll((all) => all.map((t) => t.getAttribute("data-duration")));
        assert.deepEqual(durations, ["normal", "normal", "long", "long"]);
        await page.done();
      });

      test("N1 the close button, and Escape inside a toast, take it away and return focus", async () => {
        const page = await open("/menus");
        await page.locator("#n-info").click();
        await page.locator("#n-success").click();
        await toasts(page).nth(1).waitFor();
        await toasts(page).first().locator("[data-slot=toast-close]").click();
        assert.equal(await toasts(page).count(), 1);
        await toasts(page).first().locator("[data-slot=toast-close]").focus();
        await page.keyboard.press("Escape");
        assert.equal(await toasts(page).count(), 0);
        await page.done();
      });

      test("N1 a server toast has a title and an action that posts; the action's answer is a toast", async () => {
        const page = await open("/menus");
        await page.locator("#n-form-rich button").click();
        const toast = toasts(page).filter({ hasText: "Archived" });
        await toast.waitFor();
        assert.equal(await toast.locator("[data-slot=toast-title]").innerText(), "Archived");
        assert.equal(await toast.getAttribute("data-duration"), "long", "a toast with an action stays long");
        await toast.getByRole("button", { name: "Undo" }).click();
        await toasts(page).filter({ hasText: "Undone." }).waitFor();
        assert.equal(await toasts(page).filter({ hasText: "Archived" }).count(), 0, "the toast with the action went");
        await page.done();
      });

      test("N1 a sticky toast has no life animation and stays", async () => {
        const page = await open("/menus");
        await page.locator("#n-form-sticky button").click();
        const toast = toasts(page).filter({ hasText: "Import failed" });
        await toast.waitFor();
        assert.equal(await toast.getAttribute("data-duration"), "sticky");
        assert.equal(await toast.getAttribute("role"), "alert");
        const life = await toast.evaluate((t) => t.getAnimations().filter((a) => a.animationName === "ui-toast-life").length);
        assert.equal(life, 0);
        await page.waitForTimeout(800);
        assert.equal(await toast.count(), 1);
        await page.done();
      });

      test("N1 a toast ends by itself, in the time its duration names", async () => {
        const page = await open("/menus");
        await page.locator("#n-info").click();
        const toast = toasts(page).first();
        await toast.waitFor();
        const ms = await toast.evaluate((t) => t.getAnimations().find((a) => a.animationName === "ui-toast-life").effect.getComputedTiming().duration);
        assert.equal(ms, 5000);
        await page.locator("#n-warning").click();
        const long = await toasts(page).last().evaluate((t) => t.getAnimations().find((a) => a.animationName === "ui-toast-life").effect.getComputedTiming().duration);
        assert.equal(long, 8000);
        const gone = await toasts(page).first().elementHandle();
        await page.waitForFunction((el) => !el.isConnected, gone, { timeout: 7000 });
        assert.equal(await toasts(page).count(), 1, "the longer one is still showing");
        await page.done();
      });

      test("N2 hovering or focusing a toast pauses its clock; leaving lets it run", async () => {
        const page = await open("/menus");
        await page.locator("#n-info").click();
        const toast = toasts(page).first();
        await toast.waitFor();
        const time = () => toast.evaluate((t) => t.getAnimations().find((a) => a.animationName === "ui-toast-life").currentTime);
        await toast.hover();
        const a = await time();
        await page.waitForTimeout(500);
        const b = await time();
        assert.ok(b - a < 60, `paused while hovered: ${a} -> ${b}`);
        await page.mouse.move(5, 5);
        const c = await time();
        await page.waitForTimeout(400);
        assert.ok((await time()) - c > 300, "runs again after the pointer leaves");
        await toast.locator("[data-slot=toast-close]").focus();
        const d = await time();
        await page.waitForTimeout(400);
        assert.ok((await time()) - d < 60, "paused while focused");
        await page.done();
      });

      test("N2 a hidden tab pauses the toasts and showing it again lets them run", async () => {
        const context = await browser.newContext({ viewport: { width: 1280, height: 800 } });
        // The runtime reads the page's visibility through the prototype when it starts, so the page is told what to say first.
        await context.addInitScript(() => {
          window.__hidden = false;
          Object.defineProperty(Document.prototype, "hidden", { get: () => window.__hidden, configurable: true });
        });
        const page = await context.newPage();
        await page.goto(BASE + "/menus");
        await page.locator("#n-info").click();
        const toast = toasts(page).first();
        await toast.waitFor();
        const time = () => toast.evaluate((t) => t.getAnimations().find((a) => a.animationName === "ui-toast-life").currentTime);
        await page.evaluate(() => { window.__hidden = true; document.dispatchEvent(new Event("visibilitychange")); });
        assert.equal(await page.locator("[data-slot=toaster]").getAttribute("data-paused"), "");
        const a = await time();
        await page.waitForTimeout(500);
        assert.ok((await time()) - a < 60, "the clock stood still while the tab was hidden");
        await page.evaluate(() => { window.__hidden = false; document.dispatchEvent(new Event("visibilitychange")); });
        assert.equal(await page.locator("[data-slot=toaster]").getAttribute("data-paused"), null);
        const b = await time();
        await page.waitForTimeout(400);
        assert.ok((await time()) - b > 300, "and ran again when it came back");
        await context.close();
      });

      test("N3 a toast swiped sideways with a touch goes; a short drag springs back", async () => {
        const context = await browser.newContext({ viewport: { width: 390, height: 800 }, hasTouch: true });
        const page = await context.newPage();
        const problems = [];
        page.on("pageerror", (e) => problems.push(e.message));
        await page.goto(BASE + "/menus");
        await page.locator("#n-info").click();
        const toast = toasts(page).first();
        await toast.waitFor();
        const fire = (type, x, id = 1) => toast.evaluate((t, [type, x, id]) => {
          const box = t.getBoundingClientRect();
          t.dispatchEvent(new PointerEvent(type, { pointerType: "touch", isPrimary: true, pointerId: id, bubbles: true, clientX: box.left + box.width / 2 + x, clientY: box.top + 10 }));
        }, [type, x, id]);
        // Short: springs back.
        await fire("pointerdown", 0);
        await toast.evaluate((t) => t.dispatchEvent(new PointerEvent("pointermove", { pointerType: "touch", isPrimary: true, pointerId: 1, bubbles: true, clientX: t.getBoundingClientRect().left + t.getBoundingClientRect().width / 2 + 20, clientY: 0 })));
        assert.equal(await toast.getAttribute("data-dragging"), "");
        await toast.evaluate((t) => t.dispatchEvent(new PointerEvent("pointerup", { pointerType: "touch", isPrimary: true, pointerId: 1, bubbles: true, clientX: 0, clientY: 0 })));
        await page.waitForTimeout(300);
        assert.equal(await toasts(page).count(), 1);
        assert.equal(await toast.evaluate((t) => t.style.translate), "");
        // Far: goes.
        const box = await toast.boundingBox();
        await toast.evaluate((t) => {
          const b = t.getBoundingClientRect();
          const at = (type, dx) => t.dispatchEvent(new PointerEvent(type, { pointerType: "touch", isPrimary: true, pointerId: 2, bubbles: true, clientX: b.left + 40 + dx, clientY: b.top + 10 }));
          at("pointerdown", 0);
          at("pointermove", 60);
          at("pointermove", 140);
          at("pointerup", 140);
        });
        await toasts(page).first().waitFor({ state: "detached", timeout: 2000 });
        assert.ok(box.width > 0);
        assert.deepEqual(problems, []);
        // The toast lets the page scroll vertically with a finger.
        await page.locator("#n-success").click();
        assert.equal(await toasts(page).first().evaluate((t) => getComputedStyle(t).touchAction), "pan-y");
        await context.close();
      });

      test("N3 in Chromium a real touch swipe dismisses it", { skip: name !== "chromium" }, async () => {
        const context = await browser.newContext({ viewport: { width: 390, height: 800 }, hasTouch: true, isMobile: true });
        const page = await context.newPage();
        await page.goto(BASE + "/menus");
        await page.locator("#n-info").tap();
        const toast = toasts(page).first();
        await toast.waitFor();
        const box = await toast.boundingBox();
        const client = await context.newCDPSession(page);
        const y = box.y + box.height / 2;
        const touch = (type, x) => client.send("Input.dispatchTouchEvent", { type, touchPoints: type === "touchEnd" ? [] : [{ x, y }] });
        await touch("touchStart", box.x + 30);
        for (const dx of [20, 50, 90, 140, 200]) await touch("touchMove", box.x + 30 + dx);
        await touch("touchEnd", 0);
        await toast.waitFor({ state: "detached", timeout: 2000 });
        await context.close();
      });

      test("N4 a stack shows three; the rest wait their turn and start their time when shown", async () => {
        const page = await open("/menus");
        await page.locator("#n-six").click();
        await toasts(page).nth(5).waitFor({ state: "attached" });
        const visible = () => toasts(page).evaluateAll((all) => all.map((t) => getComputedStyle(t).display !== "none"));
        assert.deepEqual(await visible(), [false, false, false, true, true, true]);
        assert.equal(await toasts(page).first().evaluate((t) => t.getAnimations().length), 0, "a waiting toast has no clock running");
        await toasts(page).first().waitFor({ state: "visible", timeout: 7000 });
        assert.equal(await toasts(page).count(), 3, "the first three, once the last three went");
        assert.match(await toasts(page).first().innerText(), /number 1/);
        await page.done();
      });

      test("N4 a toast with the same name replaces the one showing", async () => {
        const page = await open("/menus");
        await page.locator("#n-form-id button").click();
        await toasts(page).filter({ hasText: "Saved." }).waitFor();
        assert.equal(await toasts(page).count(), 1);
        assert.equal(await toasts(page).getAttribute("data-toast-id"), "save");
        await page.done();
      });

      test("N5 the region is named and each toast has its live role in the accessibility tree", async () => {
        const page = await open("/menus");
        await page.locator("#n-success").click();
        await page.locator("#n-danger").click();
        await toasts(page).nth(1).waitFor();
        const snapshot = await page.locator("[data-slot=toaster]").ariaSnapshot();
        assert.match(snapshot, /region "Notifications"/);
        assert.match(snapshot, /status/);
        assert.match(snapshot, /alert/);
        assert.match(snapshot, /Success from a command/);
        assert.equal(await page.locator("[data-slot=toaster]").evaluate((e) => e.parentElement.tagName), "BODY", "the host stands in the body, outside what a morph reaches");
        await page.done();
      });

      test("N6 after a post with ops the toast shows, and after a redirect it stays through the navigation", async () => {
        const page = await open("/menus");
        await page.locator("#n-form-plain button").click();
        await toasts(page).filter({ hasText: "Saved from the server." }).waitFor();
        await page.locator("#n-redirect button").click();
        await page.waitForURL(/landed=1/);
        const toast = toasts(page).filter({ hasText: "Saved, and moved on." });
        await toast.waitFor();
        assert.equal(await toast.count(), 1, "not shown twice");
        await page.done();
      });

      test("N6 a refused post raises its error toast", async () => {
        const page = await open("/menus");
        await page.locator("#n-form-fail button").click();
        const toast = toasts(page).filter({ hasText: "That name is taken." });
        await toast.waitFor();
        assert.equal(await toast.getAttribute("role"), "alert");
        await page.done();
      });

      test("N6 with JavaScript off the redirect's flash is a toast on the next page", async () => {
        const page = await open("/menus", { javaScriptEnabled: false });
        await page.locator("#n-redirect-plain button").click();
        await page.waitForURL(/landed=1/);
        const toast = page.locator("[data-slot=toaster] [data-slot=toast]");
        await toast.waitFor();
        assert.match(await toast.innerText(), /Saved, and moved on\./);
        assert.equal(await toast.getAttribute("role"), "status");
        assert.equal(await toast.isVisible(), true);
        await page.done();
      });

      test("N6 a toast shown when the page is loaded for real, or reloaded, shows on the next page", async () => {
        const page = await open("/menus");
        await page.locator("#n-success").click();
        await toasts(page).first().waitFor();
        await page.locator("#n-hard").click();
        await page.waitForURL(/hard=1/);
        const toast = toasts(page).filter({ hasText: "Success from a command" });
        await toast.waitFor();
        assert.equal(await toast.count(), 1);
        // The time it had left comes with it.
        const left = await toast.evaluate((t) => t.getAnimations().find((a) => a.animationName === "ui-toast-life").currentTime);
        assert.ok(left > 0, `elapsed carried over: ${left}`);
        await page.locator("#n-reload button").click();
        await page.waitForLoadState("load");
        await toasts(page).filter({ hasText: "Reloading now." }).waitFor();
        await page.done();
      });

      test("N7 a failed request raises the runtime's own toast", async () => {
        const page = await open("/menus");
        await page.route("**/menus/toast", (route) => route.abort());
        await page.locator("#n-form-plain button").click();
        await toasts(page).filter({ hasText: "No connection. Try again." }).waitFor();
        await page.done();
      });

      test("N8 a toast raised while a dialog is open shows above it", async () => {
        const page = await open("/menus");
        await page.locator("#m-basic-trigger").click();
        await page.getByRole("menuitem", { name: "Rename" }).click();
        await page.locator("#m-dialog-box[open]").waitFor();
        await page.locator("#m-dialog-toast").click();
        const toast = toasts(page).filter({ hasText: "Raised over the dialog." });
        await toast.waitFor();
        const box = await toast.boundingBox();
        const above = await page.evaluate(({ x, y }) => !!document.elementFromPoint(x, y)?.closest("[data-slot=toast]"), { x: box.x + 10, y: box.y + 10 });
        assert.equal(above, true, "the toast is the top layer");
        // The page behind a modal is inert; the toaster stands inside the dialog, so it can be used.
        await toast.locator("[data-slot=toast-close]").click();
        assert.equal(await toasts(page).count(), 0, "its close button works over a dialog");
        await page.locator("#m-dialog-box [data-slot=dialog-footer] button").click();
        await page.locator("#m-dialog-toast").waitFor({ state: "hidden" });
        // The dialog reports that it closed a task after it did.
        await page.waitForFunction(() => document.querySelector("[data-slot=toaster]").parentElement.tagName === "BODY", null, { timeout: 3000 });
        await page.locator("#n-info").click();
        await toasts(page).first().waitFor();
        await page.done();
      });

      test("N9 at 390 px a toast fits the screen, hostile text stays text and a long word wraps", async () => {
        const page = await open("/menus", { viewport: { width: 390, height: 800 } });
        await page.locator("#n-hostile").click();
        const toast = toasts(page).first();
        await toast.waitFor();
        const box = await toast.boundingBox();
        assert.ok(box.x >= 0 && box.x + box.width <= 390, JSON.stringify(box));
        assert.equal(await toast.locator("img").count(), 0);
        assert.match(await toast.innerText(), /<img src=x onerror=alert\(1\)>/);
        const wrapped = await toast.evaluate((t) => t.scrollWidth <= t.clientWidth + 1);
        assert.equal(wrapped, true, "a long word wraps inside the toast");
        await page.done();
      });

      test("N10 stored markup can't stand in for the toaster, or be closed as a toast", async () => {
        const page = await open("/menus");
        await page.evaluate(() => {
          const fake = document.createElement("div");
          fake.innerHTML = '<section data-slot="toaster" id="fake-toaster"></section><div data-slot="toast" id="fake-toast"><button data-slot="toast-close" id="fake-close">x</button></div>';
          document.querySelector("main").prepend(fake);
        });
        await page.locator("#n-info").click();
        await toasts(page).first().waitFor();
        assert.equal(await page.locator("#fake-toaster [data-slot=toast]").count(), 0, "the real toaster took it");
        assert.equal(await page.locator("body > [data-slot=toaster] [data-slot=toast]").count(), 1);
        await page.locator("#fake-close").click();
        assert.equal(await page.locator("#fake-toast").count(), 1, "a toast of stored markup isn't the runtime's to close");
        await page.done();
      });

      test("N10 a hostile op makes only a text toast: no script link, no post to another site, an unknown tone is info", async () => {
        const page = await open("/menus");
        const build = await page.evaluate(() => document.documentElement.getAttribute("data-build"));
        const ops = [
          { op: "toast", tone: "danger", text: "Script link", action: { label: "Go", href: "javascript:alert(1)" } },
          { op: "toast", tone: "success", text: "Post elsewhere", action: { label: "Pay", href: "https://evil.example/pay", post: true } },
          { op: "toast", tone: "<img src=x>", text: "Odd tone", title: "<b>t</b>", duration: "forever", id: "x\"><i>" },
          { op: "toast", tone: "info", text: { not: "text" }, closable: 1 },
        ];
        await page.route("**/menus/toast", (route) => route.fulfill({ status: 200, contentType: "application/vnd.navree.ops+json", body: JSON.stringify({ build, ops }) }));
        await page.locator("#n-form-plain button").click();
        await toasts(page).nth(3).waitFor({ state: "attached" });
        assert.equal(await page.locator("[data-slot=toast-action]").count(), 0, "neither action was drawn");
        assert.equal(await page.locator("[data-slot=toast] img, [data-slot=toast] b, [data-slot=toast] i").count(), 0, "everything is text");
        const tones = await toasts(page).evaluateAll((all) => all.map((t) => t.getAttribute("data-tone") + "/" + t.getAttribute("data-duration")));
        assert.deepEqual(tones, ["danger/long", "success/normal", "info/normal", "info/normal"]);
        await page.done();
      });

      test("N10 fields named like methods can't break the toasts, the tabs or the menus", async () => {
        const page = await open("/menus");
        await page.evaluate(() => {
          const make = () => {
            const form = document.createElement("form");
            form.innerHTML = '<input name="getAttribute"><input name="closest"><input name="contains"><input name="matches"><input name="querySelector"><input name="id"><input name="children">';
            return form;
          };
          document.body.prepend(make());
          document.querySelector("main").prepend(make());
        });
        await page.locator("#n-info").click();
        await toasts(page).first().waitFor();
        // Pointer, key and animation events whose target is such a form.
        await page.evaluate(() => {
          const form = document.querySelector("main form");
          form.dispatchEvent(new PointerEvent("pointerdown", { pointerType: "touch", isPrimary: true, bubbles: true }));
          form.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
          form.dispatchEvent(new AnimationEvent("animationend", { animationName: "ui-toast-life", bubbles: true }));
        });
        await page.locator("#t-line-tab-activity").click();
        await page.locator("#m-basic-trigger").click();
        await page.locator("#m-basic").waitFor();
        await page.keyboard.press("Escape");
        assert.equal(await toasts(page).count() >= 1, true);
        await page.done();
      });

      test("N12 a toast from a command is in the page by the end of the click", async () => {
        const page = await open("/menus");
        const at = await page.evaluate(() => {
          const button = document.querySelector("#n-success");
          button.click();
          return !!document.querySelector("[data-slot=toaster] [data-slot=toast]");
        });
        assert.equal(at, true, "no request and no frame between the click and the toast");
        await page.done();
      });

      test("N9 the six positions", async () => {
        const page = await open("/menus");
        await page.locator("#n-info").click();
        await toasts(page).first().waitFor();
        const at = async (position) => {
          await page.locator("[data-slot=toaster]").evaluate((e, p) => e.setAttribute("data-position", p), position);
          const box = await page.locator("[data-slot=toaster]").boundingBox();
          return { x: box.x + box.width / 2, y: box.y + box.height / 2 };
        };
        const be = await at("bottom-end"), bs = await at("bottom-start"), bc = await at("bottom-center"), te = await at("top-end"), ts = await at("top-start"), tc = await at("top-center");
        assert.ok(be.x > bc.x && bc.x > bs.x, "bottom: end, centre, start");
        assert.ok(te.x > tc.x && tc.x > ts.x, "top: end, centre, start");
        assert.ok(te.y < be.y, "top above bottom");
        await page.done();
      });

      test("Alt+T moves focus into the toasts and Escape returns it", async () => {
        const page = await open("/menus");
        await page.locator("#n-form-rich").scrollIntoViewIfNeeded();
        await page.locator("#n-form-rich button").focus();
        await page.keyboard.press("Enter");
        await toasts(page).filter({ hasText: "Archived" }).waitFor();
        await page.keyboard.press("Alt+t");
        assert.equal(await focused(page), "Undo");
        await page.keyboard.press("Escape");
        assert.equal(await toasts(page).count(), 0);
        assert.equal(await page.evaluate(() => document.activeElement.closest("#n-form-rich") !== null), true);
        await page.done();
      });

      test("a toast the runtime builds has the markup of the one the server draws", async () => {
        const page = await open("/menus");
        // The shape of a toast: tags and attributes, with the text and the name left out.
        const shape = (html) => page.evaluate((markup) => {
          const doc = new DOMParser().parseFromString(markup, "text/html");
          const walk = (node) => {
            const attrs = [...node.attributes].filter((a) => a.name !== "data-toast-id").map((a) => `${a.name}=${a.name === "href" && a.value.startsWith("#ui-toast-") ? "#" : a.name === "href" || a.name === "action" ? a.value : /^(data-|role|type|aria-label)/.test(a.name) ? a.value : ""}`).sort().join(" ");
            const kids = [...node.children].map(walk).join("");
            return `<${node.tagName.toLowerCase()} ${attrs}>${kids}</>`;
          };
          return walk(doc.querySelector("[data-slot=toast]"));
        }, html);
        const drawn = async (path, headers) => (await (await page.request.get(BASE + path, { headers })).text());
        // A flash is a success toast of text only, drawn by the shell.
        const flash = await shape(await drawn("/menus", { cookie: "navree_ui_flash=Saved" }));
        await page.locator("#n-success").click();
        const built = await shape(await toasts(page).first().evaluate((t) => t.outerHTML));
        assert.equal(built, flash, "a text-only success toast");
        // A toast with a title and a post action, drawn by the component and by the runtime from an op.
        const rich = await shape(await drawn("/menus/server-toast"));
        await page.locator("#n-form-rich button").click();
        const fromOp = toasts(page).filter({ hasText: "Archived" });
        await fromOp.waitFor();
        assert.equal(await shape(await fromOp.evaluate((t) => t.outerHTML)), rich, "a rich toast");
        await page.done();
      });
    });

    // ---- Toasts across dialogs, users and pages; posts from a menu in a form; hostile names ----

    describe("toasts, post items and hostile names", () => {
      let f;
      before(async () => {
        f = await start();
      });
      after(() => f.close());

      const TOASTER = `<section data-slot="toaster" aria-label="Notifications" data-position="bottom-end" data-close="Dismiss"></section>`;
      const attrValue = (s) => s.replaceAll("&", "&amp;").replaceAll('"', "&quot;");
      const cmd = (json) => `data-ui-click="${attrValue(JSON.stringify(json))}"`;
      let unique = 0;
      let uniqueRoute = 0;
      const route = (body, headers) => {
        const path = `/r${++unique}`;
        f.routes.set(path, (req, res) => f.html(res, typeof body === "function" ? body(req) : body, headers));
        return path;
      };
      /** A page document of the fixture: the runtime, and the menus module when asked. */
      const paged = (body, options) => route(doc(body, { css: true, ...options }));
      const stickyToast = (text, extra = {}) => cmd([["toast", { tone: "info", text, duration: "sticky", ...extra }]]);

      /** A page on the fixture, with problems kept as `open` keeps them. */
      async function fixture(path, options = {}, before) {
        const context = await browser.newContext({ viewport: { width: 1280, height: 800 }, ...options });
        if (before) await before(context);
        const page = await context.newPage();
        page.problems = [];
        page.on("pageerror", (e) => page.problems.push("pageerror: " + e.message));
        await page.goto(f.origin + path);
        // The page may carry names that shadow the document's own, so it is read through the prototypes.
        await page.waitForFunction(() => {
          const root = Object.getOwnPropertyDescriptor(Document.prototype, "documentElement").get.call(document);
          const host = Document.prototype.querySelector.call(document, "[data-slot=toaster]");
          return Element.prototype.hasAttribute.call(root, "data-build") && !!host && Element.prototype.hasAttribute.call(host, "popover");
        });
        page.done = async () => {
          await context.close();
          assert.deepEqual(page.problems, []);
        };
        return page;
      }

      test("R1 a toast raised by a form in an open dialog survives the redirect to a page without it", async () => {
        const page = await open("/menus");
        await page.locator("#m-basic-trigger").click();
        await page.getByRole("menuitem", { name: "Rename" }).click();
        await page.locator("#m-dialog-box[open]").waitFor();
        await page.locator("#m-dialog-save").click();
        await page.waitForURL(/\/menus\/plain\?landed=1/);
        const toast = toasts(page).filter({ hasText: "Saved, and moved on." });
        await toast.waitFor();
        assert.equal(await page.locator("[data-slot=toaster]").count(), 1);
        assert.equal(await page.locator("[data-slot=toaster]").evaluate((e) => e.parentElement.tagName), "BODY");
        const box = await toast.boundingBox();
        const top = await page.evaluate(({ x, y }) => !!document.elementFromPoint(x, y)?.closest("[data-slot=toast]"), { x: box.x + 10, y: box.y + 10 });
        assert.equal(top, true, "the toast is showing, above the page");
        await toast.locator("[data-slot=toast-close]").click();
        assert.equal(await toasts(page).count(), 0);
        await page.done();
      });

      test("R1 an op that takes the dialog away puts the toaster back in the page first", async () => {
        const path = paged(`<main id="main"><button id="open" ${cmd([["open", "d"]])}>open</button></main><dialog id="d"><form id="go" method="post" action="/swap" data-ui-form><button id="save">Save</button></form></dialog>${TOASTER}`);
        f.routes.set("/swap", (req, res) => f.ops(res, { build: "b1", ops: [
          { op: "toast", tone: "success", text: "Saved.", duration: "sticky" },
          { op: "replace", target: "d", html: '<p id="d">The dialog is gone.</p>' },
        ] }));
        const page = await fixture(path);
        await page.click("#open");
        await page.click("#save");
        await page.locator("#d", { hasText: "The dialog is gone." }).waitFor();
        const toast = toasts(page).filter({ hasText: "Saved." });
        await toast.waitFor();
        assert.equal(await page.locator("[data-slot=toaster]").evaluate((e) => e.parentElement.tagName), "BODY");
        await toast.locator("[data-slot=toast-close]").click();
        assert.equal(await toasts(page).count(), 0);
        await page.done();
      });

      test("R2 a post item in a form posts on its own and leaves the form alone", async () => {
        const page = await open("/menus");
        const sent = [];
        page.on("request", (r) => r.method() === "POST" && sent.push(new URL(r.url()).pathname));
        const shape = await page.evaluate(() => {
          const item = document.querySelector("#m-post [role=menuitem][formaction]");
          return { form: item.getAttribute("form"), nested: document.querySelectorAll("#m-post form").length, owner: item.form?.id, inside: !!item.closest("#m-post-form") };
        });
        assert.deepEqual(shape, { form: "ui-post", nested: 0, owner: "ui-post", inside: true }, "inside the page form, owned by the post form");
        await page.locator("#m-post-trigger").click();
        await page.getByRole("menuitem", { name: "Archive" }).click();
        await toasts(page).filter({ hasText: "Archived by the item." }).waitFor();
        assert.deepEqual(sent, ["/menus/archive"], "only the item's post went");
        assert.equal(await page.locator("#m-post-form [name=note]").inputValue(), "outer");
        await page.locator("#m-post-form").getByRole("button", { name: "Save outer" }).click();
        await toasts(page).filter({ hasText: "Posted: note=outer" }).waitFor();
        assert.deepEqual(sent, ["/menus/archive", "/menus/echo"], "the form's own button still posts the form");
        await page.done();
      });

      test("R2 with JavaScript off the item posts on its own", async () => {
        const page = await open("/menus", { javaScriptEnabled: false });
        const sent = [];
        page.on("request", (r) => r.method() === "POST" && sent.push(new URL(r.url()).pathname));
        await page.locator("#m-post-trigger").click();
        await page.getByRole("menuitem", { name: "Archive" }).click();
        await page.locator("[data-slot=toast]", { hasText: "Archived by the item." }).waitFor();
        assert.deepEqual(sent, ["/menus/archive"], "the post went alone, and its flash is the toast on the page it came back to");
        await page.done();
      });

      test("R2 an item whose page has no post form gets one from the runtime", async () => {
        const path = paged(`<main id="main"><form method="post" action="/outer" data-ui-form><button id="item" type="submit" form="ui-post" formaction="/item" formmethod="post">Item</button><button id="whole">Whole</button></form></main>${TOASTER}`);
        f.routes.set("/item", (req, res) => f.ops(res, { build: "b1", ops: [{ op: "toast", tone: "success", text: "The item." }] }));
        f.routes.set("/outer", (req, res) => f.ops(res, { build: "b1", ops: [{ op: "toast", tone: "success", text: "The form." }] }));
        const page = await fixture(path);
        const before = f.log.length;
        await page.click("#item");
        await toasts(page).filter({ hasText: "The item." }).waitFor();
        const posts = f.log.slice(before).filter((e) => e.method === "POST").map((e) => e.path);
        assert.deepEqual(posts, ["/item"]);
        assert.equal(await page.locator("#ui-post").count(), 1);
        await page.done();
      });

      /** A page that sets the token cookie `token`: the session's identity, as the runtime sees it. */
      const withUser = (token, body) => route(doc(`<main id="main">${body}</main>${TOASTER}`, { css: true }), { "set-cookie": `XSRF-TOKEN=${token}; Path=/` });

      test("R3 a change of user in the tab drops the toasts and their posts", async () => {
        const next = withUser("two", `<p>Signed out.</p>`);
        const same = withUser("one", `<p>Same user.</p>`);
        const first = withUser("one", `<button id="raise" ${stickyToast("Deleted.", { action: { label: "Undo", href: "/undo", post: true } })}>raise</button><a id="out" href="${next}">sign out</a><a id="stay" href="${same}">stay</a>`);
        const page = await fixture(first);
        await page.click("#raise");
        await toasts(page).first().waitFor();
        await page.click("#stay");
        await page.locator("main", { hasText: "Same user." }).waitFor();
        assert.equal(await toasts(page).count(), 1, "the same user keeps it through a boosted navigation");
        await page.goBack();
        await page.locator("#out").waitFor();
        await page.click("#out");
        await page.locator("main", { hasText: "Signed out." }).waitFor();
        assert.equal(await toasts(page).count(), 0, "another user's toast and its Undo are gone");
        await page.done();
      });

      test("R3 a change of user drops the toasts even when the next page has no toaster of its own", async () => {
        // A login page built by hand has no toaster: the standing one is still the last user's.
        const bare = route(doc(`<main id="main"><p>Sign in.</p></main>`, { css: true }), { "set-cookie": "XSRF-TOKEN=fresh; Path=/" });
        const first = withUser("one", `<button id="raise" ${stickyToast("Deleted.", { action: { label: "Undo", href: "/undo", post: true } })}>raise</button><a id="out" href="${bare}">sign out</a>`);
        const page = await fixture(first);
        await page.click("#raise");
        await toasts(page).first().waitFor();
        await page.click("#out");
        await page.locator("main", { hasText: "Sign in." }).waitFor();
        assert.equal(await toasts(page).count(), 0, "nothing of the last user stays on the sign-in page");
        assert.equal(await page.locator("[data-slot=toast-action]").count(), 0, "and no Undo");
        await page.done();
      });

      test("R3 a toast carried over a real page load is dropped when the user changed, kept when not", async () => {
        const other = withUser("two", `<p>Another user.</p>`);
        const sameUser = withUser("one", `<p>Same user.</p>`);
        const first = withUser("one", `<button id="raise" ${stickyToast("Deleted.", { action: { label: "Undo", href: "/undo", post: true } })}>raise</button><a id="out" data-ui-noboost href="${other}">other</a><a id="stay" data-ui-noboost href="${sameUser}">stay</a>`);
        const page = await fixture(first);
        await page.click("#raise");
        await toasts(page).first().waitFor();
        await page.click("#stay");
        await page.locator("main", { hasText: "Same user." }).waitFor();
        await toasts(page).first().waitFor();
        await page.goBack();
        await page.locator("#raise").waitFor();
        await page.click("#raise");
        await toasts(page).first().waitFor();
        await page.click("#out");
        await page.locator("main", { hasText: "Another user." }).waitFor();
        await page.waitForTimeout(400);
        assert.equal(await toasts(page).count(), 0, "not shown to the next user");
        assert.equal(await page.evaluate(() => sessionStorage.getItem("navree-ui-toasts")), null, "and not left in the tab");
        await page.done();
      });

      test("R3 a stored toast has the life of the flash cookie: 30 seconds", async () => {
        const target = withUser("one", `<p>Next.</p>`);
        const first = withUser("one", `<button id="raise" ${stickyToast("Old news.")}>raise</button><a id="go" data-ui-noboost href="${target}">go</a>`);
        const page = await fixture(first);
        await page.context().addInitScript(() => {
          // Time passes between the pages: what was stored a minute ago is read now.
          try {
            const raw = sessionStorage.getItem("navree-ui-toasts");
            const stored = raw && JSON.parse(raw);
            if (stored && !Array.isArray(stored)) {
              stored.at -= 60000;
              sessionStorage.setItem("navree-ui-toasts", JSON.stringify(stored));
            }
          } catch {}
        });
        await page.click("#raise");
        await toasts(page).first().waitFor();
        await page.click("#go");
        await page.locator("main", { hasText: "Next." }).waitFor();
        await page.waitForTimeout(400);
        assert.equal(await toasts(page).count(), 0, "a minute old: gone");
        await page.done();
      });

      test("R4 a section of the page is never mistaken for the toaster", async () => {
        const next = paged(`<main id="main"><p>Two.</p></main><section><p>Page content</p></section>`);
        const first = paged(`<main id="main"><button id="raise" ${stickyToast("Still here.")}>raise</button><a id="go" href="${next}">go</a></main>${TOASTER}`);
        const page = await fixture(first);
        await page.evaluate(() => { document.querySelector("[data-slot=toaster]").__same = true; });
        await page.click("#raise");
        await toasts(page).first().waitFor();
        await page.click("#go");
        await page.locator("main", { hasText: "Two." }).waitFor();
        const found = await page.evaluate(() => ({
          toasters: document.querySelectorAll("[data-slot=toaster]").length,
          same: !!document.querySelector("[data-slot=toaster]").__same,
          toasts: document.querySelectorAll("[data-slot=toaster] [data-slot=toast]").length,
          section: [...document.querySelectorAll("section:not([data-slot])")].map((s) => s.textContent),
          inside: document.querySelector("[data-slot=toaster]").textContent.includes("Page content"),
        }));
        assert.deepEqual(found, { toasters: 1, same: true, toasts: 1, section: ["Page content"], inside: false });
        await page.done();
      });

      test("R4 the page's toaster has a name of its own", async () => {
        const page = await open("/menus");
        assert.equal(await page.locator("body > #ui-toaster[data-slot=toaster]").count(), 1);
        await page.done();
      });

      /** A page with the page's one post form and the token cookie `t`, for post items. */
      const withPostForm = (items, extra = "") => paged(`<main id="main">${items}${extra}</main>${TOASTER}<form id="ui-post" method="post" hidden data-ui-form><input type="hidden" name="_csrf" value="t"></form>`);
      const post = (name, id, path) => `<button id="${id}" type="submit" form="ui-post" formaction="${path}" formmethod="post">${name}</button>`;
      const slowOps = (path, text, ms = 700) => f.routes.set(path, (req, res, body) => setTimeout(() => f.ops(res, { build: "b1", ops: [{ op: "toast", tone: "info", text }] }), ms));

      test("R5 two post items of the shared form: A then B both post, and a double click on A posts once", async () => {
        const unique = ++uniqueRoute;
        slowOps(`/pa${unique}`, "done A");
        slowOps(`/pb${unique}`, "done B");
        const path = withPostForm(post("Archive A", "a", `/pa${unique}`) + post("Archive B", "b", `/pb${unique}`));
        const page = await fixture(path);
        const count = (p) => f.log.filter((e) => e.method === "POST" && e.path === p).length;
        await page.click("#a");
        await page.click("#b");
        await toasts(page).filter({ hasText: "done B" }).waitFor();
        await toasts(page).filter({ hasText: "done A" }).waitFor();
        assert.equal(count(`/pa${unique}`), 1);
        assert.equal(count(`/pb${unique}`), 1, "B was not dropped while A was waiting");
        // A double click on one item is one post.
        await page.evaluate(() => { const a = document.getElementById("a"); HTMLElement.prototype.click.call(a); HTMLElement.prototype.click.call(a); });
        await page.waitForTimeout(1000);
        assert.equal(count(`/pa${unique}`), 2, "the second click on A, while A waited, was ignored");
        await page.done();
      });

      test("R5 a field of the page that names the post form is not sent with a menu's post", async () => {
        const unique = ++uniqueRoute;
        const got = [];
        f.routes.set(`/pf${unique}`, (req, res, body) => { got.push({ body, token: req.headers["x-xsrf-token"] || null }); f.ops(res, { build: "b1", ops: [] }); });
        const path = withPostForm(post("Archive", "a", `/pf${unique}`), `<div class="stored"><input form="ui-post" type="hidden" name="role" value="admin"></div>`);
        const page = await fixture(path);
        await page.click("#a");
        await page.waitForFunction(() => !document.querySelector("#ui-post[data-pending]"));
        await page.waitForTimeout(300);
        assert.equal(got.length, 1);
        assert.equal(got[0].body, "_csrf=t", "only the token the toolkit placed");
        await page.done();
      });

      test("R6 a Toaster drawn inside a page's layout can't take over the standing toaster on a boosted link", async () => {
        const nested = `<div class="layout"><main id="main"><h1 id="h">B</h1></main><section data-slot="toaster" id="ui-toaster" aria-label="Notifications" data-position="top-end" data-close="Dismiss" data-ui-toaster><div data-slot="toast" id="ui-toast-1" data-tone="success" data-duration="long" role="status"><div data-slot="toast-body"><p data-slot="toast-text">flash on B</p></div><a data-slot="toast-close" role="button" href="#ui-toast-1" aria-label="Dismiss"></a></div></section></div>`;
        const next = paged(nested);
        const first = paged(`<main id="main"><button id="t" ${stickyToast("from A")}>t</button><a id="go" href="${next}">b</a></main>${TOASTER}`);
        const page = await fixture(first);
        await page.evaluate(() => { document.querySelector("[data-slot=toaster]").__standing = true; });
        await page.click("#t");
        await toasts(page).first().waitFor();
        await page.click("#go");
        await page.locator("#h").waitFor();
        await page.waitForTimeout(300);
        const state = await page.evaluate(() => {
          const host = [...document.querySelectorAll("[data-slot=toaster]")].find((h) => h.__standing);
          return { standing: !!host, popover: host?.getAttribute("popover"), open: host?.matches(":popover-open"), parent: host?.parentElement.tagName, texts: [...(host?.querySelectorAll("[data-slot=toast-text]") || [])].map((t) => t.textContent) };
        });
        assert.deepEqual(state, { standing: true, popover: "manual", open: true, parent: "BODY", texts: ["from A"] });
        await page.done();
      });

      test("R6 a toaster that lost its popover gets it back with the next toast", async () => {
        const path = paged(`<main id="main"><button id="t" ${stickyToast("after")}>t</button></main>${TOASTER}`);
        const page = await fixture(path);
        await page.evaluate(() => { document.querySelector("[data-slot=toaster]").removeAttribute("popover"); });
        await page.click("#t");
        await toasts(page).filter({ hasText: "after" }).waitFor();
        assert.equal(await page.locator("[data-slot=toaster]").getAttribute("popover"), "manual");
        assert.equal(await page.locator("[data-slot=toaster]").evaluate((h) => h.matches(":popover-open")), true);
        await page.done();
      });

      test("R7 after a change of user the reply's own toast stays, even when the cookie shows late", async () => {
        const unique = ++uniqueRoute;
        f.routes.set(`/signin${unique}`, (req, res) => f.ops(res, { build: "b1", ops: [{ op: "toast", tone: "success", text: "Welcome back.", duration: "sticky" }] }, 200, { "set-cookie": "XSRF-TOKEN=new; Path=/" }));
        const path = withUser("old", `<button id="raise" ${stickyToast("Old user's toast.")}>raise</button><form id="in" method="post" action="/signin${unique}" data-ui-form><button id="go">sign in</button></form>`);
        // Some engines still show the old cookie to a script for a moment after the answer that set a new one.
        const page = await fixture(path, {}, (context) => context.addInitScript(() => {
          const real = Object.getOwnPropertyDescriptor(Document.prototype, "cookie");
          let stale = null;
          window.__lag = (ms) => { stale = real.get.call(document); setTimeout(() => { stale = null; }, ms); };
          Object.defineProperty(Document.prototype, "cookie", { configurable: true, get() { return stale === null ? real.get.call(this) : stale; }, set: real.set });
        }));
        await page.click("#raise");
        await toasts(page).filter({ hasText: "Old user's toast." }).waitFor();
        await page.evaluate(() => window.__lag(600));
        await page.click("#go");
        await toasts(page).filter({ hasText: "Welcome back." }).waitFor();
        await page.waitForTimeout(800);
        // The runtime looks again when the tab shows, among other times.
        await page.evaluate(() => document.dispatchEvent(new Event("visibilitychange")));
        await page.waitForTimeout(200);
        const texts = await page.locator("[data-slot=toast-text]").allTextContents();
        assert.deepEqual(texts, ["Welcome back."], "the old user's toast is gone and the answer's own stays");
        await page.done();
      });

      test("R8 an open lazy tab that fails before the menus module has run still shows its alert", async () => {
        const unique = ++uniqueRoute;
        const menus = readFileSync(new URL("../../../crates/navree-ui/dist/menus.js", import.meta.url));
        let asked = 0;
        f.routes.set(`/slow-menus${unique}.js`, (req, res) => setTimeout(() => { res.writeHead(200, { "content-type": "text/javascript" }); res.end(menus); }, 500));
        f.routes.set(`/lz${unique}`, (req, res) => { asked++; res.writeHead(500, { "content-type": "text/html" }); res.end("no"); });
        const tabs = `<div data-slot="tabs" id="tt" data-ui-tabs="tab" data-orientation="horizontal" data-l="{}"><div data-slot="tabs-list" role="tablist" aria-label="T"><a role="tab" id="tt-tab-a" aria-selected="true" aria-controls="tt-panel-a" data-key="a" data-ui-noboost href="?tab=a">A</a><a role="tab" id="tt-tab-b" aria-selected="false" aria-controls="tt-panel-b" data-key="b" data-ui-noboost href="?tab=b">B</a></div><div role="tabpanel" id="tt-panel-a" aria-labelledby="tt-tab-a" tabindex="0" data-ui-region data-ui-src="/lz${unique}" data-ui-lazy data-ui-loaded aria-busy="true"><div data-slot="skeleton"></div></div><div role="tabpanel" id="tt-panel-b" aria-labelledby="tt-tab-b" tabindex="0" hidden>PB</div></div>`;
        const path = route(`<!doctype html><html lang="en" data-build="b1"><head><meta charset="utf-8"><title>t</title><script type="module" src="/_navree/ui/core.js"></script><script type="module" src="/slow-menus${unique}.js"></script></head><body data-ui-boost><main id="main">${tabs}</main></body></html>`);
        const page = await fixture(path);
        await page.locator("#tt-panel-a [data-slot=tab-alert]").waitFor({ timeout: 10000 });
        assert.equal(asked, 1);
        assert.equal(await page.locator("#tt-panel-a").getAttribute("aria-busy"), null, "no longer busy");
        assert.equal(await page.locator("#tt-panel-a [data-slot=skeleton]").count(), 0);
        // Showing the tab again tries again.
        await page.click("#tt-tab-b");
        await page.click("#tt-tab-a");
        await page.waitForTimeout(500);
        assert.equal(asked, 2);
        await page.done();
      });

      test("a toast that the address hides at load counts as dismissed: it isn't kept for the next page", async () => {
        const path = paged(`<main id="main"><a id="go" data-ui-noboost href="${paged("<main id=\"main\"><p>Next.</p></main>")}">go</a></main><section data-slot="toaster" id="ui-toaster" aria-label="Notifications" data-position="bottom-end" data-close="Dismiss"><div data-slot="toast" id="ui-toast-1" data-tone="info" data-duration="sticky" role="status"><div data-slot="toast-body"><p data-slot="toast-text">Already closed.</p></div><a data-slot="toast-close" role="button" href="#ui-toast-1" aria-label="Dismiss"></a></div></section>`);
        const page = await fixture(path + "#ui-toast-1");
        await page.waitForTimeout(200);
        assert.equal(await page.locator("[data-slot=toast]").count(), 0, "it was taken away, not just hidden");
        await page.click("#go");
        await page.locator("main", { hasText: "Next." }).waitFor();
        await page.waitForTimeout(300);
        assert.equal(await page.locator("[data-slot=toast]").count(), 0, "and it doesn't come back on the next page");
        await page.done();
      });

      test("a toast the runtime builds takes an address that nothing in the page has", async () => {
        const path = paged(`<main id="main"><h2 id="ui-toast-1">The page's own heading</h2><button id="t" ${stickyToast("One")}>t</button></main>${TOASTER}`);
        const page = await fixture(path);
        await page.click("#t");
        await toasts(page).first().waitFor();
        const found = await page.evaluate(() => {
          const t = document.querySelector("[data-slot=toast]");
          return { dupes: [...document.querySelectorAll("[id]")].map((e) => e.id).filter((id, i, all) => all.indexOf(id) !== i), own: document.querySelector("h2").id, link: t.querySelector("[data-slot=toast-close]").getAttribute("href") === "#" + t.id, taken: t.id !== "ui-toast-1" };
        });
        assert.deepEqual(found, { dupes: [], own: "ui-toast-1", link: true, taken: true });
        await page.done();
      });

      test("a toast's close is a button to assistive technology, and Space and Enter close it", async () => {
        const path = paged(`<main id="main"><button id="t" ${stickyToast("Close me")}>t</button><button id="u" ${stickyToast("And me")}>u</button></main>${TOASTER}`);
        const page = await fixture(path);
        await page.click("#t");
        const toast = toasts(page).filter({ hasText: "Close me" });
        await toast.waitFor();
        assert.equal(await toast.locator("[data-slot=toast-close]").getAttribute("role"), "button");
        assert.match(await page.locator("[data-slot=toaster]").ariaSnapshot(), /button "Dismiss"/);
        await toast.locator("[data-slot=toast-close]").focus();
        await page.keyboard.press("Space");
        await toast.waitFor({ state: "detached" });
        assert.equal(await page.evaluate(() => location.hash), "", "the address was left alone");
        await page.click("#u");
        const other = toasts(page).filter({ hasText: "And me" });
        await other.waitFor();
        await other.locator("[data-slot=toast-close]").focus();
        await page.keyboard.press("Enter");
        await other.waitFor({ state: "detached" });
        await page.done();
      });

      test("hostile markup can't break the menus module: names that shadow the document", async () => {
        // What the module and the runtime read off the document, and more: a form or an image named like it stands in for it.
        const names = ["documentElement", "activeElement", "createElement", "querySelector", "querySelectorAll", "getElementById", "dispatchEvent", "addEventListener", "body"];
        const hostile = names.map((n) => `<img name="${n}" alt=""><form name="${n}-f"></form>`).join("") + names.map((n) => `<form name="${n}"></form>`).join("");
        f.routes.set("/hs/tab/b", (req, res) => { res.writeHead(500, { "content-type": "text/plain" }); res.end("no"); });
        f.routes.set("/hs/swap", (req, res) => f.ops(res, { build: "b1", ops: [{ op: "replace", target: "swapped", html: '<p id="swapped">Swapped.</p>' }] }));
        const path = paged(`${hostile}<main id="main">
          <div id="t" data-ui-tabs="tab"><div role="tablist" aria-label="Tabs"><a role="tab" id="t-tab-a" href="?tab=a" aria-selected="true" tabindex="0" aria-controls="t-panel-a" data-key="a">A</a><a role="tab" id="t-tab-b" href="?tab=b" aria-selected="false" tabindex="-1" aria-controls="t-panel-b" data-key="b">B</a></div>
          <div role="tabpanel" id="t-panel-a" aria-labelledby="t-tab-a" tabindex="0">Panel A</div>
          <div role="tabpanel" id="t-panel-b" aria-labelledby="t-tab-b" tabindex="0" hidden data-ui-region data-ui-src="/hs/tab/b" data-ui-lazy>Panel B</div></div>
          <button id="opener" popovertarget="menu" aria-haspopup="menu" aria-expanded="false" data-ui-keep-attrs="aria-expanded">Menu</button>
          <div id="menu" popover="auto" role="menu" data-ui-menu aria-labelledby="opener"><button role="menuitem" data-slot="menu-item" type="button" id="one">One</button><button role="menuitem" data-slot="menu-item" type="button">Two</button></div>
          <p id="swapped">Before.</p>
          <form id="sw" method="post" action="/hs/swap" data-ui-form><button id="swap">Swap</button></form>
          <button id="raise" ${stickyToast("Raised.")}>raise</button></main>${TOASTER}`, { menus: true });
        const page = await fixture(path);
        await page.waitForTimeout(150);
        // The page's names shadow what the test driver itself reads off the document in some engines, so the
        // steps are taken through the prototypes, with real key presses; locators are not used here.
        const click = (id) => page.evaluate((i) => HTMLElement.prototype.click.call(Document.prototype.getElementById.call(document, i)), id);
        const has = (selector) => page.waitForFunction((sel) => !!Document.prototype.querySelector.call(document, sel), selector);
        // Tabs: the arrow key moves the selection through the module.
        await page.evaluate(() => HTMLElement.prototype.focus.call(Document.prototype.getElementById.call(document, "t-tab-a")));
        await page.keyboard.press("ArrowRight");
        await page.waitForFunction(() => Element.prototype.getAttribute.call(Document.prototype.getElementById.call(document, "t-tab-b"), "aria-selected") === "true");
        // The lazy panel failed: its alert is built by the module.
        await has("#t-panel-b [data-slot=tab-alert]");
        // A menu opens from its button by the keyboard, focus goes to its first item, and Escape returns it to the button.
        await page.evaluate(() => HTMLElement.prototype.focus.call(Document.prototype.getElementById.call(document, "opener")));
        await page.keyboard.press("Enter");
        await page.waitForFunction(() => Element.prototype.matches.call(Document.prototype.getElementById.call(document, "menu"), ":popover-open"));
        await page.waitForFunction(() => Object.getOwnPropertyDescriptor(Document.prototype, "activeElement").get.call(document)?.id === "one");
        await page.keyboard.press("Escape");
        await page.waitForFunction(() => Object.getOwnPropertyDescriptor(Document.prototype, "activeElement").get.call(document)?.id === "opener");
        // A toast, an op that swaps, and the toast closed by its link.
        await click("raise");
        await has("[data-slot=toaster] [data-slot=toast]");
        await click("swap");
        await page.waitForFunction(() => {
          const el = Document.prototype.getElementById.call(document, "swapped");
          return !!el && Object.getOwnPropertyDescriptor(Node.prototype, "textContent").get.call(el) === "Swapped.";
        });
        await page.evaluate(() => HTMLElement.prototype.click.call(Document.prototype.querySelector.call(document, "[data-slot=toast-close]")));
        await page.waitForFunction(() => !Document.prototype.querySelector.call(document, "[data-slot=toaster] [data-slot=toast]"));
        await page.done();
      });

      test("the toast hover pause is for a pointer that hovers", async () => {
        const page = await open("/menus");
        const rules = await page.evaluate(() => {
          const found = [];
          const walk = (list, within) => {
            for (const rule of list) {
              if (rule instanceof CSSStyleRule) {
                if (rule.selectorText.includes(":hover") && rule.selectorText.includes("toaster")) found.push({ within, text: rule.selectorText });
                walk(rule.cssRules || [], within);
              } else if (rule.cssRules) walk(rule.cssRules, rule instanceof CSSMediaRule ? rule.conditionText || rule.media.mediaText : within);
            }
          };
          for (const sheet of document.styleSheets) { try { walk(sheet.cssRules, ""); } catch {} }
          return found;
        });
        assert.ok(rules.length >= 1, "the hover rule is there");
        for (const rule of rules) assert.match(rule.within, /hover:\s*hover/, rule.text + " is inside a (hover: hover) query");
        // On a phone the same hover is a tap left behind: it must not hold the clock.
        if (name !== "firefox") {
          const context = await browser.newContext({ viewport: { width: 390, height: 800 }, hasTouch: true, isMobile: true });
          const touch = await context.newPage();
          await touch.goto(BASE + "/menus");
          assert.equal(await touch.evaluate(() => matchMedia("(hover: hover)").matches), false, "the device doesn't hover");
          await touch.locator("#n-info").click();
          const toast = touch.locator("[data-slot=toaster] [data-slot=toast]").first();
          await toast.waitFor();
          await toast.hover();
          assert.equal(await toast.evaluate((t) => getComputedStyle(t).animationPlayState.split(",")[0].trim()), "running", "a hover that a touch screen leaves behind doesn't pause it");
          await context.close();
        }
        await page.done();
      });

      test("Escape on a toast inside a dialog dismisses the toast and keeps the dialog", async () => {
        const page = await open("/menus");
        await page.locator("#m-basic-trigger").click();
        await page.getByRole("menuitem", { name: "Rename" }).click();
        await page.locator("#m-dialog-box[open]").waitFor();
        await page.locator("#m-dialog-toast").click();
        const toast = toasts(page).filter({ hasText: "Raised over the dialog." });
        await toast.waitFor();
        await toast.locator("[data-slot=toast-close]").focus();
        await page.keyboard.press("Escape");
        await toast.waitFor({ state: "detached" });
        assert.equal(await page.locator("#m-dialog-box").evaluate((e) => e.open), true, "the dialog stays");
        await page.keyboard.press("Escape");
        assert.equal(await page.locator("#m-dialog-box").evaluate((e) => e.open), false, "a second Escape closes it");
        await page.done();
      });

      test("with dialogs stacked the toaster stands in the top one, and a toast already showing is reachable", async () => {
        const path = paged(`<main id="main"><button id="raise" ${stickyToast("Before any dialog.")}>raise</button><button id="openA" ${cmd([["open", "A"]])}>edit</button></main>
<dialog id="A"><p>Edit</p><button id="openB" ${cmd([["open", "B"]])}>delete</button><button id="closeA" ${cmd([["close", "A"]])}>close A</button></dialog>
<dialog id="B"><p>Sure?</p><button id="yes" ${stickyToast("Deleted.", { action: { label: "Undo", href: "/undo", post: true } })}>yes</button><button id="closeB" ${cmd([["close", "B"]])}>close B</button></dialog>${TOASTER}`);
        f.routes.set("/undo", (req, res) => f.ops(res, { build: "b1", ops: [{ op: "toast", tone: "info", text: "Undone." }] }));
        const page = await fixture(path);
        const where = () => page.evaluate(() => document.querySelector("[data-slot=toaster]").parentElement.id || document.querySelector("[data-slot=toaster]").parentElement.tagName);
        await page.click("#raise");
        await toasts(page).first().waitFor();
        await page.click("#openA");
        await page.waitForFunction(() => document.querySelector("[data-slot=toaster]").parentElement.id === "A");
        await toasts(page).first().locator("[data-slot=toast-close]").click({ timeout: 3000 });
        assert.equal(await toasts(page).count(), 0, "the toast that was showing before the dialog opened could be closed");
        await page.click("#openB");
        await page.click("#yes");
        await toasts(page).filter({ hasText: "Deleted." }).waitFor();
        assert.equal(await where(), "B", "the top dialog holds the toaster");
        await page.locator("[data-slot=toast-action]").click({ timeout: 3000 });
        await toasts(page).filter({ hasText: "Undone." }).waitFor();
        await page.click("#closeB");
        await page.waitForFunction(() => document.querySelector("[data-slot=toaster]").parentElement.id === "A");
        assert.equal(await where(), "A", "when the top one closes the toaster goes to the one below");
        await page.click("#closeA");
        await page.waitForFunction(() => document.querySelector("[data-slot=toaster]").parentElement.tagName === "BODY");
        await page.done();
      });

      test("a lazy tab that failed isn't fetched again by every form sent after it", async () => {
        let asked = 0;
        // The slow tab is the open one when the page loads, so it is asked for as the page starts.
        const page = await open("/menus?lz=slow", {
          before: (p) => p.route("**/menus/tab/slow", (route) => { asked++; return route.fulfill({ status: 500, body: "no" }); }),
        });
        await page.locator("#t-lazy-panel-slow [data-slot=tab-alert]").waitFor();
        assert.equal(asked, 1);
        for (let n = 0; n < 2; n++) {
          await page.locator("#n-form-plain button").click();
          await toasts(page).filter({ hasText: "Saved from the server." }).first().waitFor();
          await page.waitForTimeout(200);
        }
        assert.equal(asked, 1, "the failed tab wasn't asked for again");
        // Showing the tab again still tries again.
        await page.locator("#t-lazy-tab-fresh").click();
        await page.locator("#t-lazy-tab-slow").click();
        await page.waitForTimeout(300);
        assert.equal(asked, 2, "showing it again is a new try");
        await page.done();
      });

      test("a multiple toggle group laid out vertically is a plain group: no orientation on it", async () => {
        const page = await open("/menus");
        const found = await page.evaluate(() => ({
          group: [...document.querySelectorAll("[role=group][data-slot=toggle-group]")].filter((g) => g.hasAttribute("aria-orientation")).length,
          vertical: document.querySelector("[aria-label='Stacked days']").getAttribute("data-orientation"),
          radios: [...document.querySelectorAll("[role=radiogroup][data-orientation=vertical]")].every((g) => g.getAttribute("aria-orientation") === "vertical"),
        }));
        assert.equal(found.group, 0, "no group carries aria-orientation");
        assert.equal(found.vertical, "vertical");
        assert.equal(found.radios, true, "a vertical radio group keeps it");
        assert.deepEqual(await axe(page), []);
        await page.done();
      });

      test("a toast's close works with JavaScript off", async () => {
        const page = await open("/menus/server-toast", { javaScriptEnabled: false });
        const toast = page.locator("[data-slot=toast]");
        await toast.waitFor();
        await toast.locator("[data-slot=toast-close]").click();
        assert.equal(await toast.isVisible(), false, "the close took it away");
        await page.done();
      });

      test("the page that adopts a flash toast takes the region's translated label too", async () => {
        const next = paged(`<main id="main"><p>Zwei.</p></main><section data-slot="toaster" aria-label="Benachrichtigungen" data-position="bottom-end" data-close="Schließen"><div data-slot="toast" data-tone="success" data-duration="sticky" role="status" id="ui-toast-1"><div data-slot="toast-body"><p data-slot="toast-text">Gespeichert.</p></div><a data-slot="toast-close" href="#ui-toast-1" aria-label="Schließen"></a></div></section>`);
        const first = paged(`<main id="main"><a id="go" href="${next}">go</a></main>${TOASTER}`);
        const page = await fixture(first);
        await page.click("#go");
        await toasts(page).filter({ hasText: "Gespeichert." }).waitFor();
        assert.equal(await page.locator("[data-slot=toaster]").getAttribute("aria-label"), "Benachrichtigungen");
        assert.equal(await page.locator("[data-slot=toast-close]").getAttribute("aria-label"), "Schließen");
        await page.done();
      });

      test("a sticky toast that asks for no close button still has one", async () => {
        f.routes.set("/sticky", (req, res) => f.ops(res, { build: "b1", ops: [{ op: "toast", tone: "danger", text: "Import failed.", duration: "sticky", closable: false }] }));
        const path = paged(`<main id="main"><form method="post" action="/sticky" data-ui-form><button id="go">go</button></form></main>${TOASTER}`);
        const page = await fixture(path);
        await page.click("#go");
        const toast = toasts(page).first();
        await toast.waitFor();
        await toast.locator("[data-slot=toast-close]").click();
        assert.equal(await toasts(page).count(), 0);
        await page.done();
      });
    });

    // ---- axe, in every state ---------------------------------------------

    describe("axe", () => {
      const STATES = [
        ["tabs", "/menus", async () => {}],
        ["tabs, a later tab open", "/menus?tab=settings&seg=week&v=team&lz=fast", async () => {}],
        ["a lazy panel that failed", "/menus", async (page) => {
          await page.route("**/menus/tab/slow", (r) => r.fulfill({ status: 500, body: "x" }));
          await page.locator("#t-lazy-tab-slow").click();
          await page.locator("#t-lazy-panel-slow [data-slot=tab-alert]").waitFor();
        }],
        ["toggle groups with some on", "/menus", async () => {}],
        ["a menu open", "/menus", async (page) => {
          await page.locator("#m-basic-trigger").click();
          await page.locator("#m-basic").waitFor();
        }],
        ["a menu with its submenus open", "/menus", async (page) => {
          await page.locator("#m-sub-trigger").focus();
          for (const key of ["ArrowDown", "ArrowDown", "ArrowRight", "ArrowDown", "ArrowDown", "ArrowRight"]) await page.keyboard.press(key);
          await page.locator("#m-sub-s2").waitFor();
        }],
        ["a menu with choices", "/menus", async (page) => {
          await page.locator("#m-view-trigger").click();
          await page.locator("#m-view").waitFor();
        }],
        ["a menu in a dialog", "/menus", async (page) => {
          await page.locator("#m-basic-trigger").click();
          await page.getByRole("menuitem", { name: "Rename" }).click();
          await page.locator("#m-in-dialog-trigger").click();
          await page.locator("#m-in-dialog").waitFor();
        }],
        ["four toasts", "/menus", async (page) => {
          for (const id of ["n-info", "n-success", "n-warning", "n-danger"]) await page.locator("#" + id).click();
          await toasts(page).nth(3).waitFor();
        }],
        ["a toast with a title and an action", "/menus", async (page) => {
          await page.locator("#n-form-rich button").click();
          await toasts(page).first().waitFor();
        }],
        ["a sticky error toast", "/menus", async (page) => {
          await page.locator("#n-form-sticky button").click();
          await toasts(page).first().waitFor();
        }],
        ["a flash toast from the server", "/menus", async (page) => {
          await page.locator("#n-redirect button").click();
          await page.waitForURL(/landed=1/);
          await toasts(page).first().waitFor();
        }],
      ];
      for (const [label, path, prepare] of STATES) {
        for (const [theme, options] of [["dark", {}], ["light", { light: true }], ["phone", { viewport: { width: 390, height: 800 } }]]) {
          test(`axe, ${label}, ${theme}`, { timeout: 60000 }, async () => {
            const page = await open(path, { ...options, reducedMotion: "reduce" });
            await prepare(page);
            assert.deepEqual(await axe(page), []);
            await page.done();
          });
        }
      }
    });
  });
}
