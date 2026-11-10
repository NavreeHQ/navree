// Select, Combobox, MultiSelect and the command palette in real browsers, against
// the proof app. Start it first:
//   cargo run --release -p ui-proof -- serve          # http://127.0.0.1:8090
// UI_PROOF_URL points elsewhere; SELECT_BROWSERS="chromium firefox webkit" picks browsers.
//
// Every condition in the module's list is a test here: the APG keys of each pattern,
// the value that posts, JavaScript off, slow, failed and out-of-order replies, a
// redirect, 10,000 options, a dialog, a scrolling card, 390px, right to left, light
// and dark, and axe in every state with no report from the content security policy.

import assert from "node:assert/strict";
import { readFile, mkdir } from "node:fs/promises";
import { createRequire } from "node:module";
import { join } from "node:path";
import { after, before, describe, test } from "node:test";
import * as playwright from "playwright";
import { launch } from "./support/launch.mjs";
import { LIGHT } from "./support/light.mjs";

const BASE = process.env.UI_PROOF_URL || "http://127.0.0.1:8090";
const SHOTS = process.env.SELECT_SHOTS || "";
const require = createRequire(import.meta.url);
const axeSource = await readFile(require.resolve("axe-core/axe.min.js"), "utf8");

/** Runs axe. Violations come back as short lines. */
async function axe(page) {
  await page.evaluate(axeSource);
  return page.evaluate(async () => {
    const out = await window.axe.run(document, { runOnly: ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa", "best-practice"] });
    return out.violations.map((v) => `${v.id}: ${v.nodes.slice(0, 3).map((n) => n.target.join(" ")).join(" | ")}`);
  });
}

const browsers = (process.env.SELECT_BROWSERS || "chromium firefox").split(" ");

for (const browserName of browsers) {
  describe(browserName, () => {
    let browser;
    before(async () => {
      browser = await launch(playwright[browserName]);
    });
    after(() => browser?.close());

    /** Opens a page and watches for errors and policy reports. */
    async function open(path, options = {}) {
      const context = await browser.newContext({
        viewport: options.viewport || { width: 1440, height: 900 },
        javaScriptEnabled: options.js !== false,
        bypassCSP: !!options.light,
        hasTouch: !!options.touch,
        isMobile: !!options.touch,
      });
      const page = await context.newPage();
      const problems = [];
      const dialogs = [];
      page.on("console", (message) => {
        if (browserName === "webkit" && /Refused to apply a stylesheet/.test(message.text())) return;
        if (message.type() === "error" && !/Failed to load resource/.test(message.text())) problems.push(message.text());
      });
      page.on("pageerror", (error) => problems.push(String(error)));
      page.on("dialog", (dialog) => {
        dialogs.push(dialog.message());
        dialog.dismiss();
      });
      await page.goto(BASE + path);
      if (options.light) await page.addStyleTag({ content: LIGHT });
      const picker = (name) => ({
        root: page.locator(`#${name}`),
        control: page.locator(`#${name} [data-slot=select-trigger], #${name} [data-slot=combobox-input], #${name} [data-slot=multiselect-input]`),
        popup: page.locator(`#${name}-popup`),
        list: page.locator(`#${name}-list`),
        rows: page.locator(`#${name}-list [role=option]:not([hidden])`),
        carrier: page.locator(`#${name} [data-slot=select-native]`),
        chips: page.locator(`#${name} [data-slot=chip]`),
        status: page.locator(`#${name} [data-slot=select-status]`),
        note: page.locator(`#${name} [data-slot=select-note]`),
        value: () => page.locator(`#${name} [data-slot=select-native]`).evaluate((s) => (s.multiple ? [...s.selectedOptions].map((o) => o.value) : s.value)),
        isOpen: () => page.locator(`#${name}`).evaluate((r) => r.hasAttribute("data-open")),
        texts: () => page.locator(`#${name}-list [role=option]:not([hidden])`).allInnerTexts(),
        activeText: () => page.locator(`#${name}-list`).evaluate((l) => l.querySelector("[data-active]")?.textContent || ""),
      });
      return { page, context, problems, dialogs, picker, close: () => context.close() };
    }

    async function shot(page, name) {
      if (!SHOTS) return;
      await mkdir(SHOTS, { recursive: true });
      await page.waitForTimeout(250);
      await page.screenshot({ path: join(SHOTS, `${browserName}-${name}.png`) });
    }

    // ------------------------------------------------------------------ Select

    test("Select: roles, open by click, pick by click, the value and change", async () => {
      const { page, picker, problems, close } = await open("/select");
      const s = picker("select-basic");
      const trigger = page.locator("#f-select-basic");
      assert.equal(await trigger.getAttribute("role"), "combobox");
      assert.equal(await page.getByRole("combobox", { name: "Stage", exact: true }).count(), 1, "the label names the button");
      assert.ok((await page.getByRole("combobox", { name: "Country", exact: true }).evaluateAll((els) => els.map((e) => e.id))).includes("f-combo-basic"), "and the input of a combobox");
      assert.equal(await trigger.getAttribute("aria-haspopup"), "listbox");
      assert.equal(await trigger.getAttribute("aria-expanded"), "false");
      assert.equal(await s.list.getAttribute("role"), "listbox");
      assert.equal(await s.value(), "");
      await page.evaluate(() => {
        window.__changes = [];
        document.addEventListener("change", (e) => window.__changes.push(e.target.name + "=" + e.target.value));
      });
      await trigger.click();
      assert.equal(await trigger.getAttribute("aria-expanded"), "true");
      assert.ok(await s.popup.isVisible());
      assert.equal((await s.texts()).length, 5);
      await s.rows.nth(3).click();
      assert.equal(await s.value(), "won");
      assert.equal(await trigger.innerText(), "Won");
      assert.equal(await trigger.getAttribute("aria-expanded"), "false");
      assert.equal(await s.isOpen(), false);
      assert.deepEqual(await page.evaluate(() => window.__changes), ["stage=won"]);
      // Focus came back to the trigger.
      assert.equal(await page.evaluate(() => document.activeElement.id), "f-select-basic");
      assert.deepEqual(problems, []);
      await close();
    });

    test("Select: the keys of the APG select-only combobox", async () => {
      const { page, picker, close } = await open("/select");
      const s = picker("select-basic");
      const trigger = page.locator("#f-select-basic");
      await trigger.focus();
      // Closed: Down opens with the first option active.
      await page.keyboard.press("ArrowDown");
      assert.equal(await s.isOpen(), true);
      assert.equal(await s.activeText(), "Lead");
      assert.equal(await trigger.getAttribute("aria-activedescendant"), await s.list.locator("[data-active]").getAttribute("id"));
      await page.keyboard.press("ArrowDown");
      await page.keyboard.press("ArrowDown");
      assert.equal(await s.activeText(), "Proposal");
      await page.keyboard.press("End");
      assert.equal(await s.activeText(), "Lost");
      await page.keyboard.press("ArrowDown");
      assert.equal(await s.activeText(), "Lost", "stops at the last");
      await page.keyboard.press("Home");
      assert.equal(await s.activeText(), "Lead");
      await page.keyboard.press("PageDown");
      assert.equal(await s.activeText(), "Lost");
      await page.keyboard.press("PageUp");
      assert.equal(await s.activeText(), "Lead");
      // Esc closes with no change.
      await page.keyboard.press("ArrowDown");
      await page.keyboard.press("Escape");
      assert.equal(await s.isOpen(), false);
      assert.equal(await s.value(), "");
      assert.equal(await page.evaluate(() => document.activeElement.id), "f-select-basic");
      // Enter on the closed trigger opens; Enter picks.
      await page.keyboard.press("Enter");
      assert.equal(await s.isOpen(), true);
      await page.keyboard.press("ArrowDown");
      await page.keyboard.press("Enter");
      assert.equal(await s.value(), "qualified");
      // Space opens with the chosen one active; Alt+Up picks and closes.
      await page.keyboard.press(" ");
      assert.equal(await s.activeText(), "Qualified");
      await page.keyboard.press("ArrowDown");
      await page.keyboard.press("Alt+ArrowUp");
      assert.equal(await s.value(), "proposal");
      assert.equal(await s.isOpen(), false);
      // Tab picks the active one and moves on.
      await page.keyboard.press("ArrowDown");
      await page.keyboard.press("ArrowDown");
      await page.keyboard.press("Tab");
      assert.equal(await s.value(), "won");
      assert.equal(await s.isOpen(), false);
      assert.notEqual(await page.evaluate(() => document.activeElement.id), "f-select-basic");
      await close();
    });

    test("Select: typeahead jumps and cycles", async () => {
      const { page, picker, close } = await open("/select");
      const s = picker("select-groups");
      await page.locator("#f-select-groups").focus();
      await page.keyboard.press("t");
      assert.equal(await s.isOpen(), true);
      assert.equal(await s.activeText(), "Tohana");
      await page.keyboard.press("Escape");
      await page.keyboard.press("p");
      assert.equal(await s.activeText(), "Patiala");
      await page.keyboard.press("Escape");
      // Delhi is disabled: typeahead skips it.
      await page.keyboard.press("d");
      assert.notEqual(await s.activeText(), "Delhi");
      await close();
    });

    test("Select: groups are named, a disabled option can't be reached or picked", async () => {
      const { page, picker, close } = await open("/select");
      const s = picker("select-groups");
      await page.locator("#f-select-groups").click();
      assert.deepEqual(await page.locator("#select-groups-list [role=group]").evaluateAll((g) => g.map((x) => x.getAttribute("aria-label"))), ["Punjab", "Haryana and Delhi"]);
      await s.list.locator("[role=option]", { hasText: "Delhi" }).click({ force: true });
      assert.equal(await s.value(), "", "a disabled option is not picked by a click");
      await page.keyboard.press("End");
      assert.equal(await s.activeText(), "Sonipat");
      await page.keyboard.press("ArrowUp");
      assert.equal(await s.activeText(), "Tohana", "Up from Sonipat skips the disabled Delhi");
      await close();
    });

    test("Select: clearable clears with the button and with Delete; the clear button is hidden when empty", async () => {
      const { page, picker, close } = await open("/select");
      const s = picker("select-clearable");
      const clear = page.locator("#select-clearable [data-slot=select-clear]");
      assert.equal(await clear.isVisible(), true);
      await clear.click();
      assert.equal(await s.value(), "");
      assert.equal(await page.locator("#f-select-clearable").innerText(), "None");
      assert.equal(await clear.isVisible(), false);
      await page.locator("#f-select-clearable").focus();
      await page.keyboard.press("ArrowDown");
      await page.keyboard.press("Enter");
      assert.equal(await s.value(), "lead");
      await page.keyboard.press("Delete");
      assert.equal(await s.value(), "");
      await close();
    });

    test("Select: 0 options says so, 1 option works, and a read-only or disabled one never opens", async () => {
      const { page, picker, close } = await open("/select");
      const none = picker("select-empty");
      await page.locator("#f-select-empty").click();
      assert.equal(await none.isOpen(), true);
      assert.match(await none.note.innerText(), /No options/);
      assert.equal(await none.rows.count(), 0);
      await page.keyboard.press("Escape");
      const one = picker("select-one");
      await page.locator("#f-select-one").click();
      assert.equal(await one.rows.count(), 1);
      await page.keyboard.press("ArrowDown");
      await page.keyboard.press("Enter");
      assert.equal(await one.value(), "only");
      const ro = picker("select-readonly");
      await page.locator("#f-select-readonly").click();
      assert.equal(await ro.isOpen(), false);
      assert.equal(await ro.value(), "lead", "a read-only select still posts its value");
      assert.equal(await page.locator("#f-select-disabled").isDisabled(), true);
      assert.equal(await picker("select-disabled").carrier.isDisabled(), true, "a disabled select posts nothing");
      await close();
    });

    test("Select: very long text, Unicode, RTL and hostile strings keep the layout and run nothing", async () => {
      const { page, picker, dialogs, close } = await open("/select");
      const s = picker("select-long");
      const trigger = page.locator("#f-select-long");
      const box = await trigger.boundingBox();
      assert.ok(box.width < 700);
      await trigger.click();
      const popup = await s.popup.boundingBox();
      assert.ok(popup.x >= 0 && popup.x + popup.width <= 1440, `inside the viewport: ${JSON.stringify(popup)}`);
      const overflow = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
      assert.equal(overflow, 0, "no sideways scroll");
      assert.equal(await page.locator("#select-long-list img").count(), 0, "markup in data stays text");
      assert.ok((await s.texts()).some((t) => t.includes("<img src=x onerror=alert(1)>")));
      await s.list.locator("[role=option]", { hasText: "<img" }).click();
      assert.equal(await trigger.innerText(), "<img src=x onerror=alert(1)> \"quoted\" & 'single'");
      assert.deepEqual(dialogs, []);
      await close();
    });

    test("Select: a required select in a form says so, takes focus, posts, and Reset restores", async () => {
      const { page, picker, close } = await open("/select/form");
      const s = picker("form-select");
      await page.locator("#pick-form button[type=submit]").click();
      assert.equal(page.url().endsWith("/select/form"), true, "the form did not submit");
      assert.equal(await page.evaluate(() => document.activeElement.id), "f-form-stage");
      assert.equal(await page.locator("#f-form-stage").getAttribute("aria-invalid"), "true");
      assert.match(await page.locator("#f-form-stage").locator("xpath=ancestor::*[@data-slot='field'][1]").innerText(), /\S/);
      assert.ok(await page.locator("[data-picker-error]").count() >= 1);
      await page.locator("#f-form-stage").click();
      await page.keyboard.press("ArrowDown");
      await page.keyboard.press("Enter");
      assert.equal(await page.locator("#f-form-stage").getAttribute("aria-invalid"), null, "choosing clears the mark");
      assert.equal(await page.locator("[data-slot=field]", { has: page.locator("#f-form-stage") }).locator("[data-picker-error]").count(), 0);
      assert.equal(await page.locator("[data-slot=field]", { has: page.locator("#f-form-owner") }).locator("[data-picker-error]").count(), 1, "the other empty one still says so");
      await page.locator("#f-form-owner").fill("gre");
      await picker("form-combo").rows.first().click();
      await page.locator("#f-form-person").fill("gauri");
      await page.waitForTimeout(500);
      await picker("form-person").rows.first().click();
      await page.locator("#pick-form button[type=submit]").click();
      await page.waitForURL("**/select/echo");
      const sent = await page.locator("[data-echo]").innerText();
      assert.match(sent, /stage = qualified/);
      assert.match(sent, /owner = gr/);
      assert.match(sent, /person = \d+/);
      assert.match(sent, /note = kept/);
      await page.goBack();
      await close();
      const again = await open("/select/form");
      await again.page.locator("#f-form-stage").click();
      await again.page.keyboard.press("ArrowDown");
      await again.page.keyboard.press("Enter");
      assert.equal(await again.picker("form-select").value(), "qualified");
      await again.page.locator("#pick-form button[type=reset]").click();
      await again.page.waitForTimeout(100);
      assert.equal(await again.picker("form-select").value(), "");
      assert.equal(await again.page.locator("#f-form-stage").innerText(), "Choose a stage");
      await again.close();
    });

    test("Select with JavaScript off: the native select shows and posts", async () => {
      const { page, close } = await open("/select/form", { js: false });
      const native = page.locator("#form-select [data-slot=select-native]");
      assert.equal(await native.isVisible(), true);
      assert.equal(await page.locator("#f-form-stage").isVisible(), false, "the styled button is hidden");
      await native.selectOption("won");
      await page.locator("#form-multi [data-slot=select-native]").selectOption(["fr", "de"]);
      await page.locator("#form-combo [data-slot=select-native]").selectOption("de");
      await page.locator("#pick-form button[type=submit]").click();
      await page.waitForURL("**/select/echo");
      const sent = await page.locator("[data-echo]").innerText();
      assert.match(sent, /stage = won/);
      assert.match(sent, /owner = de/);
      assert.match(sent, /countries = fr/);
      assert.match(sent, /countries = de/);
      await close();
    });

    // ---------------------------------------------------------------- Combobox

    test("Combobox: typing filters, the first match is active, Enter picks, accents and keywords and several words match", async () => {
      const { page, picker, problems, close } = await open("/select");
      const c = picker("combo-basic");
      const input = page.locator("#f-combo-basic");
      assert.equal(await input.getAttribute("role"), "combobox");
      assert.equal(await input.getAttribute("aria-autocomplete"), "list");
      await input.click();
      assert.equal(await c.isOpen(), true, "a click opens it");
      assert.equal((await c.texts()).length, 33);
      await input.fill("");
      await page.keyboard.type("an");
      assert.deepEqual(await c.texts(), ["Canada", "Finland", "France", "Germany", "Ireland", "Japan", "Netherlands", "New Zealand", "Poland", "Switzerland"]);
      assert.equal(await c.activeText(), "Canada", "the first match is active");
      await page.keyboard.press("ArrowDown");
      await page.keyboard.press("Enter");
      assert.equal(await c.value(), "fi");
      assert.equal(await input.inputValue(), "Finland");
      assert.equal(await c.isOpen(), false);
      await input.fill("");
      await page.keyboard.type("turkiye");
      assert.deepEqual(await c.texts(), ["Türkiye"], "accents don't matter");
      await input.fill("");
      await page.keyboard.type("code jp");
      assert.deepEqual(await c.texts(), ["Japan"], "keywords find it");
      await input.fill("");
      await page.keyboard.type("zea new");
      assert.deepEqual(await c.texts(), ["New Zealand"], "words in any order");
      await input.fill("");
      await page.keyboard.type("qqqq");
      assert.equal(await c.rows.count(), 0);
      assert.match(await c.note.innerText(), /No results/);
      assert.equal(await c.activeText(), "");
      assert.deepEqual(problems, []);
      await close();
    });

    test("Combobox: Esc closes and puts the chosen label back, blur does too, Alt+Down opens, Tab commits nothing", async () => {
      const { page, picker, close } = await open("/select");
      const c = picker("combo-selected");
      const input = page.locator("#f-combo-selected");
      assert.equal(await input.inputValue(), "India");
      await input.focus();
      await page.keyboard.press("Alt+ArrowDown");
      assert.equal(await c.isOpen(), true);
      assert.equal(await c.activeText(), "", "Alt+Down opens without choosing a row");
      await page.keyboard.press("Alt+ArrowUp");
      assert.equal(await c.isOpen(), false);
      await page.keyboard.press("Control+a");
      await page.keyboard.type("fra");
      assert.equal(await c.isOpen(), true);
      await page.keyboard.press("Escape");
      assert.equal(await c.isOpen(), false);
      assert.equal(await input.inputValue(), "India", "Esc put the chosen label back");
      assert.equal(await c.value(), "in");
      await page.keyboard.press("Control+a");
      await page.keyboard.type("fra");
      await page.keyboard.press("Tab");
      assert.equal(await input.inputValue(), "India", "Tab commits nothing and restores the label");
      assert.equal(await c.value(), "in");
      await input.focus();
      await page.keyboard.press("Control+a");
      await page.keyboard.type("jap");
      await page.locator("h1").click();
      assert.equal(await input.inputValue(), "India", "blur restores the label");
      assert.equal(await c.isOpen(), false);
      await close();
    });

    test("Combobox: clearable clears with the button and with Esc when the list is closed", async () => {
      const { page, picker, close } = await open("/select");
      const c = picker("combo-selected");
      await page.locator("#combo-selected [data-slot=combobox-clear]").click();
      assert.equal(await c.value(), "");
      assert.equal(await page.locator("#f-combo-selected").inputValue(), "");
      await page.locator("#f-combo-selected").fill("ger");
      await page.keyboard.press("Enter");
      assert.equal(await c.value(), "de");
      await page.keyboard.press("Escape");
      assert.equal(await c.value(), "", "Esc on a closed list clears");
      assert.equal(await page.locator("#f-combo-selected").inputValue(), "");
      await close();
    });

    test("Combobox: the toggle opens, a click on a row picks, a press on the list keeps focus in the input", async () => {
      const { page, picker, close } = await open("/select");
      const c = picker("combo-basic");
      await page.locator("#combo-basic [data-slot=combobox-toggle]").click();
      assert.equal(await c.isOpen(), true);
      assert.equal(await page.evaluate(() => document.activeElement.id), "f-combo-basic");
      await c.list.dispatchEvent("mousedown");
      assert.equal(await page.evaluate(() => document.activeElement.id), "f-combo-basic");
      await c.rows.nth(2).click();
      assert.equal(await c.value(), "at");
      await close();
    });

    test("Combobox: 0 options says no options", async () => {
      const { page, picker, close } = await open("/select");
      const c = picker("combo-empty");
      await page.locator("#f-combo-empty").click();
      assert.equal(await c.isOpen(), true);
      assert.match(await c.note.innerText(), /No options/);
      await close();
    });

    // ----------------------------------------------------- Combobox, the server

    test("Combobox on the server: the first options on open, debounce, the newest answer wins, Show more", async () => {
      const { page, picker, close } = await open("/select");
      const c = picker("combo-search");
      const requests = [];
      page.on("request", (r) => { if (r.url().includes("/select/people")) requests.push(new URL(r.url()).searchParams.get("q")); });
      assert.equal(await page.locator("#f-combo-search").inputValue(), "Gauri Hundal 3");
      assert.equal(await c.value(), "3");
      await page.locator("#f-combo-search").click();
      await page.waitForFunction(() => document.querySelectorAll("#combo-search-list [role=option]:not([data-action])").length === 20);
      assert.equal(await page.locator("#combo-search-list [data-action=more]").count(), 1, "Show more is offered");
      await page.locator("#f-combo-search").fill("");
      requests.length = 0;
      await page.keyboard.type("anu", { delay: 40 });
      await page.waitForFunction(() => document.querySelector("#combo-search-list [role=option]")?.textContent.startsWith("Anu"));
      await page.waitForTimeout(400);
      assert.deepEqual(requests, ["anu"], "typing fast sent one request");
      assert.match((await c.texts())[0], /^Anu/);
      // Show more adds the next page.
      const before = await page.locator("#combo-search-list [role=option]:not([data-action])").count();
      await page.locator("#combo-search-list [data-action=more]").click();
      await page.waitForFunction((n) => document.querySelectorAll("#combo-search-list [role=option]:not([data-action])").length > n, before);
      // Picking a found option puts it in the carrier.
      await c.rows.first().click();
      assert.match(await page.locator("#f-combo-search").inputValue(), /^Anu /);
      assert.match(String(await c.value()), /^\d+$/);
      await close();
    });

    test("Combobox on the server: an older answer that arrives late is ignored", async () => {
      const { page, picker, close } = await open("/select");
      const c = picker("combo-search");
      await page.route("**/select/people?*", async (route) => {
        const q = new URL(route.request().url()).searchParams.get("q");
        if (q === "ga") await new Promise((r) => setTimeout(r, 1200));
        await route.continue();
      });
      await page.locator("#f-combo-search").click();
      await page.locator("#f-combo-search").fill("");
      await page.keyboard.type("ga");
      await page.waitForTimeout(400);
      await page.keyboard.type("uri lak");
      await page.waitForFunction(() => document.querySelector("#combo-search-list [role=option]")?.textContent.startsWith("Gauri Lak"));
      const fast = await c.texts();
      await page.waitForTimeout(1500);
      assert.deepEqual(await c.texts(), fast, "the slow reply for 'ga' did not replace the newer list");
      await close();
    });

    test("Combobox on the server: a slow reply shows busy within the delay and keeps the old rows, and typing stays live", async () => {
      const { page, picker, close } = await open("/select");
      const c = picker("combo-search");
      await page.locator("#f-combo-search").click();
      await page.waitForFunction(() => document.querySelectorAll("#combo-search-list [role=option]").length > 5);
      await page.route("**/select/people?*", async (route) => {
        await new Promise((r) => setTimeout(r, 900));
        await route.continue();
      });
      await page.locator("#f-combo-search").fill("");
      await page.keyboard.type("arun");
      await page.waitForFunction(() => document.querySelector("#combo-search-list")?.getAttribute("aria-busy") === "true");
      assert.match(await c.note.innerText(), /Searching/);
      assert.ok((await c.rows.count()) > 0, "the old rows stay");
      await page.keyboard.type(" t");
      assert.equal(await page.locator("#f-combo-search").inputValue(), "arun t", "typing was not blocked");
      await page.waitForFunction(() => document.querySelector("#combo-search-list")?.getAttribute("aria-busy") !== "true", null, { timeout: 5000 });
      await close();
    });

    test("Combobox on the server: a failed reply says so, keeps the text and the choice, and Enter tries again", async () => {
      const { page, picker, close } = await open("/select");
      const c = picker("combo-search");
      let fail = true;
      await page.route("**/select/people?*", async (route) => {
        if (fail) await route.fulfill({ status: 500, body: "nope" });
        else await route.continue();
      });
      await page.locator("#f-combo-search").click();
      await page.locator("#f-combo-search").fill("gauri");
      await page.waitForFunction(() => /Couldn't load/.test(document.querySelector("#combo-search [data-slot=select-note]")?.textContent || ""));
      assert.equal(await c.value(), "3", "the choice is kept");
      assert.equal(await page.locator("#f-combo-search").inputValue(), "gauri");
      assert.equal(await page.locator("#combo-search-list [data-action=retry]").count(), 1);
      fail = false;
      await page.keyboard.press("ArrowDown");
      await page.keyboard.press("Enter");
      await page.waitForFunction(() => document.querySelector("#combo-search-list [role=option]:not([data-action])")?.textContent.startsWith("Gauri"));
      await close();
    });

    test("Combobox on the server: a lost permission is said, an ended session goes to the sign-in page", async () => {
      const { page, picker, close } = await open("/select");
      const c = picker("combo-search");
      await page.route("**/select/people?q=nope*", (route) => route.fulfill({ status: 403, body: "no" }));
      await page.locator("#f-combo-search").click();
      await page.locator("#f-combo-search").fill("nope");
      await page.waitForFunction(() => /no longer have access/.test(document.querySelector("#combo-search [data-slot=select-note]")?.textContent || ""));
      assert.equal(await c.rows.count(), 0, "no stale rows");
      await page.locator("#f-combo-search").fill("session-ended");
      await page.waitForURL("**/plain");
      assert.equal(await page.locator("#combo-search").count(), 0, "no list of the old page");
      await close();
    });

    test("Combobox on the server: markup in a reply never runs and never takes an id", async () => {
      const { page, picker, dialogs, close } = await open("/select");
      const c = picker("combo-search");
      await page.route("**/select/people?*", (route) => route.fulfill({
        status: 200,
        contentType: "text/html",
        body: `<div data-slot="option-page"><div role="option" data-value="x" id="f-combo-search">Evil <img src=x onerror="alert(1)"><script>alert(2)</script></div><form id="pick"><input name="owner"></form><div role="option" data-value="y" onclick="alert(3)">Second</div></div>`,
      }));
      await page.locator("#f-combo-search").click();
      await page.locator("#f-combo-search").fill("evil");
      await page.waitForFunction(() => document.querySelector("#combo-search-list [role=option]")?.textContent.startsWith("Evil"));
      assert.equal(await page.locator("#combo-search-list img, #combo-search-list script, #combo-search-list form, #combo-search-list input").count(), 0);
      assert.equal(await page.locator("#f-combo-search").evaluate((e) => e.tagName), "INPUT", "the id stayed with the input");
      await c.rows.nth(1).click();
      assert.deepEqual(dialogs, []);
      await close();
    });

    test("Combobox on the server: a flag in a reply keeps its art, and nothing else may pass as one", async () => {
      const { page, close } = await open("/select");
      // A flag as the toolkit draws one (its own ids, a clip by one of them, a part drawn again by
      // `use`), and markup that borrows a flag's names: an id outside a flag, a clip or a `use` that
      // reaches a file, and a transform that hides a url().
      const flag = '<span data-slot="icon" data-lead aria-hidden="true"><svg data-slot="flag" viewBox="0 0 640 480" focusable="false" aria-hidden="true"><defs><clipPath id="f1-by-by-a"><path d="M0 0h640v480H0z"/></clipPath></defs><g clip-path="url(#f1-by-by-a)" fill-rule="evenodd" transform="translate(10 0) scale(.5)"><path fill="#c8313e" d="M0 0h640v480H0z"/><use href="#f1-by-by-a" fill-opacity=".5"/></g></svg></span>';
      const borrowed = '<svg data-slot="icon" viewBox="0 0 24 24"><g id="f1-xx-xx-a" clip-path="url(/evil.svg#f1-xx-xx-a)" transform="url(#f1-xx-xx-a)"><use href="/evil.svg#f1-xx-xx-a"/></g></svg>';
      await page.route("**/select/people?*", (route) => route.fulfill({
        status: 200,
        contentType: "text/html",
        body: `<div data-slot="option-page"><div role="option" data-value="by">${flag}<span data-slot="option-body"><span data-slot="option-text">Belarus</span></span></div><div role="option" data-value="xx" id="f1-zz-zz-a">${borrowed}Borrowed</div></div>`,
      }));
      await page.locator("#f-combo-search").click();
      await page.locator("#f-combo-search").fill("b");
      await page.waitForFunction(() => document.querySelectorAll("#combo-search-list [role=option]").length === 2);
      const [kept, other] = await page.locator("#combo-search-list [role=option]").evaluateAll((rows) =>
        rows.map((row) => ({
          // A row's own id is the list's (for the active option), never the reply's.
          ids: [...row.querySelectorAll("[id]")].map((e) => e.id).concat(row.id && !row.id.startsWith("combo-search-list-") ? [row.id] : []),
          clips: [...row.querySelectorAll("[clip-path]")].map((e) => e.getAttribute("clip-path")),
          uses: [...row.querySelectorAll("use")].map((e) => e.getAttribute("href")),
          transforms: [...row.querySelectorAll("[transform]")].map((e) => e.getAttribute("transform")),
          flag: !!row.querySelector("[data-slot=icon][data-lead] > svg[data-slot=flag][viewBox][focusable]"),
          paints: [...row.querySelectorAll("[fill-rule], [fill-opacity]")].length,
        })),
      );
      assert.deepEqual(kept, { ids: ["f1-by-by-a"], clips: ["url(#f1-by-by-a)"], uses: ["#f1-by-by-a"], transforms: ["translate(10 0) scale(.5)"], flag: true, paints: 2 });
      assert.deepEqual(other, { ids: [], clips: [], uses: [null], transforms: [], flag: false, paints: 0 });
      await close();
    });

    test("Combobox on the server: nothing is asked before the minimum, and the hint says why", async () => {
      const { page, picker, close } = await open("/select");
      const c = picker("combo-min");
      const requests = [];
      page.on("request", (r) => { if (r.url().includes("/select/people")) requests.push(r.url()); });
      await page.locator("#f-combo-min").click();
      await page.keyboard.type("a");
      await page.waitForTimeout(500);
      assert.equal(requests.length, 0);
      assert.match(await c.note.innerText(), /at least 2/);
      await page.keyboard.type("n");
      await page.waitForFunction(() => document.querySelector("#combo-min-list [role=option]"));
      assert.equal(requests.length, 1);
      await close();
    });

    // ------------------------------------------------------------------ Create

    test("Combobox: create offers the typed text unless it matches, Enter on it makes it the value", async () => {
      const { page, picker, close } = await open("/select");
      const c = picker("combo-free");
      await page.locator("#f-combo-free").click();
      await page.keyboard.type("Prospect");
      assert.equal(await page.locator("#combo-free-list [data-action=create]").innerText(), "Create “Prospect”");
      await page.locator("#f-combo-free").fill("LEAD");
      assert.equal(await page.locator("#combo-free-list [data-action=create]").count(), 0, "an exact match in any case hides it");
      await page.locator("#f-combo-free").fill("  Prospect  ");
      await page.keyboard.press("ArrowDown");
      assert.equal(await c.activeText(), "Create “Prospect”");
      await page.keyboard.press("Enter");
      assert.equal(await c.value(), "Prospect");
      assert.equal(await page.locator("#f-combo-free").inputValue(), "Prospect");
      assert.equal(await c.isOpen(), false);
      await close();
    });

    test("Combobox: the server makes the record and answers the option, one post only, a refusal changes nothing", async () => {
      const { page, picker, close } = await open("/select");
      const c = picker("combo-create");
      const posts = [];
      page.on("request", (r) => { if (r.method() === "POST" && r.url().includes("/select/teams")) posts.push(r.postData()); });
      await page.locator("#f-combo-create").click();
      await page.keyboard.type("forbidden");
      await page.locator("#combo-create-list [data-action=create]").dblclick();
      await page.waitForTimeout(400);
      assert.equal(posts.length, 1, "a double click posts once");
      assert.equal(await c.value(), "");
      assert.match(await c.status.innerText(), /Couldn't create it/);
      await page.locator("#f-combo-create").fill("Green team");
      await page.locator("#combo-create-list [data-action=create]").click();
      await page.waitForFunction(() => document.querySelector("#combo-create [data-slot=select-native]").value === "team-green-team");
      assert.equal(await page.locator("#f-combo-create").inputValue(), "Green team");
      assert.equal(posts[1], "name=Green+team");
      await close();
    });

    test("Combobox: without permission the create row is there, off, and says why", async () => {
      const { page, picker, close } = await open("/select");
      const c = picker("combo-denied");
      await page.locator("#f-combo-denied").click();
      await page.keyboard.type("Yellow");
      const row = page.locator("#combo-denied-list [data-action=create]");
      assert.equal(await row.getAttribute("aria-disabled"), "true");
      assert.match(await row.innerText(), /Only admins can add teams/);
      await row.click({ force: true });
      assert.equal(await c.value(), "");
      await close();
    });

    // ------------------------------------------------------------- MultiSelect

    test("MultiSelect: chips for the chosen, a pick adds one, the list stays open, the carrier posts all", async () => {
      const { page, picker, problems, close } = await open("/select");
      const m = picker("multi-basic");
      assert.deepEqual(await m.chips.locator("[data-slot=chip-text]").allInnerTexts(), ["France", "Germany"]);
      assert.deepEqual(await m.value(), ["fr", "de"]);
      await page.locator("#f-multi-basic").click();
      assert.equal(await m.list.getAttribute("aria-multiselectable"), "true");
      await page.keyboard.type("jap");
      await page.keyboard.press("Enter");
      assert.deepEqual(await m.chips.locator("[data-slot=chip-text]").allInnerTexts(), ["France", "Germany", "Japan"]);
      assert.deepEqual(await m.value(), ["fr", "de", "jp"]);
      assert.equal(await page.locator("#f-multi-basic").inputValue(), "", "the text is cleared");
      assert.equal(await m.isOpen(), true, "the list stays open");
      assert.equal(await m.list.locator('[data-value="jp"]').getAttribute("aria-selected"), "true");
      await m.list.locator('[data-value="jp"]').click();
      assert.deepEqual(await m.value(), ["fr", "de"], "picking a chosen one again takes it off");
      assert.deepEqual(problems, []);
      await close();
    });

    test("MultiSelect: Backspace on an empty input goes to the last chip, then removes it; Delete and the buttons remove; arrows move", async () => {
      const { page, picker, close } = await open("/select");
      const m = picker("multi-basic");
      const input = page.locator("#f-multi-basic");
      await input.focus();
      await page.keyboard.press("Backspace");
      assert.equal(await page.evaluate(() => document.activeElement.getAttribute("data-slot")), "chip", "the first press only moves");
      assert.deepEqual(await m.value(), ["fr", "de"]);
      await page.keyboard.press("ArrowLeft");
      assert.equal(await page.evaluate(() => document.activeElement.querySelector("[data-slot=chip-text]").textContent), "France");
      await page.keyboard.press("ArrowRight");
      assert.equal(await page.evaluate(() => document.activeElement.querySelector("[data-slot=chip-text]").textContent), "Germany");
      await page.keyboard.press("Backspace");
      assert.deepEqual(await m.value(), ["fr"], "the second press removes it");
      assert.equal(await page.evaluate(() => document.activeElement.querySelector("[data-slot=chip-text]")?.textContent), "France", "focus moved to the neighbour");
      await page.keyboard.press("Delete");
      assert.deepEqual(await m.value(), []);
      assert.equal(await page.evaluate(() => document.activeElement.id), "f-multi-basic");
      await page.locator("#f-multi-basic").click();
      await page.keyboard.type("ita");
      await page.keyboard.press("Enter");
      await m.chips.first().locator("[data-slot=chip-remove]").click();
      assert.deepEqual(await m.value(), []);
      await close();
    });

    test("MultiSelect: a maximum switches the rest off and says so, and lets go when one is removed", async () => {
      const { page, picker, close } = await open("/select");
      const m = picker("multi-max");
      await page.locator("#f-multi-max").click();
      await page.keyboard.type("ja");
      await page.keyboard.press("Enter");
      assert.deepEqual(await m.value(), ["fr", "de", "jp"]);
      await page.locator("#f-multi-max").fill("");
      assert.equal(await m.list.locator('[data-value="it"]').getAttribute("aria-disabled"), "true");
      await m.list.locator('[data-value="it"]').click({ force: true });
      assert.deepEqual(await m.value(), ["fr", "de", "jp"]);
      await page.waitForFunction(() => /at most 3/.test(document.querySelector("#multi-max [data-slot=select-status]").textContent));
      await m.chips.first().locator("[data-slot=chip-remove]").click();
      assert.equal(await m.list.locator('[data-value="it"]').getAttribute("aria-disabled"), null);
      await close();
    });

    test("MultiSelect: more than the summary count shows one 'N selected' chip", async () => {
      const { page, picker, close } = await open("/select");
      const m = picker("multi-summary");
      assert.equal(await m.root.getAttribute("data-summary"), "");
      assert.equal(await page.locator("#multi-summary [data-slot=chip-summary]").innerText(), "3 selected");
      assert.equal(await page.locator("#multi-summary [data-slot=chip]:visible").count(), 0);
      await close();
    });

    test("MultiSelect: select all takes the matches of the text and deselect all lets them go", async () => {
      const { page, picker, close } = await open("/select");
      const m = picker("multi-all");
      await page.locator("#f-multi-all").click();
      assert.equal(await page.locator("#multi-all-list [data-action=all]").innerText(), "Select all");
      await page.locator("#f-multi-all").fill("o");
      await page.locator("#multi-all-list [data-action=all]").click();
      assert.deepEqual((await m.value()).sort(), ["lost", "proposal", "won"]);
      assert.equal(await page.locator("#multi-all-list [data-action=all]").innerText(), "Deselect all");
      await page.locator("#multi-all-list [data-action=all]").click();
      assert.deepEqual(await m.value(), []);
      await close();
    });

    test("MultiSelect on the server: found people become chips and the carrier gains their options", async () => {
      const { page, picker, close } = await open("/select");
      const m = picker("multi-search");
      assert.deepEqual(await m.value(), ["1"]);
      await page.locator("#f-multi-search").click();
      await page.keyboard.type("gauri lak");
      await page.waitForFunction(() => document.querySelector("#multi-search-list [role=option]")?.textContent.startsWith("Gauri"));
      await page.keyboard.press("Enter");
      assert.equal(await m.chips.count(), 2);
      assert.equal(await m.chips.nth(1).locator("[data-slot=chip-text]").innerText().then((t) => t.startsWith("Gauri")), true);
      assert.equal((await m.value()).length, 2);
      await close();
    });

    test("MultiSelect: creating a tag from text", async () => {
      const { page, picker, close } = await open("/select");
      const m = picker("multi-create");
      await page.locator("#f-multi-create").click();
      await page.keyboard.type("Warm lead");
      await page.keyboard.press("ArrowDown");
      await page.keyboard.press("Enter");
      assert.deepEqual(await m.value(), ["Warm lead"]);
      assert.equal(await m.chips.count(), 1);
      await close();
    });

    // ----------------------------------------------------------------- Palette

    test("Palette: Ctrl+K opens it with the box focused, typing ranks, Enter goes there, Esc closes and gives focus back", async () => {
      const { page, close } = await open("/select");
      await page.locator("h1").first().click();
      await page.keyboard.press("Control+k");
      const dialog = page.locator("#palette");
      assert.equal(await dialog.evaluate((d) => d.open), true);
      assert.equal(await page.evaluate(() => document.activeElement.getAttribute("data-slot")), "command-input");
      assert.deepEqual(await page.locator("#palette [role=group]:visible").evaluateAll((g) => g.map((x) => x.getAttribute("aria-label"))), ["Navigate", "Actions"]);
      await page.keyboard.type("deal");
      const rows = await page.locator("#palette-list [role=option]:not([hidden])").allInnerTexts();
      assert.match(rows[0], /^Deals/);
      assert.match(rows[1], /^New deal/);
      assert.equal(await page.locator("#palette-list [data-active]").innerText().then((t) => t.startsWith("Deals")), true);
      await page.keyboard.press("Escape");
      assert.equal(await dialog.evaluate((d) => d.open), false);
      await page.keyboard.press("Control+k");
      await page.keyboard.type("deals");
      await page.keyboard.press("Enter");
      await page.waitForURL("**/deals?per_page=25");
      await close();
    });

    test("Palette: a disabled item says why and can't be run; a post item posts; a run item opens a dialog", async () => {
      const { page, close } = await open("/select");
      await page.keyboard.press("Control+k");
      assert.match(await page.locator("#palette [aria-disabled=true]").innerText(), /Only owners can open billing/);
      await page.locator("#palette [aria-disabled=true]").click({ force: true });
      assert.equal(await page.locator("#palette").evaluate((d) => d.open), true, "the palette stays open");
      await page.keyboard.type("open the");
      await page.keyboard.press("Enter");
      assert.equal(await page.locator("#palette").evaluate((d) => d.open), false);
      assert.equal(await page.locator("#pick-dialog").evaluate((d) => d.open), true);
      await page.keyboard.press("Escape");
      await page.keyboard.press("Control+k");
      await page.keyboard.type("post an");
      await page.keyboard.press("Enter");
      await page.waitForURL("**/select/echo");
      await close();
    });

    test("Palette: item shortcuts run the item from anywhere, but a bare letter never does", async () => {
      const { page, close } = await open("/select");
      await page.keyboard.press("Control+Shift+o");
      assert.equal(await page.locator("#pick-dialog").evaluate((d) => d.open), true);
      await page.keyboard.press("Escape");
      await page.locator("#f-combo-basic").click();
      await page.keyboard.type("d");
      assert.equal(await page.locator("#pick-dialog").evaluate((d) => d.open), false);
      await page.keyboard.press("Escape");
      await page.keyboard.press("Alt+d");
      await page.waitForURL("**/deals?per_page=25");
      await close();
    });

    test("Palette: recent items come first when the box is empty, and an item that opts out never does", async () => {
      const { page, close } = await open("/select");
      await page.evaluate(() => localStorage.removeItem("navree-ui:recent:palette"));
      await page.reload();
      await page.keyboard.press("Control+k");
      assert.equal(await page.locator("#palette [data-slot=command-recent]").isVisible(), false);
      await page.keyboard.type("gallery");
      await page.keyboard.press("Enter");
      await page.waitForURL("**/gallery");
      await page.goBack();
      await page.waitForURL("**/select");
      await page.waitForSelector("#palette", { state: "attached" });
      await page.waitForFunction(() => !document.documentElement.hasAttribute("data-ui-loading"));
      await page.keyboard.press("Control+k");
      const recent = page.locator("#palette [data-slot=command-recent]");
      assert.equal(await recent.isVisible(), true);
      assert.match(await recent.innerText(), /Recent[\s\S]*Gallery/);
      const gallery = await page.locator("#palette-list [role=option]:not([hidden])", { hasText: "Gallery" }).count();
      assert.equal(gallery, 1, "the item is not shown twice");
      await page.keyboard.type("post an");
      await page.keyboard.press("Enter");
      await page.waitForURL("**/select/echo");
      await page.goBack();
      const stored = await page.evaluate(() => localStorage.getItem("navree-ui:recent:palette"));
      assert.ok(!stored.includes("/select/echo"), stored);
      await close();
    });

    test("Palette: a server source adds groups of links, and nothing found says so", async () => {
      const { page, close } = await open("/select");
      await page.keyboard.press("Control+k");
      await page.keyboard.type("gauri");
      await page.waitForFunction(() => document.querySelector("#palette [data-slot=command-remote] [role=option]"));
      assert.match(await page.locator("#palette [data-slot=command-remote]").innerText(), /Gauri/);
      await page.keyboard.press("Escape");
      await page.keyboard.press("Control+k");
      await page.keyboard.type("zzzzqq");
      await page.waitForFunction(() => /No results/.test(document.querySelector("#palette [data-slot=select-note]").textContent), null, { timeout: 5000 });
      await close();
    });

    test("Palette with JavaScript off: the button opens it and a link item goes", async () => {
      const { page, close } = await open("/select", { js: false });
      await page.locator("button[commandfor=palette]").click();
      assert.equal(await page.locator("#palette").evaluate((d) => d.open), true);
      await page.locator("#palette a", { hasText: "Gallery" }).click();
      await page.waitForURL("**/gallery");
      await close();
    });

    // ------------------------------------------------ Waiting: typing, and picking

    /** Notes when each input happened and when each fetch to `part` started, on the page's own clock. */
    async function clock(page, part) {
      await page.evaluate((part) => {
        window.__clock = { input: [], enter: [], fetches: [] };
        document.addEventListener("input", () => window.__clock.input.push(performance.now()), true);
        document.addEventListener("keydown", (e) => { if (e.key === "Enter") window.__clock.enter.push(performance.now()); }, true);
        new PerformanceObserver((list) => {
          for (const entry of list.getEntries()) if (entry.name.includes(part)) window.__clock.fetches.push({ at: entry.startTime, url: entry.name });
        }).observe({ type: "resource" });
      }, part);
    }

    test("Server search goes out 100 ms after the last key, and debounce(0) goes at once", async () => {
      const { page, close } = await open("/select");
      await page.locator("#f-combo-search").click();
      await page.waitForFunction(() => document.querySelectorAll("#combo-search-list [role=option]:not([data-action])").length === 20);
      await clock(page, "/select/people");
      await page.locator("#f-combo-search").fill("");
      await page.keyboard.type("anu", { delay: 30 });
      await page.waitForFunction(() => window.__clock.fetches.length >= 1);
      const first = await page.evaluate(() => {
        const last = window.__clock.input.at(-1);
        return { wait: window.__clock.fetches[0].at - last, count: window.__clock.fetches.length };
      });
      assert.equal(first.count, 1, "three quick keys, one request");
      assert.ok(first.wait >= 90 && first.wait <= 160, `the request went ${first.wait} ms after the last key`);
      // Zero means no wait at all.
      await page.locator("#combo-search").evaluate((root) => root.setAttribute("data-ui-debounce", "0"));
      await page.evaluate(() => { window.__clock.fetches.length = 0; window.__clock.input.length = 0; });
      await page.keyboard.type("g");
      await page.waitForFunction(() => window.__clock.fetches.length >= 1);
      const now = await page.evaluate(() => window.__clock.fetches[0].at - window.__clock.input.at(-1));
      assert.ok(now < 40, `with no debounce the request went ${now} ms after the key`);
      await close();
    });

    test("The first key typed in a closed searched list sends one search, not two", async () => {
      const { page, close } = await open("/select");
      const asked = [];
      page.on("request", (r) => { if (r.url().includes("/select/people?")) asked.push(new URL(r.url()).searchParams.get("q")); });
      // A slow answer, so the one that waits out the debounce isn't served from what the first brought.
      await page.route("**/select/people?*", async (route) => { await new Promise((r) => setTimeout(r, 350)); await route.continue(); });
      await page.locator("#f-multi-search").focus();
      await page.keyboard.type("g");
      await page.waitForTimeout(900);
      assert.deepEqual(asked, ["g"], "the key that opened the list asked once");
      await page.keyboard.type("r");
      await page.waitForTimeout(900);
      assert.deepEqual(asked, ["g", "gr"], "and the next key asks for its text after the wait");
      await close();
    });

    test("Picking sends its change at once: a Select, a Combobox and a filter form that listens", async () => {
      const { page, close } = await open("/select");
      await page.evaluate(() => {
        window.__picks = [];
        document.addEventListener("keydown", (e) => { if (e.key === "Enter") window.__picks.push({ key: performance.now() }); }, true);
        document.addEventListener("change", () => { const p = window.__picks.at(-1); if (p && p.change === undefined) p.change = performance.now(); }, true);
      });
      await page.locator("#f-select-basic").focus();
      await page.keyboard.press("ArrowDown");
      await page.keyboard.press("ArrowDown");
      await page.keyboard.press("Enter");
      await page.locator("#f-combo-basic").click();
      await page.keyboard.type("fr");
      await page.keyboard.press("Enter");
      const picks = await page.evaluate(() => window.__picks);
      assert.ok(picks.length >= 2 && picks.every((p) => p.change !== undefined), JSON.stringify(picks));
      for (const p of picks) assert.ok(p.change - p.key < 30, `a pick took ${p.change - p.key} ms`);
      await close();

      const filter = await open("/select/filter");
      await clock(filter.page, "/select/filter?");
      await filter.page.locator("#filter-select-trigger").click();
      await filter.page.keyboard.press("ArrowDown");
      await filter.page.keyboard.press("Enter");
      await filter.page.waitForFunction(() => document.querySelector("#filter-result").textContent === "Stage: qualified");
      await filter.page.waitForTimeout(500);
      const sent = await filter.page.evaluate(() => ({ enter: window.__clock.enter.at(-1), fetches: window.__clock.fetches.map((f) => f.at) }));
      // A pick fires `input` and then `change`; the form sends one request for both, with no wait.
      assert.equal(sent.fetches.length, 1, `a pick sent ${sent.fetches.length} requests`);
      assert.ok(sent.fetches[0] - sent.enter < 60, `the request went ${sent.fetches[0] - sent.enter} ms after the pick`);
      await filter.close();
    });

    test("A pick cancels the search it was typed ahead of, in a Combobox, a MultiSelect and the palette", async () => {
      const { page, close } = await open("/select");
      const searches = [];
      page.on("request", (r) => { if (/\/select\/(people|palette-search)\?/.test(r.url())) searches.push(new URL(r.url()).searchParams.get("q")); });

      // A Combobox: type, move to a row of the list that is shown, pick, all within the wait.
      await page.locator("#f-combo-search").click();
      await page.waitForFunction(() => document.querySelectorAll("#combo-search-list [role=option]:not([data-action])").length === 20);
      await page.locator("#f-combo-search").fill("");
      searches.length = 0;
      await page.keyboard.type("zq");
      await page.keyboard.press("ArrowDown");
      await page.keyboard.press("Enter");
      await page.waitForTimeout(400);
      assert.deepEqual(searches, [], "no request for 'zq' after the pick");
      assert.match(String(await page.locator("#combo-search [data-slot=select-native]").inputValue()), /^\d+$/);

      // A MultiSelect stays open after a pick, so a search typed ahead is cancelled, and
      // a reply on its way never replaces what the pick was made from.
      await page.route("**/select/people?q=zq*", async (route) => { await new Promise((r) => setTimeout(r, 700)); await route.continue(); });
      await page.locator("#f-multi-search").click();
      await page.waitForFunction(() => document.querySelectorAll("#multi-search-list [role=option]:not([data-action])").length >= 5);
      const before = await page.locator("#multi-search-list [role=option]:not([hidden])").allInnerTexts();
      searches.length = 0;
      await page.keyboard.type("zq");
      await page.keyboard.press("ArrowDown");
      await page.keyboard.press("Enter");
      await page.waitForTimeout(1200);
      assert.equal(await page.locator("#multi-search [data-slot=chip]").count() >= 2, true, "the pick was made at once");
      assert.deepEqual(searches, [], "no request for 'zq' after the pick");
      const after = await page.locator("#multi-search-list [role=option]:not([hidden])").allInnerTexts();
      assert.deepEqual(after.slice(0, 5), before.slice(0, 5), "the list the pick was made from stays");

      // The palette: an item picked within the wait closes it, and nothing is asked after.
      await page.keyboard.press("Escape");
      await page.keyboard.press("Control+k");
      searches.length = 0;
      await page.keyboard.type("open the");
      await page.keyboard.press("Enter");
      await page.waitForTimeout(400);
      assert.equal(await page.locator("#palette").evaluate((d) => d.open), false);
      assert.deepEqual(searches, [], "no request for 'open the' after the item was picked");
      await close();
    });

    // -------------------------------------------- What a page may not take on trust

    test("Palette: a post item goes through the form runtime with the CSRF header, and with script off it carries the token", async () => {
      const { page, context, close } = await open("/select");
      await context.addCookies([{ name: "XSRF-TOKEN", value: "cookie-token-123", url: BASE }]);
      const posted = page.waitForRequest((r) => r.method() === "POST" && r.url().endsWith("/select/echo"));
      await page.keyboard.press("Control+k");
      await page.keyboard.type("post an");
      await page.keyboard.press("Enter");
      const request = await posted;
      assert.equal(request.headers()["x-xsrf-token"], "cookie-token-123", "the cookie's token goes as the header");
      assert.equal(request.headers()["x-navree-ui"], "1", "it went through the form runtime");
      await page.waitForURL("**/select/echo");
      await close();

      const off = await open("/select", { js: false });
      await off.page.locator("button[commandfor=palette]").click();
      await off.page.locator("#palette button", { hasText: "Post an action" }).click();
      await off.page.waitForURL("**/select/echo");
      assert.match(await off.page.locator("[data-echo]").innerText(), /_csrf = palette-token/, "with no script the token rides in the body");
      await off.close();
    });

    test("MultiSelect max: a new search's rows are off at the limit, a row the server disabled stays off, and making a new one stops", async () => {
      const { page, picker, close } = await open("/select");
      const m = picker("multi-cap");
      const posts = [];
      page.on("request", (r) => { if (r.method() === "POST" && r.url().includes("/select/teams")) posts.push(r.url()); });
      const state = () => page.locator("#multi-cap-list [role=option]:not([data-action])").evaluateAll((rows) => rows.map((r) => ({ text: r.textContent.slice(0, 14), selected: r.getAttribute("aria-selected"), off: r.getAttribute("aria-disabled") === "true", ours: r.hasAttribute("data-limit-off") })));
      await page.locator("#f-multi-cap").click();
      await page.waitForFunction(() => document.querySelectorAll("#multi-cap-list [role=option]:not([data-action])").length >= 5);
      let rows = await state();
      assert.equal(rows[1].off, true, "the server disabled the second row");
      assert.equal(rows[1].ours, false);
      assert.equal(rows[2].off, false);
      // The third row makes two of two.
      await page.locator("#multi-cap-list [role=option]:not([data-action])").nth(2).click();
      assert.equal(await m.chips.count(), 2);
      rows = await state();
      for (const r of rows) if (r.selected === "false") assert.equal(r.off, true, JSON.stringify(r));
      assert.equal(rows[1].ours, false, "the server's own switch is not the limit's");
      assert.equal(rows[3].ours, true);
      // Taking a chip off lets the limit go: the rows come back on, and the one the server disabled stays off.
      await page.locator("#multi-cap [data-slot=chip]").nth(1).locator("[data-slot=chip-remove]").click();
      rows = await state();
      assert.equal(rows[1].off, true, "still off, as the server drew it");
      assert.equal(rows[3].off, false);
      // Opened again, the list comes in the same way: the server's switch stays, the limit's is gone.
      await page.locator("#f-multi-cap").click();
      await page.waitForFunction(() => document.querySelector("#multi-cap-list")?.closest("[data-open]") && document.querySelectorAll("#multi-cap-list [role=option]:not([data-action])").length >= 5);
      rows = await state();
      assert.equal(rows[1].off, true);
      assert.equal(rows[2].off, false);
      // At the limit again, the rows of a new search come in off.
      await page.locator("#multi-cap-list [role=option]:not([data-action])").nth(2).click();
      await page.locator("#f-multi-cap").focus();
      await page.keyboard.type("gauri");
      await page.waitForFunction(() => document.querySelector("#multi-cap-list [role=option]")?.textContent.startsWith("Gauri"));
      rows = await state();
      assert.ok(rows.length > 3, JSON.stringify(rows));
      for (const r of rows) if (r.selected === "false") assert.equal(r.off, true, `a row of the new answer is on at the limit: ${JSON.stringify(r)}`);
      // Making a new one is off too: the row is, the keys never reach it, and a click does nothing.
      await page.keyboard.type(" zzqq");
      await page.waitForFunction(() => document.querySelector("#multi-cap-list [data-action=create]"));
      assert.equal(await page.locator("#multi-cap-list [data-action=create]").getAttribute("aria-disabled"), "true");
      assert.equal(await page.locator("#multi-cap-list [data-action=create][data-active]").count(), 0);
      await page.locator("#multi-cap-list [data-action=create]").click({ force: true });
      await page.waitForTimeout(300);
      assert.equal(posts.length, 0, "nothing was created");
      assert.equal(await m.chips.count(), 2);
      assert.match(await m.status.innerText(), /at most 2/i);
      await close();
    });

    test("MultiSelect max with any text: a new tag is made below the limit and not at it", async () => {
      const { page, picker, close } = await open("/select");
      const m = picker("multi-cap-free");
      await page.locator("#f-multi-cap-free").click();
      await page.keyboard.type("Warm lead");
      await page.locator("#multi-cap-free-list [data-action=create]").click();
      assert.equal(await m.chips.count(), 2);
      await page.keyboard.type("Another one");
      await page.waitForFunction(() => document.querySelector("#multi-cap-free-list [data-action=create]"));
      assert.equal(await page.locator("#multi-cap-free-list [data-action=create]").getAttribute("aria-disabled"), "true");
      await page.locator("#multi-cap-free-list [data-action=create]").click({ force: true });
      await page.keyboard.press("Enter");
      assert.equal(await m.chips.count(), 2);
      assert.deepEqual((await m.value()).sort(), ["Warm lead", "lead"]);
      await close();
    });

    test("A form the server checks: a value that is not one of the options is refused, and so are too many", async () => {
      const post = (page, body) => page.request.post(BASE + "/select/derive", { data: body, headers: { origin: BASE, "content-type": "application/x-www-form-urlencoded" } });
      const { page, close } = await open("/select/derive");
      // Straight from a hand-made post.
      const good = await post(page, "stage=lead&owner=it&person=2&countries=fr");
      assert.equal(good.status(), 200);
      assert.match(await good.text(), /<code>stage<\/code> = <code>lead<\/code>/);
      for (const [name, body, field] of [
        ["a stage that isn't one", "stage=forged&owner=it&countries=fr", "stage"],
        ["a country that isn't one", "stage=lead&owner=xx&countries=fr", "owner"],
        ["a person nobody searched for", "stage=lead&owner=it&person=99999999&countries=fr", "person"],
        ["a market that isn't one", "stage=lead&owner=it&countries=fr&countries=zz", "countries"],
      ]) {
        const refused = await post(page, body);
        assert.equal(refused.status(), 422, name);
        assert.match(await refused.text(), /Choose one of the options\./, `${name} (${field})`);
      }
      const many = await post(page, "stage=lead&owner=it&countries=fr&countries=de&countries=jp");
      assert.equal(many.status(), 422, "three markets of at most two");
      assert.match(await many.text(), /Choose at most 2\./);

      // From the page, with script: the runtime shows the message under the field.
      await page.evaluate(() => {
        const stage = document.querySelector("#derived-stage-picker [data-slot=select-native]");
        stage.append(new Option("Forged", "forged"));
        stage.value = "forged";
        document.querySelector("#derived-owner-picker [data-slot=select-native]").value = "it";
      });
      await page.locator("#derived button[type=submit]").click();
      await page.waitForFunction(() => document.querySelector("#derived-stage")?.getAttribute("aria-invalid") === "true");
      assert.match(await page.locator("[data-slot=field]", { has: page.locator("#derived-stage") }).innerText(), /Choose one of the options\./);
      await close();

      // With script off the form comes back with what was sent and the message.
      const off = await open("/select/derive", { js: false });
      await off.page.locator("#derived-stage-picker [data-slot=select-native]").selectOption("won");
      await off.page.locator("#derived-owner-picker [data-slot=select-native]").selectOption("it");
      await off.page.locator("#derived-countries-picker [data-slot=select-native]").selectOption(["fr", "de", "jp"]);
      await off.page.locator("#derived button[type=submit]").click();
      await off.page.waitForFunction(() => /Choose at most 2\./.test(document.body.innerText));
      assert.match(await off.page.locator("#derived").innerText(), /Choose at most 2\./);
      assert.equal(await off.page.locator("#derived-stage-picker [data-slot=select-native]").inputValue(), "won", "what was sent is kept");
      await off.close();
    });

    test("A reply from the server can't bind a shortcut or paint with a url()", async () => {
      const { page, close } = await open("/select");
      await page.route("**/select/palette-search?*", (route) => route.fulfill({
        status: 200,
        contentType: "text/html; charset=utf-8",
        body: '<div data-slot="option-page"><a role="option" href="/gallery" data-slot="command-item" data-value="x" data-shortcut="ctrl+shift+y">Evil<svg><path fill="url(#x)" stroke="url( &quot;http://evil.example/a.svg#b&quot; )" stroke-width="2" d="M0 0"/><circle fill="red" stroke="u\\72l(#y)" cx="1" cy="1" r="1"/></svg></a></div>',
      }));
      await page.keyboard.press("Control+k");
      await page.keyboard.type("evil");
      await page.waitForFunction(() => document.querySelector("#palette [data-slot=command-remote] [role=option]"));
      const out = await page.evaluate(() => {
        const row = document.querySelector("#palette [data-slot=command-remote] [role=option]");
        const path = row.querySelector("path"), circle = row.querySelector("circle");
        return { shortcut: row.hasAttribute("data-shortcut"), pathFill: path?.getAttribute("fill"), pathStroke: path?.getAttribute("stroke"), width: path?.getAttribute("stroke-width"), circleFill: circle?.getAttribute("fill"), circleStroke: circle?.getAttribute("stroke") };
      });
      assert.deepEqual(out, { shortcut: false, pathFill: null, pathStroke: null, width: "2", circleFill: "red", circleStroke: null });
      await page.keyboard.press("Escape");
      await page.keyboard.press("Control+Shift+Y");
      await page.waitForTimeout(300);
      assert.ok(page.url().endsWith("/select"), `the chord went to ${page.url()}`);
      await close();
    });

    test("A palette that was closed starts again: what the server answered for the last text is gone", async () => {
      const { page, close } = await open("/select");
      await page.keyboard.press("Control+k");
      await page.keyboard.type("gauri");
      await page.waitForFunction(() => document.querySelector("#palette [data-slot=command-remote] [role=option]"));
      await page.keyboard.press("Escape");
      await page.keyboard.press("Control+k");
      assert.equal(await page.locator("#palette [data-slot=command-remote] [role=option]").count(), 0);
      await close();
    });

    /** What a person sees in the palette: the box, the rows shown, the row on, and where the focus is. */
    const inPalette = (page) =>
      page.evaluate(() => {
        const dialog = document.getElementById("palette"), input = dialog.querySelector("[data-slot=command-input]"), row = dialog.querySelector("[data-active]");
        return {
          open: dialog.open,
          value: input.value,
          rows: [...dialog.querySelectorAll("[role=option]:not([hidden])")].map((o) => o.textContent.trim()),
          active: row ? row.textContent.trim() : null,
          inRecent: !!row?.closest("[data-slot=command-recent]"),
          pointed: !!row && input.getAttribute("aria-activedescendant") === row.id,
          focused: document.activeElement === input,
        };
      });
    const swap = (page) => page.evaluate(() => document.dispatchEvent(new Event("ui:swap")));

    test("A palette that is open keeps the text typed in it, the rows it shows and the row on it when the page swaps; the next open starts empty", async () => {
      const { page, close } = await open("/select");
      await page.keyboard.press("Control+k");
      const fresh = await inPalette(page);
      assert.deepEqual([fresh.open, fresh.value, fresh.focused, fresh.active], [true, "", true, null]);
      await swap(page);
      assert.deepEqual(await inPalette(page), fresh, "no row was on, and none is");
      // A swap between two keys, as a live region's answer comes: the first letters are not lost.
      await page.keyboard.type("d");
      await swap(page);
      await page.keyboard.type("eal");
      let now = await inPalette(page);
      assert.equal(now.value, "deal");
      assert.match(now.rows[0], /^Deals/);
      assert.match(now.rows[1], /^New deal/);
      assert.match(now.active, /^Deals/);
      // The row moved to by hand stays on, with the text, the rows shown and the focus.
      await page.keyboard.press("ArrowDown");
      now = await inPalette(page);
      assert.match(now.active, /^New deal/);
      assert.equal(now.pointed, true);
      await swap(page);
      assert.deepEqual(await inPalette(page), now);
      // A palette that was closed starts again, whatever swapped while it was open.
      await page.keyboard.press("Escape");
      await page.keyboard.press("Control+k");
      assert.deepEqual(await inPalette(page), fresh);
      // And Enter goes where the row was, not where the text would have led had its first letters gone.
      await page.keyboard.type("deal");
      await page.keyboard.press("ArrowDown");
      await swap(page);
      await page.keyboard.press("Enter");
      await page.waitForURL("**/deals/new");
      await close();
    });

    test("A morph of the page draws an open palette's Recent group again with the row on it still on, and a palette the swap replaced starts as a new page's does", async () => {
      const { page, close } = await open("/select");
      await page.evaluate(() => localStorage.setItem("navree-ui:recent:palette", JSON.stringify(["/gallery", "/deals/new"])));
      await page.reload();
      await page.keyboard.press("Control+k");
      const recent = page.locator("#palette [data-slot=command-recent]");
      assert.match(await recent.innerText(), /Recent[\s\S]*Gallery[\s\S]*New deal/);
      // Nothing is on until a key moves it; the second press lands on the second Recent row.
      await page.keyboard.press("ArrowDown");
      await page.keyboard.press("ArrowDown");
      const now = await inPalette(page);
      assert.match(now.active, /^New deal/);
      assert.deepEqual([now.open, now.inRecent, now.pointed], [true, true, true]);
      // The page is fetched and morphed into itself, as a link to it does. The server draws the Recent group
      // empty and the rows as they were first, so the swap that follows has to fill and sort them again.
      await page.evaluate(
        () =>
          new Promise((resolve) => {
            document.addEventListener("ui:swap", resolve, { once: true });
            document.querySelector('a[href="/select"]').dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true }));
          }),
      );
      assert.deepEqual(await inPalette(page), now);
      assert.match(await recent.innerText(), /Recent[\s\S]*Gallery[\s\S]*New deal/);
      // A swap that puts a new palette in the place of the open one: the new one is made ready.
      await page.evaluate(async () => {
        const doc = new DOMParser().parseFromString(await (await fetch("/select")).text(), "text/html");
        document.getElementById("palette").replaceWith(document.adoptNode(doc.getElementById("palette")));
      });
      await swap(page);
      assert.match(await recent.innerText(), /Recent[\s\S]*Gallery[\s\S]*New deal/);
      await page.keyboard.press("Control+k");
      const next = await inPalette(page);
      assert.deepEqual([next.open, next.value, next.focused, next.active], [true, "", true, null]);
      await close();
    });

    /**
     * Draws the element `id` again as the server first drew it, as a live region's answer does: it is made a
     * region of the page it is on, and fetched and morphed in place.
     */
    const redraw = (page, id) =>
      page.evaluate(async (target) => {
        const el = document.getElementById(target);
        el.setAttribute("data-ui-region", "");
        el.setAttribute("data-ui-src", location.pathname);
        const swapped = new Promise((resolve) => document.addEventListener("ui:swap", resolve, { once: true }));
        const detail = {};
        el.dispatchEvent(new CustomEvent("ui:refresh", { bubbles: true, detail }));
        await detail.done;
        await swapped;
      }, id);
    /** From now on, what the live region `selector` comes to say (not when it is emptied). */
    const listen = (page, selector) =>
      page.evaluate((sel) => {
        const live = document.querySelector(sel);
        window.__said = [];
        new MutationObserver(() => live.textContent && window.__said.push(live.textContent)).observe(live, { childList: true, characterData: true, subtree: true });
      }, selector);
    const said = async (page) => {
      // Past the wait a count of results is said after, and the frame it is written in.
      await page.waitForTimeout(700);
      return page.evaluate(() => window.__said);
    };

    test("A combobox searched on the server and open keeps its answers, the text typed and the row on when it is drawn again, and doesn't say how many again", async () => {
      const { page, close } = await open("/select");
      const input = page.locator("#combo-search [data-slot=combobox-input]");
      await input.click();
      await input.fill("");
      const answered = page.waitForResponse((r) => r.url().includes("/select/people") && new URL(r.url()).searchParams.get("q") === "gau");
      await input.pressSequentially("gau", { delay: 30 });
      await answered;
      // The count of results is said a moment after the rows are in.
      await page.waitForFunction(() => /result/.test(document.querySelector("#combo-search [data-slot=select-status]")?.textContent || ""));
      await page.waitForTimeout(700);
      await page.keyboard.press("ArrowDown");
      const seen = () =>
        page.evaluate(() => {
          const root = document.getElementById("combo-search"), box = root.querySelector("[data-slot=combobox-input]"), row = root.querySelector("[role=option][data-active]");
          return {
            open: root.hasAttribute("data-open") && root.querySelector("[data-slot=select-popup]").matches(":popover-open"),
            expanded: box.getAttribute("aria-expanded"),
            value: box.value,
            rows: [...root.querySelectorAll("[role=option]:not([hidden])")].map((o) => o.textContent.trim()),
            active: row ? row.textContent.trim() : null,
            pointed: !!row && box.getAttribute("aria-activedescendant") === row.id,
            focused: document.activeElement === box,
          };
        });
      const before = await seen();
      assert.deepEqual([before.open, before.expanded, before.value, before.focused, before.pointed], [true, "true", "gau", true, true], JSON.stringify(before));
      assert.ok(before.rows.length > 0, JSON.stringify(before.rows));
      await listen(page, "#combo-search [data-slot=select-status]");
      // The server draws the list empty: its answers are the browser's while it is open.
      await redraw(page, "combo-search");
      assert.deepEqual(await seen(), before, "the list, the text and the row on are as they were");
      assert.deepEqual(await said(page), [], "how many results there are is not said again");
      // Closed, the list is the page's again: the next open asks the server.
      await page.keyboard.press("Escape");
      assert.equal(await page.locator("#combo-search [data-slot=select-list][data-ui-keep]").count(), 0);
      await close();
    });

    test("An open palette keeps the server's answers when it is drawn again, and doesn't say how many results again", async () => {
      const { page, close } = await open("/select");
      await page.keyboard.press("Control+k");
      const answered = page.waitForResponse((r) => r.url().includes("/select/palette-search") && new URL(r.url()).searchParams.get("q") === "gauri");
      await page.keyboard.type("gauri");
      await answered;
      await page.waitForFunction(() => document.querySelector("#palette [data-slot=command-remote] [role=option]"));
      await page.waitForFunction(() => /result/.test(document.querySelector("#palette [data-slot=select-status]")?.textContent || ""));
      await page.waitForTimeout(700);
      const before = await inPalette(page);
      assert.ok(before.rows.some((r) => /Gauri/.test(r)), JSON.stringify(before.rows));
      await listen(page, "#palette [data-slot=select-status]");
      await redraw(page, "palette");
      assert.deepEqual(await inPalette(page), before, "the answers, the text and the row on are as they were");
      assert.ok((await page.locator("#palette [data-slot=command-remote] [role=option]").count()) > 0);
      assert.deepEqual(await said(page), [], "how many results there are is not said again");
      await page.keyboard.press("Escape");
      // A dialog's close event comes in a task of its own after it closes (WebKit's, later than the next step here).
      await page.waitForFunction(() => !document.querySelector("#palette[open]")).catch(() => {});
      await page.waitForFunction(() => !document.querySelector("#palette [data-slot=command-remote][data-ui-keep]"), null, { timeout: 2000 }).catch(() => {});
      assert.equal(await page.locator("#palette [data-slot=command-remote][data-ui-keep]").count(), 0, "a closed palette's answers are gone");
      await close();
    });

    test("The select module reads the page through the prototype: a named element can't stop it", async () => {
      const { page, problems, close } = await open("/select");
      // Elements named like the properties the module reads would shadow them on the document.
      await page.evaluate(() => {
        for (const name of ["querySelector", "querySelectorAll", "activeElement"]) {
          const img = Document.prototype.createElement.call(document, "img");
          Element.prototype.setAttribute.call(img, "name", name);
          document.body.append(img);
        }
        Document.prototype.getElementById.call(document, "f-combo-basic").focus();
      });
      await page.keyboard.type("fra");
      await page.keyboard.press("Enter");
      const shown = await page.evaluate(() => Document.prototype.getElementById.call(document, "f-combo-basic").value);
      assert.match(shown, /France/);
      await page.keyboard.press("Control+k");
      assert.equal(await page.evaluate(() => Document.prototype.getElementById.call(document, "palette").open), true);
      assert.deepEqual(problems, []);
      await close();

      // And a form with a field named like the method the module calls on it.
      const form = await open("/select/form");
      await form.page.evaluate(() => {
        const f = Document.prototype.getElementById.call(document, "pick-form");
        const i = Document.prototype.createElement.call(document, "input");
        Element.prototype.setAttribute.call(i, "name", "querySelectorAll");
        f.append(i);
        HTMLFormElement.prototype.reset.call(f);
      });
      await form.page.waitForTimeout(100);
      assert.deepEqual(form.problems, []);
      await form.close();
    });

    // ----------------------------------------- Where they are: dialog, card, phone

    test("A popup inside a dialog sits above it, Esc closes the popup first and the dialog second", async () => {
      const { page, picker, close } = await open("/select");
      await page.locator("button[command=show-modal][commandfor=pick-dialog]").click();
      const c = picker("dlg-combo");
      await page.locator("#f-dlg-combo").click();
      assert.equal(await c.isOpen(), true);
      const at = await c.popup.boundingBox();
      const top = await page.evaluate(([x, y]) => document.elementFromPoint(x, y)?.closest("[data-slot=select-popup]")?.id, [at.x + 20, at.y + 20]);
      assert.equal(top, "dlg-combo-popup", "the popup is the top element at its own place");
      await page.keyboard.press("Escape");
      assert.equal(await c.isOpen(), false);
      assert.equal(await page.locator("#pick-dialog").evaluate((d) => d.open), true, "the dialog stays");
      await page.keyboard.press("Escape");
      assert.equal(await page.locator("#pick-dialog").evaluate((d) => d.open), false);
      await close();
    });

    test("A popup is not clipped by a scrolling card", async () => {
      const { page, picker, close } = await open("/select");
      await page.evaluate(() => {
        const card = document.querySelector("[data-slot=scroll-area]");
        card.style.height = "140px";
        card.style.overflow = "hidden";
      });
      const c = picker("scroll-combo");
      await page.locator("#f-scroll-combo").scrollIntoViewIfNeeded();
      await page.locator("#f-scroll-combo").click();
      const at = await c.popup.boundingBox();
      const card = await page.locator("[data-slot=scroll-area]").boundingBox();
      assert.ok(at.y + at.height > card.y + card.height || at.y < card.y, "the popup reaches outside the card");
      const hit = await page.evaluate(([x, y]) => document.elementFromPoint(x, y)?.closest("[data-slot=select-popup]")?.id, [at.x + 20, at.y + at.height - 20]);
      assert.equal(hit, "scroll-combo-popup");
      await close();
    });

    test("At 390px the popup fits the screen, text is 16px for touch, nothing scrolls sideways, and a tap picks", async () => {
      const { page, picker, close } = await open("/select", { viewport: { width: 390, height: 780 }, touch: true });
      const c = picker("combo-basic");
      await page.locator("#f-combo-basic").scrollIntoViewIfNeeded();
      assert.equal(await page.locator("#f-combo-basic").evaluate((e) => getComputedStyle(e).fontSize), "16px");
      await page.locator("#f-combo-basic").tap();
      assert.equal(await c.isOpen(), true);
      const at = await c.popup.boundingBox();
      assert.ok(at.x >= 0 && at.x + at.width <= 390, JSON.stringify(at));
      assert.ok(at.y >= 0 && at.y + at.height <= 780, JSON.stringify(at));
      await c.rows.nth(1).tap();
      assert.equal(await c.value(), "au");
      const overflow = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
      assert.equal(overflow, 0);
      const s = picker("select-groups");
      await page.locator("#f-select-groups").scrollIntoViewIfNeeded();
      await page.locator("#f-select-groups").tap();
      const sb = await s.popup.boundingBox();
      assert.ok(sb.x >= 0 && sb.x + sb.width <= 390);
      await close();
    });

    test("Right to left: the popup lines up with the end, chip arrows swap", async () => {
      const { page, picker, close } = await open("/select");
      const s = picker("rtl-select");
      const trigger = page.locator("#f-rtl-select");
      await trigger.scrollIntoViewIfNeeded();
      await trigger.click();
      await page.waitForTimeout(200);
      const t = await trigger.boundingBox();
      const p = await s.popup.boundingBox();
      assert.ok(Math.abs(p.x + p.width - (t.x + t.width)) <= 2, `the popup lines up with the right edge: ${JSON.stringify([t, p])}`);
      await page.keyboard.press("Escape");
      await page.locator("#f-rtl-multi").focus();
      await page.keyboard.press("ArrowRight");
      assert.equal(await page.evaluate(() => document.activeElement.getAttribute("data-slot")), "chip", "in RTL, Right goes to the previous chip");
      await page.keyboard.press("ArrowRight");
      assert.equal(await page.evaluate(() => document.activeElement.querySelector("[data-slot=chip-text]").textContent), "France");
      await page.keyboard.press("ArrowLeft");
      assert.equal(await page.evaluate(() => document.activeElement.querySelector("[data-slot=chip-text]").textContent), "Germany");
      await close();
    });

    test("Two pickers don't talk to each other: opening one closes the other, and a morph of the page leaves an open one open", async () => {
      const { page, picker, close } = await open("/select");
      const a = picker("select-basic"), b = picker("select-selected");
      await page.locator("#f-select-basic").click();
      assert.equal(await a.isOpen(), true);
      await page.locator("#f-select-selected").click();
      assert.equal(await a.isOpen(), false);
      assert.equal(await b.isOpen(), true);
      // The runtime morphs the same page into itself, as a poll or a form answer does.
      await page.evaluate(() => document.dispatchEvent(new Event("ui:swap")));
      assert.equal(await b.isOpen(), true);
      assert.equal(await page.locator("#f-select-selected").getAttribute("aria-expanded"), "true");
      await close();
    });

    test("Back and forward: the button shows what the select holds, whatever the browser restored", async () => {
      const { page, picker, close } = await open("/select/form");
      await page.locator("#f-form-stage").click();
      await page.keyboard.press("ArrowDown");
      await page.keyboard.press("Enter");
      const chosen = await picker("form-select").value();
      await page.goto(BASE + "/plain");
      await page.goBack();
      await page.waitForSelector("#form-select");
      const held = await picker("form-select").value();
      // A page that is never stored (`no-store`) gets its form values back from some browsers and
      // not others; the button must agree with the select in either case.
      const shown = await page.locator("#f-form-stage").innerText();
      const label = held === "" ? "Choose a stage" : await page.locator(`#form-select [role=option][data-value="${held}"]`).evaluate((o) => o.textContent);
      assert.equal(shown, label, `the button shows what the select holds (${held || "nothing"}); it held ${chosen} before`);
      await close();
    });

    // ------------------------------------------------------------ 10,000 options

    for (const kind of ["select", "combobox", "multi"]) {
      test(`10,000 options in a ${kind}: opens fast, shows at most 200 rows (a ${kind === "select" ? "select shows all" : "combobox is capped"}), filters over all`, async () => {
        const { page, picker, close } = await open(`/select/big-${kind}`);
        const b = picker("big");
        const timings = {};
        const control = page.locator("#f-big");
        timings.openMs = await page.evaluate(() => new Promise((resolve) => {
          const t0 = performance.now();
          document.querySelector("#f-big").click();
          requestAnimationFrame(() => requestAnimationFrame(() => resolve(performance.now() - t0)));
        }));
        assert.equal(await b.isOpen(), true);
        const shown = await b.rows.count();
        if (kind !== "select") {
          assert.equal(shown, 200);
          assert.match(await b.note.innerText(), /Showing 200 of 10,000/);
          timings.filterMs = await page.evaluate(() => new Promise((resolve) => {
            const input = document.querySelector("#f-big");
            const t0 = performance.now();
            input.value = "number 9999";
            input.dispatchEvent(new Event("input", { bubbles: true }));
            requestAnimationFrame(() => requestAnimationFrame(() => resolve(performance.now() - t0)));
          }));
          assert.deepEqual(await b.texts(), ["Choice number 9999 of the long list"]);
          await control.fill("k5000");
          assert.deepEqual(await b.texts(), ["Choice number 5000 of the long list"], "keywords are searched in all 10,000");
        } else {
          assert.equal(shown, 10000);
          await page.keyboard.press("End");
          assert.match(await b.activeText(), /9999/);
          await page.keyboard.press("Home");
        }
        console.log(`# ${browserName} ${kind} x10000: ${JSON.stringify(timings)}`);
        assert.ok(timings.openMs < 1500, `opened in ${timings.openMs} ms`);
        if (timings.filterMs) assert.ok(timings.filterMs < 600, `filtered in ${timings.filterMs} ms`);
        await close();
      });
    }

    test("A filter form: choosing refreshes the region with no submit button pressed, and the URL and Back follow", async () => {
      const { page, picker, close } = await open("/select/filter");
      const requests = [];
      page.on("request", (r) => { if (r.url().includes("/select/filter?")) requests.push(r.url()); });
      await page.locator("#filter-select-trigger").click();
      await page.keyboard.press("ArrowDown");
      await page.keyboard.press("Enter");
      await page.waitForFunction(() => document.querySelector("#filter-result").textContent === "Stage: qualified");
      assert.equal(requests.length, 1, "one fetch for the one change");
      assert.ok(page.url().includes("stage=qualified"), page.url());
      await page.goBack();
      await page.waitForFunction(() => document.querySelector("#filter-result").textContent === "Stage: ");
      assert.equal(await picker("filter-select").value(), "", "Back shows the value the page had");
      await close();
    });

    test("A filter form with JavaScript off: the native select and the button do the same", async () => {
      const { page, close } = await open("/select/filter", { js: false });
      await page.locator("#filter-select [data-slot=select-native]").selectOption("won");
      await page.locator("#filter-form button[type=submit]").click();
      await page.waitForURL("**/select/filter?stage=won");
      assert.equal(await page.locator("#filter-result").innerText(), "Stage: won");
      assert.equal(await page.locator("#filter-select [data-slot=select-native]").inputValue(), "won");
      await close();
    });

    test("Tab visits each picker once: not the hidden select, the clear button, the toggle or a chip's button", async () => {
      const { page, close } = await open("/select/form");
      await page.locator("h1").first().click();
      const stops = [];
      for (let i = 0; i < 9; i++) {
        await page.keyboard.press("Tab");
        stops.push(await page.evaluate(() => `${document.activeElement.id || document.activeElement.getAttribute("data-slot") || document.activeElement.tagName}`));
      }
      for (const stop of stops) {
        assert.ok(!/select-native|chip-remove|combobox-toggle|combobox-clear|select-clear/.test(stop), `a stop on ${stop}: ${stops}`);
      }
      assert.ok(stops.includes("f-form-stage") && stops.includes("f-form-owner") && stops.includes("f-form-person"), stops.join(", "));
      assert.equal(stops.filter((s) => s === "f-form-owner").length, 1, "one stop for a combobox");
      await close();
    });

    test("Focus is visible: a ring on the button and on the box that holds an input", async () => {
      const { page, close } = await open("/select");
      await page.locator("#f-select-basic").focus();
      assert.notEqual(await page.locator("#f-select-basic").evaluate((e) => getComputedStyle(e).outlineStyle), "none");
      await page.locator("#f-combo-basic").focus();
      assert.notEqual(await page.locator("#combo-basic [data-slot=combobox-control]").evaluate((e) => getComputedStyle(e).outlineStyle), "none");
      await page.locator("#f-multi-basic").focus();
      await page.keyboard.press("Backspace");
      assert.notEqual(await page.evaluate(() => getComputedStyle(document.activeElement).outlineStyle), "none", "a chip shows its focus");
      await close();
    });

    test("What a screen reader is told: result counts, what was added and removed", async () => {
      const { page, picker, close } = await open("/select");
      const c = picker("combo-basic");
      await page.locator("#f-combo-basic").click();
      await page.keyboard.type("an");
      await page.waitForFunction(() => document.querySelector("#combo-basic [data-slot=select-status]").textContent === "10 results available");
      const m = picker("multi-basic");
      await page.locator("#f-multi-basic").click();
      await page.keyboard.type("jap");
      await page.keyboard.press("Enter");
      await page.waitForFunction(() => document.querySelector("#multi-basic [data-slot=select-status]").textContent === "Japan added");
      await m.chips.last().locator("[data-slot=chip-remove]").click();
      await page.waitForFunction(() => document.querySelector("#multi-basic [data-slot=select-status]").textContent === "Japan removed");
      assert.equal(await page.locator("#multi-basic [data-slot=select-status]").getAttribute("aria-live"), "polite");
      assert.equal(await c.status.getAttribute("role"), "status");
      await close();
    });

    test("In French: the words come from the page, and English is the runtime's own", async () => {
      const { page, close } = await open("/select/fr");
      assert.equal(await page.locator("#fr-combo [data-slot=combobox-clear]").getAttribute("aria-label"), "Effacer");
      await page.locator("#f-fr-combo").click();
      await page.keyboard.press("Control+a");
      await page.keyboard.type("qqq");
      assert.equal(await page.locator("#fr-combo-list [data-action=create]").innerText(), "Créer « qqq »");
      assert.match(await page.locator("#fr-combo [data-slot=select-note]").innerText(), /Aucun résultat/);
      await page.locator("#f-fr-combo").fill("an");
      await page.waitForFunction(() => /résultats disponibles/.test(document.querySelector("#fr-combo [data-slot=select-status]").textContent));
      await close();
    });

    test("A chosen option can show a shorter label in the button than in the list", async () => {
      const { page, close } = await open("/select");
      await page.locator("#f-select-rich").click();
      await page.locator("#select-rich-list [role=option]", { hasText: "Anu Lakhotia" }).click();
      assert.equal(await page.locator("#f-select-rich").innerText(), "Anu");
      await close();
    });

    test("A form drawn from a struct: the derive's pickers post, and a Many reads every value", async () => {
      const { page, picker, problems, close } = await open("/select/derive");
      await page.locator("#derived button[type=submit]").click();
      assert.ok(page.url().endsWith("/select/derive"), "an empty required picker blocks the send");
      assert.equal(await page.evaluate(() => document.activeElement.id), "derived-stage");
      await page.locator("#derived-stage").click();
      await page.keyboard.press("ArrowDown");
      await page.keyboard.press("Enter");
      await page.locator("#derived-owner").fill("ger");
      await picker("derived-owner-picker").rows.first().click();
      await page.locator("#derived-person").fill("arun");
      await page.waitForFunction(() => document.querySelector("#derived-person-picker-list [role=option]")?.textContent.startsWith("Arun"));
      await page.keyboard.press("Enter");
      await page.locator("#derived-countries").click();
      await page.keyboard.type("fra");
      await page.keyboard.press("Enter");
      await page.keyboard.type("jap");
      await page.keyboard.press("Enter");
      await page.keyboard.press("Escape");
      await page.locator("#derived button[type=submit]").click();
      await page.waitForSelector("[data-echo]");
      const sent = await page.locator("[data-echo]").innerText();
      assert.match(sent, /stage = qualified/);
      assert.match(sent, /owner = de/);
      assert.match(sent, /person = 2/);
      assert.match(sent, /countries = fr/);
      assert.match(sent, /countries = jp/);
      assert.deepEqual(problems, []);
      await close();
    });

    test("A form drawn from a struct with JavaScript off posts the same", async () => {
      const { page, close } = await open("/select/derive", { js: false });
      await page.locator("#derived-stage-picker [data-slot=select-native]").selectOption("won");
      await page.locator("#derived-owner-picker [data-slot=select-native]").selectOption("it");
      await page.locator("#derived-countries-picker [data-slot=select-native]").selectOption(["fr", "de"]);
      await page.locator("#derived button[type=submit]").click();
      await page.waitForSelector("[data-echo]");
      const sent = await page.locator("[data-echo]").innerText();
      assert.match(sent, /stage = won/);
      assert.match(sent, /owner = it/);
      assert.match(sent, /countries = fr/);
      assert.match(sent, /countries = de/);
      await close();
    });

    test("A form the server refuses: the errors land on the pickers the person uses, and the first takes focus", async () => {
      const { page, close } = await open("/select/derive");
      // Take the browser's own check away, so the server is the one to say no.
      await page.evaluate(() => document.querySelector("#derived").setAttribute("novalidate", ""));
      await page.locator("#derived button[type=submit]").click();
      await page.waitForFunction(() => document.querySelector("#derived-stage")?.getAttribute("aria-invalid") === "true");
      assert.equal(await page.locator("#derived-owner").getAttribute("aria-invalid"), "true");
      assert.match(await page.locator("[data-slot=field]", { has: page.locator("#derived-stage") }).innerText(), /Choose a stage\./);
      // The first of the fields in the answer takes focus: the controls a person uses, not the hidden selects.
      assert.ok(["derived-owner", "derived-stage"].includes(await page.evaluate(() => document.activeElement.id)));
      await page.waitForTimeout(300);
      assert.equal(await page.locator("#derived-stage").evaluate((e) => getComputedStyle(e).borderColor), await page.locator("#derived-owner").evaluate((e) => getComputedStyle(e.closest("[data-slot=combobox-control]")).borderColor), "the same red on both");
      await close();
    });

    test("Tabbing through a form opens no list", async () => {
      const { page, close } = await open("/select/form");
      await page.locator("h1").first().click();
      for (let i = 0; i < 7; i++) {
        await page.keyboard.press("Tab");
        assert.equal(await page.locator("[data-open]").count(), 0, "a list opened on focus");
      }
      await close();
    });

    test("MultiSelect: a clear button takes every chip off", async () => {
      const { page, picker, close } = await open("/select");
      const m = picker("multi-summary");
      assert.deepEqual((await m.value()).length, 3);
      await page.locator("#multi-summary [data-slot=combobox-clear]").click();
      assert.deepEqual(await m.value(), []);
      assert.equal(await m.chips.count(), 0);
      assert.equal(await page.locator("#multi-summary [data-slot=chip-summary]").count(), 0);
      await close();
    });

    test("Palette: Down at the last item wraps to the first, and the pointer makes an item active", async () => {
      const { page, close } = await open("/select");
      await page.keyboard.press("Control+k");
      await page.keyboard.press("ArrowUp");
      const last = await page.locator("#palette-list [data-active]").innerText();
      assert.match(last, /New deal|Gallery|Open the dialog|Post an action/, last);
      assert.ok(!/Billing/.test(last), "a disabled item is never active");
      await page.keyboard.press("ArrowDown");
      assert.match(await page.locator("#palette-list [data-active]").innerText(), /^Deals/);
      await page.locator("#palette-list [role=option]", { hasText: "Gallery" }).hover();
      assert.match(await page.locator("#palette-list [data-active]").innerText(), /^Gallery/);
      await close();
    });

    // ------------------------------------------------------------ axe, every state

    /** Each state is what a person sees at one moment: a list open, an error, a hint. */
    const STATES = {
      "closed, everything at rest": async () => {},
      "a select open": async ({ page }) => { await page.locator("#f-select-basic").click(); },
      "a select open with groups and a disabled option": async ({ page }) => { await page.locator("#f-select-groups").click(); },
      "a select open with descriptions, an icon and a picture": async ({ page }) => { await page.locator("#f-select-rich").click(); },
      "a select open with nothing in it": async ({ page }) => { await page.locator("#f-select-empty").click(); },
      "a combobox open": async ({ page }) => { await page.locator("#f-combo-basic").click(); },
      "a combobox typed into, matches highlighted": async ({ page }) => { await page.locator("#f-combo-basic").click(); await page.keyboard.type("an"); },
      "a combobox with no match": async ({ page }) => { await page.locator("#f-combo-basic").click(); await page.keyboard.type("qqqq"); },
      "a combobox with a create row": async ({ page }) => { await page.locator("#f-combo-free").click(); await page.keyboard.type("Prospect"); },
      "a combobox whose create row is off": async ({ page }) => { await page.locator("#f-combo-denied").click(); await page.keyboard.type("Yellow"); },
      "a combobox answered by the server": async ({ page }) => {
        await page.locator("#f-combo-search").click();
        await page.waitForFunction(() => document.querySelectorAll("#combo-search-list [role=option]:not([data-action])").length === 20);
      },
      "a combobox whose search failed": async ({ page }) => {
        await page.route("**/select/people?*", (route) => route.fulfill({ status: 500, body: "no" }));
        await page.locator("#f-combo-search").click();
        await page.waitForFunction(() => /Couldn't load/.test(document.querySelector("#combo-search [data-slot=select-note]").textContent));
      },
      "a combobox while it searches": async ({ page }) => {
        await page.route("**/select/people?*", async (route) => { await new Promise((r) => setTimeout(r, 1500)); await route.continue(); });
        await page.locator("#f-combo-search").click();
        await page.waitForFunction(() => document.querySelector("#combo-search-list").getAttribute("aria-busy") === "true");
      },
      "a multi-select open": async ({ page }) => { await page.locator("#f-multi-basic").click(); },
      "a multi-select at its maximum": async ({ page }) => { await page.locator("#f-multi-max").click(); await page.keyboard.type("ja"); await page.keyboard.press("Enter"); },
      "a multi-select with select all": async ({ page }) => { await page.locator("#f-multi-all").click(); },
      "a chip with focus": async ({ page }) => { await page.locator("#f-multi-basic").focus(); await page.keyboard.press("Backspace"); },
      "the palette open": async ({ page }) => { await page.keyboard.press("Control+k"); },
      "the palette with a search": async ({ page }) => { await page.keyboard.press("Control+k"); await page.keyboard.type("deal"); },
      "the palette with nothing found": async ({ page }) => {
        await page.keyboard.press("Control+k");
        await page.keyboard.type("zzzzqq");
        await page.waitForFunction(() => /No results/.test(document.querySelector("#palette [data-slot=select-note]").textContent));
      },
      "the palette with results from the server": async ({ page }) => {
        await page.keyboard.press("Control+k");
        await page.keyboard.type("gauri");
        await page.waitForFunction(() => document.querySelector("#palette [data-slot=command-remote] [role=option]"));
      },
      "a combobox in a dialog": async ({ page }) => {
        await page.locator("button[command=show-modal][commandfor=pick-dialog]").click();
        await page.locator("#f-dlg-combo").click();
      },
    };

    for (const [name, setup] of Object.entries(STATES)) {
      for (const scheme of ["dark", "light"]) {
        test(`axe and the content security policy, ${scheme}: ${name}`, { timeout: 60000 }, async () => {
          const ctx = await open("/select", { light: scheme === "light" });
          await setup(ctx);
          await ctx.page.waitForTimeout(250);
          assert.deepEqual(await axe(ctx.page), []);
          assert.deepEqual(ctx.problems, []);
          await ctx.close();
        });
      }
    }

    test("axe: the right-to-left page and the form that posts", async () => {
      const rtl = await open("/select");
      await rtl.page.locator("#f-rtl-select").scrollIntoViewIfNeeded();
      await rtl.page.locator("#f-rtl-multi").click();
      await rtl.page.waitForTimeout(300);
      assert.deepEqual(await axe(rtl.page), []);
      await rtl.close();
      const form = await open("/select/form");
      await form.page.locator("#pick-form button[type=submit]").click();
      assert.deepEqual(await axe(form.page), [], "with the errors showing");
      await form.close();
    });

    test("forced colours: the active row and the borders don't depend on colour alone", async () => {
      const context = await browser.newContext({ forcedColors: "active", viewport: { width: 1440, height: 900 } }).catch(() => null);
      if (!context) return;
      const page = await context.newPage();
      await page.goto(BASE + "/select");
      await page.locator("#f-select-basic").click();
      const style = await page.locator("#select-basic-list [data-active]").evaluate((e) => getComputedStyle(e).backgroundColor);
      assert.notEqual(style, "rgba(0, 0, 0, 0)", "the active row is drawn");
      await context.close();
    });

    test("reduced motion: nothing animates", async () => {
      const context = await browser.newContext({ reducedMotion: "reduce", viewport: { width: 1440, height: 900 } });
      const page = await context.newPage();
      await page.goto(BASE + "/select");
      await page.locator("#f-select-basic").click();
      const duration = await page.locator("#select-basic-popup").evaluate((e) => getComputedStyle(e).animationDuration);
      assert.ok(parseFloat(duration) <= 0.001, duration);
      await context.close();
    });

    test("every state, with screenshots, at 1440px and 390px, in dark and light", { skip: !SHOTS, timeout: 300000 }, async () => {
      for (const [scheme, viewport, tag] of [["dark", { width: 1440, height: 900 }, "1440"], ["light", { width: 1440, height: 900 }, "1440"], ["dark", { width: 390, height: 844 }, "390"], ["light", { width: 390, height: 844 }, "390"]]) {
        for (const [name, setup] of Object.entries(STATES)) {
          const ctx = await open("/select", { light: scheme === "light", viewport, touch: tag === "390" });
          await setup(ctx);
          await shot(ctx.page, `${scheme}-${tag}-${name.replace(/[^a-z0-9]+/gi, "-").toLowerCase()}`);
          await ctx.close();
        }
      }
    });
  });
}
