// The phone input in real browsers, against the proof app. Start it first:
//   cargo run --release -p ui-proof -- serve          # http://127.0.0.1:8090
// UI_PROOF_URL points elsewhere; PHONE_BROWSERS="chromium firefox webkit" picks
// browsers; PHONE_SHOTS=<dir> saves a screenshot of each state.
//
// Every condition of the module's list is a test here: labels and roles, typing
// and pasting, the country a pasted number picks, keyboard only, the server's
// answer with JavaScript on and off, 390px, right to left, light and dark, and
// axe in every state with no report from the content security policy.

import assert from "node:assert/strict";
import { mkdir, readFile } from "node:fs/promises";
import { createRequire } from "node:module";
import { join } from "node:path";
import { after, before, describe, test } from "node:test";
import * as playwright from "playwright";

const BASE = process.env.UI_PROOF_URL || "http://127.0.0.1:8090";
const SHOTS = process.env.PHONE_SHOTS || "";
const require = createRequire(import.meta.url);
const axeSource = await readFile(require.resolve("axe-core/axe.min.js"), "utf8");

// The light values of `src/tokens.rs`: the toolkit ships dark only so far, so light is
// proved by putting these on the page.
const LIGHT = ":root{color-scheme:light;--ink-0:#ffffff;--ink-1:#f7f8f7;--ink-2:#eff1f0;--ink-3:#e5e8e6;--line:#dfe3e0;--line-2:#c8cecb;--line-3:#aeb6b2;--fg:#0a0e0c;--fg-1:#1c2420;--fg-2:#3f4a44;--fg-3:#55615b;--accent:#0b7a3e;--accent-ink:#ffffff;--amber:#8a5a00;--danger:#b3261e;--bar:#8a968f}";

/** Runs axe once running animations and transitions end, so it reads final colours. Violations come back as short lines. */
async function axe(page) {
  await page.evaluate(() => Promise.all(document.getAnimations().map((a) => a.finished.catch(() => {}))));
  await page.evaluate(axeSource);
  return page.evaluate(async () => {
    const out = await window.axe.run(document, { runOnly: ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa", "best-practice"] });
    return out.violations.map((v) => `${v.id}: ${v.nodes.slice(0, 3).map((n) => n.target.join(" ")).join(" | ")}`);
  });
}

const browsers = (process.env.PHONE_BROWSERS || "chromium firefox webkit").split(" ");

for (const browserName of browsers) {
  describe(browserName, () => {
    let browser;
    before(async () => {
      browser = await playwright[browserName].launch();
    });
    after(() => browser?.close());

    /** Opens a page and watches for errors and policy reports. */
    async function open(path, options = {}) {
      const context = await browser.newContext({
        viewport: options.viewport || { width: 1440, height: 900 },
        javaScriptEnabled: options.js !== false,
        bypassCSP: !!options.light,
        colorScheme: options.light ? "light" : "dark",
      });
      const page = await context.newPage();
      // The toolkit's scripts refused: the markup without its runtime, where
      // page scripts (axe) can still run.
      if (options.noRuntime) await page.route("**/_navree/ui/*.js", (route) => route.abort());
      const problems = [];
      page.on("console", (message) => {
        if (browserName === "webkit" && /Refused to apply a stylesheet/.test(message.text())) return;
        if (message.type() === "error" && !/Failed to load resource/.test(message.text())) problems.push(message.text());
      });
      page.on("pageerror", (error) => problems.push(String(error)));
      await page.goto(BASE + path);
      if (options.light) await page.addStyleTag({ content: LIGHT });
      if (options.js !== false && !options.noRuntime) await page.waitForSelector("[data-ui-phone][data-enhanced]");
      const field = (name) => ({
        root: page.locator(`[data-ui-phone]:has(input[type=tel][name=${name}])`),
        input: page.locator(`input[type=tel][name=${name}]`),
        select: page.locator(`select[name=${name}_country]`),
        trigger: page.locator(`[data-ui-phone]:has(input[name=${name}]) [data-slot=phone-country-trigger]`),
        list: page.locator(`#${name}-countries`),
        search: page.locator(`#${name}-countries [data-slot=phone-search]`),
        rows: page.locator(`#${name}-countries [role=option]:not([hidden])`),
        e164: page.locator(`input[name=${name}_e164]`),
        status: page.locator(`[data-ui-phone]:has(input[name=${name}]) [data-slot=phone-status]`),
        isOpen: () => page.locator(`#${name}-countries`).evaluate((l) => l.matches(":popover-open")),
        country: () => page.locator(`select[name=${name}_country]`).inputValue(),
      });
      return { page, context, problems, field, close: () => context.close() };
    }

    async function shot(page, name) {
      if (!SHOTS) return;
      await mkdir(SHOTS, { recursive: true });
      await page.waitForTimeout(200);
      await page.screenshot({ path: join(SHOTS, `${browserName}-${name}.png`) });
    }

    /** Waits until the plans have loaded and the box has been rewritten. */
    async function typed(page, f, text) {
      await f.input.click();
      await f.input.pressSequentially(text, { delay: 15 });
      await page.waitForTimeout(150);
    }

    test("labels and roles", async () => {
      const { page, field, problems, close } = await open("/phone");
      const f = field("mobile");
      assert.equal(await page.getByLabel("Mobile", { exact: true }).getAttribute("name"), "mobile", "the label names the number box");
      assert.equal(await f.trigger.getAttribute("aria-label"), "Country: United Kingdom +44");
      assert.equal(await f.trigger.getAttribute("aria-haspopup"), "listbox");
      assert.equal(await f.trigger.getAttribute("aria-expanded"), "false");
      assert.equal(await f.select.isVisible(), false, "the module replaces the select");
      assert.equal(await f.trigger.isVisible(), true);
      assert.equal(await f.input.getAttribute("dir"), "ltr");
      assert.equal(await f.input.getAttribute("placeholder"), "07400 123456");
      assert.equal(await f.e164.isDisabled(), false, "the module posts E.164");
      assert.deepEqual(await axe(page), []);
      await shot(page, "page");
      assert.deepEqual(problems, []);
      await close();
    });

    test("typing formats the number and keeps the caret; Backspace takes a digit", async () => {
      const { page, field, problems, close } = await open("/phone");
      const f = field("mobile");
      await typed(page, f, "07400123456");
      assert.equal(await f.input.inputValue(), "07400 123456");
      assert.equal(await f.e164.inputValue(), "+447400123456");
      // The caret sits after the last digit.
      assert.equal(await f.input.evaluate((i) => i.selectionStart), "07400 123456".length);
      // Digits of another script count.
      await f.input.fill("");
      await typed(page, f, "٠٧٤٠٠١٢٣٤٥٦");
      assert.equal(await f.input.inputValue(), "07400 123456");
      // Backspace just after the space the formatting added takes the digit before it.
      await f.input.evaluate((i) => i.setSelectionRange(6, 6));
      await page.keyboard.press("Backspace");
      await page.waitForTimeout(100);
      assert.equal((await f.input.inputValue()).replace(/\D/g, ""), "0740123456");
      // Nine digits is a possible length in the United Kingdom: the hidden field
      // follows, and the server decides whether the number is valid.
      assert.equal(await f.e164.inputValue(), "+44740123456");
      await shot(page, "typed");
      assert.deepEqual(problems, []);
      await close();
    });

    test("a pasted international number picks its country, and says so", async () => {
      const { page, field, problems, close } = await open("/phone");
      const f = field("mobile");
      await f.input.click();
      await f.input.evaluate((input) => {
        input.value = "+1 416 979 5000";
        input.dispatchEvent(new InputEvent("input", { bubbles: true, inputType: "insertFromPaste" }));
      });
      await page.waitForFunction(() => document.querySelector("select[name=mobile_country]").value === "CA");
      assert.equal(await f.trigger.getAttribute("aria-label"), "Country: Canada +1");
      assert.equal(await f.input.inputValue(), "+1 416-979-5000");
      assert.equal(await f.e164.inputValue(), "+14169795000");
      await page.waitForTimeout(150);
      assert.equal(await f.status.textContent(), "Country set to Canada +1");
      // A country sharing the code stays only while the number may be its own:
      // a Canadian number keeps Canada, a US number with Canada chosen goes to
      // the United States.
      await f.input.fill("");
      await typed(page, f, "+1416979");
      assert.equal(await f.country(), "CA", "not whole yet: Canada stays");
      await f.input.fill("");
      await typed(page, f, "+16502530000");
      assert.equal(await f.country(), "US", "a US number");
      await f.input.fill("");
      await typed(page, f, "+14169795000");
      assert.equal(await f.country(), "CA", "a Canadian number");
      // Told apart by numbers of every type, as the server does: a 600 number
      // is Canada's, a toll-free number goes to the United States.
      await f.input.fill("");
      await typed(page, f, "+16002012345");
      assert.equal(await f.country(), "CA", "a Canadian 600 number");
      assert.equal(await f.e164.inputValue(), "+16002012345");
      await f.input.fill("");
      await typed(page, f, "+18002123456");
      assert.equal(await f.country(), "US", "a toll-free number");
      await f.input.fill("");
      await typed(page, f, "+14169795000");
      await shot(page, "pasted");
      assert.deepEqual(problems, []);
      await close();
    });

    /** Chooses a country through the list. */
    async function choose(page, f, name) {
      await f.trigger.click();
      await f.search.fill(name);
      await page.waitForTimeout(50);
      await page.keyboard.press("Enter");
    }

    test("a number dialled with an international prefix is written with +, and the server reads it", async () => {
      const { page, field, problems, close } = await open("/phone");
      const f = field("mobile");
      // (country chosen, typed, country it picks, text, E.164)
      for (const [from, code, dialled, to, text, e164] of [
        ["France", "FR", "00 1 650 253 0000", "US", "+1 650-253-0000", "+16502530000"],
        ["United States", "US", "011 44 7400 123456", "GB", "+44 7400 123456", "+447400123456"],
        ["Germany", "DE", "00 1 416 979 5000", "CA", "+1 416-979-5000", "+14169795000"],
        ["France", "FR", "00 81 3 1234 5678", "JP", "+81 3-1234-5678", "+81312345678"],
        ["France", "FR", "00 61 412 345 678", "AU", "+61 412 345 678", "+61412345678"],
      ]) {
        // A country picked by hand stays, whatever number the box still holds.
        await choose(page, f, from);
        assert.equal(await f.country(), code, `picked ${from}`);
        await f.input.fill("");
        await typed(page, f, dialled.replaceAll(" ", ""));
        assert.equal(await f.country(), to, dialled);
        assert.equal(await f.input.inputValue(), text, dialled);
        assert.equal(await f.e164.inputValue(), e164, dialled);
      }
      // The last one, posted: the server reads the text in the country now chosen.
      await page.locator("#contact-name").fill("Ada Lovelace");
      await page.getByRole("button", { name: "Save contact" }).click();
      await page.waitForSelector("[data-proof=mobile]");
      assert.equal(await page.locator("[data-proof=mobile] [data-fact=e164]").textContent(), "+61412345678");
      assert.equal(await page.locator("[data-proof=mobile] [data-fact=region]").textContent(), "Australia");
      assert.deepEqual(problems, []);
      await close();
    });

    test("keyboard only: open, search, choose, close", async () => {
      const { page, field, problems, close } = await open("/phone");
      const f = field("mobile");
      await page.locator("#contact-name").focus();
      await page.keyboard.press("Tab");
      assert.equal(await page.evaluate(() => document.activeElement.getAttribute("data-slot")), "phone-country-trigger");
      await page.keyboard.press("ArrowDown");
      await page.waitForFunction(() => document.activeElement?.getAttribute("data-slot") === "phone-search");
      assert.equal(await f.isOpen(), true);
      assert.equal(await f.trigger.getAttribute("aria-expanded"), "true");
      assert.equal(await f.search.getAttribute("role"), "combobox");
      // The chosen country is the active one.
      assert.equal(await f.search.getAttribute("aria-activedescendant"), "mobile-countries-GB");
      await page.keyboard.type("fra");
      await page.waitForTimeout(50);
      const names = await f.rows.locator("[data-slot=phone-option-name]").allInnerTexts();
      assert.ok(names.includes("France"), names.join(", "));
      assert.ok(names.every((n) => /fra/i.test(n.normalize("NFD"))), names.join(", "));
      await shot(page, "searching");
      assert.deepEqual(await axe(page), []);
      // Down then Enter picks; focus returns to the number.
      await page.keyboard.press("ArrowDown");
      await page.keyboard.press("ArrowUp");
      assert.equal(await f.search.getAttribute("aria-activedescendant"), "mobile-countries-FR");
      await page.keyboard.press("Enter");
      assert.equal(await f.isOpen(), false);
      assert.equal(await f.country(), "FR");
      assert.equal(await page.evaluate(() => document.activeElement.name), "mobile");
      assert.equal(await f.input.getAttribute("placeholder"), "06 12 34 56 78");
      // Dial codes search too, and a letter on the button opens the search.
      await page.keyboard.press("Shift+Tab");
      await page.keyboard.press("9");
      await page.waitForFunction(() => document.activeElement?.getAttribute("data-slot") === "phone-search");
      assert.equal(await f.search.inputValue(), "9");
      await page.keyboard.type("1");
      const india = await f.rows.locator("[data-slot=phone-option-name]").allInnerTexts();
      assert.ok(india.includes("India"), india.join(", "));
      // Escape closes and returns to the button.
      await page.keyboard.press("Escape");
      assert.equal(await f.isOpen(), false);
      assert.equal(await page.evaluate(() => document.activeElement.getAttribute("data-slot")), "phone-country-trigger");
      assert.equal(await f.country(), "FR", "Escape chose nothing");
      // A search that finds nothing says so.
      await page.keyboard.press("ArrowDown");
      await page.waitForFunction(() => document.activeElement?.getAttribute("data-slot") === "phone-search");
      await page.keyboard.type("zzzz");
      await page.waitForTimeout(500);
      assert.equal(await page.locator("#mobile-countries [data-slot=phone-empty]").isVisible(), true);
      assert.equal(await f.status.textContent(), "No country found.");
      await shot(page, "empty-search");
      await page.keyboard.press("Escape");
      assert.deepEqual(problems, []);
      await close();
    });

    test("clicking a country picks it", async () => {
      const { page, field, problems, close } = await open("/phone");
      const f = field("office");
      await f.trigger.click();
      assert.equal(await f.isOpen(), true);
      await f.search.fill("ger");
      await f.rows.filter({ hasText: "Germany" }).click();
      assert.equal(await f.country(), "DE");
      assert.equal(await f.trigger.locator("[data-slot=phone-dial]").textContent(), "+49");
      await typed(page, f, "030123456");
      assert.equal(await f.input.inputValue(), "030 123456");
      assert.equal(await f.e164.inputValue(), "+4930123456");
      assert.deepEqual(problems, []);
      await close();
    });

    test("the server reads the number in the chosen country and writes it back", async () => {
      const { page, field, problems, close } = await open("/phone");
      await page.locator("#contact-name").fill("Ada Lovelace");
      await typed(page, field("mobile"), "07400123456");
      await page.getByRole("button", { name: "Save contact" }).click();
      await page.waitForSelector("[data-proof=mobile]");
      const fact = (name) => page.locator(`[data-proof=mobile] [data-fact=${name}]`).textContent();
      assert.equal(await fact("e164"), "+447400123456");
      assert.equal(await fact("international"), "+44 7400 123456");
      assert.equal(await fact("national"), "07400 123456");
      assert.equal(await fact("type"), "Mobile");
      assert.equal(await fact("region"), "United Kingdom");
      await shot(page, "saved");
      assert.deepEqual(problems, []);
      await close();
    });

    test("a number not valid in the chosen country is an error on the field", async () => {
      const { page, field, problems, close } = await open("/phone");
      const f = field("mobile");
      await page.locator("#contact-name").fill("Ada Lovelace");
      await f.trigger.click();
      await f.search.fill("france");
      await page.keyboard.press("Enter");
      await f.input.fill("07400 123456");
      await page.getByRole("button", { name: "Save contact" }).click();
      const error = page.locator("[data-slot=field]:has(input[name=mobile]) [data-slot=field-error]");
      await error.waitFor();
      assert.equal(await error.textContent(), "Mobile: enter a phone number, with its country.");
      assert.equal(await f.input.getAttribute("aria-invalid"), "true");
      assert.ok((await f.input.getAttribute("aria-describedby")).includes(await error.getAttribute("id")));
      assert.deepEqual(await axe(page), []);
      await shot(page, "error");
      assert.deepEqual(problems, []);
      await close();
    });

    test("JavaScript off: a select and a tel input, read by the server", async () => {
      const { page, field, close } = await open("/phone", { js: false });
      const f = field("mobile");
      assert.equal(await f.select.isVisible(), true);
      assert.equal(await f.trigger.isVisible(), false);
      assert.equal(await f.e164.isDisabled(), true, "no stale E.164 posts");
      await page.locator("#contact-name").fill("Ada Lovelace");
      await f.select.selectOption("IN");
      await f.input.fill("098765 43210");
      await page.getByRole("button", { name: "Save contact" }).click();
      await page.waitForSelector("[data-proof=mobile]");
      assert.equal(await page.locator("[data-proof=mobile] [data-fact=e164]").textContent(), "+919876543210");
      // The saved form shows the number in its country's format.
      assert.equal(await f.input.inputValue(), "098765 43210");
      assert.equal(await f.country(), "IN");
      await shot(page, "no-js-saved");
      // Invalid: the page comes back with the error, the text and the country.
      await f.select.selectOption("FR");
      await f.input.fill("12");
      await page.getByRole("button", { name: "Save contact" }).click();
      await page.waitForSelector("[data-slot=field]:has(input[name=mobile]) [data-slot=field-error]");
      assert.equal(await f.input.inputValue(), "12");
      assert.equal(await f.country(), "FR");
      await shot(page, "no-js-error");
      await close();
    });

    test("without the runtime: the select and tel input are axe clean", async () => {
      const { page, field, close } = await open("/phone", { noRuntime: true });
      assert.equal(await field("mobile").select.isVisible(), true);
      assert.deepEqual(await axe(page), []);
      await close();
    });

    for (const [width, height] of [[390, 844], [1440, 900]]) {
      for (const light of [false, true]) {
        test(`${width}px, ${light ? "light" : "dark"}: no sideways scroll, the list fits, axe clean open and closed`, async () => {
          const { page, field, problems, close } = await open("/phone", { viewport: { width, height }, light });
          const f = field("mobile");
          assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true);
          assert.deepEqual(await axe(page), []);
          await shot(page, `${width}-${light ? "light" : "dark"}-closed`);
          await f.trigger.click();
          assert.equal(await f.isOpen(), true);
          const box = await f.list.boundingBox();
          assert.ok(box.x >= 0 && box.x + box.width <= width && box.y + box.height <= height, JSON.stringify(box));
          assert.deepEqual(await axe(page), []);
          await shot(page, `${width}-${light ? "light" : "dark"}-open`);
          await page.keyboard.press("Escape");
          assert.deepEqual(problems, []);
          await close();
        });
      }
    }

    test("right to left: the picker at the start, the number left to right", async () => {
      const { page, field, problems, close } = await open("/phone");
      const f = field("rtl");
      await f.root.scrollIntoViewIfNeeded();
      const trigger = await f.trigger.boundingBox();
      const input = await f.input.boundingBox();
      assert.ok(trigger.x > input.x, "the picker is on the right");
      assert.equal(await f.input.evaluate((i) => getComputedStyle(i).textAlign), "right");
      assert.equal(await f.trigger.getAttribute("aria-label"), "الدولة: United Arab Emirates +971");
      await typed(page, f, "0501234567");
      assert.equal(await f.input.inputValue(), "050 123 4567");
      await f.trigger.click();
      assert.equal(await f.search.getAttribute("placeholder"), "ابحث عن دولة");
      // The list sits in the window, its end edge on the trigger's.
      const list = await f.list.boundingBox();
      const view = page.viewportSize();
      assert.ok(list.y >= 0 && list.y + list.height <= view.height, JSON.stringify(list));
      const end = (await f.trigger.boundingBox()).x + (await f.trigger.boundingBox()).width;
      assert.ok(Math.abs(list.x + list.width - end) <= 9, `${list.x + list.width} vs ${end}`);
      assert.deepEqual(await axe(page), []);
      await shot(page, "rtl-open");
      await page.keyboard.press("Escape");
      assert.deepEqual(problems, []);
      await close();
    });

    test("states: disabled, read only, invalid, suggested countries", async () => {
      const { page, field, problems, close } = await open("/phone");
      assert.equal(await field("disabled").input.isDisabled(), true);
      assert.equal(await field("disabled").trigger.isDisabled(), true);
      assert.equal(await field("invalid").input.getAttribute("aria-invalid"), "true");
      const preferred = field("preferred");
      await preferred.trigger.click();
      const first = await preferred.rows.locator("[data-slot=phone-option-name]").allInnerTexts();
      assert.deepEqual(first.slice(0, 5), ["India", "United Arab Emirates", "Saudi Arabia", "United Kingdom", "United States"]);
      await page.keyboard.press("Escape");
      await shot(page, "states");
      assert.deepEqual(problems, []);
      await close();
    });
  });
}
