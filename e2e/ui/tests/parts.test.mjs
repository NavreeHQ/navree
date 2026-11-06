// The parts module and its stylesheet in real browsers, against the proof app's /parts: an
// alert's close button and actions, the description under a box and a box that is neither
// checked nor unchecked, a long breadcrumb folded, an input with a clear button, the colour
// picker, an avatar group and an avatar whose picture fails, a trace, a questionnaire, a
// select with a flag in each choice, flags drawn by pointer, and a sparkline in each row of a
// table. Each with JavaScript on and off where it claims both, with axe in dark and light.
//
//   cargo run --release -p ui-proof -- serve          # UI_PROOF_URL, default http://127.0.0.1:8090
//   UI_BROWSERS=chromium,firefox,webkit node --test tests/parts.test.mjs

import assert from "node:assert/strict";
import { test } from "node:test";
import { axe, inBrowsers, shot } from "./support/flow.mjs";

const PHONE = { width: 390, height: 844 };

inBrowsers(({ open }) => {
  test("an alert closes itself with a button that only a script draws, and keeps its actions", async () => {
    const { page, close } = await open("/parts");
    const alert = page.locator("#trial-notice");
    assert.ok(await alert.isVisible());
    assert.equal(await alert.locator("[data-slot=alert-actions] a").first().innerText(), "Add payment");
    const closeButton = alert.locator("[data-slot=alert-close] button");
    assert.equal(await closeButton.getAttribute("aria-label"), "Dismiss this notice");
    await closeButton.click();
    await alert.waitFor({ state: "hidden" });
    await close();

    const off = await open("/parts", { js: false });
    assert.ok(await off.page.locator("#trial-notice").isVisible());
    assert.equal(await off.page.locator("#trial-notice [data-slot=alert-close] button").isVisible(), false, "no close button without a script");
    await off.close();
  });

  test("a description is read after the label, and an indeterminate box becomes checked when pressed", async () => {
    const { page, close } = await open("/parts");
    const box = page.locator("#parts-news");
    const described = await box.getAttribute("aria-describedby");
    assert.ok(described);
    assert.match(await page.locator("#" + described).innerText(), /a month at most/i);
    const mixed = page.locator("#parts-all");
    assert.equal(await mixed.evaluate((e) => e.indeterminate), true, "the module made it so");
    await mixed.click();
    assert.equal(await mixed.evaluate((e) => [e.checked, e.indeterminate].join()), "true,false");
    const switchBox = page.locator("#parts-alerts");
    assert.equal(await switchBox.getAttribute("role"), "switch");
    assert.match(await page.locator("#" + (await switchBox.getAttribute("aria-describedby"))).innerText(), /payment fails/i);
    await close();
  });

  test("a long breadcrumb folds its middle into a list that opens without a script", async () => {
    for (const js of [true, false]) {
      const { page, close } = await open("/parts", { js });
      const trail = page.locator("#parts-trail");
      assert.equal(await trail.locator("> ol > li").count(), 4, "the first, the fold, and the last two");
      assert.equal(await trail.locator("[data-slot=breadcrumb-hidden] a").count(), 3);
      assert.equal(await trail.locator("[data-slot=breadcrumb-hidden]").isVisible(), false);
      await trail.locator("summary").click();
      await trail.locator("[data-slot=breadcrumb-hidden]").waitFor({ state: "visible" });
      assert.equal(await trail.locator("summary").getAttribute("aria-label"), "Show hidden pages");
      assert.equal(await trail.locator("[aria-current=page]").innerText(), "Invoices");
      await close();
    }
  });

  test("an input with a clear button shows it while it holds something, empties it and keeps the caret in it", async () => {
    const { page, close } = await open("/parts");
    const input = page.locator("#parts-q");
    const clear = page.locator("#parts-q-group [data-ui-clear]");
    assert.equal(await input.inputValue(), "kurta");
    assert.ok(await clear.isVisible());
    await clear.click();
    assert.equal(await input.inputValue(), "");
    assert.equal(await page.evaluate(() => document.activeElement?.id), "parts-q", "the focus is back in the field");
    assert.equal(await clear.isVisible(), false, "nothing to clear");
    await input.pressSequentially("lehenga");
    assert.ok(await clear.isVisible());
    assert.equal(await page.locator("#parts-small").evaluate((e) => e.getBoundingClientRect().height < document.querySelector("#parts-q").getBoundingClientRect().height), true, "the small input is shorter");
    await close();

    const off = await open("/parts", { js: false });
    assert.equal(await off.page.locator("#parts-q-group [data-ui-clear]").isVisible(), false, "a script draws it");
    await off.close();
  });

  test("the colour picker keeps its colour field, hex field and swatches in step, and posts a colour", async () => {
    const { page, close } = await open("/parts");
    const native = page.locator("#parts-accent");
    const hex = page.locator("#parts-colour [data-slot=color-hex]");
    assert.equal(await native.inputValue(), "#ff9933");
    const green = page.locator("#parts-colour [data-ui-color-value='#138808']");
    assert.equal(await green.getAttribute("aria-label"), "Green");
    await green.click();
    assert.equal(await native.inputValue(), "#138808");
    assert.equal(await hex.inputValue(), "#138808");
    assert.equal(await green.getAttribute("aria-pressed"), "true");
    assert.equal(await page.locator("#parts-colour [aria-pressed=true]").count(), 1);
    // Typing a short code changes the colour as soon as it is whole, and the field shows the long form when left.
    await hex.fill("#abc");
    assert.equal(await native.inputValue(), "#aabbcc");
    await hex.press("Tab");
    assert.equal(await hex.inputValue(), "#aabbcc");
    // Something that is no colour goes back.
    await hex.fill("saffron");
    await hex.press("Tab");
    assert.equal(await hex.inputValue(), "#aabbcc");
    assert.equal(await native.inputValue(), "#aabbcc");
    // Only the colour field posts.
    const names = await page.locator("#parts-colour-form").evaluate((form) => [...new FormData(form).keys()].join());
    assert.equal(names, "accent");
    await close();

    const off = await open("/parts", { js: false });
    assert.equal(await off.page.locator("#parts-colour [data-slot=color-hex]").isVisible(), false, "the hex field is for a script");
    assert.equal(await off.page.locator("#parts-accent").isVisible(), true, "the colour field posts alone");
    await off.close();
  });

  test("an avatar group counts the rest, and an avatar whose picture fails shows the person icon", async () => {
    const { page, close } = await open("/parts");
    const group = page.locator("#parts-people");
    assert.equal(await group.locator("[data-slot=avatar]").count(), 4, "three people and the tile");
    assert.equal(await group.locator("[data-count]").innerText(), "+45");
    assert.equal(await group.locator("[data-count]").getAttribute("aria-label"), "45 more");
    const broken = page.locator("#parts-broken");
    await broken.waitFor();
    await page.waitForFunction(() => document.querySelector("#parts-broken")?.hasAttribute("data-failed"));
    assert.equal(await broken.locator("img").evaluate((img) => getComputedStyle(img).opacity), "0");
    assert.equal(await broken.locator("img").getAttribute("alt"), "Gurpreet Kaur", "the name stays for a reader");
    assert.ok((await broken.evaluate((e) => getComputedStyle(e, "::after").content)) !== "none", "the icon is drawn");
    await close();
  });

  test("a trace names each state to a reader and marks the running step as current", async () => {
    const { page, close } = await open("/parts");
    const trace = page.locator("#parts-trace");
    assert.equal(await trace.locator("[aria-current=step]").count(), 1);
    assert.deepEqual(await trace.locator("[data-slot=trace-state]").allTextContents(), ["Done", "Running", "Not started", "Failed"]);
    const detail = trace.locator("details[data-slot=trace-detail]").first();
    assert.equal(await detail.evaluate((e) => e.open), true);
    await detail.locator("summary").click();
    assert.equal(await detail.evaluate((e) => e.open), false, "it closes with no script");
    await close();
  });

  test("a questionnaire shows one question and posts its buttons' words", async () => {
    const { page, close } = await open("/parts");
    const form = page.locator("#parts-quiz");
    assert.equal(await form.locator("[data-slot=questionnaire-question] legend").innerText(), "What do you use today?");
    assert.equal(await form.locator("progress").getAttribute("aria-label"), "Question 2 of 3");
    const buttons = await form.locator("[data-slot=questionnaire-actions] button").evaluateAll((all) => all.map((b) => `${b.name}=${b.value}${b.hasAttribute("formnovalidate") ? "!" : ""}`));
    assert.deepEqual(buttons, ["_move=back!", "_move=skip!", "_move=next"]);
    assert.equal(await form.locator("input[type=hidden][name=team]").getAttribute("value"), "small");
    assert.equal(await form.locator("input[type=checkbox][name=tools]").count(), 3);
    await close();
  });

  test("a flag leads a choice in the list and in the control, and flags by pointer draw from one definition", async () => {
    const { page, close } = await open("/parts");
    const trigger = page.locator("#parts-country [data-slot=select-trigger]");
    await trigger.waitFor();
    assert.equal(await trigger.locator("[data-slot=flag]").count(), 1);
    const before = await trigger.locator("[data-slot=flag]").innerHTML();
    await trigger.click();
    await page.getByRole("option", { name: "Nepal" }).click();
    const after = await trigger.locator("[data-slot=flag]").innerHTML();
    assert.notEqual(after, before, "the control shows the flag of the new choice");
    assert.equal(await trigger.locator("[data-slot=select-value]").innerText(), "Nepal");
    // A hundred flags drawn by pointer: one definition, and every flag has a size.
    const sprite = page.locator("[data-slot=flag-sprite]");
    assert.equal(await sprite.locator("symbol").count(), 2, "Serbia and India, once");
    const sizes = await page.locator("#parts-flags [data-slot=flag]").evaluateAll((all) => all.map((svg) => svg.getBoundingClientRect().width));
    assert.equal(sizes.length, 40);
    assert.ok(sizes.every((w) => w > 10), `${sizes}`);
    const bytes = await page.locator("#parts-flags").evaluate((e) => e.outerHTML.length);
    assert.ok(bytes < 40 * 400, `${bytes} bytes for forty flags`);
    await close();
  });

  test("a sparkline in each row of a table is one named picture, and the table holds no other table", async () => {
    const { page, close } = await open("/parts");
    const cells = page.locator("#parts-spark [data-slot=sparkline-cell]");
    assert.equal(await cells.count(), 50);
    assert.equal(await page.locator("#parts-spark table table").count(), 0);
    assert.equal(await cells.first().getAttribute("role"), "img");
    assert.match(await cells.first().getAttribute("aria-label"), /Orders of/);
    const box = await cells.first().boundingBox();
    assert.ok(box && box.width > 40 && box.height > 10, JSON.stringify(box));
    await close();
  });

  for (const [name, options] of [
    ["dark at 1440 px", {}],
    ["light at 1440 px", { light: true }],
    ["dark on a phone", { viewport: PHONE }],
  ]) {
    test(`axe finds nothing on the page, ${name}`, async () => {
      const { page, close } = await open("/parts", options);
      await page.locator("#parts-all").evaluate((e) => e.indeterminate);
      await shot(page, `parts-${name.replace(/\W+/g, "-")}`);
      assert.deepEqual(await axe(page), []);
      // With a popup open and the folded trail open, too.
      await page.locator("#parts-trail summary").click();
      assert.deepEqual(await axe(page), []);
      const overflow = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
      assert.ok(overflow <= 0, `${overflow}px of sideways scroll`);
      await close();
    });
  }
});
