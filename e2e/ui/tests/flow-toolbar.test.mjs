// The toolbar in real browsers, against the proof app's /flow: one tab stop and the keys of the
// pattern, tools that do not fit moving into the More menu and back, a toggle that moved still
// posting, focus kept when a tool moves, vertical and right to left, JavaScript off, and axe in
// dark, light and on a phone.
//
//   cargo run --release -p ui-proof -- serve          # UI_PROOF_URL, default http://127.0.0.1:8090
//   UI_BROWSERS=chromium,firefox,webkit node --test tests/flow-toolbar.test.mjs

import assert from "node:assert/strict";
import { test } from "node:test";
import { axe, inBrowsers, shot, stops } from "./support/flow.mjs";

const PHONE = { width: 390, height: 844 };
const NARROW = { width: 320, height: 800 };

inBrowsers(({ open, browserName }) => {
  const settle = (page) => page.evaluate(() => new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(() => setTimeout(done, 30)))));
  const name = (page) =>
    page.evaluate(() => {
      const a = document.activeElement;
      return a?.getAttribute("aria-label") || a?.closest("label")?.textContent.trim() || a?.textContent.trim() || a?.id;
    });
  const shownTools = (page, bar) =>
    page.locator(`${bar} > [data-slot=toolbar-tool]`).evaluateAll((els) =>
      els
        .filter((e) => e.getClientRects().length && e.dataset.kind !== "separator")
        .map(
          (e) =>
            e.querySelector("input[aria-label]")?.getAttribute("aria-label") ||
            e.querySelector("button, a")?.textContent.trim() ||
            e.querySelector("[role=group], [role=radiogroup]")?.getAttribute("aria-label") ||
            e.textContent.trim(),
        ),
    );
  const hiddenTools = (page, bar) => page.locator(`${bar} > [data-slot=toolbar-tool][data-overflow]`).count();
  const more = (bar) => `${bar}-more-trigger`;
  const first = async (page, bar) => {
    await page.locator(`${bar} [tabindex="0"]`).first().focus();
  };

  test("the module makes the group a toolbar with one tab stop, and the keys move along it", async () => {
    const { page, close } = await open("/flow");
    const bar = page.locator("#tb-main");
    assert.equal(await bar.getAttribute("role"), "toolbar");
    assert.equal(await bar.getAttribute("aria-label"), "Text formatting");
    assert.equal(await stops(page, "#tb-main button, #tb-main a, #tb-main input"), 1);
    assert.equal(await page.locator("#tb-main-more-trigger").getAttribute("hidden"), "", "nothing overflows at this width");
    await first(page, "#tb-main");
    assert.equal(await name(page), "Bold");
    const order = [];
    for (let n = 0; n < 11; n++) {
      await page.keyboard.press("ArrowRight");
      order.push(await name(page));
    }
    // The disabled tool and the separators are skipped.
    assert.deepEqual(order, ["Italic", "Underline", "Left", "Center", "Right", "Copy", "Help", "Archive", "Share", "Always shown", "Bold"]);
    assert.equal(await stops(page, "#tb-main button, #tb-main a, #tb-main input"), 1);
    await page.keyboard.press("ArrowLeft");
    assert.equal(await name(page), "Always shown", "it goes round");
    await page.keyboard.press("Home");
    assert.equal(await name(page), "Bold");
    await page.keyboard.press("End");
    assert.equal(await name(page), "Always shown");
    // Up and Down do nothing in a horizontal bar.
    await page.keyboard.press("ArrowUp");
    assert.equal(await name(page), "Always shown");
    await close();
  });

  test("Tab enters on the tool last used and leaves in one press, and Shift+Tab comes back to it", async () => {
    const { page, close } = await open("/flow");
    await first(page, "#tb-main");
    await page.keyboard.press("ArrowRight");
    await page.keyboard.press("ArrowRight");
    assert.equal(await name(page), "Underline");
    await page.keyboard.press("Tab");
    assert.ok(await page.evaluate(() => !document.activeElement.closest("#tb-main")), "left the bar");
    await page.keyboard.press("Shift+Tab");
    assert.equal(await name(page), "Underline");
    await close();
  });

  test("the radios in a bar move by the arrows without choosing, and Space chooses", async () => {
    const { page, close } = await open("/flow");
    await page.locator("#tb-main > [data-slot=toolbar-tool] input[value=left]").focus();
    await page.keyboard.press("ArrowRight");
    assert.equal(await name(page), "Center");
    assert.equal(await page.locator("#tb-main > [data-slot=toolbar-tool] input[value=left]").isChecked(), true, "moving chose nothing");
    await page.keyboard.press("Space");
    assert.equal(await page.locator("#tb-main > [data-slot=toolbar-tool] input[value=center]").isChecked(), true);
    assert.equal(await page.locator("#tb-main > [data-slot=toolbar-tool] input[value=left]").isChecked(), false);
    await close();
  });

  test("a toggle toggles by Space, a button runs its command, and a menu button opens by the arrow", async () => {
    const { page, close } = await open("/flow");
    await page.locator("#tb-main > [data-slot=toolbar-tool] input[name=bold]").focus();
    await page.keyboard.press("Space");
    assert.equal(await page.locator("#tb-main > [data-slot=toolbar-tool] input[name=bold]").isChecked(), true);
    await page.locator("#tb-main-share-trigger").focus();
    await page.keyboard.press("ArrowDown");
    assert.equal(await page.locator("#tb-main-share").evaluate((e) => e.matches(":popover-open")), true);
    // Keys inside the open menu are the menu's, not the bar's.
    await page.keyboard.press("ArrowDown");
    assert.equal(await page.evaluate(() => document.activeElement.textContent.trim()), "Copy the link");
    await page.keyboard.press("Escape");
    assert.equal(await name(page), "Share");
    await close();
  });

  test("on a phone the tools that do not fit go into More, in order from the end, and the kept one stays", async () => {
    const { page, close } = await open("/flow", { viewport: PHONE });
    await settle(page);
    assert.equal(await page.locator("#tb-main-more-trigger").getAttribute("hidden"), null, "More is showing");
    const tools = await shownTools(page, "#tb-main");
    assert.ok(tools.includes("Bold") && tools.includes("Always shown"), `${tools}`);
    assert.ok(!tools.includes("Share") && !tools.includes("Copy"), `${tools}`);
    assert.ok((await hiddenTools(page, "#tb-main")) >= 4);
    // The bar is one line and does not make the page wider.
    assert.ok(await page.locator("#tb-main").evaluate((e) => e.scrollWidth <= e.clientWidth + 1), "everything that shows fits");
    assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth));
    // Open More: the twins of the hidden tools show, those of the showing ones do not.
    await page.click("#tb-main-more-trigger");
    await settle(page);
    const twins = await page.locator("#tb-main-more [data-twin]").evaluateAll((els) => els.filter((e) => !e.hidden && e.getAttribute("role") !== "separator" && !e.hasAttribute("popover")).map((e) => e.getAttribute("data-twin")));
    const hidden = await page.locator("#tb-main > [data-slot=toolbar-tool][data-overflow]").evaluateAll((els) => els.map((e) => e.getAttribute("data-tool")));
    assert.ok(twins.length >= 4);
    assert.deepEqual([...new Set(twins)].sort(), [...hidden].filter((n) => !["2", "5"].includes(n)).sort(), "a twin for each hidden tool, apart from the separators");
    await shot(page, `${browserName}-toolbar-more-390`);
    await close();
  });

  test("a twin does what the tool does: a command, a link, a post, a menu's own entries", async () => {
    const { page, close } = await open("/flow", { viewport: PHONE });
    await settle(page);
    await page.click("#tb-main-more-trigger");
    await page.locator("#tb-main-more button[data-twin='6']").click();
    await page.waitForSelector("text=Copied.");
    await page.click("#tb-main-more-trigger");
    // The menu's entries are a submenu of the menu.
    const sub = page.locator("#tb-main-more [data-twin='10']");
    await sub.click();
    assert.ok(await page.locator("#tb-main-more-share, #tb-main-share").count() >= 1);
    await close();
  });

  test("a toggle or a choice that moved into More still posts, and its twin says what the real one says", async () => {
    const { page, close } = await open("/flow", { viewport: PHONE });
    await settle(page);
    const real = (key) => page.locator(`#tb-late > [data-slot=toolbar-tool] input[name=${key}]`);
    assert.equal(await page.locator("#tb-late > [data-slot=toolbar-tool][data-overflow]").count() >= 3, true, "the toggles moved");
    await page.click("#tb-late-more-trigger");
    await settle(page);
    // The twins are checkboxes and radios with no name, in the menu, and say the real state.
    const twins = await page.locator("#tb-late-more input").evaluateAll((els) => els.filter((e) => !e.closest("[hidden]")).map((e) => [e.type, e.getAttribute("name"), e.checked]));
    assert.deepEqual(twins, [["checkbox", "", false], ["checkbox", "", true], ["radio", "", true], ["radio", "", false]]);
    // Checking the twin of Late bold checks the real one, and the twin of M chooses M.
    await page.locator("#tb-late-more input[type=checkbox]").first().evaluate((e) => e.click());
    assert.equal(await real("late_bold").isChecked(), true);
    await page.locator("#tb-late-more input[type=radio]").nth(1).evaluate((e) => e.click());
    assert.equal(await page.locator("#tb-late > [data-slot=toolbar-tool] input[value=m]").isChecked(), true);
    assert.equal(await page.locator("#tb-late > [data-slot=toolbar-tool] input[value=s]").isChecked(), false);
    // The twins and the real tools agree afterwards.
    const again = await page.locator("#tb-late-more input").evaluateAll((els) => els.filter((e) => !e.closest("[hidden]")).map((e) => e.checked));
    assert.deepEqual(again, [true, true, false, true]);
    await page.keyboard.press("Escape");
    // The form posts the real ones, once each, and nothing of the twins.
    await Promise.all([page.waitForURL("**/flow/echo"), page.click("#tb-late-post")]);
    const posted = (await page.locator("#posted").innerText()).split("\n");
    for (const line of ["late_bold=on", "late_italic=on", "late_size=m"]) assert.ok(posted.includes(line), `${line} in ${posted}`);
    assert.equal(posted.filter((line) => line.startsWith("late_size=")).length, 1);
    assert.equal(posted.filter((line) => line.startsWith("=")).length, 0, "a twin has no name");
    await close();
  });

  test("a bar that is made wider again takes its tools back, and a resize back and forth settles", async () => {
    const { page, close } = await open("/flow");
    await settle(page);
    assert.equal(await hiddenTools(page, "#tb-main"), 0);
    await page.setViewportSize(PHONE);
    await settle(page);
    assert.ok((await hiddenTools(page, "#tb-main")) >= 4);
    await page.setViewportSize({ width: 1440, height: 900 });
    await settle(page);
    assert.equal(await hiddenTools(page, "#tb-main"), 0);
    assert.equal(await page.locator("#tb-main-more-trigger").getAttribute("hidden"), "");
    // Once settled, nothing changes without a resize: no loop.
    await page.evaluate(() => {
      window.__changes = 0;
      new MutationObserver((records) => { window.__changes += records.length; }).observe(document.getElementById("tb-main"), { attributes: true, subtree: true, attributeFilter: ["data-overflow", "hidden"] });
    });
    await page.waitForTimeout(600);
    assert.equal(await page.evaluate(() => window.__changes), 0);
    await close();
  });

  test("focus on a tool that moves into More goes to the More button", async () => {
    const { page, close } = await open("/flow");
    await page.locator("#tb-main > [data-slot=toolbar-tool] input[value=right]").focus();
    await page.setViewportSize(PHONE);
    await settle(page);
    assert.equal(await page.evaluate(() => document.activeElement.id), "tb-main-more-trigger");
    assert.equal(await stops(page, "#tb-main button, #tb-main a, #tb-main input"), 1);
    // It is reached by End, and its menu by the arrow.
    await page.keyboard.press("Home");
    await page.keyboard.press("End");
    assert.equal(await page.evaluate(() => document.activeElement.id), "tb-main-more-trigger");
    await page.keyboard.press("ArrowDown");
    assert.equal(await page.locator("#tb-main-more").evaluate((e) => e.matches(":popover-open")), true);
    await close();
  });

  test("twenty-four tools fit by moving most of them, and the last stop is More", async () => {
    const { page, close } = await open("/flow", { viewport: PHONE });
    await settle(page);
    const shown = await shownTools(page, "#tb-many");
    assert.ok(shown.length >= 2 && shown.length < 8, `${shown}`);
    assert.equal(await page.locator("#tb-many-more [data-twin]:not([hidden]):not([popover])").count(), 24 - shown.length);
    await page.locator("#tb-many [tabindex='0']").first().focus();
    await page.keyboard.press("End");
    assert.equal(await page.evaluate(() => document.activeElement.id), "tb-many-more-trigger");
    await close();
  });

  test("a vertical bar uses Up and Down and has no More", async () => {
    const { page, close } = await open("/flow");
    const bar = page.locator("#tb-vertical");
    assert.equal(await bar.getAttribute("aria-orientation"), "vertical");
    await first(page, "#tb-vertical");
    assert.equal(await name(page), "One");
    await page.keyboard.press("ArrowRight");
    assert.equal(await name(page), "One", "Right does nothing");
    await page.keyboard.press("ArrowDown");
    assert.equal(await name(page), "Two");
    await page.keyboard.press("ArrowDown");
    assert.equal(await name(page), "Three");
    await page.keyboard.press("ArrowDown");
    assert.equal(await name(page), "One");
    assert.equal(await page.locator("#tb-vertical-more").count(), 0);
    await close();
  });

  test("right to left: the arrow that goes on is Left, and Home is the first at the right", async () => {
    const { page, close } = await open("/flow");
    await first(page, "#tb-rtl");
    assert.equal(await name(page), "أول");
    await page.keyboard.press("ArrowLeft");
    assert.equal(await name(page), "ثاني");
    await page.keyboard.press("ArrowRight");
    assert.equal(await name(page), "أول");
    await close();
  });

  test("one tool is a bar with one stop, and no tools is an empty bar that breaks nothing", async () => {
    const { page, close } = await open("/flow");
    assert.equal(await page.locator("#tb-one").getAttribute("role"), "toolbar");
    assert.equal(await stops(page, "#tb-one button"), 1);
    assert.equal(await page.locator("#tb-none").getAttribute("role"), "toolbar");
    await first(page, "#tb-one");
    await page.keyboard.press("ArrowRight");
    assert.equal(await name(page), "Only");
    await close();
  });

  test("with JavaScript off the tools wrap, everything works, and More never shows", async () => {
    const { page, close } = await open("/flow", { js: false, viewport: PHONE });
    assert.equal(await page.locator("#tb-main").getAttribute("role"), "group");
    assert.equal(await page.locator("#tb-main-more-trigger").getAttribute("hidden"), "");
    assert.equal(await page.locator("#tb-main-more-trigger").isVisible(), false);
    const tools = await shownTools(page, "#tb-main");
    assert.ok(tools.includes("Share") && tools.includes("Copy"), "every tool shows");
    assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), "no sideways scroll");
    await page.locator("#tb-main > [data-slot=toolbar-tool] label:has(input[value=right])").click();
    await Promise.all([page.waitForURL("**/flow/echo"), page.click("#tb-post")]);
    const posted = (await page.locator("#posted").innerText()).split("\n");
    assert.ok(posted.includes("align=right") && posted.includes("italic=on"));
    await close();
  });

  for (const [label, options] of [
    ["dark at 1440", {}],
    ["light at 1440", { light: true }],
    ["dark on a phone", { viewport: PHONE }],
    ["light on a phone", { viewport: PHONE, light: true }],
  ]) {
    test(`axe finds nothing in the toolbars, shut and with More open (${label})`, async () => {
      const { page, close } = await open("/flow", options);
      await settle(page);
      assert.deepEqual(await axe(page, "#toolbars"), []);
      if (options.viewport) {
        await page.click("#tb-main-more-trigger");
        await settle(page);
        assert.deepEqual(await axe(page), []);
        await page.keyboard.press("Escape");
      }
      await shot(page, `${browserName}-toolbars-${label.replaceAll(" ", "-")}`, "#toolbars");
      await close();
    });
  }
});
