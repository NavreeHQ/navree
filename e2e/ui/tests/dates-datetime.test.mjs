// The joint date-and-time control in real browsers, against the proof app's /dates/datetime
// page: one value from two halves, half a value held back at the empty box, the rules of
// each half, the label and the group, and the native field with JavaScript off. Needs the
// proof app serving (see dates.test.mjs).

import assert from "node:assert/strict";
import { test } from "node:test";
import { axe, day, events, inBrowsers, record, val } from "./support/dates.mjs";

/** The id of the dates part the focus is in. */
const focusedPart = (page) => page.evaluate(() => document.activeElement?.closest("[data-ui-dates]")?.id ?? null);
/** Whether a box holds the form back with a reason of the page's own. */
const held = (box) => box.evaluate((e) => e.validity.customError);

inBrowsers(({ open }) => {
  test("a date and a time post as one value, and the page hears the joint field alone", async () => {
    const { page, close } = await open("/dates/datetime");
    // One field posts: the halves' own fields have no name.
    assert.equal(await page.locator("[name=starts]").count(), 1);
    assert.equal(await page.locator("[name=starts]").getAttribute("type"), "datetime-local");
    assert.equal(await page.locator("#dtp-starts [data-ui-native][name]").count(), 0);
    assert.equal(await page.locator("#dtp-starts-date [data-ui-text]").inputValue(), "11/12/2026");
    const time = page.locator("#dtp-starts-time [data-ui-text]");
    assert.equal(await time.inputValue(), "2:30 PM");
    await record(page);
    // A day from the calendar keeps the time.
    await page.locator("#dtp-starts-date [data-ui-act=open]").click();
    await day(page, "#dtp-starts-date-pop", "2026-11-18").click();
    assert.equal(await val(page, "starts"), "2026-11-18T14:30");
    assert.deepEqual(await events(page), ["input:starts", "change:starts"]);
    // A typed time keeps the day. (Enter in a box of a form also sends the form, as in any field.)
    await time.fill("4:15 pm");
    await time.blur();
    assert.equal(await val(page, "starts"), "2026-11-18T16:15");
    assert.equal(await time.inputValue(), "4:15 PM");
    assert.deepEqual(await events(page), ["input:starts", "change:starts", "input:starts", "change:starts"]);
    // A time from the list, and leaving the box with the time it holds says nothing more.
    await page.locator("#dtp-starts-time [data-ui-act=open]").click();
    await page.locator("#dtp-starts-time-list [role=option][data-t='36000']").click();
    assert.equal(await val(page, "starts"), "2026-11-18T10:00");
    assert.equal(await time.inputValue(), "10:00 AM");
    assert.equal((await events(page)).length, 6, "one input and one change for each new value");
    await close();
  });

  test("half a value posts nothing, and the empty box holds the form back and says why", async () => {
    const { page, close } = await open("/dates/datetime");
    const date = page.locator("#dtp-visit-date [data-ui-text]");
    const time = page.locator("#dtp-visit-time [data-ui-text]");
    await date.fill("11/12/2026");
    await date.blur();
    assert.equal(await val(page, "visit"), "", "a date with no time is not sent as midnight");
    assert.equal(await held(time), true);
    assert.equal(await time.evaluate((e) => e.validationMessage), "Enter a time like 2:30 PM.");
    await page.getByRole("button", { name: "Book" }).click();
    await page.waitForTimeout(300);
    assert.equal(new URL(page.url()).search, "", "the form was not sent");
    assert.equal(await focusedPart(page), "dtp-visit-time", "the box that says why has the focus");
    // The other half filled: the reason goes, and the form goes.
    await time.fill("10:00 am");
    await time.blur();
    assert.equal(await held(time), false);
    assert.equal(await val(page, "visit"), "2026-11-12T10:00");
    await page.getByRole("button", { name: "Book" }).click();
    await page.waitForURL(/\/dates\/datetime\?/);
    assert.deepEqual(await page.locator("[data-result]").allInnerTexts(), ["starts=2026-11-12T14:30 ok", "visit=2026-11-12T10:00 ok", "call= empty"]);
    await close();
  });

  test("clearing a half empties the value, and the other half's box is held until it is filled again", async () => {
    const { page, close } = await open("/dates/datetime");
    await record(page);
    const date = page.locator("#dtp-starts-date [data-ui-text]");
    await page.locator("#dtp-starts-date [data-ui-act=clear]").first().click();
    assert.equal(await val(page, "starts"), "");
    assert.equal(await date.inputValue(), "");
    assert.deepEqual(await events(page), ["input:starts", "change:starts"]);
    assert.equal(await held(date), true, "a time with no date holds the form back at the date box");
    await date.fill("11/20/2026");
    await date.blur();
    assert.equal(await held(date), false);
    assert.equal(await val(page, "starts"), "2026-11-20T14:30");
    await close();
  });

  test("each half keeps its rules: what can't be chosen is out of the calendar and the list, and said at the box when typed", async () => {
    const { page, close } = await open("/dates/datetime");
    const date = page.locator("#dtp-visit-date [data-ui-text]");
    const time = page.locator("#dtp-visit-time [data-ui-text]");
    await date.fill("11/14/2026"); // a Saturday
    await date.blur();
    assert.match(await page.locator("#dtp-visit-date [data-slot=field-error]").innerText(), /isn't available/);
    await time.fill("8:00 am");
    await time.blur();
    assert.match(await page.locator("#dtp-visit-time [data-slot=field-error]").innerText(), /isn't available/);
    assert.equal(await val(page, "visit"), "");
    await page.locator("#dtp-visit-date [data-ui-act=open]").click();
    assert.equal(await day(page, "#dtp-visit-date-pop", "2026-11-14").getAttribute("aria-disabled"), "true");
    await page.keyboard.press("Escape");
    await page.locator("#dtp-visit-time [data-ui-act=open]").click();
    const times = await page.locator("#dtp-visit-time-list [role=option]").allInnerTexts();
    assert.equal(times[0], "9:00 AM");
    assert.equal(times.at(-1), "5:00 PM");
    assert.equal(times.length, 17);
    await page.keyboard.press("Escape");
    // Mended, each box's words go and the value is whole.
    await date.fill("11/16/2026");
    await date.blur();
    await time.fill("9:30 am");
    await time.blur();
    assert.equal(await page.locator("#dtp-visit [data-slot=field-error]").count(), 0);
    assert.equal(await val(page, "visit"), "2026-11-16T09:30");
    await close();
  });

  test("the picker is a group its label names, the label names the date box, and the joint field stays out of the way", async () => {
    const { page, close } = await open("/dates/datetime", { light: true });
    assert.equal(await page.getByRole("group", { name: "Starts", exact: true }).count(), 1);
    assert.equal(await page.getByRole("textbox", { name: "Starts", exact: true }).count(), 1);
    assert.equal(await page.locator("#dtp-starts-time [data-ui-text]").getAttribute("aria-label"), "Pick a time");
    await page.locator("label[for=dtp-starts-date-text]").click();
    assert.equal(await focusedPart(page), "dtp-starts-date");
    // A required picker requires both boxes; the field behind them requires nothing and takes no tab stop.
    assert.deepEqual(await page.locator("#dtp-starts [data-ui-text]").evaluateAll((boxes) => boxes.map((b) => b.required)), [true, true]);
    const joint = page.locator("[name=starts]");
    assert.equal(await joint.evaluate((e) => e.required), false);
    assert.equal(await joint.getAttribute("tabindex"), "-1");
    assert.equal(await joint.getAttribute("aria-hidden"), "true");
    assert.equal(await page.locator("[name=visit]").getAttribute("min"), null, "the server drew a first day; the boxes hold it now");
    // The zone is said under the picker.
    assert.equal(await page.locator("#dtp-call > [data-slot=field-description]").innerText(), "Times are in Asia/Kolkata (IST).");
    assert.deepEqual(await axe(page), []);
    await close();
  });

  test("a morph keeps both halves and the value", async () => {
    const { page, close } = await open("/dates/datetime");
    const time = page.locator("#dtp-starts-time [data-ui-text]");
    await time.fill("6:00 pm");
    await time.blur();
    await page.evaluate(() => document.dispatchEvent(new Event("ui:swap")));
    assert.equal(await val(page, "starts"), "2026-11-12T18:00");
    assert.equal(await time.inputValue(), "6:00 PM");
    assert.deepEqual(await page.locator("#dtp-starts [data-ui-text]").evaluateAll((boxes) => boxes.map((b) => b.required)), [true, true]);
    await close();
  });

  test("JavaScript off: a native date-and-time field stands in and posts the same value", async () => {
    const { page, close } = await open("/dates/datetime", { js: false });
    const starts = page.locator("[name=starts]");
    assert.equal(await starts.isVisible(), true);
    assert.equal(await starts.inputValue(), "2026-11-12T14:30");
    assert.equal(await page.locator("#dtp-starts-date [data-ui-text]").isVisible(), false);
    // The visit's field is told its first day, and nothing the server would take is refused.
    assert.equal(await page.locator("[name=visit]").getAttribute("min"), "2026-11-03T00:00");
    await starts.fill("2026-11-20T09:30");
    await page.locator("[name=visit]").fill("2026-11-16T11:00");
    await page.getByRole("button", { name: "Book" }).click();
    await page.waitForURL(/\/dates\/datetime\?/);
    assert.deepEqual(await page.locator("[data-result]").allInnerTexts(), ["starts=2026-11-20T09:30 ok", "visit=2026-11-16T11:00 ok", "call= empty"]);
    await close();
  });
});
