// The range picker and the time picker in real browsers, against the proof app's /dates
// pages (see dates.test.mjs for how to serve them).

import assert from "node:assert/strict";
import { test } from "node:test";
import { BASE, cases, day, events, field, focusedDate, inBrowsers, isOpen, label, record, val } from "./support/dates.mjs";

inBrowsers(({ open }) => {
  // ---- range picker ----

  test("a preset fills the range, closes the popover, and reads short on the button", async () => {
    const { page, close } = await open("/dates");
    await record(page);
    await page.locator("#dr-a_from [data-ui-act=open]").click();
    assert.equal(await isOpen(page, "dr-a_from-pop"), true);
    await page.locator("#dr-a_from-pop [data-ui-preset=last_7_days]").click();
    assert.equal(await isOpen(page, "dr-a_from-pop"), false);
    assert.equal(await val(page, "a_from"), "2026-10-28");
    assert.equal(await val(page, "a_to"), "2026-11-03");
    assert.equal(await label(page, "#dr-a_from"), "Oct 28 – Nov 3");
    assert.equal(await page.locator("#dr-a_from [data-ui-act=open]").getAttribute("title"), "10/28/2026 – 11/03/2026");
    assert.deepEqual(await events(page), ["input:a_to", "change:a_to"], "the page hears one change");
    assert.equal(await page.evaluate(() => document.activeElement?.getAttribute("data-ui-act")), "open", "focus returns to the button");
    // The matching preset is pressed when it opens again, and Custom is not.
    await page.locator("#dr-a_from [data-ui-act=open]").click();
    assert.equal(await page.locator("#dr-a_from-pop [data-ui-preset=last_7_days]").getAttribute("aria-pressed"), "true");
    assert.equal(await page.locator("#dr-a_from-pop [data-ui-preset=custom]").getAttribute("aria-pressed"), "false");
    await close();
  });

  test("a custom range: two clicks, across months; nothing posts half picked; Escape puts the old one back", async () => {
    const { page, close } = await open("/dates");
    const pop = "#dr-b_from-pop";
    assert.equal(await label(page, "#dr-b_from"), "Nov 1 – 3");
    await page.locator("#dr-b_from [data-ui-act=open]").click();
    assert.equal(await page.locator(`${pop} [data-slot=calendar-month]`).count(), 2, "two months at this width");
    await day(page, pop, "2026-11-10").click();
    assert.equal(await val(page, "b_from"), "", "half picked posts nothing");
    assert.equal(await val(page, "b_to"), "");
    await page.keyboard.press("Escape");
    assert.equal(await isOpen(page, "dr-b_from-pop"), false);
    assert.equal(await val(page, "b_from"), "2026-11-01", "the old range is back");
    assert.equal(await val(page, "b_to"), "2026-11-03");
    assert.equal(await label(page, "#dr-b_from"), "Nov 1 – 3");
    // Across the months on show.
    await page.locator("#dr-b_from [data-ui-act=open]").click();
    await day(page, pop, "2026-11-28").click();
    await day(page, pop, "2026-12-04").click();
    assert.equal(await val(page, "b_from"), "2026-11-28");
    assert.equal(await val(page, "b_to"), "2026-12-04");
    assert.equal(await label(page, "#dr-b_from"), "Nov 28 – Dec 4");
    // Where the range was chosen, the matching marks are on the days.
    await page.locator("#dr-b_from [data-ui-act=open]").click();
    assert.equal(await page.locator(`${pop} [data-range-middle]`).count(), 5);
    await close();
  });

  test("keyboard: open the range picker, move, choose both ends, and the focus comes back", async () => {
    const { page, close } = await open("/dates");
    await page.locator("#dr-b_from [data-ui-act=open]").focus();
    await page.keyboard.press("Enter");
    assert.equal(await isOpen(page, "dr-b_from-pop"), true);
    assert.equal(await focusedDate(page), "2026-11-01", "the start of the range holds the tab stop");
    await page.keyboard.press("ArrowDown");
    await page.keyboard.press("Enter"); // 11/08 starts
    await page.keyboard.press("ArrowRight");
    await page.keyboard.press("ArrowRight");
    await page.keyboard.press("Space"); // 11/10 ends
    assert.equal(await val(page, "b_from"), "2026-11-08");
    assert.equal(await val(page, "b_to"), "2026-11-10");
    assert.equal(await isOpen(page, "dr-b_from-pop"), false);
    assert.equal(await page.evaluate(() => document.activeElement?.getAttribute("data-ui-act")), "open");
    await close();
  });

  test("confirm: a choice waits for Apply, and Cancel, Escape and Clear do as they say", async () => {
    const { page, close } = await open("/dates");
    const pop = "#dr-c_from-pop";
    await record(page);
    await page.locator("#dr-c_from [data-ui-act=open]").click();
    await day(page, pop, "2026-11-16").click();
    await day(page, pop, "2026-11-18").click();
    assert.equal(await isOpen(page, "dr-c_from-pop"), true, "still open for Apply");
    assert.equal(await val(page, "c_from"), "2026-10-27", "nothing was applied");
    assert.deepEqual(await events(page), []);
    await page.locator(`${pop} [data-ui-act=apply]`).click();
    assert.equal(await val(page, "c_from"), "2026-11-16");
    assert.equal(await val(page, "c_to"), "2026-11-18");
    assert.equal(await isOpen(page, "dr-c_from-pop"), false);
    assert.deepEqual(await events(page), ["input:c_to", "change:c_to"]);
    // Cancel drops a draft.
    await page.locator("#dr-c_from [data-ui-act=open]").click();
    await day(page, pop, "2026-11-02").click();
    await day(page, pop, "2026-11-04").click();
    await page.locator(`${pop} [data-ui-act=cancel]`).click();
    assert.equal(await val(page, "c_from"), "2026-11-16");
    assert.equal(await label(page, "#dr-c_from"), "Nov 16 – 18");
    // Escape drops one too.
    await page.locator("#dr-c_from [data-ui-act=open]").click();
    await day(page, pop, "2026-11-02").click();
    await day(page, pop, "2026-11-04").click();
    await page.keyboard.press("Escape");
    assert.equal(await val(page, "c_from"), "2026-11-16");
    // Clear empties it, applied at once.
    await page.locator("#dr-c_from [data-ui-act=open]").click();
    await page.locator(`${pop} [data-ui-act=clear]`).click();
    assert.equal(await val(page, "c_from"), "");
    assert.equal(await val(page, "c_to"), "");
    assert.equal(await label(page, "#dr-c_from"), "Pick a date range");
    await close();
  });

  test("limits: presets outside them are off, and a half-picked range can't reach too far or too near", async () => {
    const { page, close } = await open("/dates");
    const pop = "#dr-d_from-pop";
    await page.locator("#dr-d_from [data-ui-act=open]").click();
    assert.equal(await page.locator(`${pop} [data-ui-preset=today]`).isDisabled(), true, "one day is under the least of two");
    assert.equal(await page.locator(`${pop} [data-ui-preset=all_time]`).isDisabled(), true, "20 days is over the most of ten");
    assert.equal(await page.locator(`${pop} [data-ui-preset=last_7_days]`).isDisabled(), false);
    // Calendar days outside 15 October to 20 November are off.
    assert.equal(await day(page, pop, "2026-10-14").getAttribute("aria-disabled"), "true");
    assert.equal(await day(page, pop, "2026-11-21").getAttribute("aria-disabled"), "true");
    await day(page, pop, "2026-11-05").click();
    // Ten days at most, two at least: 5th is the start; the 14th is the last, the 5th itself too short.
    assert.equal(await day(page, pop, "2026-11-14").getAttribute("aria-disabled"), null);
    assert.equal(await day(page, pop, "2026-11-15").getAttribute("aria-disabled"), "true");
    assert.equal(await day(page, pop, "2026-11-05").getAttribute("aria-disabled"), "true");
    assert.equal(await day(page, pop, "2026-11-06").getAttribute("aria-disabled"), null);
    assert.equal(await day(page, pop, "2026-10-27").getAttribute("aria-disabled"), null, "ten days back from the 5th reaches the 27th");
    assert.equal(await day(page, pop, "2026-10-26").getAttribute("aria-disabled"), "true");
    await close();
  });

  test("a range of limited days can't hold a day that can't be chosen", async () => {
    const { page, close } = await open("/dates");
    // The calendar with the rules: 7th is a Saturday, so a range from the 5th can't reach past it.
    const probe = await page.evaluate(() => document.querySelector("#cal-limits") !== null);
    assert.equal(probe, true);
    await day(page, "#cal-limits", "2026-11-02").click();
    assert.equal(await day(page, "#cal-limits", "2026-11-06").getAttribute("aria-disabled"), null, "five days");
    assert.equal(await day(page, "#cal-limits", "2026-11-07").getAttribute("aria-disabled"), "true", "six days: over the most");
    await close();
  });

  test("a start day waiting for its end looks chosen, a calendar that only shows looks like any other, a disabled one is dimmed", async () => {
    const { page, close } = await open("/dates");
    // As it looks once the pointer is away and no transition is still running.
    const look = async (root, iso) => {
      await page.mouse.move(0, 0);
      await page.evaluate(() => Promise.all(document.getAnimations().map((a) => a.finished)));
      return day(page, root, iso).evaluate((e) => {
        const s = getComputedStyle(e);
        return { opacity: Number(s.opacity), background: s.backgroundColor, color: s.color };
      });
    };
    const chosen = await look("#cal-single", "2026-11-12");
    const plain = await look("#cal-single", "2026-11-18");
    assert.equal(chosen.opacity, 1);
    // Two days at least: the day just picked can't also be the end, and still shows as the start.
    await day(page, "#cal-limits", "2026-11-02").click();
    assert.equal(await day(page, "#cal-limits", "2026-11-02").getAttribute("aria-disabled"), "true");
    assert.deepEqual(await look("#cal-limits", "2026-11-02"), chosen, "the start looks as a chosen day does");
    // A calendar that only shows: its days look as they do anywhere, and the chosen one stands out.
    assert.deepEqual(await look("#cal-ro", "2026-11-12"), chosen);
    assert.deepEqual(await look("#cal-ro", "2026-11-18"), plain);
    // A disabled calendar is dimmed all over, the chosen day too.
    assert.ok((await look("#cal-off", "2026-11-18")).opacity < 1);
    assert.ok((await look("#cal-off", "2026-11-12")).opacity < 1);
    await close();
  });

  test("the report's window is in the address: choose, reload, back, a shared link, bad values", async () => {
    const { page, close } = await open("/dates/report");
    assert.equal(await page.locator("[data-report=window]").innerText(), "none");
    assert.equal(await label(page, "#report-range"), "Pick a date range");
    const requests = [];
    page.on("request", (r) => { if (/\/dates\/report/.test(r.url())) requests.push(r.url().replace(BASE, "")); });
    await page.locator("#report-range [data-ui-act=open]").click();
    await page.locator("#report-range-pop [data-ui-preset=today]").click();
    await page.waitForFunction(() => document.querySelector("[data-report=window]")?.textContent !== "none");
    assert.equal(await page.locator("[data-report=window]").innerText(), "2026-11-03 to 2026-11-03");
    assert.equal(await page.locator("[data-report=days]").innerText(), "1");
    assert.match(page.url(), /\?from=2026-11-03&to=2026-11-03$/);
    assert.equal(requests.length, 1, "one fetch for one choice");
    assert.equal(await label(page, "#report-range"), "Nov 3");
    // Reload, back and forward.
    await page.reload();
    assert.equal(await page.locator("[data-report=window]").innerText(), "2026-11-03 to 2026-11-03", "reload shows the same window");
    assert.equal(await val(page, "from"), "2026-11-03");
    await page.goBack();
    await page.waitForFunction(() => document.querySelector("[data-report=window]")?.textContent === "none");
    assert.equal(await label(page, "#report-range"), "Pick a date range");
    await page.goForward();
    await page.waitForFunction(() => document.querySelector("[data-report=window]")?.textContent !== "none");
    assert.equal(await label(page, "#report-range"), "Nov 3");
    await close();
  });

  test("a window in the address: a pair backwards is put right, a bad value drops it, a day is counted whole", async () => {
    const { page, close } = await open("/dates/report?from=2026-11-05&to=2026-11-01");
    assert.equal(await page.locator("[data-report=window]").innerText(), "2026-11-01 to 2026-11-05");
    assert.equal(await val(page, "from"), "2026-11-01");
    assert.equal(await label(page, "#report-range"), "Nov 1 – 5");
    for (const bad of ["from=2026-11-03&to=nope", "from=2026-13-40&to=2026-11-03", "from=&to=", "from=2026-11-03", "to=2026-11-03", "from=%3Cscript%3E&to=1"]) {
      await page.goto(`${BASE}/dates/report?${bad}`);
      assert.equal(await page.locator("[data-report=window]").innerText(), "none", bad);
      assert.equal(await label(page, "#report-range"), "Pick a date range", bad);
    }
    // The 23-hour and 25-hour days of New York, and a range across the change.
    await page.goto(`${BASE}/dates/report?from=2026-03-08&to=2026-03-08`);
    assert.equal(await page.locator("[data-report=query]").innerText(), "created_at >= 2026-03-08T05:00:00Z AND created_at < 2026-03-09T04:00:00Z");
    await page.goto(`${BASE}/dates/report?from=2026-11-01&to=2026-11-01`);
    assert.equal(await page.locator("[data-report=query]").innerText(), "created_at >= 2026-11-01T04:00:00Z AND created_at < 2026-11-02T05:00:00Z");
    await page.goto(`${BASE}/dates/report?from=2026-03-07&to=2026-03-09`);
    assert.equal(await page.locator("[data-report=days]").innerText(), "3");
    assert.equal(await page.locator("[data-report=query]").innerText(), "created_at >= 2026-03-07T05:00:00Z AND created_at < 2026-03-10T04:00:00Z");
    await close();
  });

  test("a region that morphs under an open popover keeps it open, and the page's choice stands", async () => {
    const { page, close } = await open("/dates/report?from=2026-11-01&to=2026-11-03");
    await page.locator("#report-range [data-ui-act=open]").click();
    await day(page, "#report-range-pop", "2026-11-10").click(); // half picked
    // The form refetches its region (a poll or another filter would do the same).
    await page.locator("#report-form").evaluate((f) => f.requestSubmit());
    await page.waitForTimeout(600);
    assert.equal(await isOpen(page, "report-range-pop"), true, "the popover stays open");
    assert.equal(await page.locator("#report-range-pop [data-pending]").count(), 1, "and the half-picked day with it");
    await day(page, "#report-range-pop", "2026-11-12").click();
    await page.waitForFunction(() => document.querySelector("[data-report=window]")?.textContent === "2026-11-10 to 2026-11-12");
    await close();
  });

  test("a table's filter bar holds the window: choose, the rows follow, the address keeps it and the table's own state", async () => {
    const { page, close } = await open("/dates/table?per_page=25&sort=-amount");
    const total = async () => (await page.locator("[data-slot=table-summary]").innerText()).replace(/\s+/g, " ");
    assert.match(await total(), /1,000/);
    await page.locator("#table-window [data-ui-act=open]").click();
    await page.locator("#table-window-pop [data-ui-preset=last_30_days]").click();
    await page.waitForFunction(() => !document.querySelector("[data-slot=table-summary]")?.textContent.includes("1,000"));
    assert.match(await total(), /69/, "the 23 days of October that hold rows, three rows each");
    assert.match(page.url(), /from=2026-10-05&to=2026-11-03/);
    assert.match(page.url(), /sort=-amount/, "the table's own state stays in the address");
    const dates = await page.locator("#window-deals tbody tr[data-key] td:last-child").allInnerTexts();
    assert.ok(dates.length > 0);
    for (const text of dates) assert.ok(text >= "2026-10-05" && text <= "2026-11-03", text);
    assert.equal(await label(page, "#table-window"), "Oct 5 – Nov 3");
    // Back and reload show the same window; clearing the window brings the rows back.
    await page.reload();
    assert.equal(await label(page, "#table-window"), "Oct 5 – Nov 3");
    assert.match(await total(), /69/);
    await page.locator("#table-window [data-ui-act=clear]").first().click();
    await page.waitForFunction(() => document.querySelector("[data-slot=table-summary]")?.textContent.includes("1,000"));
    assert.doesNotMatch(page.url(), /from=/);
    await close();
  });

  // ---- time picker ----

  test("a time picker: 12 hour text, the list opens at the value, the keys choose, one change is heard", async () => {
    const { page, close } = await open("/dates");
    await record(page);
    assert.equal(await page.locator("#tp-twelve [data-ui-text]").inputValue(), "2:30 PM");
    assert.equal(await val(page, "twelve"), "14:30");
    await page.locator("#tp-twelve [data-ui-act=open]").click();
    assert.equal(await isOpen(page, "tp-twelve-pop"), true);
    assert.equal(await page.locator("#tp-twelve-pop [role=option]").count(), 48);
    assert.equal(await page.locator("#tp-twelve-pop [aria-selected=true]").innerText(), "2:30 PM");
    assert.equal(await page.evaluate(() => document.activeElement?.getAttribute("role")), "listbox");
    await page.keyboard.press("ArrowDown");
    await page.keyboard.press("ArrowDown");
    await page.keyboard.press("Enter");
    assert.equal(await val(page, "twelve"), "15:30");
    assert.equal(await page.locator("#tp-twelve [data-ui-text]").inputValue(), "3:30 PM");
    assert.equal(await isOpen(page, "tp-twelve-pop"), false);
    assert.deepEqual(await events(page), ["input:twelve", "change:twelve"]);
    assert.equal(await page.evaluate(() => document.activeElement?.hasAttribute("data-ui-text")), true);
    await close();
  });

  test("the list shows the chosen time when it opens, the keys keep their option in view, and the page stays put", async () => {
    const { page, close } = await open("/dates");
    await page.locator("#tp-twelve").scrollIntoViewIfNeeded();
    const top = await page.evaluate(() => scrollY);
    await page.locator("#tp-twelve [data-ui-act=open]").click();
    assert.equal(await isOpen(page, "tp-twelve-pop"), true);
    // Whether an option is inside the box of the list, as seen on screen.
    const inView = (selector) =>
      page.evaluate((s) => {
        const ul = document.querySelector("#tp-twelve-pop [data-slot=time-list]");
        const a = ul.getBoundingClientRect();
        const b = ul.querySelector(s).getBoundingClientRect();
        return b.height > 0 && b.top >= a.top - 1 && b.bottom <= a.bottom + 1;
      }, selector);
    await page.waitForFunction(() => {
      const ul = document.querySelector("#tp-twelve-pop [data-slot=time-list]");
      return ul && ul.scrollTop > 0;
    });
    assert.equal(await inView("[aria-selected=true]"), true, "2:30 PM is in view, though it is 29 rows down");
    assert.equal(await inView("[data-active]"), true);
    await page.keyboard.press("End");
    assert.equal(await inView("[data-t='84600']"), true, "11:30 PM, the last, scrolled into view");
    await page.keyboard.press("Home");
    assert.equal(await inView("[data-t='0']"), true, "12:00 AM, the first");
    await page.keyboard.press("PageDown");
    await page.keyboard.press("PageDown");
    assert.equal(await inView("[data-active]"), true, "the active option stays in view");
    assert.equal(await page.evaluate(() => scrollY), top, "the page did not scroll");
    await close();
  });

  test("a time picker on a 24 hour clock lists and posts the same way", async () => {
    const { page, close } = await open("/dates");
    assert.equal(await page.locator("#tp-h24 [data-ui-text]").inputValue(), "14:30");
    await page.locator("#tp-h24 [data-ui-act=open]").click();
    await page.locator("#tp-h24-pop [data-t='32400']").click(); // 09:00
    assert.equal(await val(page, "h24"), "09:00");
    assert.equal(await page.locator("#tp-h24 [data-ui-text]").inputValue(), "09:00");
    await close();
  });

  test("typed times: every shared case reads the same in the browser as on the server", async () => {
    const { page, close } = await open("/dates/probe?loc=en");
    let seen = 0;
    const byLocale = new Map();
    for (const c of cases.times) byLocale.set(c.locale, [...(byLocale.get(c.locale) || []), c]);
    for (const [locale, list] of byLocale) {
      await page.goto(`${BASE}/dates/probe?loc=${locale}`);
      for (const c of list) {
        await page.fill("#probe-time [data-ui-text]", c.text);
        await page.locator("#probe-time [data-ui-text]").blur();
        const native = await page.locator("#probe-time input[type=time]").inputValue();
        const invalid = await page.locator("#probe-time [data-ui-text]").getAttribute("aria-invalid");
        if (c.ok) {
          const want = c.ok.endsWith(":00") ? c.ok.slice(0, 5) : c.ok;
          assert.equal(native, want, `${locale} ${JSON.stringify(c.text)}`);
          assert.equal(invalid, null, `${locale} ${JSON.stringify(c.text)}`);
        } else {
          assert.equal(native, "", `${locale} ${JSON.stringify(c.text)}`);
          assert.equal(invalid, c.err === "empty" ? null : "true", `${locale} ${JSON.stringify(c.text)}`);
        }
        seen++;
      }
    }
    assert.ok(seen >= 50, `${seen}`);
    await close();
  });

  test("time rules: the list holds only what can be chosen, typed times are held to them", async () => {
    const { page, close } = await open("/dates");
    await page.locator("#tp-hours [data-ui-act=open]").click();
    const options = await page.locator("#tp-hours-pop [role=option]").allInnerTexts();
    assert.equal(options.length, 29, "9:00 to 17:00 by 15, without 12:00 to 12:59");
    assert.equal(options[0], "9:00 AM");
    assert.equal(options.at(-1), "5:00 PM");
    assert.ok(!options.includes("12:15 PM"));
    await page.keyboard.press("Escape");
    const text = page.locator("#tp-hours [data-ui-text]");
    await text.fill("12:30 pm");
    await text.blur();
    assert.equal(await text.getAttribute("aria-invalid"), "true");
    assert.match(await page.locator("#tp-hours [data-slot=field-error]").innerText(), /isn't available/);
    await text.fill("8:00");
    await text.blur();
    assert.equal(await val(page, "hours"), "");
    await text.fill("9:07"); // off the step: accepted, the step is for the list
    await text.blur();
    assert.equal(await val(page, "hours"), "09:07");
    assert.equal(await text.getAttribute("aria-invalid"), null);
    await close();
  });

  test("a strict step refuses a time off it, at the box, and the hidden field carries no step of its own", async () => {
    const { page, close } = await open("/dates");
    const text = page.locator("#tp-strict [data-ui-text]");
    await text.fill("10:10");
    await text.blur();
    assert.equal(await text.getAttribute("aria-invalid"), "true");
    await text.fill("10:15");
    await text.blur();
    assert.equal(await val(page, "strict"), "10:15");
    // With the script on the box holds the step: the hidden field would count it from another start.
    assert.equal(await page.locator("#tp-strict input[type=time]").getAttribute("step"), null);
    await close();
  });

  test("seconds: shown, typed and posted", async () => {
    const { page, close } = await open("/dates");
    assert.equal(await page.locator("#tp-seconds [data-ui-text]").inputValue(), "2:30:15 PM");
    assert.equal(await val(page, "seconds"), "14:30:15");
    const text = page.locator("#tp-seconds [data-ui-text]");
    await text.fill("3:45:30 pm");
    await text.blur();
    assert.equal(await val(page, "seconds"), "15:45:30");
    assert.equal(await text.inputValue(), "3:45:30 PM");
    await close();
  });

  test("every minute has no list: the arrow keys step, and the open button is gone", async () => {
    const { page, close } = await open("/dates");
    assert.equal(await page.locator("#tp-fine [data-ui-act=open]").isVisible(), false);
    const text = page.locator("#tp-fine [data-ui-text]");
    await text.focus();
    await page.keyboard.press("ArrowUp");
    assert.equal(await val(page, "fine"), "00:00");
    await page.keyboard.press("ArrowUp");
    assert.equal(await val(page, "fine"), "00:01");
    await page.keyboard.press("ArrowDown");
    assert.equal(await val(page, "fine"), "00:00");
    await close();
  });

  test("the zone is said under the picker, and clear empties the time", async () => {
    const { page, close } = await open("/dates");
    assert.match(await page.locator("#tp-zone [data-slot=field-description]").innerText(), /America\/New_York/);
    await page.locator("#tp-clear [data-ui-act=clear]").click();
    assert.equal(await val(page, "clear"), "");
    assert.equal(await page.locator("#tp-clear [data-ui-text]").inputValue(), "");
    await close();
  });

  test("a time picker in Japanese and Arabic writes and reads its own markers", async () => {
    const { page, close } = await open("/dates");
    assert.equal(await page.locator("#tp-tja [data-ui-text]").inputValue(), "15:05");
    assert.equal(await page.locator("#tp-tar [data-ui-text]").inputValue(), "3:30 م");
    const text = page.locator("#tp-tar [data-ui-text]");
    await text.fill("٩:٠٥ ص");
    await text.blur();
    assert.equal(await val(page, "tar"), "09:05");
    await close();
  });

  test("JavaScript off: a time field and a range's two dates are native", async () => {
    const { page, close } = await open("/dates", { js: false });
    assert.equal(await page.locator("#tp-twelve input[type=time]").isVisible(), true);
    assert.equal(await page.locator("#tp-twelve [data-ui-text]").isVisible(), false);
    assert.equal(await page.locator("#dr-b_from input[type=date]").first().isVisible(), true);
    assert.equal(await page.locator("#dr-b_from [data-ui-act=open]").isVisible(), false);
    await close();
  });

  test("JavaScript off: Back to the report shows the window the server drew, not dates typed before leaving it", async () => {
    const { page, close } = await open("/dates/report?from=2026-11-01&to=2026-11-03", { js: false });
    const from = page.locator("#report-range input[name=from]");
    const to = page.locator("#report-range input[name=to]");
    await from.fill("2026-11-10");
    await to.fill("2026-11-20");
    const asked = [];
    page.on("request", (r) => { if (/\/dates\/report/.test(r.url())) asked.push(r.url()); });
    await page.goto(`${BASE}/dates`);
    await page.goBack();
    await page.waitForLoadState("load");
    // A page the back-forward cache kept is the same page, typed dates and all. One loaded again is the
    // server's, and the browser must not fill its fields back in.
    if (asked.length) {
      assert.equal(await from.inputValue(), "2026-11-01", "the start the server drew");
      assert.equal(await to.inputValue(), "2026-11-03", "the end the server drew");
    }
    await close();
  });
});
