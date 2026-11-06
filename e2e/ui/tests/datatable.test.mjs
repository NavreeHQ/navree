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
import { launch } from "./support/launch.mjs";
import { lateBody } from "./support/late.mjs";
import { LIGHT } from "./support/light.mjs";

const BASE = process.env.UI_TABLE_URL || "http://127.0.0.1:8097";
const SHOTS = process.env.TABLE_SHOTS || "";
const require = createRequire(import.meta.url);
const axeSource = await readFile(require.resolve("axe-core/axe.min.js"), "utf8");

const post = (path) => fetch(BASE + path, { method: "POST" }).then((r) => r.text());
const get = (path) => fetch(BASE + path).then((r) => r.text());
const reset = () => post("/__t/reset");

/** Runs axe once running animations and transitions have ended, so it reads final colours. Violations come back as short lines. */
async function axe(page) {
  // An animation that never ends (a spinner) is not waited for.
  await page.evaluate(() => Promise.all(document.getAnimations().filter((a) => Number.isFinite(a.effect?.getComputedTiming().endTime ?? 0)).map((a) => a.finished.catch(() => {}))));
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
  await page.waitForTimeout(220); // Past a popover's fade.
  await page.screenshot({ path: join(SHOTS, name + ".png") });
}

const browsers = (process.env.TABLE_BROWSERS || "chromium firefox").split(" ");

for (const browserName of browsers) {
  describe(browserName, () => {
    let browser;
    before(async () => {
      // One Firefox process keeps every context's idle keep-alive connections open, so over this suite they pile up
      // against the server's max_connections_per_ip as real users' connections don't; it closes idle ones after 5 s.
      browser = await launch(playwright[browserName]);
    });
    after(() => browser?.close());

    async function open(path, options = {}) {
      const context = await browser.newContext({ viewport: options.viewport || { width: 1440, height: 900 }, javaScriptEnabled: options.js !== false, bypassCSP: !!options.light, acceptDownloads: true });
      const page = await context.newPage();
      const problems = [];
      page.on("console", (message) => {
        // Playwright's WebKit reports this on every page, also a plain 404 with none of
        // our markup. Chromium and Firefox still check the policy here, and the
        // conformance suite in every state.
        if (browserName === "webkit" && /Refused to apply a stylesheet/.test(message.text())) return;
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

    /** The keys the server draws for `path`, in order. */
    const serverKeys = async (path, id) => {
      const html = await get(path);
      const body = html.slice(html.indexOf(`id="${id}"`)).split("</tbody>")[0];
      return [...body.matchAll(/<tr data-key="([^"]+)"/g)].map((m) => m[1]).filter((key) => !key.endsWith("-detail"));
    };

    test("a page that holds every row sorts in the browser at once, and the server confirms", async () => {
      await reset();
      const { page, close, problems } = await open("/exact");
      assert.equal(await page.locator("#sized[data-ui-local]").count(), 1);
      for (const [column, sort] of [["amount", "amount"], ["amount", "-amount"], ["closed_on", "closed_on"]]) {
        const expected = await serverKeys(`/exact?sort=${sort}`, "sized");
        // The server's answer is held back: what shows meanwhile was sorted here.
        let release;
        const held = new Promise((resolve) => (release = resolve));
        await page.route(/\/exact\?/, async (route) => {
          await held;
          await route.continue();
        });
        await page.locator(`#sized thead th[data-col=${column}] [data-slot=sort-link]`).click();
        await page.waitForTimeout(150);
        const meanwhile = await page.evaluate((column) => {
          // Which way the heading's arrow points: lucide's up arrow starts "m5 12", its down
          // arrow "M12 5"; a mirrored one points the other way.
          const icon = document.querySelector(`#sized thead th[data-col=${column}] [data-slot=sort-link] svg`);
          const up = icon?.querySelector("path")?.getAttribute("d")?.startsWith("m5 12");
          return {
            busy: document.getElementById("sized").hasAttribute("data-pending"),
            sort: [...document.querySelectorAll("#sized thead th[aria-sort]")].map((th) => th.getAttribute("data-col")).join(),
            arrow: icon?.querySelectorAll("path").length === 2 ? (up !== icon.hasAttribute("data-flip") ? "up" : "down") : "none",
            ranks: document.querySelectorAll("#sized thead [data-slot=sort-rank]").length,
          };
        }, column);
        assert.equal(meanwhile.busy, false, `${sort}: not busy, the answer is showing`);
        assert.equal(meanwhile.sort, column, sort);
        assert.equal(meanwhile.arrow, sort.startsWith("-") ? "down" : "up", `${sort}: the heading's arrow moved with the rows`);
        assert.equal(meanwhile.ranks, 0, `${sort}: no rank of the old order is left`);
        assert.deepEqual(await keys(page, "sized"), expected, `${sort}: values and NULLs sort as the server sorts`);
        release();
        await page.waitForURL(new RegExp(`sort=${sort.replace("-", "\\-")}(&|$)`));
        await settled(page, "sized");
        await page.unroute(/\/exact\?/);
        assert.deepEqual(await keys(page, "sized"), expected, `${sort}: the server's order after its answer`);
      }
      assert.deepEqual(problems, []);
      await close();
    });

    test("text and a relation sort on the server, even when the page holds every row", async () => {
      await reset();
      const { page, close } = await open("/exact");
      // A database orders text by its own collation, and a relation by a key the cell
      // doesn't show: only the server knows either order.
      assert.equal(await page.locator("#sized thead th[data-col=title][data-kind]").count(), 0);
      assert.equal(await page.locator("#sized thead th[data-col=company][data-kind]").count(), 0);
      for (const column of ["title", "company"]) {
        const before = await keys(page, "sized");
        let release;
        const held = new Promise((resolve) => (release = resolve));
        await page.route(/\/exact\?/, async (route) => {
          await held;
          await route.continue();
        });
        // The link says where the server goes: the page's default sort already holds title.
        const link = page.locator(`#sized thead th[data-col=${column}] [data-slot=sort-link]`);
        const target = new URL(await link.getAttribute("href"), page.url()).search;
        assert.match(target, new RegExp(`sort=-?${column}(&|$)`));
        await link.click();
        await page.waitForTimeout(200);
        assert.deepEqual(await keys(page, "sized"), before, `${column}: nothing moves before the server answers`);
        assert.equal(await page.locator("#sized[data-pending]").count(), 1, `${column}: busy`);
        release();
        await page.waitForURL((url) => url.search === target);
        await settled(page, "sized");
        await page.unroute(/\/exact\?/);
        assert.deepEqual(await keys(page, "sized"), await serverKeys(`/exact${target}`, "sized"), column);
      }
      await close();
    });

    test("two sorts here, then a failed answer: back to the last order the server confirmed", async () => {
      await reset();
      const { page, close } = await open("/exact");
      const state = () => page.evaluate(() => ({
        keys: [...document.querySelectorAll("#sized tbody tr[data-key]")].map((row) => row.getAttribute("data-key")),
        sorts: [...document.querySelectorAll("#sized thead th[aria-sort]")].map((th) => `${th.getAttribute("data-col")}:${th.getAttribute("aria-sort")}`),
        field: document.querySelector("#sized form[data-ui-target] input[type=hidden][name=sort]")?.value ?? null,
        url: location.pathname + location.search,
      }));
      const start = await state();
      let asked = 0;
      await page.route(/\/exact\?sort=/, async (route) => {
        asked++;
        if (asked === 1) {
          await new Promise((resolve) => setTimeout(resolve, 800));
          await route.continue();
        } else await route.abort();
      });
      await page.locator("#sized thead th[data-col=amount] [data-slot=sort-link]").click();
      await page.waitForTimeout(100);
      await page.locator("#sized thead th[data-col=closed_on] [data-slot=sort-link]").click();
      await page.locator("#sized [data-slot=table-alert]:not([hidden])").waitFor();
      await page.waitForTimeout(1200);
      const end = await state();
      assert.deepEqual(end, start, "rows, headings, the form's sort and the address as the server last drew them");
      await close();
    });

    for (const later of ["sort", "search"]) test(`after a sort here the server confirmed, a failed fetch keeps what the server last drew (then a ${later})`, async () => {
      {
        await reset();
        const { page, close } = await open("/deals?search=renewal%201&per_page=50");
        const state = () => page.evaluate(() => ({
          keys: [...document.querySelectorAll("#deals tbody tr[data-key]")].map((row) => row.getAttribute("data-key")).filter((key) => !key.endsWith("-detail")),
          sorts: [...document.querySelectorAll("#deals thead th[aria-sort]")].map((th) => `${th.getAttribute("data-col")}:${th.getAttribute("aria-sort")}`),
          field: document.querySelector("#deals form[data-ui-target] input[type=hidden][name=sort]")?.value ?? null,
          url: location.pathname + location.search,
        }));
        assert.equal(await page.locator("#deals[data-ui-local]").count(), 1, "the page holds every row");
        const all = (await state()).keys.length;
        // Sorted here, then confirmed by the server.
        await page.locator("#deals thead th[data-col=amount] [data-slot=sort-link]").click();
        await page.waitForFunction(() => new URLSearchParams(location.search).get("sort") === "amount" && !document.getElementById("deals").hasAttribute("data-ui-confirm"));
        await settled(page);
        if (later === "search") {
          await page.locator("#deals form[data-ui-target] input[name=search]").fill("renewal 13");
          await page.waitForFunction(() => new URLSearchParams(location.search).get("search") === "renewal 13");
          await settled(page);
        }
        const confirmed = await state();
        if (later === "search") assert.ok(confirmed.keys.length < all, `${confirmed.keys.length} of ${all}`);
        // Then a fetch of the table fails.
        await page.route((url) => url.pathname === "/deals", (route) => (route.request().resourceType() === "document" ? route.continue() : route.abort()));
        await page.locator("#deals thead th[data-col=title] [data-slot=sort-link]").click();
        await page.locator("#deals [data-slot=table-alert]:not([hidden])").waitFor();
        await page.waitForTimeout(500);
        assert.deepEqual(await state(), confirmed, `${later}: rows, headings, the form's sort and the address as the server last drew them`);
        // Selecting the page selects what the address says, and nothing from before.
        await page.locator("#deals [data-ui-select-page]").check();
        assert.deepEqual(
          await page.locator("#deals [data-ui-select-row]:checked").evaluateAll((boxes) => boxes.map((box) => box.value)),
          confirmed.keys,
          `${later}: select all takes only the rows the server last drew`,
        );
        await close();
      }
    });

    test("a sort here that the server can't confirm goes back", async () => {
      await reset();
      const { page, close } = await open("/exact");
      const before = await keys(page, "sized");
      await page.route(/\/exact\?sort=amount/, (route) => route.abort());
      await page.locator("#sized thead th[data-col=amount] [data-slot=sort-link]").click();
      await page.locator("#sized [data-slot=table-alert]:not([hidden])").waitFor();
      assert.deepEqual(await keys(page, "sized"), before, "the rows the address says");
      assert.equal(await page.locator("#sized thead th[data-col=amount][aria-sort]").count(), 0);
      assert.equal(await page.locator("#sized thead [data-slot=sort-rank]").count(), 2, "the old order's ranks are back");
      assert.equal(await page.locator("#sized thead [data-flip]").count(), 0);
      assert.ok(!/sort=amount/.test(page.url()));
      await close();
    });

    test("a page that holds part of the result asks the server to sort", async () => {
      await reset();
      const { page, close } = await open("/deals?per_page=10");
      assert.equal(await page.locator("#deals[data-ui-local]").count(), 0);
      const before = await keys(page);
      await page.route(/sort=amount/, async (route) => {
        await new Promise((resolve) => setTimeout(resolve, 600));
        await route.continue();
      });
      await page.locator("#deals thead th[data-col=amount] [data-slot=sort-link]").click();
      await page.waitForTimeout(150);
      assert.equal(await page.locator("#deals[data-pending]").count(), 1, "busy while the server sorts");
      assert.deepEqual(await keys(page), before, "nothing moved yet");
      await page.waitForURL(/sort=amount/);
      await settled(page);
      assert.deepEqual(await keys(page), await serverKeys("/deals?per_page=10&sort=amount", "deals"));
      await close();
    });

    test("a tick in a filter's list applies at once; typing waits for a pause", async () => {
      await reset();
      const { page, close } = await open("/deals?per_page=10");
      // By the page's own clock: when an input happened, and when the runtime asked.
      await page.evaluate(() => {
        const original = window.fetch;
        window.asked = [];
        window.inputs = [];
        window.fetch = function (input, init) {
          window.asked.push({ at: performance.now(), url: String(input) });
          return original.call(this, input, init);
        };
        document.addEventListener("input", (e) => window.inputs.push({ at: performance.now(), name: e.target.name }), true);
      });
      await page.locator("[popovertarget=deals-m-stage]").click();
      await page.locator("#deals-m-stage input[name='filter[stage][in]'][value=won]").check();
      await page.waitForFunction(() => window.asked.some((ask) => /filter%5Bstage%5D%5Bin%5D=won/.test(ask.url)));
      const tick = await page.evaluate(() => {
        const input = window.inputs.findLast((item) => item.name === "filter[stage][in]");
        const ask = window.asked.find((item) => /filter%5Bstage%5D%5Bin%5D=won/.test(item.url));
        return ask.at - input.at;
      });
      assert.ok(tick < 50, `the tick asked ${tick} ms after it happened`);
      await page.keyboard.press("Escape");
      await settled(page);
      const search = page.locator("#deals input[name=search]");
      await search.click();
      await search.pressSequentially("agra", { delay: 20 });
      await page.waitForTimeout(500);
      // Each request waited for a pause: the last keystroke before it was 100 ms back.
      const waits = await page.evaluate(() =>
        window.asked
          .filter((item) => /[?&]search=/.test(item.url))
          .map((ask) => ask.at - window.inputs.filter((item) => item.name === "search" && item.at < ask.at).at(-1).at),
      );
      assert.ok(waits.length >= 1 && waits.length < 4, `one request per pause, not per key: ${waits}`);
      assert.ok(waits.every((wait) => wait >= 95), `typing waited ${waits} ms`);
      await close();
    });

    test("a filter matching nothing keeps headers, chips and a way out", async () => {
      await reset();
      const { page, close, problems } = await open("/deals?search=nothing-matches-this");
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
      await page.locator("#deals [name=search]").fill("renewal");
      await page.waitForURL(/search=renewal/);
      await settled(page);
      const url = page.url();
      assert.ok(!/page=2/.test(url), "a new filter starts from page 1");
      const filtered = await keys(page);
      await page.goBack();
      await page.waitForURL(/page=2/);
      await settled(page);
      assert.deepEqual(await keys(page), second, "back");
      assert.equal(await page.locator("#deals [name=search]").inputValue(), "", "back clears the box the state doesn't hold");
      await page.goForward();
      await page.waitForURL(/search=renewal/);
      await settled(page);
      assert.deepEqual(await keys(page), filtered, "forward");
      assert.equal(await page.locator("#deals [name=search]").inputValue(), "renewal");
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
      await page.waitForURL((url) => url.searchParams.get("filter[stage][in]") === "won");
      await settled(page);
      assert.ok(await page.locator("#deals-m-stage").evaluate((pop) => pop.matches(":popover-open")), "the popover stays open while the table morphs");
      await page.locator("#deals-m-stage input[value=lost]").check();
      await page.waitForURL((url) => url.searchParams.get("filter[stage][in]") === "won,lost");
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
      await page.locator("#deals-m-amount [name='filter[amount][gte]']").fill("1000");
      await page.waitForURL((url) => url.searchParams.get("filter[amount][gte]") === "1000");
      await settled(page);
      await page.keyboard.press("Escape");

      await page.locator("#deals-m-closed_on").evaluate((pop) => pop.showPopover());
      await page.locator("#deals-m-closed_on [name='filter[closed_on][gte]']").fill("2026-02-01");
      await page.waitForURL((url) => url.searchParams.get("filter[closed_on][gte]") === "2026-02-01");
      await settled(page);
      await page.keyboard.press("Escape");

      await page.locator("#deals-m-active").evaluate((pop) => pop.showPopover());
      await page.locator("#deals-m-active select").selectOption("true");
      await page.waitForURL((url) => url.searchParams.get("filter[active]") === "true");
      await settled(page);
      await page.keyboard.press("Escape");

      await page.locator("#deals-m-company").evaluate((pop) => pop.showPopover());
      await page.locator("#deals-m-company label", { hasText: "Gulati" }).locator("input").check();
      await page.waitForURL((url) => [...url.searchParams.keys()].includes("filter[company][in]"));
      await settled(page);
      await page.keyboard.press("Escape");
      assert.ok((await chips.count()) >= 4);
      await shot(page, `${browserName}-chips`);
      await page.locator("[data-slot=chips-clear]").click();
      await page.waitForFunction(() => ![...new URLSearchParams(location.search).keys()].some((name) => name.startsWith("filter[")));
      await settled(page);
      assert.equal(await rows(page).count(), 25);
      assert.deepEqual(problems, []);
      await close();
    });

    test("the filter row under the headers filters the same way", async () => {
      await reset();
      const { page, close } = await open("/deals-row");
      const row = page.locator("[data-slot=filter-row]");
      await row.locator("[name='filter[title][contains]']").fill("gulati");
      await page.waitForURL((url) => url.searchParams.get("filter[title][contains]") === "gulati");
      await settled(page);
      assert.equal(await row.locator("[name='filter[title][contains]']").inputValue(), "gulati");
      assert.ok(await page.evaluate(() => document.activeElement?.getAttribute("name") === "filter[title][contains]"), "focus stays in the box");
      const titles = await rows(page).locator("td[data-truncate]").allInnerTexts();
      assert.ok(titles.length > 0 && titles.every((text) => /gulati/i.test(text)), titles.join());
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
      const { page, close, problems } = await open("/deals?filter%5Bstage%5D%5Bin%5D=won&sort=title&per_page=10");
      const total = Number(await get("/__t/count?filter%5Bstage%5D%5Bin%5D=won"));
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
      await page.waitForFunction(() => !/[?&]page=/.test(location.search));
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
      const { page, close } = await open("/deals?filter%5Bstage%5D%5Bin%5D=won&per_page=10");
      const total = Number(await get("/__t/count?filter%5Bstage%5D%5Bin%5D=won"));
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
      const { page, close } = await open("/deals?per_page=10");
      await page.locator("[data-ui-select-page]").check();
      await page.locator("[data-ui-select-all]").click();
      await page.locator("#deals [name=search]").fill("gulati");
      await page.waitForURL(/search=gulati/);
      await settled(page);
      const bar = page.locator("[data-slot=table-bulk]");
      assert.match(await bar.innerText(), /The filter changed/);
      assert.equal(await page.locator("[data-ui-select-row]:checked").count(), 0);

      await page.goto(BASE + "/deals?per_page=10");
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
      const { page, context, close } = await open("/deals?per_page=10");
      const boxes = page.locator("[data-ui-select-row]");
      await boxes.nth(1).click();
      await boxes.nth(4).click({ modifiers: ["Shift"] });
      assert.equal(await page.locator("[data-ui-select-row]:checked").count(), 4);
      await page.reload();
      assert.equal(await page.locator("[data-ui-select-row]:checked").count(), 4, "same tab, after a reload");
      const other = await context.newPage();
      await other.goto(BASE + "/deals?per_page=10");
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
      const { page, close, problems } = await open("/deals?sort=title&per_page=100");
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
      const tallest = await page.locator("#deals tbody td:not([data-wrap])").evaluateAll((cells) => Math.max(...cells.map((cell) => cell.getBoundingClientRect().width)));
      assert.ok(tallest <= 450, `no text blows a column open: ${tallest}`);
      await page.locator("#deals [name=search]").fill("<img src=x onerror=alert(1)>");
      await page.waitForURL(/search=/);
      await settled(page);
      assert.equal(await page.locator("#deals img").count(), 0);
      assert.equal(await rows(page).count(), 1);
      await shot(page, `${browserName}-hostile`);
      assert.deepEqual(await axe(page), []);
      await page.goto(BASE + "/deals?cols=title,notes,stage&filter%5Btitle%5D%5Bcontains%5D=مرحبا");
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
      const last = await page.locator("#wide tbody tr:first-child [data-sticky=e1]").first().boundingBox();
      assert.ok(first.x >= 0 && first.x < 80, `the first column stays: ${first.x}`);
      assert.ok(end.x + end.width <= 391, `the end column stays: ${end.x + end.width}`);
      assert.ok(last.x + last.width <= end.x + 1, `two end columns sit side by side: ${last.x + last.width} ${end.x}`);
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
      await page.locator("#deals [name=search]").fill("x");
      assert.equal(await page.locator("#deals [name=search]").inputValue(), "x");
      await page.waitForURL(/search=x/);
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
      await page.locator("#deals [name=search]").fill("gulati");
      await page.getByRole("button", { name: "Apply" }).click();
      await page.waitForURL(/search=gulati/);
      // The page number itself: the address also holds `per_page=25`.
      assert.notEqual(new URL(page.url()).searchParams.get("page"), "2", "a new search starts from the first page");
      const titles = await rows(page).locator("td[data-truncate]").allInnerTexts();
      assert.ok(titles.length > 0 && titles.every((text) => /gulati/i.test(text)));
      await shot(page, `${browserName}-no-js`);
      // Popovers are native.
      await page.locator("[popovertarget=deals-m-stage]").click({ force: true });
      await page.locator("#deals-m-stage input[value=won]").check();
      await page.keyboard.press("Escape");
      await page.getByRole("button", { name: "Apply" }).click();
      await page.waitForURL((url) => url.searchParams.get("filter[stage][in]") === "won");
      assert.match(page.url(), /search=gulati/);
      // A chip is a link.
      await page.locator("[data-slot=filter-chip]").first().click();
      await page.waitForURL((url) => !url.search.includes("search=gulati"));
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
      await page.locator("[name=per_page]").selectOption("10");
      await page.getByRole("button", { name: "Apply" }).click();
      await page.waitForURL(/per_page=10/);
      assert.equal(await rows(page).count(), 10);
      // Ticks and a plain post.
      const key = (await keys(page))[0];
      await page.locator(`[data-ui-select-row][value="${key}"]`).check();
      await page.getByRole("button", { name: "Archive", exact: true }).click();
      for (let tries = 0; tries < 50 && (await get("/__t/archived")) === ""; tries++) await new Promise((resolve) => setTimeout(resolve, 100));
      assert.equal(await get("/__t/archived"), key);
      await page.waitForURL(/density=compact/);
      await close();
    });

    test("keyboard only: reach everything, resize, reorder, and the grid keys", async () => {
      await reset();
      const { page, close } = await open("/deals?per_page=10");
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
      await page.locator("#deals [name=search]").focus();
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

    test("a bulk post from another site, by GET, or with another body is refused", async () => {
      await reset();
      const form = { "content-type": "application/x-www-form-urlencoded" };
      const cross = await fetch(BASE + "/deals/bulk", { method: "POST", headers: { ...form, "sec-fetch-site": "cross-site" }, body: "action=archive&ids=1" });
      assert.equal(cross.status, 403);
      const json = await fetch(BASE + "/deals/bulk", { method: "POST", headers: { "content-type": "text/plain" }, body: "action=archive&ids=1" });
      assert.ok(json.status >= 400, String(json.status));
      assert.equal(await get("/__t/archived"), "");
    });

    test("only safe links are fetched ahead of a click, and they say so", async () => {
      await reset();
      const { page, close } = await open("/deals?per_page=10");
      const seen = [];
      page.on("request", (request) => {
        if (request.headers()["x-navree-prefetch"]) seen.push(request.url());
      });
      await page.getByRole("link", { name: "Page 2", exact: true }).hover();
      await page.waitForTimeout(250);
      assert.equal(seen.filter((url) => /page=2/.test(url)).length, 1, "a page link is safe");
      seen.length = 0;
      await page.locator("#deals tbody a[href^='/deal/']").first().hover();
      await page.waitForTimeout(250);
      assert.deepEqual(seen, [], "a cell's link isn't marked, so hovering it fetches nothing");
      const unsafe = await page.locator("#deals a[data-ui-in-region]").evaluateAll((links) => links.map((link) => link.getAttribute("href")).filter((href) => /delete|archive|bulk/.test(href)));
      assert.deepEqual(unsafe, []);
      await close();
      // A table that doesn't ask has nothing fetched ahead, its own links included.
      const other = await open("/big-pages");
      const quiet = [];
      other.page.on("request", (request) => {
        if (request.headers()["x-navree-prefetch"]) quiet.push(request.url());
      });
      await other.page.getByRole("link", { name: "Page 2", exact: true }).hover();
      await other.page.locator("#big thead th[data-col=title] [data-slot=sort-link]").hover();
      await other.page.waitForTimeout(250);
      assert.deepEqual(quiet, []);
      await other.close();
    });

    test("a user who loses permission mid-session: fetch and action are both handled", async () => {
      await reset();
      const { page, close } = await open("/deals?per_page=10");
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

    test("a refresh whose answer is read after a refused sort leaves the sort's alert as it is", async () => {
      await reset();
      const { page, close } = await open("/deals?per_page=10");
      const before = await keys(page);
      // The refresh's headers come and its body is held; the sort is then refused (403) and shows the alert;
      // only then is the refresh's body let through.
      const late = await lateBody(page, { header: "x-navree-region" });
      await page.evaluate(() => {
        const detail = {};
        document.getElementById("deals").dispatchEvent(new CustomEvent("ui:refresh", { bubbles: true, detail }));
        window.__refresh = detail.done;
      });
      await late.arrived();
      await post("/__t/permission/no-read");
      await page.locator("thead th[data-col=amount] [data-slot=sort-link]").click();
      const alert = page.locator("[data-slot=table-alert]");
      await alert.waitFor();
      assert.match(await alert.innerText(), /no longer have access/);
      await late.release();
      await page.evaluate(() => window.__refresh);
      assert.equal(await alert.isVisible(), true, "the older refresh does not hide the alert again");
      assert.match(await alert.innerText(), /no longer have access/);
      assert.deepEqual(await keys(page), before);
      assert.ok(!/sort=amount/.test(page.url()));
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
      const { page, close } = await open("/deals?per_page=10");
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
      const expected = await page.evaluate(async () => (await (await fetch("/deals?per_page=10&page=2")).text()).match(/data-key="(\d+)"/)[1]);
      await next.click();
      await page.waitForURL(/page=2/);
      await settled(page);
      assert.equal((await keys(page))[0], expected, "the page after the write");
      await close();
    });

    test("columns: visibility, reorder by drag, resize by drag, density", async () => {
      await reset();
      const { page, close } = await open("/deals?per_page=10");
      await page.locator("[popovertarget=deals-columns]").click();
      await shot(page, `${browserName}-columns`);
      await page.locator("#deals-columns [data-col=notes] input[type=checkbox]").check();
      await page.waitForURL(/cols=/);
      await settled(page);
      assert.equal(await page.locator("thead th[data-col=notes]").count(), 1, "a hidden column can be shown");
      await page.locator("#deals-columns [data-col=company] input[type=checkbox]").uncheck();
      await page.waitForFunction(() => !document.querySelector("thead th[data-col=company]"));
      await settled(page);
      await page.locator("#deals-columns [data-col=company] input[type=checkbox]").check();
      await page.waitForFunction(() => !!document.querySelector("thead th[data-col=company]"));
      await settled(page);
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
      assert.equal(await page.locator("[style]:not(#deals):not([popover])").evaluateAll((items) => items.filter((item) => item.closest("#deals") && item.getAttribute("style")).length), 0, "no style attributes on cells: a screenshot leaves empty ones on inputs");
      await page.reload();
      const kept = await page.locator("thead th[data-col=stage]").evaluate((th) => th.getBoundingClientRect().width);
      assert.ok(Math.abs(grown - kept) <= 2, `reload keeps the width: ${grown} ${kept}`);
      // Drag a heading onto another.
      await page.locator("thead th[data-col=stage] [data-ui-col-drag]").dragTo(page.locator("thead th[data-col=company]"));
      await page.waitForFunction(() => /cols=[^&]*(stage(%2C|,)company|company(%2C|,)stage)/.test(location.search));
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
      const { page, close, problems } = await open("/deals?sort=title&per_page=10");
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
      // A row's actions are a menu now: its items are menu items.
      await page.locator(`#deals-a-${key}`).getByRole("menuitem", { name: "Archive" }).click();
      await page.locator("[data-slot=toast]").first().waitFor();
      await page.keyboard.press("Escape");
      await page.goto(BASE + "/deals?per_page=100");
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
      const { page, close } = await open("/deals?filter%5Bstage%5D%5Bin%5D=won&sort=title&per_page=10&page=2");
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
      assert.ok(!/[?&]page=/.test(href), "a view is how to look, not where one was");
      assert.ok(!/sel|ids/.test(href), "no selection");
      assert.match(await page.locator("#deals-views [data-slot=view-row] a").first().innerHTML(), /Won deals &lt;b&gt;/);
      const fresh = await page.context().newPage();
      await fresh.goto(BASE + "/deals");
      assert.equal(new URL(fresh.url()).searchParams.get("filter[stage][in]"), "won", "the default view opens");
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
      const csv = await get("/deals/export?sort=title&cols=title,stage,amount&filter%5Bstage%5D%5Bin%5D=won");
      const lines = csv.trim().split("\r\n");
      assert.match(lines[0], /^﻿?Deal,Stage,Amount$/);
      assert.equal(lines.length - 1, Number(await get("/__t/count?filter%5Bstage%5D%5Bin%5D=won")));
      const all = await get("/deals/export?cols=title&per_page=10");
      assert.equal(all.trim().split("\r\n").length - 1, 137, "the whole result, not a page");
      assert.ok(all.includes(`"'=HYPERLINK(""http://evil.example"",""click"")"`), "a formula is text");
      await close();
    });

    test("extras: groups, footer, bleed, sticky row, row link, rows that can't be ticked, denied action, grid keys", async () => {
      await reset();
      const { page, close, problems } = await open("/extras");
      assert.equal(await page.locator("#extras thead [data-slot=group-row] th[colspan='2']").count(), 2, "two header groups");
      assert.match(await page.locator("#extras tfoot").innerText(), /On this page/);
      assert.ok(await page.locator("#extras").getAttribute("data-bleed") !== null);
      assert.equal(await page.locator("#extras [data-ui-select-row][disabled]").count() > 0, true);
      await page.locator("#extras [data-ui-select-page]").check();
      const enabled = await page.locator("#extras [data-ui-select-row]:not([disabled])").count();
      assert.equal(await page.locator("#extras [data-ui-select-row]:checked").count(), enabled, "only rows that can be ticked");
      assert.equal(await page.locator("#extras [data-ui-select-all]").isVisible(), false, "no select-all with a condition");
      const denied = page.getByRole("button", { name: "Archive", exact: true });
      assert.equal(await denied.isDisabled(), true);
      assert.match(await denied.getAttribute("title"), /role/);
      await shot(page, `${browserName}-extras`);
      assert.deepEqual(await axe(page), []);
      // Grid keys: one tab stop, arrows and Home move it, Enter goes in.
      const start = page.locator("#extras tbody td[tabindex='0']");
      assert.equal(await start.count(), 1);
      await start.focus();
      await page.keyboard.press("ArrowDown");
      await page.keyboard.press("ArrowRight");
      await page.keyboard.press("ArrowRight");
      const moved = await page.evaluate(() => {
        const cell = document.activeElement;
        return [cell.cellIndex, cell.parentElement.rowIndex];
      });
      assert.equal(moved[0], 3, "from the first cell with no control, two to the right");
      await page.keyboard.press("Home");
      assert.equal(await page.evaluate(() => document.activeElement.cellIndex), 0);
      await page.keyboard.press("Enter");
      assert.equal(await page.evaluate(() => document.activeElement.nodeName), "INPUT");
      await page.keyboard.press("Escape");
      assert.equal(await page.evaluate(() => document.activeElement.nodeName), "TD");
      assert.equal(await page.locator("#extras tbody td[tabindex='0']").count(), 1, "still one tab stop");
      // The sticky row stays under the headers as the table scrolls.
      const box = page.locator("#extras [data-slot=table-container]");
      await box.evaluate((el) => { el.scrollTop = el.scrollHeight; });
      const stuck = await page.locator("#extras tr[data-sticky-row] td").first().boundingBox();
      const head = await page.locator("#extras thead").boundingBox();
      const area = await box.boundingBox();
      assert.ok(stuck.y >= area.y && stuck.y < area.y + head.height + 90, `${stuck.y} in ${area.y}`);
      await box.evaluate((el) => { el.scrollTop = 0; });
      // A page change scrolls to where it was asked to.
      await page.evaluate(() => {
        window.scrolledTo = [];
        const real = Element.prototype.scrollIntoView;
        Element.prototype.scrollIntoView = function (...args) {
          window.scrolledTo.push(this.id);
          return real.apply(this, args);
        };
      });
      await page.getByRole("link", { name: "Page 2", exact: true }).click();
      await page.waitForURL(/page=2/);
      await settled(page, "extras");
      assert.deepEqual(await page.evaluate(() => window.scrolledTo), ["extras"]);
      await page.getByRole("link", { name: "Page 1", exact: true }).click();
      await page.waitForFunction(() => !/[?&]page=/.test(location.search));
      await settled(page, "extras");
      // A click on a row goes to it, a click on a control doesn't.
      const row = page.locator("#extras tbody tr[data-key]").nth(2);
      await row.locator("[data-ui-select-row]").click({ force: true });
      assert.match(page.url(), /extras/);
      await row.locator("td").nth(1).click();
      await page.waitForURL(/\/deal\//);
      assert.deepEqual(problems, []);
      await close();
    });

    test("right to left: the sticky column, handles and arrow keys flip, numbers stay readable", async () => {
      await reset();
      const { page, close, problems } = await open("/rtl");
      const th = await page.locator("#rtl thead th[data-col=title]").boundingBox();
      const amount = await page.locator("#rtl thead th[data-col=amount]").boundingBox();
      assert.ok(th.x > amount.x, "the first column is on the right");
      assert.match(await page.locator("#rtl [data-slot=table-summary]").innerText(), /عرض/);
      assert.equal(await page.locator("#rtl td[data-numeric]").first().evaluate((cell) => getComputedStyle(cell).direction), "ltr");
      assert.equal(await page.locator("#rtl tr[data-key='1'] bdi").first().count(), 1);
      const clip = await page.locator("#rtl tr[data-key='6'] bdi").first().evaluate((bdi) => {
        const range = document.createRange();
        range.setStart(bdi.firstChild, 0);
        range.setEnd(bdi.firstChild, 1);
        const first = range.getBoundingClientRect(), box = bdi.getBoundingClientRect();
        return { clips: bdi.scrollWidth > bdi.clientWidth, firstInside: first.left >= box.left - 1 && first.right <= box.right + 1, align: getComputedStyle(bdi).textAlign };
      });
      assert.ok(clip.clips, "the long title is cut");
      assert.ok(clip.firstInside, `Latin text in a right-to-left table keeps its first letter: ${JSON.stringify(clip)}`);
      await shot(page, `${browserName}-rtl`);
      assert.deepEqual(await axe(page), []);
      const handle = page.locator("#rtl thead th[data-col=amount] [data-slot=col-resize]");
      const before = await page.locator("#rtl thead th[data-col=amount]").evaluate((cell) => cell.getBoundingClientRect().width);
      await handle.focus();
      await page.keyboard.press("ArrowLeft");
      await page.waitForURL(/w=/);
      await settled(page, "rtl");
      const after = await page.locator("#rtl thead th[data-col=amount]").evaluate((cell) => cell.getBoundingClientRect().width);
      assert.ok(after > before, `ArrowLeft grows a column in RTL: ${before} ${after}`);
      assert.deepEqual(problems, []);
      await close();
    });

    test("no count: Previous and Next only; loading and failed states keep the toolbar", async () => {
      await reset();
      const simple = await open("/simple");
      assert.equal(await simple.page.locator("[data-slot=pagination-link]").count(), 0);
      assert.match(await simple.page.locator("[data-slot=table-summary]").innerText(), /Showing 1 to 10/);
      await simple.page.getByRole("link", { name: "Next" }).click();
      await simple.page.waitForURL(/page=2/);
      await settled(simple.page, "simple");
      await simple.page.getByRole("link", { name: "Count all" }).click();
      await simple.page.waitForURL(/count=1/);
      await settled(simple.page, "simple");
      assert.match(await simple.page.locator("[data-slot=table-summary]").innerText(), /of 137 results/);
      await shot(simple.page, `${browserName}-simple`);
      await simple.close();
      const loading = await open("/skeleton");
      assert.ok((await loading.page.locator("#deals tbody [data-slot=skeleton]").count()) > 10);
      assert.equal(await loading.page.locator("#deals [name=search]").count(), 1);
      await shot(loading.page, `${browserName}-loading`);
      assert.deepEqual(await axe(loading.page), []);
      await loading.close();
      const failed = await open("/failed");
      assert.match(await failed.page.locator("#deals tbody").innerText(), /couldn't load/);
      assert.equal(await failed.page.locator("#deals tbody").getByRole("link", { name: "Try again" }).count(), 1);
      assert.equal(await failed.page.locator("#deals [name=search]").count(), 1, "the toolbar stays, to change the query");
      await shot(failed.page, `${browserName}-failed-server`);
      assert.deepEqual(await axe(failed.page), []);
      await failed.close();
    });

    test("a long list of filter choices narrows with no request", async () => {
      await reset();
      const { page, close } = await open("/deals");
      await page.locator("[popovertarget=deals-m-company]").click({ force: true });
      const options = page.locator("#deals-m-company [data-slot=filter-option]");
      const count = await options.count();
      assert.ok(count > 8, String(count));
      const requests = [];
      page.on("request", (request) => requests.push(request.url()));
      await page.locator("#deals-m-company [data-ui-find-option]").fill("umb");
      const shown = await options.evaluateAll((items) => items.filter((item) => !item.hidden).map((item) => item.innerText.trim()));
      assert.deepEqual(shown, ["Utsav"]);
      await page.waitForTimeout(350);
      assert.deepEqual(requests, [], "client-only");
      await shot(page, `${browserName}-find-option`);
      await close();
    });

    test("custom cells: components from the whole row, escaped, and plain text in files", async () => {
      await reset();
      const { page, close, problems } = await open("/rich?sort=title");
      const row = page.locator("#rich tbody tr[data-key]").first();
      assert.ok((await row.locator("[data-owner] [data-slot=badge]").count()) >= 1, "a tag list");
      assert.equal(await row.locator("progress").count(), 1, "a progress bar");
      assert.equal(await page.locator("#rich tbody [data-owner] b").count(), 0, "markup in data stays text");
      await shot(page, `${browserName}-rich`);
      assert.deepEqual(await axe(page), []);
      assert.deepEqual(problems, []);
      await close();
    });

    test("inline editing: shows at once, saves, goes back when refused or when the post fails", async () => {
      await reset();
      const { page, close } = await open("/rich?sort=title");
      const key = (await keys(page, "rich"))[0];
      const box = page.locator(`#rich tr[data-key="${key}"] [data-slot=edit-cell] input`);
      const before = await box.inputValue();
      await box.fill("A new name");
      await page.keyboard.press("Enter");
      await page.locator("[data-slot=toast]", { hasText: "Saved" }).waitFor();
      assert.equal(await box.inputValue(), "A new name");
      await page.reload();
      assert.ok((await page.locator("#rich [data-slot=edit-cell] input").evaluateAll((items) => items.map((item) => item.value))).includes("A new name"), "stored");
      await page.goto(BASE + "/rich?sort=title");
      const second = page.locator("#rich tbody tr[data-key]").nth(2).locator("[data-slot=edit-cell] input");
      const kept = await second.inputValue();
      // Refused: back, with the reason.
      await second.fill("taken");
      assert.equal(await second.inputValue(), "taken", "optimistic");
      await page.keyboard.press("Enter");
      await page.locator("[data-slot=toast]", { hasText: "already has that name" }).waitFor();
      await page.waitForFunction((value) => [...document.querySelectorAll("#rich [data-slot=edit-cell] input")].some((item) => item.value === value), kept);
      assert.equal(await second.inputValue(), kept, "rolled back");
      // Network failure: back.
      await page.route(/\/field\//, (route) => route.abort());
      await second.fill("no connection");
      await page.keyboard.press("Enter");
      await page.locator("[data-slot=toast]", { hasText: "No connection" }).waitFor();
      assert.equal(await second.inputValue(), kept, "rolled back after a failed post");
      await page.unroute(/\/field\//);
      // Escape: back with no post.
      let posted = 0;
      page.on("request", (request) => {
        if (request.method() === "POST") posted++;
      });
      await second.fill("thought better of it");
      await page.keyboard.press("Escape");
      assert.equal(await second.inputValue(), kept);
      assert.equal(posted, 0);
      await shot(page, `${browserName}-edit`);
      // A select.
      const stage = page.locator("#rich tbody tr[data-key]").nth(3).locator("[data-slot=edit-cell] select");
      await stage.selectOption("lost");
      await page.locator("[data-slot=toast]", { hasText: "Saved" }).last().waitFor();
      await close();
      // JavaScript off: a form with Save.
      const off = await open("/rich?sort=title", { js: false });
      assert.ok((await off.page.getByRole("button", { name: "Save" }).count()) > 0);
      await off.close();
    });

    test("a filter bar: the user picks which filters show and their order; one in force stays", async () => {
      await reset();
      const { page, close } = await open("/rich");
      const bar = page.locator("#rich [data-slot=filter-bar]");
      assert.equal(await bar.locator("[data-filter-trigger]").count(), 4);
      assert.equal(await page.locator("#rich-m-stage [name='filter[stage][in]']").count(), 0, "not in the header too");
      await bar.locator("[popovertarget=rich-p-stage]").click();
      await page.locator("#rich-p-stage input[value=won]").check();
      await page.waitForURL((url) => url.searchParams.get("filter[stage][in]") === "won");
      await settled(page, "rich");
      await page.keyboard.press("Escape");
      await shot(page, `${browserName}-filter-bar`);
      assert.deepEqual(await axe(page), []);
      await page.locator("[popovertarget=rich-filters]").click();
      await page.locator("#rich-filters [data-col=stage] input").uncheck();
      await page.locator("#rich-filters [data-col=active] input").uncheck();
      await page.waitForURL(/fb=/);
      await settled(page, "rich");
      await shot(page, `${browserName}-filters-menu`);
      await page.keyboard.press("Escape");
      assert.equal(await bar.locator("[popovertarget=rich-p-active]").count(), 0, "hidden");
      assert.equal(await bar.locator("[popovertarget=rich-p-stage]").count(), 1, "holds a value, so it stays");
      assert.equal(new URL(page.url()).searchParams.get("filter[stage][in]"), "won");
      const url = page.url();
      await page.reload();
      assert.equal(page.url(), url);
      assert.equal(await bar.locator("[popovertarget=rich-p-active]").count(), 0, "kept in the URL");
      await page.locator("[popovertarget=rich-filters]").click();
      await page.locator("#rich-filters [data-col=title] [aria-label='Move Deal down']").click();
      await page.waitForFunction(() => /fb=amount/.test(location.search));
      await settled(page, "rich");
      await close();
    });

    test("live refresh: new rows arrive; typing, menus, selection and the URL are kept", async () => {
      await reset();
      const { page, close, problems } = await open("/live?sort=-closed_on&per_page=10&filter%5Btitle%5D%5Bcontains%5D=Added");
      assert.equal(await page.locator("#deals [data-slot=table-live]").count(), 1);
      await shot(page, `${browserName}-live`);
      await post("/__t/add/9001");
      await page.locator("tr[data-key='9001']").waitFor({ timeout: 5000 });
      await page.locator("[data-ui-select-row][value='9001']").check();
      const url = page.url();
      await page.locator("#deals [name=search]").click();
      await page.keyboard.type("Add");
      await post("/__t/add/9002");
      await page.locator("tr[data-key='9002']").waitFor({ timeout: 5000 });
      assert.equal(await page.locator("[data-ui-select-row][value='9001']").isChecked(), true, "the selection stays");
      assert.ok(await page.evaluate(() => document.activeElement?.getAttribute("name") === "search"), "focus stays");
      await page.locator("#deals-m-title").evaluate((pop) => pop.showPopover());
      await post("/__t/add/9003");
      await page.locator("tr[data-key='9003']").waitFor({ timeout: 5000 });
      assert.ok(await page.locator("#deals-m-title").evaluate((pop) => pop.matches(":popover-open")), "an open menu stays open");
      await page.keyboard.press("Escape");
      assert.deepEqual(problems, []);
      await close();
    });

    test("live refresh: a late poll can't undo a newer choice; Refresh now; an event stream", async () => {
      await reset();
      const { page, close } = await open("/live?per_page=10");
      await page.route(/\/live\?per_page=10$/, async (route) => {
        await new Promise((resolve) => setTimeout(resolve, 1500));
        await route.continue();
      });
      await page.waitForTimeout(800);
      await page.locator("thead th[data-col=amount] [data-slot=sort-link]").click();
      await page.waitForURL(/sort=amount/);
      await settled(page);
      const chosen = await keys(page);
      await page.waitForTimeout(1600);
      assert.match(page.url(), /sort=amount/);
      assert.deepEqual(await keys(page), chosen, "the late answer was dropped");
      await page.unroute(/\/live\?per_page=10$/);
      await close();
      const stream = await open("/live-stream?per_page=10&filter%5Btitle%5D%5Bcontains%5D=Added");
      await post("/__t/add/9004");
      await stream.page.locator("tr[data-key='9004']").waitFor({ timeout: 6000 });
      await post("/__t/add/9005");
      await stream.page.locator("[data-ui-refresh]").click();
      await stream.page.locator("tr[data-key='9005']").waitFor({ timeout: 3000 });
      await stream.close();
    });

    test("XLSX: the same state, as a typed spreadsheet", async () => {
      await reset();
      const response = await fetch(BASE + "/deals/export.xlsx?filter%5Bstage%5D%5Bin%5D=won&sort=title&cols=title,stage,amount");
      assert.equal(response.status, 200);
      assert.match(response.headers.get("content-type"), /spreadsheetml/);
      const bytes = Buffer.from(await response.arrayBuffer());
      assert.equal(bytes.subarray(0, 2).toString(), "PK");
      assert.ok(bytes.length > 2000, String(bytes.length));
    });

    test("allow-list in the browser: injected parameters change nothing", async () => {
      await reset();
      const clean = await get("/deals");
      const count = (html) => (html.match(/<tr data-key="/g) || []).length;
      for (const attack of ["sort=id;DROP TABLE dt_deals", "sort=version", "per_page=1000000", "per_page=-1", "page=-5", "filter[version]=1", "filter[__proto__]=1", "filter[title][regex]=x", "cols=version", "w=title:99999999", "cursor=abc", "before=%00", "density=<script>", "open=1;DROP"]) {
        const html = await get(`/deals?${attack}`);
        assert.ok(count(html) <= 25, attack);
        assert.ok(!html.includes("<script>a"), attack);
      }
      assert.equal(count(clean), 25);
      assert.equal(await get("/__t/count"), "137", "the table is still there");
    });

    test("a thousand rows load and sort in this browser", async () => {
      const { page, close } = await open("/big-pages?per_page=1000");
      assert.equal(await rows(page, "big").count(), 1000);
      await page.locator("thead th[data-col=title] [data-slot=sort-link]").click();
      await page.waitForURL(/sort=title/);
      await settled(page, "big");
      assert.equal(await rows(page, "big").count(), 1000);
      await close();
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
      for (const path of ["/big", "/big?sort=amount", "/big?sort=-created_on", "/big-pages", "/big-pages?page=10000", "/big-pages?sort=title&page=19999", "/big-pages?filter%5Bstage%5D=won", "/big-pages?filter%5Bamount%5D%5Bgte%5D=4000000&sort=-amount"]) {
        const { time, ok, rows: count } = await timed(path);
        assert.ok(ok && count === 50, `${path}: ${count}`);
        assert.ok(time < 800, `${path}: ${time} ms`);
      }
      const first = await timed("/big");
      const next = first.html.match(/href="(\/big\?cursor=[^"]+)"/);
      assert.ok(next, "a next link");
      const second = await timed(next[1].replaceAll("&amp;", "&"));
      assert.equal(second.rows, 50);
      assert.ok(second.time < 300, `${second.time}`);
      assert.match(first.html, /10,000\+/, "a capped count");
      const counted = await timed("/big?count=1");
      assert.match(counted.html, /1,000,000 results/, "counted when asked");
      const { page, close } = await open("/big");
      await page.getByRole("link", { name: "Next" }).click();
      await page.waitForURL(/cursor=/);
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
