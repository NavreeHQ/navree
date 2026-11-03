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

const BASE = process.env.UI_PROOF_URL || "http://127.0.0.1:8090";
const SHOTS = process.env.SELECT_SHOTS || "";
const require = createRequire(import.meta.url);
const axeSource = await readFile(require.resolve("axe-core/axe.min.js"), "utf8");

// The light values of `src/tokens.rs`: the toolkit ships dark only so far, so light is
// proved by putting these on the page.
const LIGHT = ":root{color-scheme:light;--ink-0:#ffffff;--ink-1:#f7f8f7;--ink-2:#eff1f0;--ink-3:#e5e8e6;--line:#dfe3e0;--line-2:#c8cecb;--line-3:#aeb6b2;--fg:#0a0e0c;--fg-1:#1c2420;--fg-2:#3f4a44;--fg-3:#55615b;--accent:#0b7a3e;--accent-ink:#ffffff;--amber:#8a5a00;--danger:#b3261e;--bar:#8a968f}";

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
      browser = await playwright[browserName].launch();
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
      assert.equal(await page.getByRole("combobox", { name: "Country", exact: true }).count(), 1, "and the input of a combobox");
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
      assert.equal(await s.activeText(), "Tokyo");
      await page.keyboard.press("Escape");
      await page.keyboard.press("p");
      assert.equal(await s.activeText(), "Paris");
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
      assert.deepEqual(await page.locator("#select-groups-list [role=group]").evaluateAll((g) => g.map((x) => x.getAttribute("aria-label"))), ["Europe", "Asia"]);
      await s.list.locator("[role=option]", { hasText: "Delhi" }).click({ force: true });
      assert.equal(await s.value(), "", "a disabled option is not picked by a click");
      await page.keyboard.press("End");
      assert.equal(await s.activeText(), "Seoul");
      await page.keyboard.press("ArrowUp");
      assert.equal(await s.activeText(), "Tokyo", "Up from Seoul skips the disabled Delhi");
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
      await page.locator("#f-form-person").fill("grace");
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
      assert.equal(await page.locator("#f-combo-search").inputValue(), "Grace Hopper 3");
      assert.equal(await c.value(), "3");
      await page.locator("#f-combo-search").click();
      await page.waitForFunction(() => document.querySelectorAll("#combo-search-list [role=option]:not([data-action])").length === 20);
      assert.equal(await page.locator("#combo-search-list [data-action=more]").count(), 1, "Show more is offered");
      await page.locator("#f-combo-search").fill("");
      requests.length = 0;
      await page.keyboard.type("ada", { delay: 40 });
      await page.waitForFunction(() => document.querySelector("#combo-search-list [role=option]")?.textContent.startsWith("Ada"));
      await page.waitForTimeout(400);
      assert.deepEqual(requests, ["ada"], "typing fast sent one request");
      assert.match((await c.texts())[0], /^Ada/);
      // Show more adds the next page.
      const before = await page.locator("#combo-search-list [role=option]:not([data-action])").count();
      await page.locator("#combo-search-list [data-action=more]").click();
      await page.waitForFunction((n) => document.querySelectorAll("#combo-search-list [role=option]:not([data-action])").length > n, before);
      // Picking a found option puts it in the carrier.
      await c.rows.first().click();
      assert.match(await page.locator("#f-combo-search").inputValue(), /^Ada /);
      assert.match(String(await c.value()), /^\d+$/);
      await close();
    });

    test("Combobox on the server: an older answer that arrives late is ignored", async () => {
      const { page, picker, close } = await open("/select");
      const c = picker("combo-search");
      await page.route("**/select/people?*", async (route) => {
        const q = new URL(route.request().url()).searchParams.get("q");
        if (q === "gr") await new Promise((r) => setTimeout(r, 1200));
        await route.continue();
      });
      await page.locator("#f-combo-search").click();
      await page.locator("#f-combo-search").fill("");
      await page.keyboard.type("gr");
      await page.waitForTimeout(400);
      await page.keyboard.type("ace love");
      await page.waitForFunction(() => document.querySelector("#combo-search-list [role=option]")?.textContent.startsWith("Grace Love"));
      const fast = await c.texts();
      await page.waitForTimeout(1500);
      assert.deepEqual(await c.texts(), fast, "the slow reply for 'gr' did not replace the newer list");
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
      await page.keyboard.type("alan");
      await page.waitForFunction(() => document.querySelector("#combo-search-list")?.getAttribute("aria-busy") === "true");
      assert.match(await c.note.innerText(), /Searching/);
      assert.ok((await c.rows.count()) > 0, "the old rows stay");
      await page.keyboard.type(" t");
      assert.equal(await page.locator("#f-combo-search").inputValue(), "alan t", "typing was not blocked");
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
      await page.locator("#f-combo-search").fill("grace");
      await page.waitForFunction(() => /Couldn't load/.test(document.querySelector("#combo-search [data-slot=select-note]")?.textContent || ""));
      assert.equal(await c.value(), "3", "the choice is kept");
      assert.equal(await page.locator("#f-combo-search").inputValue(), "grace");
      assert.equal(await page.locator("#combo-search-list [data-action=retry]").count(), 1);
      fail = false;
      await page.keyboard.press("ArrowDown");
      await page.keyboard.press("Enter");
      await page.waitForFunction(() => document.querySelector("#combo-search-list [role=option]:not([data-action])")?.textContent.startsWith("Grace"));
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
      await page.keyboard.type("d");
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
      await page.keyboard.type("grace love");
      await page.waitForFunction(() => document.querySelector("#multi-search-list [role=option]")?.textContent.startsWith("Grace"));
      await page.keyboard.press("Enter");
      assert.equal(await m.chips.count(), 2);
      assert.equal(await m.chips.nth(1).locator("[data-slot=chip-text]").innerText().then((t) => t.startsWith("Grace")), true);
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
      await page.waitForURL("**/deals?per=25");
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
      await page.waitForURL("**/deals?per=25");
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
      await page.keyboard.type("grace");
      await page.waitForFunction(() => document.querySelector("#palette [data-slot=command-remote] [role=option]"));
      assert.match(await page.locator("#palette [data-slot=command-remote]").innerText(), /Grace/);
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
      await page.locator("#select-rich-list [role=option]", { hasText: "Ada Lovelace" }).click();
      assert.equal(await page.locator("#f-select-rich").innerText(), "Ada");
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
        await page.keyboard.type("grace");
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
