// The carousel in real browsers, against the proof app's /carousel: roles and names, the
// buttons, the keys of the pickers, going round, slides in view at once, vertical and right
// to left, rotation that stops for the pointer, focus, the pause button and a device that
// wants less motion, JavaScript off (links and native scrolling), a phone, hostile text, and
// axe in dark and light at 1440 and 390 px.
//
// UI-GENUI-020
//
//   cargo run --release -p ui-proof -- serve          # UI_PROOF_URL, default http://127.0.0.1:8090
//   UI_BROWSERS=chromium,firefox,webkit node --test tests/carousel.test.mjs

import assert from "node:assert/strict";
import { test } from "node:test";
import { axe, inBrowsers, shot } from "./support/flow.mjs";

const PHONE = { width: 390, height: 844 };
/** The tests run with the device's motion setting at "reduce" unless a test says otherwise, so a scroll lands at once. */
const MOTION = (page) => page.emulateMedia({ reducedMotion: "no-preference" });

/** Which slide is at the start of the track. */
const index = (page, id) =>
  page.locator(`#${id}`).evaluate((root) => {
    const track = root.querySelector("[data-slot=carousel-track]");
    const t = track.getBoundingClientRect();
    const vertical = root.getAttribute("data-orientation") === "vertical";
    const rtl = getComputedStyle(track).direction === "rtl";
    let best = 0;
    let min = Infinity;
    root.querySelectorAll("[data-slot=carousel-slide]").forEach((slide, i) => {
      const b = slide.getBoundingClientRect();
      const d = Math.abs(vertical ? b.top - t.top : rtl ? t.right - b.right : b.left - t.left);
      if (d < min) {
        min = d;
        best = i;
      }
    });
    return best;
  });

/** Waits until `read()` gives `want`, or fails with what it last gave. */
async function until(read, want, what, ms = 4000) {
  const end = Date.now() + ms;
  let got;
  while (Date.now() < end) {
    got = await read();
    if (JSON.stringify(got) === JSON.stringify(want)) return;
    await new Promise((resolve) => setTimeout(resolve, 40));
  }
  assert.deepEqual(got, want, what);
}

const state = (page, id) =>
  page.locator(`#${id}`).evaluate((root) => ({
    prev: root.querySelector("[data-carousel=prev]")?.getAttribute("aria-disabled"),
    next: root.querySelector("[data-carousel=next]")?.getAttribute("aria-disabled"),
    selected: [...root.querySelectorAll("[role=tab]")].map((t) => t.getAttribute("aria-selected")).indexOf("true"),
    stops: [...root.querySelectorAll("[role=tab]")].filter((t) => t.tabIndex === 0).length,
  }));

inBrowsers(({ open, browserName }) => {
  test("a carousel is a labelled region of slides, with a focusable track and named buttons", async () => {
    const { page, close } = await open("/carousel");
    const root = page.locator("#c-basic");
    assert.deepEqual(
      await root.evaluate((e) => [e.getAttribute("role"), e.getAttribute("aria-roledescription"), e.getAttribute("aria-label")]),
      ["region", "carousel", "Offers from Ludhiana"],
    );
    const slides = await root.locator("[data-slot=carousel-slide]").evaluateAll((els) => els.map((e) => [e.getAttribute("role"), e.getAttribute("aria-roledescription"), e.getAttribute("aria-label")]));
    assert.deepEqual(slides, [1, 2, 3, 4].map((n) => ["group", "slide", `${n} of 4`]));
    const track = root.locator("[data-slot=carousel-track]");
    assert.equal(await track.getAttribute("tabindex"), "0");
    assert.equal(await track.getAttribute("aria-live"), "polite");
    assert.equal(await root.getByRole("button", { name: "Previous slide" }).count(), 1);
    assert.equal(await root.getByRole("button", { name: "Next slide" }).count(), 1);
    await close();
  });

  test("the module marks the carousel ready and the buttons say where the ends are", async () => {
    const { page, close } = await open("/carousel");
    await page.waitForSelector("#c-basic[data-ready]");
    assert.deepEqual(await state(page, "c-basic"), { prev: "true", next: "false", selected: -1, stops: 0 });
    assert.equal(await page.locator("#c-basic [data-carousel=next]").isVisible(), true);
    await close();
  });

  test("next and previous move one slide, and stop at the ends", async () => {
    const { page, close } = await open("/carousel");
    await page.waitForSelector("#c-basic[data-ready]");
    const next = page.locator("#c-basic [data-carousel=next]");
    const prev = page.locator("#c-basic [data-carousel=prev]");
    await prev.click({ force: true });
    assert.equal(await index(page, "c-basic"), 0, "previous at the start does nothing");
    for (const want of [1, 2, 3]) {
      await next.click();
      await until(() => index(page, "c-basic"), want, `next goes to slide ${want + 1}`);
    }
    await until(() => state(page, "c-basic").then((s) => s.next), "true", "next is disabled on the last slide");
    await next.click({ force: true });
    assert.equal(await index(page, "c-basic"), 3, "next at the end does nothing");
    await prev.click();
    await until(() => index(page, "c-basic"), 2, "previous goes back one");
    // The buttons follow the scroll's end, a moment after the slide is in place.
    await until(() => state(page, "c-basic").then((s) => s.next), "false", "next is enabled again");
    await close();
  });

  test("the buttons answer Enter and Space, and keep focus when they are disabled", async () => {
    const { page, close } = await open("/carousel");
    await page.waitForSelector("#c-basic[data-ready]");
    await page.locator("#c-basic [data-carousel=next]").focus();
    await page.keyboard.press("Enter");
    await until(() => index(page, "c-basic"), 1, "Enter on next");
    await page.keyboard.press("Space");
    await until(() => index(page, "c-basic"), 2, "Space on next");
    await page.keyboard.press("Enter");
    await until(() => index(page, "c-basic"), 3, "Enter on next again");
    await until(() => state(page, "c-basic").then((s) => s.next), "true", "disabled at the end");
    assert.equal(await page.evaluate(() => document.activeElement?.getAttribute("data-carousel")), "next", "a button that becomes disabled keeps focus");
    await close();
  });

  test("the buttons come before the track in the page, so Tab meets them first", async () => {
    const { page, close } = await open("/carousel");
    await page.waitForSelector("#c-pickers[data-ready]");
    await page.locator("#c-pickers-before").focus();
    const order = [];
    for (let i = 0; i < 4; i += 1) {
      await page.keyboard.press("Tab");
      order.push(await page.evaluate(() => document.activeElement?.getAttribute("data-carousel") || document.activeElement?.getAttribute("data-slot") || document.activeElement?.getAttribute("role")));
    }
    assert.deepEqual(order, ["prev", "next", "carousel-track", "carousel-picker"]);
    await close();
  });

  test("with going round on, next on the last slide goes to the first and previous goes to the last", async () => {
    const { page, close } = await open("/carousel");
    await page.waitForSelector("#c-loop[data-ready]");
    assert.deepEqual(await state(page, "c-loop"), { prev: "false", next: "false", selected: -1, stops: 0 }, "no end to disable");
    await page.locator("#c-loop [data-carousel=prev]").click();
    await until(() => index(page, "c-loop"), 2, "previous at the start goes to the last");
    await page.locator("#c-loop [data-carousel=next]").click();
    await until(() => index(page, "c-loop"), 0, "next at the end goes to the first");
    await close();
  });

  test("the pickers are tabs with one tab stop, and the arrows, Home and End move and open", async () => {
    const { page, close } = await open("/carousel");
    await page.waitForSelector("#c-pickers[data-ready]");
    assert.deepEqual(await state(page, "c-pickers"), { prev: "true", next: "false", selected: 0, stops: 1 });
    assert.equal(await page.locator("#c-pickers [role=tablist]").getAttribute("aria-label"), "Choose a slide");
    await page.locator("#c-pickers-tab-1").focus();
    await page.keyboard.press("ArrowRight");
    await until(() => index(page, "c-pickers"), 1, "ArrowRight opens the second slide");
    assert.equal(await page.evaluate(() => document.activeElement?.id), "c-pickers-tab-2");
    await until(() => state(page, "c-pickers").then((s) => [s.selected, s.stops]), [1, 1], "the stop moves with the open tab");
    await page.keyboard.press("End");
    await until(() => index(page, "c-pickers"), 3, "End opens the last");
    await page.keyboard.press("ArrowRight");
    await until(() => index(page, "c-pickers"), 0, "ArrowRight on the last goes round to the first");
    await page.keyboard.press("ArrowLeft");
    await until(() => index(page, "c-pickers"), 3, "ArrowLeft on the first goes round to the last");
    await page.keyboard.press("Home");
    await until(() => index(page, "c-pickers"), 0, "Home opens the first");
    await page.keyboard.press("Space");
    assert.equal(await index(page, "c-pickers"), 0);
    assert.equal(new URL(page.url()).hash, "", "a picker does not touch the address");
    await close();
  });

  test("clicking a picker opens its slide, and scrolling by hand moves the open tab", async () => {
    const { page, close } = await open("/carousel");
    await page.waitForSelector("#c-pickers[data-ready]");
    await page.locator("#c-pickers-tab-3").click();
    await until(() => index(page, "c-pickers"), 2, "the third slide");
    await until(() => state(page, "c-pickers").then((s) => s.selected), 2, "the third tab is open");
    assert.equal(new URL(page.url()).hash, "");
    await page.locator("#c-pickers [data-slot=carousel-track]").evaluate((t) => t.scrollTo({ left: 0 }));
    await until(() => state(page, "c-pickers").then((s) => s.selected), 0, "scrolling back moves the open tab");
    await close();
  });

  test("a tab names its slide by title, and its slide is the panel it controls", async () => {
    const { page, close } = await open("/carousel");
    const tabs = await page.locator("#c-pickers [role=tab]").evaluateAll((els) => els.map((e) => [e.getAttribute("aria-label"), document.getElementById(e.getAttribute("aria-controls")).getAttribute("role"), document.getElementById(e.getAttribute("aria-controls")).getAttribute("aria-labelledby") === e.id]));
    assert.deepEqual(tabs, [
      ["Phulkari dupatta", "tabpanel", true],
      ["Basmati rice, 5 kg", "tabpanel", true],
      ["Desi ghee, 1 kg", "tabpanel", true],
      ["Pashmina shawl", "tabpanel", true],
    ]);
    await close();
  });

  test("a focus that lands in a slide that is out of view brings it into view", async () => {
    const { page, close } = await open("/carousel");
    await page.waitForSelector("#c-basic[data-ready]");
    await page.locator("#c-basic-order").focus();
    await until(() => index(page, "c-basic"), 2, "the slide with the link is shown");
    await close();
  });

  test("two slides in view move one at a time, and a phone shows one", async () => {
    const { page, close } = await open("/carousel");
    await page.waitForSelector("#c-two[data-ready]");
    const widths = await page.locator("#c-two").evaluate((root) => {
      const t = root.querySelector("[data-slot=carousel-track]").getBoundingClientRect().width;
      return [...root.querySelectorAll("[data-slot=carousel-slide]")].map((s) => Math.round((s.getBoundingClientRect().width / t) * 100));
    });
    assert.ok(widths.every((w) => w >= 47 && w <= 50), `each slide is about half the track: ${widths}`);
    await page.locator("#c-two [data-carousel=next]").click();
    await until(() => index(page, "c-two"), 1, "one slide, not two");
    await close();
    const phone = await open("/carousel", { viewport: PHONE });
    const full = await phone.page.locator("#c-two").evaluate((root) => {
      const t = root.querySelector("[data-slot=carousel-track]").getBoundingClientRect().width;
      return Math.round((root.querySelector("[data-slot=carousel-slide]").getBoundingClientRect().width / t) * 100);
    });
    assert.equal(full, 100, "one slide fills a phone");
    assert.ok(await phone.page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), "no sideways scroll");
    await shot(phone.page, `${browserName}-carousel-390`, "#carousels");
    await phone.close();
  });

  test("a vertical carousel scrolls from top to bottom, with its buttons at the top and bottom", async () => {
    const { page, close } = await open("/carousel");
    await page.waitForSelector("#c-vertical[data-ready]");
    const where = await page.locator("#c-vertical").evaluate((root) => {
      const track = root.querySelector("[data-slot=carousel-track]").getBoundingClientRect();
      const prev = root.querySelector("[data-carousel=prev]").getBoundingClientRect();
      const next = root.querySelector("[data-carousel=next]").getBoundingClientRect();
      return { prevTop: prev.top - track.top < 20, nextBottom: track.bottom - next.bottom < 20, height: Math.round(track.height) };
    });
    assert.deepEqual([where.prevTop, where.nextBottom], [true, true]);
    assert.ok(where.height >= 280 && where.height <= 300, `18rem tall: ${where.height}`);
    await page.locator("#c-vertical [data-carousel=next]").click();
    await until(() => index(page, "c-vertical"), 1, "next goes down one");
    assert.ok((await page.locator("#c-vertical [data-slot=carousel-track]").evaluate((t) => t.scrollTop)) > 100);
    await close();
  });

  test("right to left: the first slide is at the right, next goes left, and the arrows follow", async () => {
    const { page, close } = await open("/carousel");
    await page.waitForSelector("#c-rtl[data-ready]");
    const first = await page.locator("#c-rtl").evaluate((root) => {
      const track = root.querySelector("[data-slot=carousel-track]").getBoundingClientRect();
      const slide = root.querySelector("[data-slot=carousel-slide]").getBoundingClientRect();
      const prev = root.querySelector("[data-carousel=prev]").getBoundingClientRect();
      return { atRight: Math.abs(track.right - slide.right) < 2, prevAtRight: prev.left > track.left + track.width / 2 };
    });
    assert.deepEqual(first, { atRight: true, prevAtRight: true });
    await page.locator("#c-rtl [data-carousel=next]").click();
    await until(() => index(page, "c-rtl"), 1, "next goes to the second slide");
    assert.ok((await page.locator("#c-rtl [data-slot=carousel-track]").evaluate((t) => t.scrollLeft)) < 0, "toward the left");
    // The tabs: Left goes on, as the bars run from the right.
    await page.locator("#c-rtl-tab-2").focus();
    await page.keyboard.press("ArrowLeft");
    await until(() => index(page, "c-rtl"), 2, "ArrowLeft opens the next slide");
    await page.keyboard.press("ArrowRight");
    await until(() => index(page, "c-rtl"), 1, "ArrowRight goes back");
    const flipped = await page.locator("#c-rtl [data-carousel=prev] svg").evaluate((e) => getComputedStyle(e).scale);
    assert.match(flipped, /^-1/, "the chevron is mirrored");
    await close();
  });

  test("one slide has nothing to move to", async () => {
    const { page, close } = await open("/carousel");
    await page.waitForSelector("#c-one[data-ready]");
    assert.deepEqual(await state(page, "c-one"), { prev: "true", next: "true", selected: -1, stops: 0 });
    assert.equal(await page.locator("#c-one [role=tab]").count(), 0, "a picker of one slide is not drawn");
    await close();
  });

  test("rotation moves on its own, is quiet to a screen reader, and stops for the pointer, focus, the pause button and less motion", async () => {
    const { page, close } = await open("/carousel", { before: MOTION });
    await page.waitForSelector("#c-auto[data-ready]");
    const track = page.locator("#c-auto [data-slot=carousel-track]");
    assert.equal(await track.getAttribute("aria-live"), "off", "a rotating carousel is not announced");
    assert.equal(await page.locator("#c-auto [data-carousel=rotation]").getAttribute("aria-label"), "Pause automatic rotation");
    await page.locator("#c-auto").scrollIntoViewIfNeeded();
    await until(() => index(page, "c-auto").then((i) => i > 0), true, "it turned to the next slide by itself", 6000);

    // The pointer on it holds it still.
    await page.locator("#c-auto [data-slot=carousel-track]").hover();
    const held = await index(page, "c-auto");
    await page.waitForTimeout(4400);
    assert.equal(await index(page, "c-auto"), held, "hover holds it");

    // Off it, it goes on.
    await page.mouse.move(2, 2);
    await until(() => index(page, "c-auto").then((i) => i !== held), true, "it goes on when the pointer leaves", 6000);

    // The pause button stops it, and says so.
    await page.locator("#c-auto [data-carousel=rotation]").click();
    assert.equal(await page.locator("#c-auto").getAttribute("data-stopped"), "");
    assert.equal(await page.locator("#c-auto [data-carousel=rotation]").getAttribute("aria-label"), "Start automatic rotation");
    assert.equal(await track.getAttribute("aria-live"), "polite", "stopped, changes are announced");
    await page.evaluate(() => document.activeElement?.blur());
    await page.mouse.move(2, 2);
    const stopped = await index(page, "c-auto");
    await page.waitForTimeout(4400);
    assert.equal(await index(page, "c-auto"), stopped, "stopped means stopped");

    // Play starts it again.
    await page.locator("#c-auto [data-carousel=rotation]").click();
    assert.equal(await page.locator("#c-auto").getAttribute("data-stopped"), null);
    await close();
  });

  test("rotation stops while focus is in the carousel, and never runs for a device that wants less motion", async () => {
    const { page, close } = await open("/carousel", { before: MOTION });
    await page.waitForSelector("#c-auto[data-ready]");
    await page.locator("#c-auto [data-carousel=next]").focus();
    const at = await index(page, "c-auto");
    await page.waitForTimeout(4400);
    assert.equal(await index(page, "c-auto"), at, "focus holds it");
    await page.emulateMedia({ reducedMotion: "reduce" });
    await page.evaluate(() => document.activeElement?.blur());
    await page.mouse.move(2, 2);
    await page.waitForTimeout(2600);
    assert.equal(await page.locator("#c-auto [data-slot=carousel-track]").getAttribute("aria-live"), "polite", "less motion: it is a plain carousel");
    const still = await index(page, "c-auto");
    await page.waitForTimeout(4400);
    assert.equal(await index(page, "c-auto"), still, "less motion: it does not move");
    await close();
  });

  test("a rotating carousel's buttons go round at the end", async () => {
    const { page, close } = await open("/carousel");
    assert.equal(await page.locator("#c-auto").getAttribute("data-loop"), "");
    await close();
  });

  test("with JavaScript off the buttons are not shown, the pickers are links, and the track scrolls by itself", async () => {
    const { page, close } = await open("/carousel", { js: false });
    assert.equal(await page.locator("#c-pickers [data-carousel=next]").isVisible(), false);
    assert.equal(await page.locator("#c-auto [data-carousel=rotation]").isVisible(), false);
    assert.equal(await page.locator("#c-pickers[data-ready]").count(), 0);
    assert.equal(await page.locator("#c-pickers [role=tab]").first().evaluate((e) => e.tagName), "A");
    const stops = await page.locator("#c-pickers [role=tab]").evaluateAll((els) => els.every((e) => e.tabIndex === 0));
    assert.equal(stops, true, "every link can be reached by keyboard");
    await page.locator("#c-pickers-tab-3").click();
    await until(() => index(page, "c-pickers"), 2, "the link scrolls the track to its slide");
    // The wheel scrolls the track, and it settles on a slide.
    await page.locator("#c-basic [data-slot=carousel-track]").hover();
    await page.mouse.wheel(1000, 0);
    await until(() => index(page, "c-basic").then((i) => i > 0), true, "the wheel scrolls the track");
    // The arrow keys scroll the focused track by a slide where the engine snaps keyboard scrolling.
    if (browserName !== "webkit") {
      await page.locator("#c-pickers [data-slot=carousel-track]").focus();
      await page.keyboard.press("ArrowRight");
      await until(() => index(page, "c-pickers").then((i) => i > 2), true, "ArrowRight scrolls the focused track");
    }
    await close();
  });

  test("hostile text stays text", async () => {
    const { page, close } = await open("/carousel");
    const hostile = page.locator("#c-hostile");
    assert.match(await hostile.getAttribute("aria-label"), /<img src=x onerror=/);
    assert.equal(await hostile.locator("img").count(), 0);
    assert.equal(await page.evaluate(() => window.__pwned), undefined);
    await close();
  });

  for (const [label, options] of [
    ["dark at 1440", {}],
    ["light at 1440", { light: true }],
    ["dark on a phone", { viewport: PHONE }],
    ["light on a phone", { viewport: PHONE, light: true }],
  ]) {
    test(`axe finds nothing in the carousels (${label})`, async () => {
      const { page, close } = await open("/carousel", options);
      await page.waitForSelector("#c-basic[data-ready]");
      assert.deepEqual(await axe(page, "#carousels"), []);
      await page.locator("#c-pickers [data-carousel=next]").click();
      await until(() => index(page, "c-pickers"), 1, "moved");
      assert.deepEqual(await axe(page, "#carousels"), []);
      await close();
    });
  }
});
