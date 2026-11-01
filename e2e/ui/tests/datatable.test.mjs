// The data table in real browsers, against a real database. Needs the table proof:
//   ui-table-proof seed 1000000 && PORT=8097 ui-table-proof serve
// UI_TABLE_URL points elsewhere; TABLE_BROWSERS="chromium firefox webkit" picks browsers.
//
// Every condition from the module's list is a test here or in
// apps/ui-table-proof/tests. Each test starts from the seeded rows.

import assert from "node:assert/strict";
import { readFile, mkdir, writeFile } from "node:fs/promises";
import { createRequire } from "node:module";
import { join } from "node:path";
import { after, before, describe, test } from "node:test";
import * as playwright from "playwright";

const BASE = process.env.UI_TABLE_URL || "http://127.0.0.1:8097";
const SHOTS = process.env.TABLE_SHOTS || "";
const require = createRequire(import.meta.url);
const axeSource = await readFile(require.resolve("axe-core/axe.min.js"), "utf8");

// The light values of `src/tokens.rs`: the toolkit ships dark only so far, so light
// is proved by putting these on the page.
const LIGHT = ":root{color-scheme:light;--ink-0:#ffffff;--ink-1:#f7f8f7;--ink-2:#eff1f0;--ink-3:#e5e8e6;--line:#dfe3e0;--line-2:#c8cecb;--line-3:#aeb6b2;--fg:#0a0e0c;--fg-1:#1c2420;--fg-2:#3f4a44;--fg-3:#55615b;--accent:#0b7a3e;--accent-ink:#ffffff;--amber:#8a5a00;--danger:#b3261e;--bar:#8a968f}";

const post = (path) => fetch(BASE + path, { method: "POST" }).then((r) => r.text());
const get = (path) => fetch(BASE + path).then((r) => r.text());
const reset = () => post("/__t/reset");

/** Runs axe. Violations come back as short lines. */
async function axe(page) {
  await page.evaluate(axeSource);
  return page.evaluate(async () => {
    const out = await window.axe.run(document, { runOnly: ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa", "best-practice"] });
    return out.violations.map((v) => `${v.id}: ${v.nodes.slice(0, 3).map((n) => n.target.join(" ")).join(" | ")}`);
  });
}

const rows = (page, id = "deals") => page.locator(`#${id} tbody tr[data-key]`);
const keys = (page, id = "deals") => rows(page, id).evaluateAll((items) => items.map((row) => row.getAttribute("data-key")));
const settled = async (page, id = "deals") => page.waitForFunction((table) => !document.getElementById(table)?.hasAttribute("data-pending") && !document.documentElement.hasAttribute("data-ui-loading"), id);

async function shot(page, name) {
  if (!SHOTS) return;
  await mkdir(SHOTS, { recursive: true });
  await page.screenshot({ path: join(SHOTS, name + ".png") });
}

const browsers = (process.env.TABLE_BROWSERS || "chromium firefox").split(" ");

for (const browserName of browsers) {
  describe(browserName, () => {
    let browser;
    before(async () => {
      browser = await playwright[browserName].launch();
    });
    after(() => browser?.close());

    async function open(path, options = {}) {
      const context = await browser.newContext({ viewport: options.viewport || { width: 1440, height: 900 }, javaScriptEnabled: options.js !== false, bypassCSP: !!options.light, acceptDownloads: true });
      const page = await context.newPage();
      const problems = [];
      page.on("console", (message) => {
        if (message.type() === "error" && !/Failed to load resource/.test(message.text())) problems.push(message.text());
      });
      page.on("pageerror", (error) => problems.push(String(error)));
      await page.goto(BASE + path);
      if (options.light) await page.addStyleTag({ content: LIGHT });
      return { page, context, problems, close: () => context.close() };
    }

    test("0 rows: empty state, no pagination, no select-all to offer", async () => {
      await reset();
      const { page, close } = await open("/empty");
      assert.equal(await page.locator("#sized tbody [data-slot=empty]").count(), 1);
      assert.match(await page.locator("#sized tbody").innerText(), /Nothing here yet/);
      assert.equal(await page.locator("#sized [data-slot=table-footer]").count(), 0);
      assert.equal(await page.locator("#sized [data-ui-select-row]").count(), 0);
      assert.deepEqual(await axe(page), []);
      await close();
    });

    test("1 row: one result and no pagination", async () => {
      const { page, close } = await open("/one");
      assert.equal(await rows(page, "sized").count(), 1);
      assert.match(await page.locator("#sized [data-slot=table-summary]").innerText(), /^1 result$/);
      assert.equal(await page.locator("#sized [data-slot=pagination-link]").count(), 0);
      await close();
    });

    test("exactly one page: right count, no next", async () => {
      const { page, close } = await open("/exact");
      assert.equal(await rows(page, "sized").count(), 25);
      assert.match(await page.locator("#sized [data-slot=table-summary]").innerText(), /Showing 1 to 25 of 25 results/);
      assert.equal(await page.locator("#sized [data-slot=pagination-link]").count(), 0);
      await close();
    });

    test("a filter matching nothing keeps headers, chips and a way out", async () => {
      await reset();
      const { page, close, problems } = await open("/deals?q=nothing-matches-this");
      assert.match(await page.locator("#deals tbody").innerText(), /No results match/);
      assert.equal(await page.locator("#deals [data-slot=filter-chip]").count(), 1);
      assert.equal(await page.locator("#deals thead th[data-col=title]").count(), 1);
      await shot(page, `${browserName}-filtered-empty`);
      assert.deepEqual(await axe(page), []);
      await page.locator("#deals tbody a", { hasText: "Clear filters" }).click();
      await page.waitForFunction(() => document.querySelectorAll("#deals tbody tr[data-key]").length === 25);
      assert.equal(new URL(page.url()).search, "");
      assert.deepEqual(problems, []);
      await close();
    });

    test("a page past the end after rows were deleted is the last page, with the URL corrected", async () => {
      await reset();
      const { page, close } = await open("/deals?page=6");
      assert.ok((await rows(page).count()) > 0);
      await post("/__t/delete-above/30");
      await page.goto(BASE + "/deals?page=6");
      assert.equal(new URL(page.url()).searchParams.get("page"), "2");
      assert.equal(await rows(page).count(), 5);
      assert.match(await page.locator("[data-slot=table-summary]").innerText(), /Showing 26 to 30 of 30 results/);
      await close();
    });

    test("sort, then back, forward, reload and a shared link restore everything", async () => {
      await reset();
      const { page, context, close } = await open("/deals");
      const states = ["/deals"];
      await page.locator("thead th[data-col=amount] [data-slot=sort-link]").click();
      await page.waitForURL(/sort=amount/);
      await settled(page);
      states.push(page.url());
      const first = await keys(page);
      await page.getByRole("link", { name: "Page 2", exact: true }).click();
      await page.waitForURL(/page=2/);
      await settled(page);
      const second = await keys(page);
      assert.notDeepEqual(first, second);
      await page.locator("#deals [name=q]").fill("renewal");
      await page.waitForURL(/q=renewal/);
      await settled(page);
      const url = page.url();
      assert.ok(!/page=2/.test(url), "a new filter starts from page 1");
      const filtered = await keys(page);
      await page.goBack();
      await page.waitForURL(/page=2/);
      await settled(page);
      assert.deepEqual(await keys(page), second, "back");
      assert.equal(await page.locator("#deals [name=q]").inputValue(), "", "back clears the box the state doesn't hold");
      await page.goForward();
      await page.waitForURL(/q=renewal/);
      await settled(page);
      assert.deepEqual(await keys(page), filtered, "forward");
      assert.equal(await page.locator("#deals [name=q]").inputValue(), "renewal");
      await page.reload();
      assert.deepEqual(await keys(page), filtered, "reload");
      const other = await context.newPage();
      await other.goto(url);
      assert.deepEqual(await keys(other), filtered, "a shared link");
      assert.equal(await other.locator("thead th[data-col=amount]").getAttribute("aria-sort"), "ascending");
      await close();
    });

    test("filters from popovers, multi select, range, date, boolean, relation, and their chips", async () => {
      await reset();
      const { page, close, problems } = await open("/deals");
      await page.locator("#deals-m-stage").evaluate((pop) => pop.showPopover());
      await page.locator("#deals-m-stage input[value=won]").check();
      await page.waitForURL(/f\.stage=won/);
      await settled(page);
      assert.ok(await page.locator("#deals-m-stage").evaluate((pop) => pop.matches(":popover-open")), "the popover stays open while the table morphs");
      await page.locator("#deals-m-stage input[value=lost]").check();
      await page.waitForURL(/f\.stage=lost/);
      await settled(page);
      const stages = await rows(page).locator("td:nth-child(5)").allInnerTexts();
      assert.ok(stages.length > 0 && stages.every((text) => /Won|Lost/.test(text)), stages.join());
      await shot(page, `${browserName}-filter-popover`);
      await page.keyboard.press("Escape");
      assert.deepEqual(await axe(page), []);
      const chips = page.locator("#deals [data-slot=filter-chip]");
      assert.equal(await chips.count(), 1);
      assert.match(await chips.first().innerText(), /Won, Lost/);
      await chips.first().click();
      await page.waitForFunction(() => !location.search.includes("stage"));
      await settled(page);
      assert.equal(await page.locator("#deals-m-stage input[value=won]").isChecked(), false, "a removed filter unticks its box");

      await page.locator("#deals-m-amount").evaluate((pop) => pop.showPopover());
      await page.locator("#deals-m-amount [name='f.amount.min']").fill("1000");
      await page.waitForURL(/f\.amount\.min=1000/);
      await settled(page);
      await page.keyboard.press("Escape");

      await page.locator("#deals-m-closed_on").evaluate((pop) => pop.showPopover());
      await page.locator("#deals-m-closed_on [name='f.closed_on.min']").fill("2026-02-01");
      await page.waitForURL(/f\.closed_on\.min=2026-02-01/);
      await settled(page);
      await page.keyboard.press("Escape");

      await page.locator("#deals-m-active").evaluate((pop) => pop.showPopover());
      await page.locator("#deals-m-active select").selectOption("true");
      await page.waitForURL(/f\.active=true/);
      await settled(page);
      await page.keyboard.press("Escape");

      await page.locator("#deals-m-company").evaluate((pop) => pop.showPopover());
      await page.locator("#deals-m-company label", { hasText: "Globex" }).locator("input").check();
      await page.waitForURL(/f\.company=/);
      await settled(page);
      await page.keyboard.press("Escape");
      assert.ok((await chips.count()) >= 4);
      await shot(page, `${browserName}-chips`);
      await page.locator("[data-slot=chips-clear]").click();
      await page.waitForFunction(() => !location.search.includes("f."));
      await settled(page);
      assert.equal(await rows(page).count(), 25);
      assert.deepEqual(problems, []);
      await close();
    });

    test("the filter row under the headers filters the same way", async () => {
      await reset();
      const { page, close } = await open("/deals-row");
      const row = page.locator("[data-slot=filter-row]");
      await row.locator("[name='f.title']").fill("globex");
      await page.waitForURL(/f\.title=globex/);
      await settled(page);
      assert.equal(await row.locator("[name='f.title']").inputValue(), "globex");
      assert.ok(await page.evaluate(() => document.activeElement?.getAttribute("name") === "f.title"), "focus stays in the box");
      const titles = await rows(page).locator("td[data-truncate]").allInnerTexts();
      assert.ok(titles.length > 0 && titles.every((text) => /globex/i.test(text)), titles.join());
      await shot(page, `${browserName}-filter-row`);
      assert.deepEqual(await axe(page), []);
      await close();
    });

    test("multi-column sort: shift-click, a menu, a rank, a way off", async () => {
      await reset();
      const { page, close } = await open("/deals");
      await page.locator("thead th[data-col=stage] [data-slot=sort-link]").click();
      await page.waitForURL(/sort=stage/);
      await settled(page);
      await page.locator("thead th[data-col=amount] [data-slot=sort-link]").click({ modifiers: ["Shift"] });
      await page.waitForURL(/sort=stage%2Camount|sort=stage,amount/);
      await settled(page);
      assert.equal(await page.locator("thead [data-slot=sort-rank]").count(), 2);
      await shot(page, `${browserName}-multi-sort`);
      const sortChips = page.locator("#deals [data-slot=filter-chip]", { hasText: "Sorted by" });
      assert.equal(await sortChips.count(), 2);
      await sortChips.first().click();
      await page.waitForFunction(() => !/stage/.test(location.search));
      await settled(page);
      assert.equal(await page.locator("thead [data-slot=sort-rank]").count(), 0);
      assert.equal(new URL(page.url()).searchParams.get("sort"), "amount");
      await close();
    });

    test("select all on this page, across pages, excluding some, then a bulk action on exactly that", async () => {
      await reset();
      const { page, close, problems } = await open("/deals?f.stage=won&sort=title&per=10");
      const total = Number(await get("/__t/count?f.stage=won"));
      assert.ok(total > 20, String(total));
      await page.locator("[data-ui-select-page]").check();
      const bar = page.locator("[data-slot=table-bulk]");
      assert.match(await bar.innerText(), /All 10 on this page are selected/);
      await shot(page, `${browserName}-select-page`);
      await page.locator("[data-ui-select-all]").click();
      assert.match(await bar.innerText(), new RegExp(`All ${total} results are selected`));
      await shot(page, `${browserName}-select-all`);
      // Off this page, and back: the boxes know.
      const firstKey = (await keys(page))[0];
      await page.getByRole("link", { name: "Page 2", exact: true }).click();
      await page.waitForURL(/page=2/);
      await settled(page);
      assert.equal(await page.locator("[data-ui-select-row]:checked").count(), await rows(page).count(), "page 2 is ticked");
      const secondKeys = await keys(page);
      await page.locator(`[data-ui-select-row][value="${secondKeys[0]}"]`).uncheck();
      assert.match(await bar.innerText(), new RegExp(`${total - 1} of ${total} results are selected`));
      assert.equal(await page.locator("[data-ui-select-page]").evaluate((box) => box.indeterminate), true);
      await page.getByRole("link", { name: "Page 1", exact: true }).click();
      await page.waitForFunction(() => !location.search.includes("page="));
      await settled(page);
      await page.locator(`[data-ui-select-row][value="${firstKey}"]`).uncheck();
      assert.match(await bar.innerText(), new RegExp(`${total - 2} of ${total} results are selected`));
      await shot(page, `${browserName}-select-some`);
      assert.deepEqual(await axe(page), []);
      await page.getByRole("button", { name: "Archive", exact: true }).click();
      await page.locator("[data-slot=toast]").first().waitFor();
      await settled(page);
      const archived = (await get("/__t/archived")).split(",").filter(Boolean);
      assert.equal(archived.length, total - 2, "exactly what was shown");
      assert.ok(!archived.includes(firstKey) && !archived.includes(secondKeys[0]), "the excluded rows weren't touched");
      assert.equal(await bar.isVisible(), false, "a successful action ends the selection");
      assert.deepEqual(problems, []);
      await close();
    });

    test("the set changed between seeing and acting: refused, nothing changed, selection kept", async () => {
      await reset();
      const { page, close } = await open("/deals?f.stage=won&per=10");
      const total = Number(await get("/__t/count?f.stage=won"));
      await page.locator("[data-ui-select-page]").check();
      await page.locator("[data-ui-select-all]").click();
      await post("/__t/add/9001");
      await page.getByRole("button", { name: "Archive", exact: true }).click();
      await page.locator("[data-slot=toast]").first().waitFor();
      assert.match(await page.locator("[data-slot=toast]").first().innerText(), new RegExp(`you selected ${total}, but ${total + 1} match`));
      assert.equal(await get("/__t/archived"), "", "nothing was archived");
      await settled(page);
      await page.waitForFunction((n) => document.getElementById("deals").getAttribute("data-total") === String(n), total + 1);
      const bar = page.locator("[data-slot=table-bulk]");
      assert.match(await bar.innerText(), new RegExp(`All ${total + 1} results are selected`), "what's shown is now the truth");
      await page.getByRole("button", { name: "Archive", exact: true }).click();
      await page.waitForFunction(() => document.querySelectorAll("[data-slot=toast]").length >= 1);
      await page.waitForFunction(async () => (await (await fetch("/__t/archived")).text()) !== "");
      assert.equal((await get("/__t/archived")).split(",").length, total + 1, "the second try, with the new count shown, runs");
      await close();
    });

    test("a filter change ends select-all; ids survive it and say so; a destructive action asks", async () => {
      await reset();
      const { page, close } = await open("/deals?per=10");
      await page.locator("[data-ui-select-page]").check();
      await page.locator("[data-ui-select-all]").click();
      await page.locator("#deals [name=q]").fill("globex");
      await page.waitForURL(/q=globex/);
      await settled(page);
      const bar = page.locator("[data-slot=table-bulk]");
      assert.match(await bar.innerText(), /The filter changed/);
      assert.equal(await page.locator("[data-ui-select-row]:checked").count(), 0);

      await page.goto(BASE + "/deals?per=10");
      const list = await keys(page);
      await page.locator(`[data-ui-select-row][value="${list[0]}"]`).check();
      await page.locator(`[data-ui-select-row][value="${list[1]}"]`).check();
      await page.getByRole("link", { name: "Page 2", exact: true }).click();
      await page.waitForURL(/page=2/);
      await settled(page);
      assert.match(await bar.innerText(), /2 selected · 2 not on this page/);
      await page.locator(`[data-ui-select-row]`).first().check();
      assert.match(await bar.innerText(), /3 selected · 2 not on this page/);
      await page.getByRole("button", { name: "Delete", exact: true }).click();
      const dialog = page.locator("#deals-confirm");
      await dialog.waitFor();
      assert.match(await dialog.innerText(), /Delete 3 deals/);
      await shot(page, `${browserName}-confirm`);
      assert.deepEqual(await axe(page), []);
      const before = Number(await get("/__t/count"));
      await dialog.getByRole("button", { name: "Cancel" }).click();
      assert.equal(Number(await get("/__t/count")), before, "cancel does nothing");
      await page.getByRole("button", { name: "Delete", exact: true }).click();
      await dialog.getByRole("button", { name: "Yes, go ahead" }).click();
      await page.locator("[data-slot=toast]").first().waitFor();
      await page.waitForFunction(async (n) => Number(await (await fetch("/__t/count")).text()) === n, before - 3);
      await close();
    });

    test("shift-click ticks a range; reload keeps the selection; a new tab doesn't share it", async () => {
      await reset();
      const { page, context, close } = await open("/deals?per=10");
      const boxes = page.locator("[data-ui-select-row]");
      await boxes.nth(1).click();
      await boxes.nth(4).click({ modifiers: ["Shift"] });
      assert.equal(await page.locator("[data-ui-select-row]:checked").count(), 4);
      await page.reload();
      assert.equal(await page.locator("[data-ui-select-row]:checked").count(), 4, "same tab, after a reload");
      const other = await context.newPage();
      await other.goto(BASE + "/deals?per=10");
      assert.equal(await other.locator("[data-ui-select-row]:checked").count(), 0, "two tabs, two selections");
      await other.locator("[data-ui-select-row]").first().check();
      assert.equal(await page.locator("[data-ui-select-row]:checked").count(), 4, "no cross-talk");
      await other.getByRole("button", { name: "Archive", exact: true }).click();
      await other.locator("[data-slot=toast]").first().waitFor();
      await settled(other);
      assert.equal(await page.locator("[data-ui-select-row]:checked").count(), 4);
      await close();
    });

    test("very long text, Unicode, RTL and hostile strings hold the layout and run nothing", async () => {
      await reset();
      const { page, close, problems } = await open("/deals?sort=title&per=100");
      await page.evaluate(() => {
        window.ran = 0;
        window.alert = () => window.ran++;
      });
      assert.equal(await page.locator("#deals img").count(), 0);
      assert.equal(await page.locator("#deals td[data-truncate] script").count(), 0);
      assert.equal(await page.evaluate(() => window.ran), 0);
      const hostile = page.locator("tr[data-key='3'] td[data-truncate]");
      assert.match(await hostile.innerText(), /<img src=x/);
      const overflow = await page.evaluate(() => document.documentElement.scrollWidth - innerWidth);
      assert.ok(overflow <= 1, `the page doesn't scroll sideways: ${overflow}`);
      await page.locator("#deals [name=q]").fill("<img src=x onerror=alert(1)>");
      await page.waitForURL(/q=/);
      await settled(page);
      assert.equal(await page.locator("#deals img").count(), 0);
      assert.equal(await rows(page).count(), 1);
      await shot(page, `${browserName}-hostile`);
      assert.deepEqual(await axe(page), []);
      await page.goto(BASE + "/deals?cols=title,notes,stage&f.title=مرحبا");
      assert.equal(await rows(page).count(), 1);
      assert.deepEqual(problems, []);
      await close();
    });

    test("many columns at 390 px: the table scrolls, not the page, and sticky holds", async () => {
      const { page, close } = await open("/wide", { viewport: { width: 390, height: 800 } });
      assert.ok((await page.locator("#wide thead th[data-col]").count()) === 30);
      const measure = () => page.evaluate(() => ({ page: document.documentElement.scrollWidth - innerWidth, table: document.querySelector("#wide [data-slot=table-container]").scrollWidth - document.querySelector("#wide [data-slot=table-container]").clientWidth }));
      const sizes = await measure();
      assert.ok(sizes.page <= 1, JSON.stringify(sizes));
      assert.ok(sizes.table > 1000, JSON.stringify(sizes));
      await page.locator("#wide [data-slot=table-container]").evaluate((box) => { box.scrollLeft = 900; });
      const first = await page.locator("#wide tbody tr:first-child td[data-sticky='1']").boundingBox();
      const end = await page.locator("#wide tbody tr:first-child [data-sticky=e]").first().boundingBox();
      assert.ok(first.x >= 0 && first.x < 80, `the first column stays: ${first.x}`);
      assert.ok(end.x + end.width <= 391, `the end column stays: ${end.x + end.width}`);
      await shot(page, `${browserName}-wide-390`);
      assert.deepEqual(await axe(page), []);
      await close();
    });

    test("390 px, the real table: nothing spills, and axe is clean with light tokens too", async () => {
      await reset();
      const { page, close } = await open("/deals", { viewport: { width: 390, height: 800 } });
      assert.ok((await page.evaluate(() => document.documentElement.scrollWidth - innerWidth)) <= 1);
      await shot(page, `${browserName}-deals-390`);
      assert.deepEqual(await axe(page), []);
      await close();
      const light = await open("/deals", { viewport: { width: 390, height: 800 }, light: true });
      await shot(light.page, `${browserName}-deals-390-light`);
      assert.deepEqual(await axe(light.page), []);
      await light.close();
      const wide = await open("/deals", { light: true });
      await shot(wide.page, `${browserName}-deals-1440-light`);
      assert.deepEqual(await axe(wide.page), []);
      await wide.close();
    });

    test("slow network: pending within a frame, typing stays live", async () => {
      await reset();
      const { page, close } = await open("/deals");
      await page.route(/\/deals\?/, async (route) => {
        await new Promise((resolve) => setTimeout(resolve, 600));
        await route.continue();
      });
      const inFrame = await page.evaluate(() => new Promise((resolve) => {
        document.querySelector("thead th[data-col=amount] [data-slot=sort-link]").click();
        requestAnimationFrame(() => resolve(document.getElementById("deals").hasAttribute("data-pending") && document.getElementById("deals").getAttribute("aria-busy") === "true"));
      }));
      assert.ok(inFrame, "busy within a frame");
      await shot(page, `${browserName}-pending`);
      await page.locator("#deals [name=q]").fill("x");
      assert.equal(await page.locator("#deals [name=q]").inputValue(), "x");
      await page.waitForURL(/q=x/);
      await settled(page);
      await close();
    });

    test("a failed request: rows stay, an alert, the URL stays, Retry works", async () => {
      await reset();
      const { page, close } = await open("/deals");
      const before = await keys(page);
      const url = page.url();
      let fail = true;
      await page.route(/\/deals\?/, async (route) => {
        if (fail) await route.abort();
        else await route.continue();
      });
      await page.locator("thead th[data-col=amount] [data-slot=sort-link]").click();
      const alert = page.locator("[data-slot=table-alert]");
      await alert.waitFor();
      assert.match(await alert.innerText(), /didn't load/);
      assert.equal(page.url(), url, "the address stays what's shown");
      assert.deepEqual(await keys(page), before, "the rows stay");
      assert.equal(await page.locator("#deals").getAttribute("aria-busy"), null);
      await shot(page, `${browserName}-failed`);
      assert.deepEqual(await axe(page), []);
      fail = false;
      await alert.getByRole("link", { name: "Try again" }).click();
      await page.waitForURL(/sort=amount/);
      await settled(page);
      assert.equal(await alert.isVisible(), false);
      assert.notDeepEqual(await keys(page), before);
      await close();
    });

    test("out-of-order replies: only the newest counts", async () => {
      await reset();
      const { page, close } = await open("/deals");
      await page.route(/sort=amount/, async (route) => {
        await new Promise((resolve) => setTimeout(resolve, 800));
        await route.continue();
      });
      await page.locator("thead th[data-col=amount] [data-slot=sort-link]").click();
      await page.route(/sort=stage/, (route) => route.continue());
      await page.locator("thead th[data-col=stage] [data-slot=sort-link]").click();
      await page.waitForURL(/sort=stage/);
      await settled(page);
      const newest = await keys(page);
      await page.waitForTimeout(1200);
      assert.deepEqual(await keys(page), newest, "the older reply changed nothing");
      assert.match(page.url(), /sort=stage/);
      assert.equal(await page.locator("thead th[data-col=stage]").getAttribute("aria-sort"), "ascending");
      await close();
    });

    test("JavaScript off: filter, sort, page, columns, density, per page and a bulk post all work", async () => {
      await reset();
      const { page, close } = await open("/deals", { js: false });
      assert.equal(await rows(page).count(), 25);
      await page.locator("thead th[data-col=amount] [data-slot=sort-link]").click();
      assert.match(page.url(), /sort=amount/);
      await page.getByRole("link", { name: "Page 2", exact: true }).click();
      assert.match(page.url(), /page=2/);
      assert.match(page.url(), /sort=amount/);
      await page.locator("#deals [name=q]").fill("globex");
      await page.getByRole("button", { name: "Apply" }).click();
      await page.waitForURL(/q=globex/);
      assert.ok(!/page=2/.test(page.url()));
      const titles = await rows(page).locator("td[data-truncate]").allInnerTexts();
      assert.ok(titles.length > 0 && titles.every((text) => /globex/i.test(text)));
      await shot(page, `${browserName}-no-js`);
      // Popovers are native.
      await page.locator("[popovertarget=deals-m-stage]").click({ force: true });
      await page.locator("#deals-m-stage input[value=won]").check();
      await page.keyboard.press("Escape");
      await page.getByRole("button", { name: "Apply" }).click();
      await page.waitForURL(/f\.stage=won/);
      assert.match(page.url(), /q=globex/);
      // A chip is a link.
      await page.locator("[data-slot=filter-chip]").first().click();
      await page.waitForURL((url) => !url.search.includes("q=globex"));
      // Columns, density, per page.
      await page.goto(BASE + "/deals");
      await page.locator("[popovertarget=deals-columns]").click();
      await page.locator("#deals-columns [data-col=company] input[type=checkbox]").uncheck();
      await page.keyboard.press("Escape");
      await page.getByRole("button", { name: "Apply" }).click();
      await page.waitForURL(/cols=/);
      assert.equal(await page.locator("thead th[data-col=company]").count(), 0);
      await page.locator("[popovertarget=deals-view]").click();
      await page.locator("#deals-view a", { hasText: "Compact" }).click();
      await page.waitForURL(/density=compact/);
      assert.equal(await page.locator("#deals").getAttribute("data-density"), "compact");
      await page.locator("[name=per]").selectOption("10");
      await page.getByRole("button", { name: "Apply" }).click();
      await page.waitForURL(/per=10/);
      assert.equal(await rows(page).count(), 10);
      // Ticks and a plain post.
      const key = (await keys(page))[0];
      await page.locator(`[data-ui-select-row][value="${key}"]`).check();
      await page.getByRole("button", { name: "Archive", exact: true }).click();
      await page.waitForFunction(async () => (await (await fetch("/__t/archived")).text()) !== "").catch(() => {});
      await page.waitForURL(/density=compact/);
      assert.equal(await get("/__t/archived"), key);
      await close();
    });

    test("keyboard only: reach everything, resize, reorder, and the grid keys", async () => {
      await reset();
      const { page, close } = await open("/deals?per=10");
      // Sorting by Enter on a focused link.
      await page.locator("thead th[data-col=amount] [data-slot=sort-link]").focus();
      await page.keyboard.press("Enter");
      await page.waitForURL(/sort=amount/);
      await settled(page);
      // A column menu opens with a key, and Escape closes it.
      await page.locator("[popovertarget=deals-m-stage]").focus();
      await page.keyboard.press("Enter");
      assert.ok(await page.locator("#deals-m-stage").evaluate((pop) => pop.matches(":popover-open")));
      await page.keyboard.press("Escape");
      assert.ok(!(await page.locator("#deals-m-stage").evaluate((pop) => pop.matches(":popover-open"))));
      // Resize with the arrow keys.
      const handle = page.locator("thead th[data-col=stage] [data-slot=col-resize]");
      await handle.focus();
      const width = await page.locator("thead th[data-col=stage]").evaluate((th) => th.getBoundingClientRect().width);
      await page.keyboard.press("ArrowRight");
      await page.keyboard.press("ArrowRight");
      await page.waitForURL(/w=/);
      await settled(page);
      const grown = await page.locator("thead th[data-col=stage]").evaluate((th) => th.getBoundingClientRect().width);
      assert.ok(Math.abs(grown - width - 32) <= 2, `${width} to ${grown}`);
      assert.ok(await page.evaluate(() => document.activeElement?.getAttribute("data-slot") === "col-resize"), "focus stays on the handle");
      // Reorder from the Columns menu, by keyboard.
      await page.locator("[popovertarget=deals-columns]").focus();
      await page.keyboard.press("Enter");
      const down = page.locator("#deals-columns [data-col=company] [aria-label='Move Company down']");
      await down.focus();
      await page.keyboard.press("Enter");
      await page.waitForURL(/cols=/);
      await settled(page);
      const order = await page.locator("thead th[data-col]").evaluateAll((cells) => cells.map((cell) => cell.getAttribute("data-col")));
      assert.ok(order.indexOf("stage") < order.indexOf("company"), order.join());
      await page.keyboard.press("Escape");
      // Tabbing never gets stuck, and focus is visible.
      await page.locator("#deals [name=q]").focus();
      for (let at = 0; at < 12; at++) {
        await page.keyboard.press("Tab");
        const shown = await page.evaluate(() => {
          const el = document.activeElement;
          if (!el || el === document.body) return "none";
          const style = getComputedStyle(el);
          return style.outlineStyle !== "none" || style.boxShadow !== "none" || el.matches("[data-slot=col-resize]");
        });
        assert.ok(shown, `a visible focus at tab ${at}`);
      }
      await close();
    });

    test("screen reader semantics: caption, scope, sort state, names, live status", async () => {
      await reset();
      const { page, close } = await open("/deals?sort=amount");
      assert.equal(await page.locator("#deals caption").innerText(), "Deals in the pipeline");
      assert.equal(await page.locator("thead th[data-col=title]").getAttribute("scope"), "col");
      assert.equal(await page.locator("thead th[data-col=amount]").getAttribute("aria-sort"), "ascending");
      assert.equal(await page.locator("#deals [data-slot=table-summary]").getAttribute("aria-live"), "polite");
      assert.equal(await page.locator("[data-slot=bulk-count]").getAttribute("aria-live"), "polite");
      assert.match(await page.locator("[data-ui-select-row]").first().getAttribute("aria-label"), /^Select /);
      assert.ok(await page.getByRole("region", { name: "Deals in the pipeline" }).count());
      const named = await page.locator("#deals button, #deals input:not([type=hidden]), #deals select, #deals a[href]").evaluateAll((items) => items.filter((item) => !item.closest("[popover]") && !(item.getAttribute("aria-label") || item.textContent?.trim() || item.labels?.length || item.getAttribute("placeholder"))).map((item) => item.outerHTML.slice(0, 120)));
      assert.deepEqual(named, []);
      await close();
    });

    test("a read-only user: no boxes, no actions, and the server refuses a forged post", async () => {
      await reset();
      const { page, close } = await open("/viewer");
      assert.equal(await page.locator("[data-ui-select-row],[data-ui-select-page],[data-slot=table-bulk]").count(), 0);
      assert.deepEqual(await axe(page), []);
      await shot(page, `${browserName}-read-only`);
      await post("/__t/permission/no-act");
      const response = await fetch(BASE + "/deals/bulk", { method: "POST", headers: { "content-type": "application/x-www-form-urlencoded", "x-navree-ui": "1" }, body: "action=archive&ids=1" });
      assert.equal(response.status, 403);
      assert.equal(await get("/__t/archived"), "");
      await post("/__t/permission/ok");
      await close();
    });

    test("a user who loses permission mid-session: fetch and action are both handled", async () => {
      await reset();
      const { page, close } = await open("/deals?per=10");
      const before = await keys(page);
      await page.locator("[data-ui-select-page]").check();
      await post("/__t/permission/no-act");
      await page.getByRole("button", { name: "Archive", exact: true }).click();
      await page.locator("[data-slot=toast]").first().waitFor();
      assert.match(await page.locator("[data-slot=toast]").first().innerText(), /can't do this/);
      assert.equal(await get("/__t/archived"), "");
      await post("/__t/permission/no-read");
      await page.locator("thead th[data-col=amount] [data-slot=sort-link]").click();
      const alert = page.locator("[data-slot=table-alert]");
      await alert.waitFor();
      assert.match(await alert.innerText(), /no longer have access/);
      assert.deepEqual(await keys(page), before, "the rows stay until the user leaves");
      assert.ok(!/sort=amount/.test(page.url()));
      await shot(page, `${browserName}-lost-access`);
      await post("/__t/permission/ok");
      await close();
    });

    test("an expired session on a table fetch goes to the sign-in page, never into the region", async () => {
      await reset();
      const { page, close } = await open("/deals");
      await page.route(/sort=amount/, (route) => route.fulfill({ status: 200, contentType: "text/html", headers: { location: "/" }, body: "<!doctype html><html><head><title>Sign in</title></head><body><form><input type=password name=password></form></body></html>" }));
      await page.locator("thead th[data-col=amount] [data-slot=sort-link]").click();
      await page.waitForFunction(() => document.querySelector("#deals") === null || document.querySelector("input[type=password]") !== null);
      assert.equal(await page.locator("#deals input[type=password]").count(), 0, "not inside the table");
      await close();
    });

    test("no GET a hover can fire changes anything; writes clear prefetched pages", async () => {
      await reset();
      const { page, close } = await open("/deals?per=10");
      const before = await get("/__t/count");
      const links = await page.locator("#deals a[href]").evaluateAll((items) => [...new Set(items.map((item) => item.getAttribute("href")))]);
      assert.ok(links.length > 10);
      assert.ok(links.every((href) => !/delete|archive/.test(href)), links.filter((href) => /delete|archive/.test(href)).join());
      for (const href of links.filter((href) => href.startsWith("/deals")).slice(0, 25)) await fetch(BASE + href);
      assert.equal(await get("/__t/count"), before);
      // Hover prefetches page 2; a write happens; the click must show the new rows.
      const next = page.getByRole("link", { name: "Page 2", exact: true });
      await next.hover();
      await page.waitForTimeout(200);
      const key = (await keys(page))[0];
      await page.locator(`[data-ui-select-row][value="${key}"]`).check();
      await page.getByRole("button", { name: "Delete", exact: true }).click();
      await page.locator("#deals-confirm").getByRole("button", { name: "Yes, go ahead" }).click();
      await page.locator("[data-slot=toast]").first().waitFor();
      await settled(page);
      const expected = await page.evaluate(async () => (await (await fetch("/deals?per=10&page=2")).text()).match(/data-key="(\d+)"/)[1]);
      await next.click();
      await page.waitForURL(/page=2/);
      await settled(page);
      assert.equal((await keys(page))[0], expected, "the page after the write");
      await close();
    });

    test("columns: visibility, reorder by drag, resize by drag, density", async () => {
      await reset();
      const { page, close } = await open("/deals?per=10");
      await page.locator("[popovertarget=deals-columns]").click();
      await shot(page, `${browserName}-columns`);
      await page.locator("#deals-columns [data-col=notes] input[type=checkbox]").check();
      await page.waitForURL(/cols=/);
      await settled(page);
      assert.equal(await page.locator("thead th[data-col=notes]").count(), 1, "a hidden column can be shown");
      await page.keyboard.press("Escape");
      const handle = page.locator("thead th[data-col=stage] [data-slot=col-resize]");
      const box = await handle.boundingBox();
      const width = await page.locator("thead th[data-col=stage]").evaluate((th) => th.getBoundingClientRect().width);
      await page.mouse.move(box.x + 3, box.y + 10);
      await page.mouse.down();
      await page.mouse.move(box.x + 83, box.y + 10, { steps: 4 });
      await page.mouse.up();
      await page.waitForURL(/w=/);
      await settled(page);
      const grown = await page.locator("thead th[data-col=stage]").evaluate((th) => th.getBoundingClientRect().width);
      assert.ok(Math.abs(grown - width - 80) <= 3, `${width} to ${grown}`);
      assert.match(await page.locator("#deals col[data-col=stage]").getAttribute("width"), /^\d+$/);
      assert.equal(await page.locator("[style]:not(#deals):not([popover])").evaluateAll((items) => items.filter((item) => item.closest("#deals")).length), 0, "no style attributes on cells");
      await page.reload();
      const kept = await page.locator("thead th[data-col=stage]").evaluate((th) => th.getBoundingClientRect().width);
      assert.ok(Math.abs(grown - kept) <= 2, `reload keeps the width: ${grown} ${kept}`);
      // Drag a heading onto another.
      await page.locator("thead th[data-col=stage] [data-ui-col-drag]").dragTo(page.locator("thead th[data-col=company]"));
      await page.waitForFunction(() => /cols=[^&]*stage(%2C|,)company/.test(location.search));
      await settled(page);
      await page.locator("[popovertarget=deals-view]").click();
      await page.locator("#deals-view a", { hasText: "Spacious" }).click();
      await page.waitForURL(/density=spacious/);
      await settled(page);
      await page.keyboard.press("Escape");
      await shot(page, `${browserName}-spacious`);
      await close();
    });

    test("details open in the URL, row actions post, sticky header stays", async () => {
      await reset();
      const { page, close, problems } = await open("/deals?sort=title&per=10");
      const key = (await keys(page))[0];
      const toggle = page.locator(`tr[data-key="${key}"] [data-slot=row-toggle]`);
      assert.equal(await toggle.getAttribute("aria-expanded"), "false");
      await toggle.click();
      await page.waitForURL(/open=/);
      await settled(page);
      assert.equal(await page.locator(`tr[data-key="${key}-detail"]`).count(), 1);
      assert.equal(await page.locator(`tr[data-key="${key}"] [data-slot=row-toggle]`).getAttribute("aria-expanded"), "true");
      await shot(page, `${browserName}-detail`);
      await page.locator(`tr[data-key="${key}"] [data-slot=cell-actions] [popovertarget]`).click();
      await shot(page, `${browserName}-row-actions`);
      assert.deepEqual(await axe(page), []);
      await page.locator(`#deals-a-${key}`).getByRole("button", { name: "Archive" }).click();
      await page.locator("[data-slot=toast]").first().waitFor();
      await page.keyboard.press("Escape");
      await page.goto(BASE + "/deals?per=100");
      const container = page.locator("#deals [data-slot=table-container]");
      await container.evaluate((box) => { box.scrollTop = 800; });
      const head = await page.locator("#deals thead").boundingBox();
      const area = await container.boundingBox();
      assert.ok(Math.abs(head.y - area.y) <= 2, `the header stays: ${head.y} ${area.y}`);
      await shot(page, `${browserName}-sticky`);
      assert.deepEqual(problems, []);
      await close();
    });

    test("saved views: save, apply, default, delete; a stale view is harmless; no selection held", async () => {
      await reset();
      const { page, close } = await open("/deals?f.stage=won&sort=title&per=10&page=2");
      await page.locator("[data-ui-select-row]").first().check();
      await page.locator("[popovertarget=deals-views]").click();
      await page.locator("#deals-views [name=_name]").fill("Won deals <b>");
      await page.locator("#deals-views [name=_default]").check();
      await page.locator("#deals-views").getByRole("button", { name: "Save this view" }).click();
      await page.locator("[data-slot=toast]").first().waitFor();
      await page.locator("#deals-views [data-slot=view-row]").first().waitFor();
      await shot(page, `${browserName}-views`);
      assert.deepEqual(await axe(page), []);
      const href = await page.locator("#deals-views [data-slot=view-row] a").first().getAttribute("href");
      assert.ok(!/page=/.test(href), "a view is how to look, not where one was");
      assert.ok(!/sel|ids/.test(href), "no selection");
      assert.match(await page.locator("#deals-views [data-slot=view-row] a").first().innerHTML(), /Won deals &lt;b&gt;/);
      const fresh = await page.context().newPage();
      await fresh.goto(BASE + "/deals");
      assert.equal(new URL(fresh.url()).searchParams.get("f.stage"), "won", "the default view opens");
      await fresh.close();
      await page.locator("#deals-views [data-slot=view-row] button").first().click();
      await page.waitForFunction(() => !document.querySelector("#deals-views [data-slot=view-row]"));
      await close();
    });

    test("CSV: the current filter, sort and columns, with formulas made text", async () => {
      await reset();
      const { page, close } = await open("/deals?sort=title&cols=title,stage,amount");
      const link = page.locator("#deals a[download]");
      assert.match(await link.getAttribute("href"), /^\/deals\/export\?/);
      const csv = await get("/deals/export?sort=title&cols=title,stage,amount&f.stage=won");
      const lines = csv.trim().split("\r\n");
      assert.match(lines[0], /^﻿?Deal,Stage,Amount$/);
      assert.equal(lines.length - 1, Number(await get("/__t/count?f.stage=won")));
      const all = await get("/deals/export?cols=title&per=10");
      assert.equal(all.trim().split("\r\n").length - 1, 137, "the whole result, not a page");
      assert.ok(all.includes(`"'=HYPERLINK(""http://evil.example"",""click"")"`), "a formula is text");
      await close();
    });

    test("allow-list in the browser: injected parameters change nothing", async () => {
      await reset();
      const clean = await get("/deals");
      const count = (html) => (html.match(/data-key="/g) || []).length;
      for (const attack of ["sort=id;DROP TABLE dt_deals", "sort=version", "per=1000000", "per=-1", "page=-5", "f.version=1", "cols=version", "w=title:99999999", "after=abc", "before=%00", "density=<script>", "open=1;DROP"]) {
        const html = await get(`/deals?${attack}`);
        assert.ok(count(html) <= 25, attack);
        assert.ok(!html.includes("<script>a"), attack);
      }
      assert.equal(count(clean), 25);
      assert.equal(await get("/__t/count"), "137", "the table is still there");
    });

    test("the million: pages by cursor and by number, filtered and sorted, stay under 500 ms", async () => {
      const timed = async (path) => {
        const start = performance.now();
        const response = await fetch(BASE + path);
        const html = await response.text();
        return { time: performance.now() - start, ok: response.ok, rows: (html.match(/<tr data-key=/g) || []).length, html };
      };
      // Warm first: this measures the table, not a cold disk on a busy machine.
      for (const path of ["/big", "/big-pages", "/big-pages?page=10000", "/big-pages?sort=title&page=19999", "/big?sort=amount", "/big?sort=-created_on"]) await timed(path);
      for (const path of ["/big", "/big?sort=amount", "/big?sort=-created_on", "/big-pages", "/big-pages?page=10000", "/big-pages?sort=title&page=19999", "/big-pages?f.stage=won", "/big-pages?f.amount.min=4000000&sort=-amount"]) {
        const { time, ok, rows: count } = await timed(path);
        assert.ok(ok && count === 50, `${path}: ${count}`);
        assert.ok(time < 800, `${path}: ${time} ms`);
      }
      const first = await timed("/big");
      const next = first.html.match(/href="(\/big\?after=[^"]+)"/);
      assert.ok(next, "a next link");
      const second = await timed(next[1].replaceAll("&amp;", "&"));
      assert.equal(second.rows, 50);
      assert.ok(second.time < 300, `${second.time}`);
      assert.match(first.html, /10,000\+/, "a capped count");
      const { page, close } = await open("/big");
      await page.getByRole("link", { name: "Next" }).click();
      await page.waitForURL(/after=/);
      await settled(page, "big");
      await page.getByRole("link", { name: "Previous" }).click();
      await page.waitForURL(/before=/);
      await settled(page, "big");
      await shot(page, `${browserName}-big`);
      assert.deepEqual(await axe(page), []);
      await close();
    });
  });
}
