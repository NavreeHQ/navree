// The calendar family against hostile ids, stored values the rules now refuse, and a live
// month caption, in real browsers. Needs the proof app serving (see dates.test.mjs).

import assert from "node:assert/strict";
import { test } from "node:test";
import { BASE, inBrowsers } from "./support/dates.mjs";

inBrowsers(({ open, browserName }) => {
  test("a hostile id never becomes markup: not in the months the browser draws, nor in lists, nor in what posts", async () => {
    const { page, close } = await open("/dates/hostile");
    const planted = () =>
      page.evaluate(() => ({
        metas: document.querySelectorAll("meta[http-equiv]").length,
        roles: document.querySelectorAll("input[name=role]").length,
        links: document.querySelectorAll("a#planted").length,
        scripts: document.querySelectorAll("script:not([src])").length,
      }));
    const quiet = { metas: 0, roles: 0, links: 0, scripts: 0 };
    assert.deepEqual(await planted(), quiet, "as the server drew it");
    let moved = 0;
    page.on("framenavigated", (frame) => {
      if (frame === page.mainFrame()) moved++;
    });
    // Turn every calendar a few months, and use the lists on the one with its month and year in lists.
    const next = page.locator("[data-ui-cal=next]:visible");
    for (let i = 0; i < (await next.count()); i++) {
      await next.nth(i).click();
      await next.nth(i).click();
    }
    await page.locator("[data-ui-cal=month]:visible").first().selectOption("5");
    await page.locator("[data-ui-cal=year]:visible").first().selectOption("2030");
    // Open every picker's popover and close it again: each draws its months, or its list, as it opens.
    const buttons = page.locator("[data-ui-act=open]");
    for (let i = 0; i < (await buttons.count()); i++) {
      await buttons.nth(i).click();
      await page.keyboard.press("Escape");
    }
    await page.waitForTimeout(400);
    assert.deepEqual(await planted(), quiet, "after the browser drew the months and the list");
    assert.equal(moved, 0, "the page was not sent anywhere");
    assert.match(page.url(), /\/dates\/hostile$/);
    // The grid of each month is named by its own caption, whatever the id says.
    const dangling = await page.evaluate(() =>
      [...document.querySelectorAll("table[aria-labelledby]")]
        .filter((table) => !document.getElementById(table.getAttribute("aria-labelledby")))
        .length,
    );
    assert.equal(dangling, 0, "every grid finds its caption");
    // What a form posts is the fields a page made, and no more.
    await page.getByRole("button", { name: "Send" }).click();
    await page.waitForURL(/\/dates\/hostile\?/);
    const query = new URL(page.url()).searchParams;
    assert.equal(query.has("role"), false);
    assert.equal(query.get("due"), "2026-11-12");
    await close();
  });

  test("turning a month changes the words of its caption in place, so a screen reader hears the new month", async () => {
    const { page, close } = await open("/dates");
    const same = () =>
      page.evaluate(() => {
        const now = [...document.querySelectorAll("#cal-range [data-slot=calendar-caption]")];
        return { kept: now.length === window.__caps.length && now.every((c, i) => c === window.__caps[i]), words: now.map((c) => c.textContent), live: now.map((c) => c.getAttribute("aria-live")) };
      });
    await page.evaluate(() => {
      window.__caps = [...document.querySelectorAll("#cal-range [data-slot=calendar-caption]")];
    });
    const first = await same();
    assert.equal(first.kept, true);
    assert.deepEqual(first.live, ["polite", "polite"]);
    await page.locator("#cal-range [data-ui-cal=next]").click();
    const turned = await same();
    assert.equal(turned.kept, true, "the same two elements");
    assert.notDeepEqual(turned.words, first.words);
    assert.deepEqual(turned.words, ["December 2026", "January 2027"]);
    // Each grid still names its own month by the caption beside it.
    const named = await page.evaluate(() =>
      [...document.querySelectorAll("#cal-range [data-slot=calendar-month]")].map((m) => document.getElementById(m.querySelector("table").getAttribute("aria-labelledby")) === m.querySelector("[data-slot=calendar-caption]")),
    );
    assert.deepEqual(named, [true, true]);
    // A picker's calendar in a popover does the same.
    await page.locator("#dp-value [data-ui-act=open]").click();
    await page.evaluate(() => {
      window.__pop = document.querySelector("#dp-value-pop [data-slot=calendar-caption]");
    });
    await page.locator("#dp-value-pop [data-ui-cal=next]").click();
    assert.equal(await page.evaluate(() => window.__pop === document.querySelector("#dp-value-pop [data-slot=calendar-caption]") && !!window.__pop.isConnected), true);
    assert.match(await page.locator("#dp-value-pop [data-slot=calendar-caption]").innerText(), /December 2026/);
    await close();
  });

  test("a stored value the rules now refuse is said at the box, nothing stops the form unseen, and mending it lets the form go", async () => {
    const { page, close } = await open("/dates/stale");
    // The hidden fields carry no limits of their own.
    const natives = await page.evaluate(() =>
      [...document.querySelectorAll("input[data-ui-native]")].map((n) => ({ name: n.name, min: n.getAttribute("min"), max: n.getAttribute("max"), step: n.getAttribute("step"), valid: n.validity.valid })),
    );
    assert.deepEqual(
      natives.map((n) => [n.name, n.min, n.max, n.step, n.valid]),
      [["due", null, null, null, true], ["at", null, null, null, true], ["stepped", null, null, null, true], ["quarter", null, null, null, true]],
    );
    // The date and the time that are out of bounds say so, in words, at their box.
    const dueBox = page.locator("#dp-stale-due [data-ui-text]");
    const atBox = page.locator("#tp-stale-at [data-ui-text]");
    assert.equal(await dueBox.inputValue(), "10/01/2026");
    assert.equal(await dueBox.getAttribute("aria-invalid"), "true");
    assert.equal(await page.locator("#dp-stale-due [data-slot=field-error]").innerText(), "That date isn't available.");
    assert.equal(await atBox.inputValue(), "8:00 AM");
    assert.equal(await atBox.getAttribute("aria-invalid"), "true");
    assert.equal(await page.locator("#tp-stale-at [data-slot=field-error]").innerText(), "That time isn't available.");
    // A stored time off a strict step is said too.
    assert.equal(await page.locator("#tp-stale-quarter [data-ui-text]").inputValue(), "9:07 AM");
    assert.equal(await page.locator("#tp-stale-quarter [data-slot=field-error]").innerText(), "That time isn't available.");
    // A time on the step is fine, though the least time is not on it.
    assert.equal(await page.locator("#tp-stale-step [data-ui-text]").inputValue(), "9:15 AM");
    assert.equal(await page.locator("#tp-stale-step [data-slot=field-error]").count(), 0);
    assert.notEqual(await page.locator("#tp-stale-step [data-ui-text]").getAttribute("aria-invalid"), "true");
    // Sending is held by the box that says why.
    await page.getByRole("button", { name: "Send" }).click();
    await page.waitForTimeout(300);
    assert.equal(new URL(page.url()).search, "");
    assert.equal(await page.evaluate(() => document.activeElement?.getAttribute("data-ui-text") !== null), true, "the box that says why has the focus");
    // Mend both: the words go, the form goes, and the server agrees with what it was sent.
    await dueBox.fill("11/12/2026");
    await dueBox.blur();
    await atBox.fill("10:00 AM");
    await atBox.blur();
    await page.locator("#tp-stale-quarter [data-ui-text]").fill("9:15 AM");
    await page.locator("#tp-stale-quarter [data-ui-text]").blur();
    assert.equal(await page.locator("[data-slot=field-error]").count(), 0);
    await page.getByRole("button", { name: "Send" }).click();
    await page.waitForURL(/\/dates\/stale\?/);
    assert.deepEqual(await page.locator("[data-result]").allInnerTexts(), ["due=2026-11-12 ok", "at=10:00 ok", "stepped=09:15 ok", "quarter=09:15 ok"]);
    await close();
  });

  test("a form's pickers carry the rules the server holds the form to", async () => {
    const { page, close } = await open("/dates/form");
    await page.locator("#dp-start [data-ui-act=open]").click();
    const off = (iso) => page.locator(`#dp-start-pop button[data-date="${iso}"]`).getAttribute("aria-disabled");
    assert.equal(await off("2026-11-07"), "true", "a Saturday");
    assert.equal(await off("2026-11-08"), "true", "a Sunday");
    assert.equal(await off("2026-11-02"), "true", "before the first day");
    assert.equal(await off("2026-11-09"), null, "a Monday in range");
    await page.keyboard.press("Escape");
    await page.locator("#tp-arrives [data-ui-act=open]").click();
    const times = await page.locator("#tp-arrives-pop [role=option]").allInnerTexts();
    assert.equal(times.length, 15, "9:00 to 11:30 and 1:00 to 5:00 at half hours");
    assert.equal(times.some((text) => /^12:/.test(text)), false, "no time at lunch");
    assert.equal(times[0], "9:00 AM");
    await close();
  });

  test("a report holds the address to the limits its picker was given, and a repeated key is not an error", async () => {
    const { page, close } = await open("/dates/report?from=2026-11-01&from=zzz&to=2026-11-03&to=x");
    assert.equal(await page.locator("[data-report=window]").innerText(), "2026-11-01 to 2026-11-03", "the first of a key stands");
    assert.equal(await page.locator("#report-range").getAttribute("data-max-days"), "400", "the picker is given the same most days");
    const window = async (query) => {
      await page.goto(`${BASE}/dates/report?${query}`);
      return [await page.locator("[data-report=window]").innerText(), await page.locator("[data-report=refused]").count()];
    };
    const refusal = () => page.locator("[data-report=refused]").innerText();
    assert.deepEqual(await window("from=2026-01-01&to=2026-12-31"), ["2026-01-01 to 2026-12-31", 0]);
    assert.deepEqual(await window("from=2025-01-01&to=2026-12-31"), ["none", 1], "730 days is more than 400");
    assert.equal(await refusal(), "Choose at most 400 days.", "a window too long says so");
    assert.deepEqual(await window("from=2026-11-01&to=2028-02-01"), ["none", 1], "past the latest day");
    assert.equal(await refusal(), "That date isn't available.");
    assert.deepEqual(await window("from=2019-12-31&to=2020-01-05"), ["none", 1], "before the earliest");
    // Years the calendar doesn't draw are no range at all, said nothing of, and nothing breaks.
    assert.deepEqual(await window("from=-262143-01-01&to=2026-11-03"), ["none", 0]);
    assert.deepEqual(await window("from=2026-11-01&to=%2B262142-12-31"), ["none", 0]);
    await close();
  });

  test("a range picker's hidden dates carry no limits either", async () => {
    const { page, close } = await open("/dates");
    const limits = await page.evaluate(() =>
      [...document.querySelectorAll("#dr-d_from input[type=date]")].map((n) => [n.getAttribute("min"), n.getAttribute("max")]),
    );
    assert.deepEqual(limits, [[null, null], [null, null]]);
    await close();
  });

  test("JavaScript off: the native fields keep their limits, and the server takes what they take", async () => {
    const { page, close } = await open("/dates/stale", { js: false });
    const natives = await page.evaluate(() =>
      [...document.querySelectorAll("input[data-ui-native]")].map((n) => ({ name: n.name, min: n.getAttribute("min"), step: n.getAttribute("step"), valid: n.validity.valid })),
    );
    // The date is before its least: the browser says so itself, on a field a person can see.
    // (This WebKit build draws a date or time field as plain text, with no limit to check.)
    assert.equal(natives[0].min, "2026-11-03");
    if (browserName !== "webkit") assert.equal(natives[0].valid, false);
    // The step is counted from the least time, so the least is the first time on the step the
    // server takes: 9:10 is not a quarter hour, 9:15 is.
    assert.equal(natives[2].min, "09:15");
    assert.equal(natives[2].step, "900");
    assert.equal(natives[2].valid, true, "9:15 is on the step the server counts from midnight");
    // A strict step with no least time: the least is midnight, so the step is counted from where
    // the server counts it, and not from the stored 9:07 (which would refuse 9:15 and take 9:22).
    assert.equal(natives[3].min, "00:00");
    assert.equal(natives[3].step, "900");
    await page.locator("#tp-stale-quarter input[type=time]").fill("09:15");
    if (browserName !== "webkit") assert.equal(await page.locator("#tp-stale-quarter input[type=time]").evaluate((n) => n.validity.valid), true, "9:15 is on the step");
    await page.locator("#dp-stale-due input[type=date]").fill("2026-11-12");
    await page.locator("#tp-stale-at input[type=time]").fill("10:00");
    await page.getByRole("button", { name: "Send" }).click();
    await page.waitForURL(/\/dates\/stale\?/);
    assert.deepEqual(await page.locator("[data-result]").allInnerTexts(), ["due=2026-11-12 ok", "at=10:00 ok", "stepped=09:15 ok", "quarter=09:15 ok"]);
    await close();
  });
});
