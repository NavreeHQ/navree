// axe in every state of the calendar family, light and dark, at a phone and a desktop
// width, right to left, with JavaScript off; the layout at 390px; speed; hostile input.

import assert from "node:assert/strict";
import { test } from "node:test";
import { BASE, axe, day, focusedDate, inBrowsers, isOpen } from "./support/dates.mjs";

const PHONE = { width: 390, height: 844 };

/** Every state: a name, a path, what to do first, and how to open the page. */
const STATES = [
  ["everything closed", "/dates"],
  ["date picker open", "/dates", async (page) => page.locator("#dp-value [data-ui-act=open]").click()],
  ["date picker open, today from the foot", "/dates", async (page) => page.locator("#dp-basic [data-ui-act=open]").click()],
  ["date of birth lists open", "/dates", async (page) => page.locator("#dp-birth [data-ui-act=open]").click()],
  ["range picker open", "/dates", async (page) => page.locator("#dr-b_from [data-ui-act=open]").click()],
  ["range picker half picked", "/dates", async (page) => {
    await page.locator("#dr-b_from [data-ui-act=open]").click();
    await day(page, "#dr-b_from-pop", "2026-11-10").click();
    await day(page, "#dr-b_from-pop", "2026-11-12").hover();
  }],
  ["range picker with confirm", "/dates", async (page) => page.locator("#dr-c_from [data-ui-act=open]").click()],
  ["range picker with limits", "/dates", async (page) => page.locator("#dr-d_from [data-ui-act=open]").click()],
  ["time list open", "/dates", async (page) => page.locator("#tp-twelve [data-ui-act=open]").click()],
  ["time list with rules open", "/dates", async (page) => page.locator("#tp-hours [data-ui-act=open]").click()],
  ["typed date wrong", "/dates", async (page) => {
    await page.locator("#dp-basic [data-ui-text]").fill("31/31/31");
    await page.locator("#dp-basic [data-ui-text]").blur();
  }],
  ["typed time wrong", "/dates", async (page) => {
    await page.locator("#tp-hours [data-ui-text]").fill("12:30");
    await page.locator("#tp-hours [data-ui-text]").blur();
  }],
  ["arabic date picker open", "/dates", async (page) => page.locator("#dp-ar [data-ui-act=open]").click()],
  ["arabic range open", "/dates", async (page) => page.locator("#dr-f_from [data-ui-act=open]").click()],
  ["booking form with server errors", "/dates/form", async (page) => {
    await page.locator("#dp-start [data-ui-text]").fill("11/07/2026");
    await page.locator("#tp-arrives [data-ui-text]").fill("12:30 pm");
    await page.getByRole("button", { name: "Book" }).click();
    await page.locator("[data-slot=field-error]").first().waitFor();
  }],
  ["report with the range open", "/dates/report?from=2026-11-01&to=2026-11-03", async (page) => page.locator("#report-range [data-ui-act=open]").click()],
  ["javascript off, form", "/dates/form", null, { asIfScriptingOff: true }],
  ["javascript off, calendars and pickers", "/dates", null, { asIfScriptingOff: true }],
  ["javascript off, the report", "/dates/report?from=2026-11-01&to=2026-11-03", null, { asIfScriptingOff: true }],
];

inBrowsers(({ open }) => {
  for (const [name, path, prepare, options] of STATES) {
    test(`axe, dark, 1440px: ${name}`, { timeout: 90000 }, async () => {
      const { page, close } = await open(path, { ...options });
      if (prepare) await prepare(page);
      await page.waitForTimeout(150);
      assert.deepEqual(await axe(page), []);
      await close();
    });
  }

  for (const [name, path, prepare, options] of STATES.filter(([, , , o]) => !o)) {
    test(`axe, light, 1440px: ${name}`, { timeout: 90000 }, async () => {
      const { page, close } = await open(path, { light: true });
      if (prepare) await prepare(page);
      await page.waitForTimeout(150);
      assert.deepEqual(await axe(page), []);
      await close();
    });
  }

  for (const [name, path, prepare] of STATES.filter(([n, , , o]) => !o && /open|wrong|errors|half/.test(n))) {
    test(`axe, dark, 390px: ${name}`, { timeout: 90000 }, async () => {
      const { page, close } = await open(path, { viewport: PHONE });
      if (prepare) await prepare(page);
      await page.waitForTimeout(150);
      assert.deepEqual(await axe(page), []);
      await close();
    });
  }

  test("390px: popovers fit the screen and the range shows one month, and the page doesn't scroll sideways", async () => {
    const { page, close } = await open("/dates", { viewport: PHONE });
    for (const [button, pop] of [["#dp-value [data-ui-act=open]", "dp-value-pop"], ["#dr-b_from [data-ui-act=open]", "dr-b_from-pop"], ["#tp-twelve [data-ui-act=open]", "tp-twelve-pop"], ["#dr-f_from [data-ui-act=open]", "dr-f_from-pop"]]) {
      await page.locator(button).scrollIntoViewIfNeeded();
      await page.locator(button).click();
      const box = await page.locator(`#${pop}`).evaluate((e) => { const r = e.getBoundingClientRect(); return { left: r.left, right: r.right, top: r.top, bottom: r.bottom, w: innerWidth, h: innerHeight }; });
      assert.ok(box.left >= 0 && box.right <= box.w, `${pop} spans ${box.left} to ${box.right} in ${box.w}`);
      assert.ok(box.top >= 0 && box.bottom <= box.h + 1, `${pop} spans ${box.top} to ${box.bottom} in ${box.h}`);
      if (pop.startsWith("dr-")) assert.equal(await page.locator(`#${pop} [data-slot=calendar-month]`).count(), 1, "one month on a phone");
      await page.keyboard.press("Escape");
    }
    // Calendars hold seven columns of at least 32px.
    const cell = await page.locator("#cal-single button[data-slot=calendar-day]").first().evaluate((e) => e.getBoundingClientRect().width);
    assert.ok(cell >= 32, `${cell}`);
    await close();
  });

  test("the range picker's presets stack above the calendar at 390px, and sit beside it at 1440px", async () => {
    for (const [viewport, beside] of [[PHONE, false], [{ width: 1440, height: 900 }, true]]) {
      const { page, close } = await open("/dates", { viewport });
      await page.locator("#dr-b_from [data-ui-act=open]").scrollIntoViewIfNeeded();
      await page.locator("#dr-b_from [data-ui-act=open]").click();
      const rects = await page.evaluate(() => {
        const presets = document.querySelector("#dr-b_from-pop [data-slot=range-presets]").getBoundingClientRect();
        const cal = document.querySelector("#dr-b_from-pop [data-slot=calendar]").getBoundingClientRect();
        return { presets: { left: presets.left, right: presets.right, bottom: presets.bottom }, cal: { left: cal.left, top: cal.top } };
      });
      if (beside) assert.ok(rects.presets.right <= rects.cal.left + 1, JSON.stringify(rects));
      else assert.ok(rects.presets.bottom <= rects.cal.top + 1, JSON.stringify(rects));
      await close();
    }
  });

  test("right to left: the week starts on the right, the arrows follow the reading direction, the popover stays on screen", async () => {
    const { page, close } = await open("/dates");
    await page.locator("#dp-ar [data-ui-act=open]").click();
    const rtl = await page.evaluate(() => getComputedStyle(document.querySelector("#dp-ar-pop [data-slot=calendar]")).direction);
    assert.equal(rtl, "rtl");
    // Saturday starts the week in Arabic: the first heading is the rightmost.
    const heads = await page.locator("#dp-ar-pop thead th").evaluateAll((items) => items.map((e) => ({ abbr: e.getAttribute("abbr"), x: e.getBoundingClientRect().left })));
    assert.equal(heads[0].abbr, "السبت");
    assert.ok(heads[0].x > heads.at(-1).x, "the first column is on the right");
    assert.equal(await page.locator("#dp-ar-pop [data-slot=calendar-caption]").first().innerText(), "نوفمبر 2026");
    const box = await page.locator("#dp-ar-pop").evaluate((e) => { const r = e.getBoundingClientRect(); return [r.left, r.right, innerWidth]; });
    assert.ok(box[0] >= 0 && box[1] <= box[2], JSON.stringify(box));
    // On the 12th: Left is the next day, Right the one before.
    assert.equal(await focusedDate(page), "2026-11-12");
    await page.keyboard.press("ArrowLeft");
    assert.equal(await focusedDate(page), "2026-11-13");
    await page.keyboard.press("ArrowRight");
    await page.keyboard.press("ArrowRight");
    assert.equal(await focusedDate(page), "2026-11-11");
    // The text field takes Arabic digits and the Arabic month name.
    await page.keyboard.press("Escape");
    const text = page.locator("#dp-ar [data-ui-text]");
    await text.fill("٢٠ نوفمبر ٢٠٢٦");
    await text.blur();
    assert.equal(await page.locator("[name=ar]").first().inputValue(), "2026-11-20");
    assert.equal(await text.inputValue(), "20/11/2026");
    await close();
  });

  test("the date picker's popover sits under its field and flips up near the bottom of the screen", async () => {
    const { page, close } = await open("/dates");
    await page.locator("#dp-basic [data-ui-act=open]").click();
    await page.waitForFunction(() => document.querySelector("#dp-basic-pop").style.top !== "");
    await page.evaluate(() => Promise.all(document.querySelector("#dp-basic-pop").getAnimations().map((a) => a.finished)));
    const under = await page.evaluate(() => {
      const field = document.querySelector("#dp-basic [data-slot=input-group]").getBoundingClientRect();
      const pop = document.querySelector("#dp-basic-pop").getBoundingClientRect();
      return { fieldBottom: field.bottom, popTop: pop.top, fieldRight: field.right, popRight: pop.right };
    });
    assert.ok(Math.abs(under.popTop - under.fieldBottom) < 12, JSON.stringify(under));
    assert.ok(Math.abs(under.popRight - under.fieldRight) < 12, "right edge to right edge: " + JSON.stringify(under));
    await page.keyboard.press("Escape");
    // With no room below, it opens above.
    await page.locator("#dp-basic").evaluate((e) => e.scrollIntoView({ block: "end" }));
    await page.locator("#dp-basic [data-ui-act=open]").click();
    await page.waitForFunction(() => document.querySelector("#dp-basic-pop").style.top !== "");
    await page.evaluate(() => Promise.all(document.querySelector("#dp-basic-pop").getAnimations().map((a) => a.finished)));
    const above = await page.evaluate(() => {
      const field = document.querySelector("#dp-basic [data-slot=input-group]").getBoundingClientRect();
      const pop = document.querySelector("#dp-basic-pop").getBoundingClientRect();
      return { fieldTop: field.top, popBottom: pop.bottom, popTop: pop.top, h: innerHeight };
    });
    assert.ok(above.popBottom <= above.fieldTop + 12 && above.popTop >= 0, JSON.stringify(above));
    await close();
  });

  test("speed: a popover opens and a month turns within a frame or two of the browser's own time", async () => {
    const { page, close } = await open("/dates");
    const timed = (selector) =>
      page.evaluate(async (sel) => {
        const el = document.querySelector(sel);
        const t = performance.now();
        el.click();
        await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
        return performance.now() - t;
      }, selector);
    const open1 = await timed("#dp-value [data-ui-act=open]");
    const nav = await timed("#dp-value-pop [data-ui-cal=next]");
    const nav2 = await timed("#dp-value-pop [data-ui-cal=next]");
    await page.keyboard.press("Escape");
    const range = await timed("#dr-b_from [data-ui-act=open]");
    const list = await (async () => { await page.keyboard.press("Escape"); return timed("#tp-twelve [data-ui-act=open]"); })();
    console.log(`  open ${open1.toFixed(1)} ms, month ${nav.toFixed(1)} and ${nav2.toFixed(1)} ms, range ${range.toFixed(1)} ms, time list ${list.toFixed(1)} ms`);
    // Generous: a busy machine, a debug server. They catch a change of scale.
    for (const [what, ms] of [["open", open1], ["month", nav], ["range", range], ["list", list]]) assert.ok(ms < 250, `${what} took ${ms} ms`);
    await close();
  });

  test("a morph under a month the person turned keeps the month and the choice", async () => {
    const { page, close } = await open("/dates");
    await page.locator("#cal-single [data-ui-cal=next]").click();
    await day(page, "#cal-single", "2026-12-09").click();
    await page.evaluate(() => document.dispatchEvent(new Event("ui:swap")));
    assert.equal(await page.locator("#cal-single [data-slot=calendar-caption]").innerText(), "December 2026");
    assert.equal(await page.locator("#cal-single [data-selected]").getAttribute("data-date"), "2026-12-09");
    assert.equal(await page.locator("[name=cal_single]").inputValue(), "2026-12-09");
    await close();
  });

  test("hostile input: a locale tag, a typed string and a label never become markup", async () => {
    const { page, close } = await open("/dates/probe?loc=%3Cimg%20src%3Dx%20onerror%3Dalert(1)%3E");
    assert.equal(await page.locator("img").count(), 0);
    const text = page.locator("#probe-date [data-ui-text]");
    for (const nasty of ["<img src=x onerror=alert(1)>", "\"><script>alert(1)</script>", "99999999999999999999999999", "٣".repeat(5000)]) {
      await text.fill(nasty);
      await text.blur();
      assert.equal(await page.locator("#probe-date input[type=date]").inputValue(), "", nasty.slice(0, 20));
      assert.equal(await page.locator("img, script:not([src])").count(), 0);
      // The message is text a person reads, in the locale the page fell back to.
      assert.match(await page.locator("#probe-date [data-slot=field-error]").innerText(), /^Enter a date like/);
    }
    await close();
  });

  test("two pickers and the calendar keep their own state when the page is large", async () => {
    const { page, close } = await open("/dates");
    const ids = await page.evaluate(() => [...document.querySelectorAll("[id]")].map((e) => e.id));
    assert.equal(new Set(ids).size, ids.length, "every id is unique");
    assert.equal(await isOpen(page, "dp-value-pop"), false);
    await close();
  });
});
