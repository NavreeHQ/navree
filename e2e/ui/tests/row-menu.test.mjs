// The menu of actions on a table row, against the data table proof (the same server as
// datatable.test.mjs):
//   ui-table-proof seed 1000000 && PORT=8097 ui-table-proof serve
// UI_TABLE_URL points elsewhere; TABLE_BROWSERS="chromium firefox webkit" picks browsers.
//
// The button names its row, the menu has the roles and keys of a menu, a link goes, a post
// posts and a destructive one is marked; a thousand rows carry no control tied to a form;
// and the menu works with JavaScript off.

import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { createRequire } from "node:module";
import { after, before, describe, test } from "node:test";
import * as playwright from "playwright";

const BASE = process.env.UI_TABLE_URL || "http://127.0.0.1:8097";
const require = createRequire(import.meta.url);
const axeSource = await readFile(require.resolve("axe-core/axe.min.js"), "utf8");
const PHONE = { width: 390, height: 844 };

// The light values of the tokens: the toolkit ships dark only so far, so light is proved by putting these on the page.
const LIGHT = ":root{color-scheme:light;--ink-0:#ffffff;--ink-1:#f7f8f7;--ink-2:#eff1f0;--ink-3:#e5e8e6;--line:#dfe3e0;--line-2:#c8cecb;--line-3:#aeb6b2;--fg:#0a0e0c;--fg-1:#1c2420;--fg-2:#3f4a44;--fg-3:#55615b;--accent:#0b7a3e;--accent-ink:#ffffff;--amber:#8a5a00;--danger:#b3261e;--bar:#8a968f}";

const post = (path) => fetch(BASE + path, { method: "POST" }).then((r) => r.text());
const get = (path) => fetch(BASE + path).then((r) => r.text());

async function axe(page, selector) {
  await page.evaluate(() => Promise.all(document.getAnimations().filter((a) => Number.isFinite(a.effect?.getComputedTiming().endTime ?? 0)).map((a) => a.finished.catch(() => {}))));
  await page.evaluate(axeSource);
  return page.evaluate(async (scope) => {
    const out = await window.axe.run(scope ? document.querySelector(scope) : document, { runOnly: ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa", "best-practice"] });
    return out.violations.map((v) => `${v.id}: ${v.nodes.slice(0, 3).map((n) => n.target.join(" ")).join(" | ")}`);
  }, selector || null);
}

const settled = (page, id = "deals") => page.waitForFunction((table) => !document.getElementById(table)?.hasAttribute("data-pending") && !document.documentElement.hasAttribute("data-ui-loading"), id);
const rows = (page, id = "deals") => page.locator(`#${id} tbody tr[data-key]`);
const keys = (page, id = "deals") => rows(page, id).evaluateAll((items) => items.map((row) => row.getAttribute("data-key")).filter((key) => !key.endsWith("-detail")));
const isOpen = (page, selector) => page.locator(selector).evaluate((e) => e.matches(":popover-open"));
const focusedText = (page) =>
  page.evaluate(() => {
    const e = document.activeElement;
    return (e?.closest?.("[data-slot=menu-item]")?.querySelector("[data-slot=menu-text]")?.textContent || e?.id || e?.tagName || "").trim();
  });

const browsers = (process.env.TABLE_BROWSERS || "chromium firefox").split(" ");

for (const browserName of browsers) {
  describe(browserName, () => {
    let browser;
    before(async () => {
      browser = await playwright[browserName].launch();
    });
    after(() => browser?.close());

    async function open(path, options = {}) {
      const context = await browser.newContext({ viewport: options.viewport || { width: 1440, height: 900 }, javaScriptEnabled: options.js !== false, bypassCSP: !!options.light, reducedMotion: "reduce" });
      const page = await context.newPage();
      const problems = [];
      page.on("console", (message) => {
        if (browserName === "webkit" && /Refused to apply a stylesheet/.test(message.text())) return;
        if (message.type() === "error" && !/Failed to load resource/.test(message.text())) problems.push(message.text());
      });
      page.on("pageerror", (error) => problems.push(String(error)));
      await page.goto(BASE + path);
      if (options.light) await page.addStyleTag({ content: LIGHT });
      return {
        page,
        problems,
        close: async () => {
          await context.close();
          assert.deepEqual(problems, [], "nothing in the console and no script error");
        },
      };
    }

    /** Opens the first row's menu with the keyboard and returns the row's key. */
    async function firstMenu(page) {
      const key = (await keys(page))[0];
      await page.locator(`#deals-a-${key}-t`).focus();
      await page.keyboard.press("Enter");
      return key;
    }

    // UI-GENUI-012
    test("a row's button names its row and opens a menu with the roles of a menu", async () => {
      await post("/__t/reset");
      const { page, close } = await open("/deals?sort=title&per=10");
      const key = (await keys(page))[0];
      const trigger = page.locator(`#deals-a-${key}-t`);
      assert.equal(await trigger.getAttribute("aria-haspopup"), "menu");
      assert.equal(await trigger.getAttribute("aria-expanded"), "false");
      assert.match(await trigger.getAttribute("aria-label"), /^Actions for .+/);
      await trigger.click();
      const menu = page.locator(`#deals-a-${key}`);
      assert.equal(await menu.getAttribute("role"), "menu");
      assert.equal(await menu.getAttribute("aria-labelledby"), `deals-a-${key}-t`);
      assert.equal(await trigger.getAttribute("aria-expanded"), "true");
      assert.deepEqual(await menu.locator("[role=menuitem]").evaluateAll((all) => all.map((e) => e.textContent.trim())), ["View", "Archive", "Delete"]);
      assert.equal(await menu.locator("[data-destructive]").count(), 1);
      assert.equal(await menu.locator("[data-destructive]").innerText(), "Delete");
      await close();
    });

    // UI-GENUI-012
    test("the keys of a menu: arrows, Home, End, type-ahead, Escape back to the button, Tab out", async () => {
      await post("/__t/reset");
      const { page, close } = await open("/deals?sort=title&per=10");
      const key = await firstMenu(page);
      assert.ok(await isOpen(page, `#deals-a-${key}`));
      assert.equal(await focusedText(page), "View", "the keyboard opens it on the first item");
      await page.keyboard.press("ArrowDown");
      assert.equal(await focusedText(page), "Archive");
      await page.keyboard.press("ArrowDown");
      assert.equal(await focusedText(page), "Delete");
      await page.keyboard.press("ArrowDown");
      assert.equal(await focusedText(page), "View", "wraps");
      await page.keyboard.press("End");
      assert.equal(await focusedText(page), "Delete");
      await page.keyboard.press("Home");
      await page.keyboard.press("a");
      assert.equal(await focusedText(page), "Archive");
      await page.keyboard.press("Escape");
      assert.ok(!(await isOpen(page, `#deals-a-${key}`)));
      assert.equal(await focusedText(page), `deals-a-${key}-t`, "Escape returns to the row's button");
      assert.equal(await page.locator(`#deals-a-${key}-t`).getAttribute("aria-expanded"), "false");
      // Up from the button opens it on the last item.
      await page.keyboard.press("ArrowUp");
      assert.equal(await focusedText(page), "Delete");
      await page.keyboard.press("Tab");
      assert.ok(!(await isOpen(page, `#deals-a-${key}`)), "Tab closes it");
      assert.notEqual(await page.evaluate(() => document.activeElement.tagName), "BODY");
      await close();
    });

    // UI-GENUI-012
    test("View is a link, Archive posts, Delete posts and is marked, each from the keyboard", async () => {
      await post("/__t/reset");
      const { page, close } = await open("/deals?sort=title&per=10");
      let key = await firstMenu(page);
      await page.keyboard.press("Enter");
      await page.waitForURL(new RegExp(`/deal/${key}$`));
      await page.goBack();
      await page.waitForSelector("#deals");

      key = (await keys(page))[0];
      await page.locator(`#deals-a-${key}-t`).focus();
      await page.keyboard.press("Enter");
      await page.keyboard.press("ArrowDown");
      await page.keyboard.press("Enter");
      await page.locator("[data-slot=toast]").first().waitFor();
      assert.match(await page.locator("[data-slot=toast]").first().innerText(), /Archived/);
      assert.ok(!(await isOpen(page, `#deals-a-${key}`)), "choosing closes the menu");
      for (let tries = 0; tries < 50 && !(await get("/__t/archived")).split(",").includes(key); tries++) await new Promise((resolve) => setTimeout(resolve, 100));
      assert.ok((await get("/__t/archived")).split(",").includes(key), "the server archived exactly that row");

      await settled(page);
      const next = (await keys(page)).find((other) => other !== key);
      await page.locator(`#deals-a-${next}-t`).click();
      await page.locator(`#deals-a-${next}`).getByRole("menuitem", { name: "Delete" }).click();
      await page.waitForFunction((gone) => !document.querySelector(`tr[data-key="${gone}"]`), next);
      await close();
    });

    // UI-GENUI-012
    test("a pointer opens it with focus on the menu, and hover lights an item", async () => {
      const { page, close } = await open("/deals?sort=title&per=10");
      const key = (await keys(page))[0];
      await page.locator(`#deals-a-${key}-t`).click();
      assert.equal(await page.evaluate(() => document.activeElement.id), `deals-a-${key}`);
      await page.locator(`#deals-a-${key}`).getByRole("menuitem", { name: "Archive" }).hover();
      assert.equal(await focusedText(page), "Archive");
      await page.mouse.click(5, 5);
      assert.ok(!(await isOpen(page, `#deals-a-${key}`)), "a press outside closes it");
      await close();
    });

    // UI-GENUI-012
    test("a thousand rows: a menu each, no control tied to a form, and the first opens at once", async () => {
      const { page, close } = await open("/thousand?per=1000");
      assert.equal(await page.locator("#thousand [role=menu]").count(), 1000);
      assert.equal(await page.locator('[form="ui-post"]').count(), 0, "nothing is tied to the page's post form");
      const owned = await page.evaluate(() => [...document.querySelectorAll("#thousand tbody [form]")].length);
      assert.equal(owned, 1000, "only each row's own selection box names a form");
      const key = (await keys(page, "thousand"))[0];
      const started = Date.now();
      await page.locator(`#thousand-a-${key}-t`).click();
      await page.waitForFunction((k) => document.getElementById(`thousand-a-${k}`).matches(":popover-open"), key);
      assert.ok(Date.now() - started < 1500, `opened in ${Date.now() - started} ms`);
      await close();
    });

    // UI-GENUI-012
    test("a menu on the last visible row opens above its button when there is no room below", async () => {
      const { page, close } = await open("/deals?per=100", { viewport: { width: 1440, height: 700 } });
      const key = (await keys(page)).at(-1);
      await page.locator(`#deals-a-${key}-t`).scrollIntoViewIfNeeded();
      await page.locator(`#deals-a-${key}-t`).click();
      const menu = await page.locator(`#deals-a-${key}`).boundingBox();
      assert.ok(menu.y >= 0 && menu.y + menu.height <= 700, JSON.stringify(menu));
      await close();
    });

    // UI-GENUI-012
    test("a table with no actions loads no menus module", async () => {
      const { page, close } = await open("/viewer");
      assert.equal(await page.locator("[data-slot=cell-actions]").count(), 0);
      assert.equal(await page.locator('script[src*="/menus-"]').count(), 0);
      await close();
    });

    // UI-GENUI-012
    test("with JavaScript off the button opens the menu, a post is a form that works, and Escape closes it", async () => {
      await post("/__t/reset");
      const { page, close } = await open("/deals?sort=title&per=10", { js: false });
      const key = (await keys(page))[0];
      await page.locator(`#deals-a-${key}-t`).click();
      assert.ok(await isOpen(page, `#deals-a-${key}`));
      await page.keyboard.press("Escape");
      assert.ok(!(await isOpen(page, `#deals-a-${key}`)));
      await page.locator(`#deals-a-${key}-t`).click();
      await page.locator(`#deals-a-${key}`).getByRole("menuitem", { name: "Archive" }).click();
      for (let tries = 0; tries < 50 && !(await get("/__t/archived")).split(",").includes(key); tries++) await new Promise((resolve) => setTimeout(resolve, 100));
      assert.ok((await get("/__t/archived")).split(",").includes(key));
      await close();
    });

    for (const [label, options] of [
      ["dark at 1440", {}],
      ["light at 1440", { light: true }],
      ["dark on a phone", { viewport: PHONE }],
      ["light on a phone", { light: true, viewport: PHONE }],
    ]) {
      // UI-GENUI-012
      test(`axe finds nothing with a row's menu open (${label})`, async () => {
        await post("/__t/reset");
        const { page, close } = await open("/deals?sort=title&per=10", options);
        const key = (await keys(page))[0];
        await page.locator(`#deals-a-${key}-t`).click();
        await page.keyboard.press("ArrowDown");
        assert.deepEqual(await axe(page), []);
        await close();
      });
    }
  });
}
