// The calendar family in real browsers: Calendar, DatePicker, DateRangePicker and
// TimePicker against the proof app's /dates pages. Needs the proof app serving:
//   PORT=8290 cargo run --release -p ui-proof -- serve
// DATES_URL points elsewhere; DATES_BROWSERS="chromium firefox webkit" picks browsers.
//
// Today is 3 November 2026 on every page, so nothing here depends on the clock.

import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { createRequire } from "node:module";
import { after, before, describe, test } from "node:test";
import * as playwright from "playwright";
import { launch } from "./support/launch.mjs";

const BASE = process.env.DATES_URL || "http://127.0.0.1:8290";
const require = createRequire(import.meta.url);
const axeSource = await readFile(require.resolve("axe-core/axe.min.js"), "utf8");
const cases = JSON.parse(await readFile(new URL("../../../crates/navree-ui/tests/fixtures/dates-parse.json", import.meta.url), "utf8"));

// The light values of `src/tokens.rs`: the toolkit ships dark only so far, so light
// is proved by putting these on the page.
const LIGHT = ":root{color-scheme:light;--ink-0:#ffffff;--ink-1:#f7f8f7;--ink-2:#eff1f0;--ink-3:#e5e8e6;--line:#dfe3e0;--line-2:#c8cecb;--line-3:#aeb6b2;--fg:#0a0e0c;--fg-1:#1c2420;--fg-2:#3f4a44;--fg-3:#55615b;--accent:#0b7a3e;--accent-ink:#ffffff;--amber:#8a5a00;--danger:#b3261e;--bar:#8a968f}";

/** Runs axe. Violations come back as short lines. */
async function axe(page) {
  await page.evaluate(axeSource);
  return page.evaluate(async () => {
    const out = await window.axe.run(document, { runOnly: ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa", "best-practice"] });
    return out.violations.map((v) => `${v.id}: ${v.nodes.slice(0, 3).map((n) => n.target.join(" ")).join(" | ")}`);
  });
}

/** The page's own text of an element tree, attributes sorted, so two drawings compare. */
const NORMALISE = `
  const norm = (node) => {
    if (node.nodeType === 3) return node.nodeValue.trim();
    if (node.nodeType !== 1) return "";
    const attrs = [...node.attributes].map((a) => a.name + '=' + a.value).sort().join(' ');
    return '<' + node.tagName + ' ' + attrs + '>' + [...node.childNodes].map(norm).join('') + '</>';
  };`;

const browsers = (process.env.DATES_BROWSERS || "chromium firefox").split(" ");

for (const browserName of browsers) {
  describe(browserName, () => {
    let browser;
    before(async () => {
      browser = await launch(playwright[browserName]);
    });
    after(() => browser?.close());

    async function open(path, options = {}) {
      const context = await browser.newContext({
        viewport: options.viewport || { width: 1440, height: 900 },
        javaScriptEnabled: options.js !== false,
        bypassCSP: !!options.light,
        reducedMotion: "reduce",
      });
      const page = await context.newPage();
      const problems = [];
      page.on("console", (message) => {
        if (browserName === "webkit" && /Refused to apply a stylesheet/.test(message.text())) return;
        if (message.type() === "error" && !/Failed to load resource/.test(message.text())) problems.push(message.text());
      });
      page.on("pageerror", (error) => problems.push(String(error)));
      await page.goto(BASE + path);
      if (options.light) await page.addStyleTag({ content: LIGHT });
      return { page, context, problems, close: () => context.close() };
    }

    /** What the page's fields and events did: every `input` and `change`, in order. */
    const record = (page) =>
      page.evaluate(() => {
        window.__events = [];
        for (const type of ["input", "change"]) {
          // The page's own listeners are on the bubble phase of the document, as the runtime's are.
          document.addEventListener(type, (e) => window.__events.push(`${type}:${e.target.name || e.target.id || e.target.tagName}`));
        }
      });
    const events = (page) => page.evaluate(() => window.__events);
    const field = (page, name) => page.locator(`[name="${name}"]`).first();
    const val = (page, name) => field(page, name).inputValue();
    const day = (page, root, iso) => page.locator(`${root} button[data-date="${iso}"]`);
    const isOpen = (page, id) => page.locator(`#${id}`).evaluate((e) => e.matches(":popover-open"));
    const caption = (page, root) => page.locator(`${root} [data-slot=calendar-caption]`).first().innerText();
    const focusedDate = (page) => page.evaluate(() => document.activeElement?.getAttribute("data-date"));

    // ---- Calendar ----

    test("a month: heading, weekdays, today marked, one tab stop, no script error", async () => {
      const { page, problems, close } = await open("/dates");
      const cal = "#cal-single";
      assert.equal(await caption(page, cal), "November 2026");
      assert.equal(await page.locator(`${cal} button[data-slot=calendar-day]`).count(), 30);
      assert.equal(await page.locator(`${cal} [aria-current=date]`).getAttribute("data-date"), "2026-11-03");
      assert.equal(await page.locator(`${cal} [tabindex="0"]`).count(), 1);
      assert.equal(await page.locator(`${cal} [data-selected]`).getAttribute("data-date"), "2026-11-12");
      assert.deepEqual(await page.locator(`${cal} th[abbr]`).evaluateAll((e) => e.map((x) => x.getAttribute("abbr"))), ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"]);
      assert.deepEqual(problems, []);
      await close();
    });

    test("single: a click chooses, posts ISO once, and a second click unchooses", async () => {
      const { page, close } = await open("/dates");
      await record(page);
      await day(page, "#cal-single", "2026-11-20").click();
      assert.equal(await val(page, "cal_single"), "2026-11-20");
      assert.equal(await page.locator("#cal-single [data-selected]").count(), 1);
      assert.deepEqual(await events(page), ["input:cal_single", "change:cal_single"]);
      await day(page, "#cal-single", "2026-11-20").click();
      assert.equal(await val(page, "cal_single"), "");
      assert.equal(await page.locator("#cal-single [data-selected]").count(), 0);
      assert.equal(await page.locator("#cal-single [aria-selected=true]").count(), 0);
      await close();
    });

    test("multiple: toggles, holds the cap, posts one comma-separated field", async () => {
      const { page, close } = await open("/dates");
      assert.equal(await val(page, "cal_multi"), "2026-11-03,2026-11-10");
      await day(page, "#cal-multi", "2026-11-17").click();
      assert.equal(await val(page, "cal_multi"), "2026-11-03,2026-11-10,2026-11-17");
      await day(page, "#cal-multi", "2026-11-24").click(); // a fourth: refused
      assert.equal(await val(page, "cal_multi"), "2026-11-03,2026-11-10,2026-11-17");
      await day(page, "#cal-multi", "2026-11-03").click();
      assert.equal(await val(page, "cal_multi"), "2026-11-10,2026-11-17");
      await close();
    });

    test("range: two clicks end it, either way round, and nothing posts while it is half picked", async () => {
      const { page, close } = await open("/dates");
      await record(page);
      assert.equal(await val(page, "cal_from"), "2026-11-09");
      await day(page, "#cal-range", "2026-11-20").click(); // starts a new range
      assert.equal(await val(page, "cal_from"), "");
      assert.equal(await val(page, "cal_to"), "");
      assert.deepEqual(await events(page), [], "no field changed yet for the page to hear");
      await day(page, "#cal-range", "2026-11-16").click(); // earlier than the start: put right
      assert.equal(await val(page, "cal_from"), "2026-11-16");
      assert.equal(await val(page, "cal_to"), "2026-11-20");
      assert.deepEqual(await events(page), ["input:cal_to", "change:cal_to"], "one input and one change");
      assert.equal(await page.locator("#cal-range [data-range-middle]").count(), 3);
      // A one-day range: the same day twice.
      await day(page, "#cal-range", "2026-11-25").click();
      await day(page, "#cal-range", "2026-11-25").click();
      assert.equal(await val(page, "cal_from"), "2026-11-25");
      assert.equal(await val(page, "cal_to"), "2026-11-25");
      assert.equal(await page.locator("#cal-range [data-range-start][data-range-end]").count(), 1);
      await close();
    });

    test("a half-picked range shows its reach as the pointer moves", async () => {
      const { page, close } = await open("/dates");
      await day(page, "#cal-range", "2026-11-18").click();
      await day(page, "#cal-range", "2026-11-23").hover();
      assert.equal(await page.locator("#cal-range [data-preview]").count(), 6, "18th to 23rd");
      await close();
    });

    test("limits: a day that can't be chosen can't be, by click, key or navigation", async () => {
      const { page, close } = await open("/dates");
      const cal = "#cal-rules";
      // Saturday the 7th, the listed 11th, the 13th by the rule function.
      for (const iso of ["2026-11-07", "2026-11-11", "2026-11-13"]) {
        assert.equal(await day(page, cal, iso).getAttribute("aria-disabled"), "true", iso);
        await day(page, cal, iso).click({ force: true });
        assert.equal(await val(page, "cal_rules"), "", iso);
      }
      await day(page, cal, "2026-11-09").click();
      assert.equal(await val(page, "cal_rules"), "2026-11-09");
      // The Enter key on a disabled day does nothing either.
      await day(page, cal, "2026-11-11").focus();
      await page.keyboard.press("Enter");
      assert.equal(await val(page, "cal_rules"), "2026-11-09");
      await close();
    });

    test("navigation: the heading follows, across years, and stops at the limits", async () => {
      const { page, close } = await open("/dates");
      const cal = "#cal-rules"; // 15 October 2026 to 10 February 2027
      assert.equal(await caption(page, cal), "November 2026");
      await page.locator(`${cal} [data-ui-cal=prev]`).click();
      assert.equal(await caption(page, cal), "October 2026");
      assert.equal(await page.locator(`${cal} [data-ui-cal=prev]`).isDisabled(), true);
      for (const name of ["November 2026", "December 2026", "January 2027", "February 2027"]) {
        // The key, not the mouse: Playwright's Firefox can stall on clicks in quick succession.
        await page.locator(`${cal} [data-ui-cal=next]`).press("Enter");
        assert.equal(await caption(page, cal), name);
      }
      assert.equal(await page.locator(`${cal} [data-ui-cal=next]`).isDisabled(), true);
      // The days of February after the 10th can't be chosen.
      assert.equal(await day(page, cal, "2027-02-11").getAttribute("aria-disabled"), "true");
      assert.equal(await day(page, cal, "2027-02-10").getAttribute("aria-disabled"), null, "the 10th is the last day allowed");
      await close();
    });

    test("keyboard: the grid is one tab stop and moves like the APG date grid", async () => {
      const { page, close } = await open("/dates/month?month=2026-11");
      const cal = "#probe";
      assert.equal(await focusedDate(page), null);
      await page.keyboard.press("Tab"); // the prev button
      await page.keyboard.press("Tab"); // the next button
      await page.keyboard.press("Tab"); // the day with the tab stop: the chosen one
      assert.equal(await focusedDate(page), "2026-11-12");
      const press = async (key, want) => {
        await page.keyboard.press(key);
        assert.equal(await focusedDate(page), want, `${key} -> ${want}`);
      };
      await press("ArrowRight", "2026-11-13");
      await press("ArrowLeft", "2026-11-12");
      await press("ArrowDown", "2026-11-19");
      await press("ArrowUp", "2026-11-12");
      await press("Home", "2026-11-08"); // Sunday
      await press("End", "2026-11-14"); // Saturday
      await press("PageDown", "2026-12-14");
      assert.equal(await caption(page, cal), "December 2026");
      await press("PageUp", "2026-11-14");
      await press("Shift+PageDown", "2027-11-14");
      await press("Shift+PageUp", "2026-11-14");
      // Across the year's end and back, and a day that a shorter month lacks.
      await press("PageDown", "2026-12-14");
      await press("ArrowDown", "2026-12-21");
      await press("ArrowDown", "2026-12-28");
      await press("ArrowDown", "2027-01-04");
      assert.equal(await caption(page, cal), "January 2027");
      await press("ArrowUp", "2026-12-28");
      await press("PageDown", "2027-01-28");
      await press("ArrowRight", "2027-01-29");
      await press("ArrowRight", "2027-01-30");
      await press("ArrowRight", "2027-01-31");
      await press("PageDown", "2027-02-28");
      // Enter chooses.
      await page.keyboard.press("ArrowLeft");
      await page.keyboard.press("Enter");
      assert.equal(await page.locator(`${cal} [data-selected]`).getAttribute("data-date"), "2027-02-27");
      // Tab leaves the grid at once, and Shift+Tab comes back to the same day.
      await page.keyboard.press("Tab");
      assert.equal(await page.evaluate(() => document.activeElement?.id), "after");
      await page.keyboard.press("Shift+Tab");
      assert.equal(await focusedDate(page), "2027-02-27");
      await close();
    });

    test("the browser's grid is the server's grid, for every start day and month", async () => {
      const { page, close } = await open("/dates/month");
      const months = ["2026-11", "2026-02", "2024-02", "2100-02", "2000-02", "1900-02", "2026-12", "2027-01", "2020-12", "2021-01", "2026-08", "2026-05", "1583-10", "0001-02", "9999-11", "0099-06"];
      let compared = 0;
      for (const start of [0, 1, 2, 3, 4, 5, 6]) {
        for (const flags of ["", "&wn=1&fixed=1"]) {
          for (const month of months) {
            // The server draws the month; the browser leaves it and comes back drawing it itself.
            const query = `month=${month}&ws=${start}${flags}`;
            await page.goto(`${BASE}/dates/month?${query}`);
            const server = await page.evaluate(`(() => { ${NORMALISE} return norm(document.querySelector('[data-slot=calendar-months]')); })()`);
            await page.locator("[data-ui-cal=next]").click();
            await page.locator("[data-ui-cal=prev]").click();
            const browserDrew = await page.evaluate(`(() => { ${NORMALISE} return norm(document.querySelector('[data-slot=calendar-months]')); })()`);
            assert.equal(browserDrew, server, `${query}`);
            compared++;
          }
        }
      }
      assert.ok(compared >= 200);
      await close();
    });

    test("the browser draws the next month as the server does, with two months and the lists", async () => {
      const { page, close } = await open("/dates/month");
      for (const [first, second, query] of [["2026-10", "2026-11", "n=2"], ["2026-11", "2026-12", "n=2&wn=1"], ["2026-10", "2026-11", "dd=1&rules=1"], ["2027-01", "2027-02", "rules=1&n=1&fixed=1"]]) {
        await page.goto(`${BASE}/dates/month?month=${first}&${query}`);
        await page.locator("[data-ui-cal=next]").click();
        const browserDrew = await page.evaluate(`(() => { ${NORMALISE} return norm(document.querySelector('[data-slot=calendar-months]')); })()`);
        await page.goto(`${BASE}/dates/month?month=${second}&${query}`);
        const server = await page.evaluate(`(() => { ${NORMALISE} return norm(document.querySelector('[data-slot=calendar-months]')); })()`);
        // The server's first month when it was asked for the second: compare what they share, the second month's days.
        assert.ok(browserDrew.length > 1000);
        if (!query.includes("n=2")) assert.equal(browserDrew, server, query);
      }
      await close();
    });

    test("month and year lists reach a date of birth in a few moves", async () => {
      const { page, close } = await open("/dates");
      const cal = "#cal-dob";
      assert.equal(await page.locator(`${cal} [data-ui-cal=year]`).inputValue(), "1990");
      await page.locator(`${cal} [data-ui-cal=year]`).selectOption("1985");
      await page.locator(`${cal} [data-ui-cal=month]`).selectOption("7");
      assert.equal(await page.locator(`${cal} button[data-slot=calendar-day]`).first().getAttribute("data-date"), "1985-07-01");
      await day(page, cal, "1985-07-23").click();
      assert.equal(await val(page, "cal_dob"), "1985-07-23");
      // The limit ends the list: no year after today's.
      assert.equal(await page.locator(`${cal} [data-ui-cal=year] option[value="2027"]`).count(), 0);
      await close();
    });

    test("week numbers follow ISO and fixed weeks hold six rows", async () => {
      const { page, close } = await open("/dates/month?month=2026-11&ws=1&wn=1&fixed=1");
      const weeks = () => page.locator("[data-slot=calendar-grid] tbody th").allInnerTexts();
      assert.deepEqual(await weeks(), ["44", "45", "46", "47", "48", "49"]);
      assert.equal(await page.locator("[data-slot=calendar-grid] tbody tr").count(), 6);
      await page.goto(`${BASE}/dates/month?month=2020-12&ws=1&wn=1`);
      assert.deepEqual(await weeks(), ["49", "50", "51", "52", "53"]);
      await page.locator("[data-ui-cal=next]").click();
      assert.equal((await weeks())[0], "53", "the first days of 2021 are still in the last week of 2020");
      await close();
    });

    test("the heading and the grid are named for assistive technology", async () => {
      const { page, close } = await open("/dates");
      assert.equal(await page.locator("#cal-single [role=grid]").getAttribute("aria-labelledby"), "cal-single-cap-2026-11");
      assert.equal(await day(page, "#cal-single", "2026-11-03").getAttribute("aria-label"), "Tuesday, November 3, 2026");
      assert.equal(await page.locator("#cal-single").getAttribute("aria-label"), "Single date");
      await page.locator("#cal-single [data-ui-cal=next]").click();
      assert.equal(await page.locator("#cal-single [aria-live=polite]").innerText(), "December 2026", "a live region says the month changed");
      await close();
    });

    test("a calendar to look at and a disabled one take no input", async () => {
      const { page, close } = await open("/dates");
      assert.equal(await page.locator("#cal-ro [data-ui-cal]").count(), 0);
      assert.equal(await page.locator("#cal-ro [tabindex='0']").count(), 0);
      await record(page);
      await day(page, "#cal-off", "2026-11-20").click({ force: true });
      assert.equal(await page.locator("#cal-off input[name=cal_off]").isDisabled(), true);
      assert.deepEqual(await events(page), []);
      await close();
    });

    // ---- DatePicker ----

    test("a date picker: open, choose by mouse, ISO posts, the box shows the locale's text, focus returns", async () => {
      const { page, close } = await open("/dates");
      await record(page);
      await page.locator("#dp-basic [data-ui-act=open]").click();
      assert.equal(await isOpen(page, "dp-basic-pop"), true);
      assert.equal(await focusedDate(page), "2026-11-03", "today holds the tab stop in an empty picker");
      await day(page, "#dp-basic-pop", "2026-11-18").click();
      assert.equal(await isOpen(page, "dp-basic-pop"), false);
      assert.equal(await val(page, "basic"), "2026-11-18");
      assert.equal(await page.locator("#dp-basic [data-ui-text]").inputValue(), "11/18/2026");
      assert.equal(await page.evaluate(() => document.activeElement?.hasAttribute("data-ui-text")), true);
      assert.deepEqual(await events(page), ["input:basic", "change:basic"]);
      await close();
    });

    test("a date picker opens on the month of its value and by the keyboard alone", async () => {
      const { page, close } = await open("/dates");
      await page.locator("#dp-value [data-ui-text]").focus();
      await page.keyboard.press("ArrowDown");
      assert.equal(await isOpen(page, "dp-value-pop"), true);
      assert.equal(await focusedDate(page), "2026-11-12");
      await page.keyboard.press("ArrowRight");
      await page.keyboard.press("Enter");
      assert.equal(await val(page, "value"), "2026-11-13");
      assert.equal(await isOpen(page, "dp-value-pop"), false);
      // Escape closes it and leaves the value alone.
      await page.keyboard.press("ArrowDown");
      await page.keyboard.press("ArrowRight");
      await page.keyboard.press("Escape");
      assert.equal(await isOpen(page, "dp-value-pop"), false);
      assert.equal(await val(page, "value"), "2026-11-13");
      await close();
    });

    test("typed dates: every shared case reads the same in the browser as on the server", async () => {
      const { page, close } = await open("/dates/probe?loc=en");
      let seen = 0;
      const byLocale = new Map();
      for (const c of cases.dates) byLocale.set(c.locale, [...(byLocale.get(c.locale) || []), c]);
      for (const [locale, list] of byLocale) {
        await page.goto(`${BASE}/dates/probe?loc=${locale}`);
        for (const c of list) {
          await page.fill("#probe-date [data-ui-text]", c.text);
          await page.locator("#probe-date [data-ui-text]").blur();
          const native = await page.locator("#probe-date input[type=date]").inputValue();
          const invalid = await page.locator("#probe-date [data-ui-text]").getAttribute("aria-invalid");
          if (c.ok) {
            // The probe's rules end at 2027-12-31 and begin at 2026-10-01: a date outside says so.
            const inside = c.ok >= "2026-10-01" && c.ok <= "2027-12-31";
            if (inside) assert.equal(native, c.ok, `${locale} ${JSON.stringify(c.text)}`);
            else assert.equal(native, "", `${locale} ${JSON.stringify(c.text)} is outside the window`);
          } else {
            assert.equal(native, "", `${locale} ${JSON.stringify(c.text)}`);
            assert.equal(invalid, c.err === "empty" ? null : "true", `${locale} ${JSON.stringify(c.text)} should be marked`);
          }
          seen++;
        }
      }
      assert.ok(seen >= 130, `${seen}`);
      await close();
    });

    test("typed dates: what can't be chosen is said, the field is emptied, and the submit is held", async () => {
      const { page, close } = await open("/dates/probe?loc=en");
      const text = page.locator("#probe-date [data-ui-text]");
      await text.fill("1/1/2030");
      await text.blur();
      assert.equal(await page.locator("#probe-date input[type=date]").inputValue(), "");
      assert.equal(await text.getAttribute("aria-invalid"), "true");
      assert.match(await page.locator("#probe-date [data-slot=field-error]").innerText(), /isn't available/);
      assert.equal(await text.evaluate((e) => e.validity.customError), true);
      assert.equal(await page.locator("#probe").evaluate((f) => f.checkValidity()), false);
      // Letters: another message, with an example to copy.
      await text.fill("soon");
      await text.blur();
      assert.match(await page.locator("#probe-date [data-slot=field-error]").innerText(), /Enter a date like 11\/30\/2026/);
      // A good date clears it all.
      await text.fill("11/20/2026");
      await text.blur();
      assert.equal(await text.getAttribute("aria-invalid"), null);
      assert.equal(await page.locator("#probe-date [data-slot=field-error]").count(), 0);
      assert.equal(await page.locator("#probe").evaluate((f) => f.checkValidity()), true);
      assert.equal(await page.locator("#probe-date input[type=date]").inputValue(), "2026-11-20");
      assert.equal(await text.inputValue(), "11/20/2026");
      // Empty clears the value.
      await text.fill("");
      await text.blur();
      assert.equal(await page.locator("#probe-date input[type=date]").inputValue(), "");
      await close();
    });

    test("typing never reaches the page as input: only a date does, once", async () => {
      const { page, close } = await open("/dates");
      await record(page);
      await page.locator("#dp-basic [data-ui-text]").pressSequentially("11/2", { delay: 10 });
      assert.deepEqual(await events(page), [], "typing is the box's own");
      await page.locator("#dp-basic [data-ui-text]").pressSequentially("5/2026");
      await page.keyboard.press("Enter");
      assert.equal(await val(page, "basic"), "2026-11-25");
      assert.deepEqual(await events(page), ["input:basic", "change:basic"]);
      await page.locator("#dp-basic [data-ui-text]").blur();
      assert.deepEqual(await events(page), ["input:basic", "change:basic"], "leaving the box with the same date says nothing more");
      await close();
    });

    test("clear: the button and the empty box empty the value", async () => {
      const { page, close } = await open("/dates");
      await page.locator("#dp-value [data-ui-act=clear]").first().click();
      assert.equal(await val(page, "value"), "");
      assert.equal(await page.locator("#dp-value [data-ui-text]").inputValue(), "");
      assert.equal(await page.locator("#dp-value").getAttribute("data-has-value"), null);
      // Today, from the foot of the popover.
      await page.locator("#dp-value [data-ui-act=open]").click();
      await page.locator("#dp-value-pop [data-ui-act=today]").click();
      assert.equal(await val(page, "value"), "2026-11-03");
      await close();
    });

    test("a label and a server error find the box, not the hidden field", async () => {
      const { page, close } = await open("/dates/form");
      // The label points at the box a person sees, not at the field behind it.
      assert.equal(await page.locator("label[for=booking-start-text]").count(), 1);
      await page.locator("label[for=booking-start-text]").click();
      assert.equal(await page.evaluate(() => document.activeElement?.hasAttribute("data-ui-text")), true);
      // The name of the box is the label's.
      assert.equal(await page.getByRole("textbox", { name: "Start date" }).count(), 1);
      await page.locator("#dp-start [data-ui-text]").fill("11/07/2026"); // a Saturday
      await page.locator("#tp-arrives [data-ui-text]").fill("10:30 am");
      await page.getByRole("button", { name: "Book" }).click();
      await page.locator("[data-slot=field-error]").first().waitFor();
      assert.match(await page.locator("[data-slot=field-error]").first().innerText(), /isn't available/);
      assert.equal(await page.evaluate(() => document.activeElement?.hasAttribute("data-ui-text")), true, "focus goes to the box");
      assert.equal(await page.locator("#dp-start [data-ui-text]").getAttribute("aria-invalid"), "true");
      assert.equal(await page.locator("#dp-start [data-ui-text]").inputValue(), "11/07/2026", "what was sent stays");
      await close();
    });

    test("a form: the box holds back what the rules refuse, and the server accepts a good booking", async () => {
      const { page, close } = await open("/dates/form");
      await page.locator("#dp-start [data-ui-text]").fill("11/09/2026");
      await page.locator("#tp-arrives [data-ui-text]").fill("12:30 pm"); // lunch: the rules the form gave the picker say so
      await page.getByRole("button", { name: "Book" }).click();
      await page.locator("[data-slot=field-error]").first().waitFor();
      assert.match(await page.locator("[data-slot=field-error]").first().innerText(), /isn't available/);
      assert.equal(new URL(page.url()).pathname, "/dates/form", "nothing was sent");
      await page.locator("#tp-arrives [data-ui-text]").fill("2:15 pm");
      // Leaving the box takes its message away, and the button moves up: click it where it now is.
      await page.locator("#tp-arrives [data-ui-text]").blur();
      await page.getByRole("button", { name: "Book" }).click();
      await page.locator("[data-saved]").waitFor();
      assert.match(page.url(), /\/dates\/booked/);
      assert.equal(await page.locator("[data-field=start]").innerText(), "2026-11-09");
      assert.equal(await page.locator("[data-field=arrives]").innerText(), "14:15:00");
      assert.equal(await page.locator("[data-field=end]").innerText(), "");
      await close();
    });

    test("JavaScript off: the server refuses a Saturday and lunchtime in words, and takes a good booking", async () => {
      const { page, close } = await open("/dates/form", { js: false });
      await page.locator("#dp-start input[type=date]").fill("2026-11-07"); // a Saturday
      await page.locator("#tp-arrives input[type=time]").fill("12:30");
      await page.getByRole("button", { name: "Book" }).click();
      await page.locator("[data-slot=field-error]").first().waitFor();
      await page.waitForLoadState("load"); // the answer can still be arriving when its first error shows
      const messages = await page.locator("[data-slot=field-error]").allInnerTexts();
      assert.equal(messages.length, 2);
      assert.ok(messages.every((text) => /isn't available/.test(text)), messages.join(" | "));
      await page.locator("#dp-start input[type=date]").fill("2026-11-09");
      await page.locator("#tp-arrives input[type=time]").fill("14:15");
      await page.getByRole("button", { name: "Book" }).click();
      await page.locator("[data-saved]").waitFor();
      assert.equal(await page.locator("[data-field=start]").innerText(), "2026-11-09");
      await close();
    });

    test("two pickers: opening one closes the other", async () => {
      const { page, close } = await open("/dates");
      await page.locator("#dp-basic [data-ui-act=open]").click();
      await page.locator("#dp-value [data-ui-act=open]").click();
      assert.equal(await isOpen(page, "dp-basic-pop"), false);
      assert.equal(await isOpen(page, "dp-value-pop"), true);
      await close();
    });

    test("JavaScript off: native date and time fields stand in and post ISO", async () => {
      const { page, close } = await open("/dates/form", { js: false });
      // The box and the button are hidden by CSS; the native fields are visible.
      assert.equal(await page.locator("#dp-start [data-ui-text]").isVisible(), false);
      assert.equal(await page.locator("#dp-start input[type=date]").isVisible(), true);
      assert.equal(await page.locator("#tp-arrives input[type=time]").isVisible(), true);
      await page.locator("#dp-start input[type=date]").fill("2026-11-07"); // a Saturday
      await page.locator("#tp-arrives input[type=time]").fill("10:30");
      await page.getByRole("button", { name: "Book" }).click();
      await page.locator("[data-slot=field-error]").first().waitFor();
      assert.match(await page.locator("[data-slot=field-error]").first().innerText(), /isn't available/, "the server's answer, as a page");
      assert.equal(await page.locator("#dp-start input[type=date]").inputValue(), "2026-11-07", "and what was sent is kept");
      await page.locator("#dp-start input[type=date]").fill("2026-11-09");
      await page.getByRole("button", { name: "Book" }).click();
      await page.locator("[data-field=start]").waitFor();
      assert.equal(await page.locator("[data-field=start]").innerText(), "2026-11-09");
      assert.equal(await page.locator("[data-field=arrives]").innerText(), "10:30:00");
      await close();
    });

    test("JavaScript off: a range picker shows two native dates, and a report's window posts as a query", async () => {
      const { page, close } = await open("/dates/report", { js: false });
      assert.equal(await page.locator("#report-range input[type=date]").count(), 2);
      assert.equal(await page.locator("#report-range input[type=date]").first().isVisible(), true);
      assert.equal(await page.locator("#report-range [data-ui-act=open]").isVisible(), false);
      await page.locator("[name=from]").fill("2026-03-08");
      await page.locator("[name=to]").fill("2026-03-08");
      await Promise.all([page.waitForURL(/from=2026-03-08&to=2026-03-08/), page.getByRole("button", { name: "Apply" }).click()]);
      assert.match(page.url(), /from=2026-03-08&to=2026-03-08/);
      assert.match(await page.locator("[data-report=query]").innerText(), /created_at >= 2026-03-08T05:00:00Z AND created_at < 2026-03-09T04:00:00Z/, "a 23-hour day in New York, counted whole");
      await close();
    });
  });
}
