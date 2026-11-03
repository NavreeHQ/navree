// Tabs, toggle groups, dropdown menus and toasts in real browsers: the keys of each
// pattern, the address bar, requests, JavaScript off, placement, the live region, the
// content security policy and axe in every state. Run with the proof app serving:
//
//   cargo run -p ui-proof -- serve          # UI_PROOF_URL, default http://127.0.0.1:8090
//   UI_BROWSERS=chromium,firefox,webkit npm test -- tests/menus.test.mjs

import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { createRequire } from "node:module";
import { after, before, describe, test } from "node:test";
import { chromium, firefox, webkit } from "playwright";

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
    });
    after(() => browser.close());

    /**
     * A page on the proof app. Console errors and content security policy reports are
     * kept in `page.problems`, and each test ends by asserting there are none.
     */
    async function open(path, options = {}) {
      const { light, ...rest } = options;
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
        await page.waitForTimeout(100);
        const box = await page.locator("#m-basic").boundingBox();
        const trigger = await page.locator("#m-basic-trigger").boundingBox();
        assert.ok(box.y >= 0 && box.y + box.height <= 520, JSON.stringify(box));
        assert.ok(box.y + box.height <= trigger.y + 1 || box.y >= trigger.y + trigger.height, "it doesn't cover its button");
        await page.keyboard.press("Escape");
        // A row menu at the end of a table lines up with the end of its button and stays inside.
        await page.locator("#m-row-1-trigger").scrollIntoViewIfNeeded();
        await page.locator("#m-row-1-trigger").click();
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
        const small = await page.locator("#m-right").boundingBox();
        assert.ok(small.x >= 7 && small.x + small.width <= 390 - 7, JSON.stringify(small));
        await page.keyboard.press("Escape");
        await page.setViewportSize({ width: 1280, height: 420 });
        await page.locator("#m-long-trigger").scrollIntoViewIfNeeded();
        await page.locator("#m-long-trigger").click();
        await page.locator("#m-long").waitFor();
        const tall = await page.locator("#m-long").evaluate((e) => ({ h: e.getBoundingClientRect().height, scroll: e.scrollHeight, client: e.clientHeight }));
        assert.ok(tall.h <= 420 - 8, JSON.stringify(tall));
        assert.ok(tall.scroll > tall.client, "it scrolls inside");
        await page.keyboard.press("End");
        assert.equal(await focused(page), "Item 60");
        const inView = await page.evaluate(() => {
          const m = document.querySelector("#m-long").getBoundingClientRect();
          const i = document.activeElement.getBoundingClientRect();
          return i.top >= m.top - 1 && i.bottom <= m.bottom + 1;
        });
        assert.equal(inView, true, "the focused item was scrolled into the menu's box");
        await page.setViewportSize({ width: 1280, height: 600 });
        await page.waitForTimeout(150);
        const after = await page.locator("#m-long").boundingBox();
        assert.ok(after.y >= 0 && after.y + after.height <= 600, "placed again after a resize: " + JSON.stringify(after));
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
        assert.equal(await page.locator("[data-slot=toaster]").evaluate((e) => e.parentElement.tagName), "BODY", "the toaster went back to the page");
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
            const attrs = [...node.attributes].filter((a) => a.name !== "data-toast-id").map((a) => `${a.name}=${a.name === "href" || a.name === "action" ? a.value : /^(data-|role|type|aria-label)/.test(a.name) ? a.value : ""}`).sort().join(" ");
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
