// The context menu, the menu bar and the navigation menu in real browsers, against the proof app's
// /menubar: the keys of each pattern, the pointer, a long press, placement at the edges, right to
// left, a phone, JavaScript off, hostile text, and axe in dark and light with a menu open.
//
//   cargo run --release -p ui-proof -- serve          # UI_PROOF_URL, default http://127.0.0.1:8090
//   UI_BROWSERS=chromium,firefox,webkit node --test tests/menubar.test.mjs

import assert from "node:assert/strict";
import { test } from "node:test";
import { axe, inBrowsers, shot, tab } from "./support/flow.mjs";

const PHONE = { width: 390, height: 844 };
const PATH = "/menubar";

/** What has focus: its id, its role, and the words of the menu item it is in. */
const focused = (page) =>
  page.evaluate(() => {
    const e = document.activeElement;
    const item = e?.closest?.("[data-slot=menu-item]");
    return {
      id: e?.id || "",
      tag: e?.tagName?.toLowerCase() || "",
      text: (item?.querySelector("[data-slot=menu-text]")?.textContent || e?.textContent || "").trim(),
    };
  });
const isOpen = (page, selector) => page.locator(selector).evaluate((e) => e.matches(":popover-open"));
const box = (page, selector) => page.locator(selector).boundingBox();
const expanded = (page, selector) => page.locator(selector).getAttribute("aria-expanded");
/**
 * Waits for a popup to finish opening. The suite runs with reduced motion, whose rule leaves every
 * change a transition of one frame, and the popup its scale-in: a box read in that frame is where
 * the popup was before it was placed, a little scaled.
 */
const settled = (page, selector) => page.waitForFunction((sel) => document.querySelector(sel).getAnimations().every((a) => a.playState === "finished"), selector);
/** Waits for the overlay module to place a popup: it does when the browser reports the popup open, a frame after it shows. */
const placed = (page, selector) => page.waitForFunction((sel) => document.querySelector(sel).style.top !== "", selector);
/** Waits for focus to reach the item `text`: a menu a button opens takes focus when the browser reports it open, as the dropdown's does. */
const focusOn = (page, text) =>
  page
    .waitForFunction((t) => {
      const e = document.activeElement;
      const item = e?.closest?.("[data-slot=menu-item]");
      return (item?.querySelector("[data-slot=menu-text]")?.textContent || e?.textContent || "").trim() === t;
    }, text, { timeout: 2000 })
    .catch(() => {});

inBrowsers(({ open, browserName }) => {
  // ---- Context menu -------------------------------------------------------

  /** Right clicks the middle of the order card. */
  async function rightClick(page, selector = "#ctx-order-area", at) {
    const area = await box(page, selector);
    const x = at?.x ?? area.x + 20;
    const y = at?.y ?? area.y + 8;
    await page.mouse.click(x, y, { button: "right" });
    return { x, y };
  }

  test("a right click opens the menu where the pointer is, and the menu has the roles of a menu", async () => {
    const { page, close } = await open(PATH);
    const at = await rightClick(page);
    assert.ok(await isOpen(page, "#ctx-order"));
    await settled(page, "#ctx-order");
    const menu = await box(page, "#ctx-order");
    assert.ok(Math.abs(menu.x - at.x) <= 2 && Math.abs(menu.y - at.y) <= 2, `at the pointer: ${JSON.stringify(menu)} ${JSON.stringify(at)}`);
    assert.equal(await page.locator("#ctx-order").getAttribute("role"), "menu");
    assert.equal(await page.locator("#ctx-order").getAttribute("aria-label"), "Order actions");
    assert.equal((await focused(page)).id, "ctx-order", "a pointer opens it with focus on the menu, as a dropdown does");
    await shot(page, `${browserName}-context-menu`);
    await close();
  });

  test("the arrow keys, Home, End and type-ahead move through the items, and Enter takes the one in focus", async () => {
    const { page, close } = await open(PATH);
    await rightClick(page);
    await page.keyboard.press("ArrowDown");
    assert.equal((await focused(page)).text, "Open order");
    await page.keyboard.press("ArrowDown");
    assert.equal((await focused(page)).text, "Copy the order number");
    await page.keyboard.press("End");
    assert.equal((await focused(page)).text, "Cancel order");
    await page.keyboard.press("Home");
    assert.equal((await focused(page)).text, "Open order");
    await page.keyboard.press("m");
    assert.equal((await focused(page)).text, "Mark as paid");
    // A disabled item is reached and does nothing. (Letters typed together are one word: wait between.)
    await page.waitForTimeout(650);
    await page.keyboard.press("r");
    assert.equal((await focused(page)).text, "Refund");
    await page.keyboard.press("Enter");
    assert.ok(await isOpen(page, "#ctx-order"), "a disabled item doesn't close the menu");
    await page.keyboard.press("Home");
    await page.keyboard.press("Enter");
    await page.waitForURL(/\/menubar\/go\?to=open/);
    await close();
  });

  test("Escape closes it and focus goes back to the region; a press outside closes it too", async () => {
    const { page, close } = await open(PATH);
    await rightClick(page);
    await page.keyboard.press("ArrowDown");
    await page.keyboard.press("Escape");
    assert.ok(!(await isOpen(page, "#ctx-order")));
    assert.equal(await page.evaluate(() => document.activeElement?.getAttribute("data-ui-context")), "ctx-order", "the focusable region has focus again");
    await rightClick(page);
    assert.ok(await isOpen(page, "#ctx-order"));
    await page.mouse.click(5, 400);
    assert.ok(!(await isOpen(page, "#ctx-order")));
    // A plain left click does not open it.
    await page.locator("#ctx-order-area").click();
    assert.ok(!(await isOpen(page, "#ctx-order")));
    await close();
  });

  test("Tab closes it and moves on from the region", async () => {
    const { page, close } = await open(PATH);
    await rightClick(page);
    await page.keyboard.press("ArrowDown");
    await page.keyboard.press("Tab");
    assert.ok(!(await isOpen(page, "#ctx-order")));
    assert.notEqual(await page.evaluate(() => document.activeElement?.tagName), "BODY", "focus didn't fall to the page");
    await close();
  });

  test("Shift+F10 and the menu key open it from the keyboard, at the focused element, with focus on the first item", async () => {
    const { page, close } = await open(PATH);
    await page.locator("[data-ui-context=ctx-order]").focus();
    await page.keyboard.press("Shift+F10");
    assert.ok(await isOpen(page, "#ctx-order"));
    assert.equal((await focused(page)).text, "Open order");
    const region = await box(page, "[data-ui-context=ctx-order]");
    const menu = await box(page, "#ctx-order");
    assert.ok(Math.abs(menu.x - region.x) <= 2 && menu.y >= region.y, "under the focused region");
    await page.keyboard.press("Escape");
    assert.ok(!(await isOpen(page, "#ctx-order")));
    await page.keyboard.press("ContextMenu");
    assert.ok(await isOpen(page, "#ctx-order"));
    await page.keyboard.press("Escape");
    // From a link inside a region that isn't focusable itself, the keys work from the link.
    await page.locator("#ctx-link").focus();
    await page.keyboard.press("Shift+F10");
    assert.ok(await isOpen(page, "#ctx-mixed"));
    await page.keyboard.press("Escape");
    assert.equal((await focused(page)).id, "ctx-link", "focus goes back to the link");
    await close();
  });

  test("a submenu opens with Right, closes with Left and Escape closes one level at a time", async () => {
    const { page, close } = await open(PATH);
    await rightClick(page);
    await page.keyboard.press("ArrowDown");
    await page.keyboard.press("m");
    await page.keyboard.press("m");
    assert.equal((await focused(page)).text, "Move to");
    await page.keyboard.press("ArrowRight");
    assert.equal((await focused(page)).text, "Jalandhar");
    await page.keyboard.press("ArrowLeft");
    assert.equal((await focused(page)).text, "Move to");
    await page.keyboard.press("ArrowRight");
    await page.keyboard.press("Escape");
    assert.ok(await isOpen(page, "#ctx-order"), "the first Escape closed the submenu only");
    await page.keyboard.press("Escape");
    assert.ok(!(await isOpen(page, "#ctx-order")));
    await close();
  });

  test("a checkbox and a radio choose and close the menu, and a post item posts", async () => {
    const { page, close } = await open(PATH);
    await rightClick(page);
    const flag = page.locator("#ctx-order input[name=flag]");
    assert.ok(await flag.isChecked());
    await page.locator("#ctx-order").getByText("Flag for follow-up").click();
    assert.ok(!(await flag.isChecked()), "the choice was made");
    assert.ok(!(await isOpen(page, "#ctx-order")), "choosing closes the menu");
    await rightClick(page);
    await page.locator("#ctx-order").getByText("Amritsar").click();
    assert.ok(await page.locator("#ctx-order input[name=warehouse][value=asr]").isChecked());
    await rightClick(page);
    await page.locator("#ctx-order").getByText("Mark as paid").click();
    await page.locator("[data-slot=toast]").first().waitFor();
    assert.match(await page.locator("[data-slot=toast]").first().innerText(), /Posted: paid/);
    assert.ok(!(await isOpen(page, "#ctx-order")));
    await close();
  });

  test("the menu is placed inside the window when the pointer is at its far corner", async () => {
    const { page, close } = await open(PATH, { viewport: { width: 800, height: 600 } });
    await page.locator("#ctx-order-area").scrollIntoViewIfNeeded();
    // Right click as near the bottom right corner of the window as the card reaches.
    const area = await box(page, "#ctx-order-area");
    await page.mouse.click(Math.min(area.x + area.width - 4, 795), Math.min(area.y + area.height - 4, 595), { button: "right" });
    await settled(page, "#ctx-order");
    const menu = await box(page, "#ctx-order");
    assert.ok(menu.x >= 0 && menu.y >= 0 && menu.x + menu.width <= 800 && menu.y + menu.height <= 600, JSON.stringify(menu));
    await close();
  });

  test("a field keeps the browser's own menu, a link and the card take the menu, and so does a right click on the menu itself", async () => {
    const { page, close } = await open(PATH);
    const cancelled = (selector) =>
      page.locator(selector).evaluate((e) => !e.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: 20, clientY: 20 })));
    assert.equal(await cancelled("#ctx-input"), false, "the field's menu (copy and paste) stays");
    assert.equal(await cancelled("#ctx-link"), true);
    assert.ok(await isOpen(page, "#ctx-mixed"));
    assert.equal(await cancelled("#ctx-mixed"), true, "a right click on the open menu is not the browser's");
    await close();
  });

  test("a second right click moves the menu, and a scroll closes it", async () => {
    // A window shorter than the page, so that the page scrolls.
    const { page, close } = await open(PATH, { viewport: { width: 1440, height: 600 } });
    const area = await box(page, "#ctx-order-area");
    await rightClick(page, "#ctx-order-area", { x: area.x + 400, y: area.y + 30 });
    await settled(page, "#ctx-order");
    const first = await box(page, "#ctx-order");
    await rightClick(page, "#ctx-order-area", { x: area.x + 20, y: area.y + 30 });
    await settled(page, "#ctx-order");
    const second = await box(page, "#ctx-order");
    assert.ok(second.x < first.x - 100, `it moved with the pointer: ${first.x} ${second.x}`);
    await page.mouse.move(1380, 300);
    await page.mouse.wheel(0, 300);
    await page.waitForFunction(() => !document.getElementById("ctx-order").matches(":popover-open"), null, { timeout: 2000 });
    await close();
  });

  test("a long press opens it on a touch screen, a press that moves or ends early does not", async () => {
    const { page, close } = await open(PATH);
    const area = await box(page, "#ctx-order-area");
    const x = area.x + 40;
    const y = area.y + 30;
    const fire = (type, dx = 0) =>
      page.locator("#ctx-order-area").evaluate(
        (e, [t, cx, cy]) => e.dispatchEvent(new PointerEvent(t, { bubbles: true, cancelable: true, pointerType: "touch", pointerId: 7, isPrimary: true, clientX: cx, clientY: cy, button: 0 })),
        [type, x + dx, y],
      );
    await fire("pointerdown");
    await page.waitForTimeout(250);
    await fire("pointerup");
    await page.waitForTimeout(500);
    assert.ok(!(await isOpen(page, "#ctx-order")), "released early");
    await fire("pointerdown");
    await page.waitForTimeout(150);
    await fire("pointermove", 30);
    await page.waitForTimeout(600);
    assert.ok(!(await isOpen(page, "#ctx-order")), "moved: it's a scroll or a drag");
    await fire("pointerup", 30);
    await fire("pointerdown");
    await page.waitForTimeout(650);
    assert.ok(await isOpen(page, "#ctx-order"), "held");
    await fire("pointerup");
    assert.ok(await isOpen(page, "#ctx-order"), "lifting the finger leaves it open");
    await close();
  });

  test("with JavaScript off the menu stays closed and the browser keeps its own", async () => {
    const { page, close } = await open(PATH, { js: false });
    await page.mouse.click(300, 400, { button: "right" });
    assert.equal(await page.locator("#ctx-order").isVisible(), false);
    await close();
  });

  test("hostile text is text: no element, no script, and an unsafe link goes nowhere", async () => {
    const { page, close, problems } = await open(PATH);
    await rightClick(page, "#ctx-hostile-area");
    assert.ok(await isOpen(page, "#ctx-hostile"));
    assert.equal(await page.locator("#ctx-hostile img, #contexts img").count(), 0);
    assert.match(await page.locator("#ctx-hostile").innerText(), /<img src=x onerror=alert\(1\)>/);
    assert.equal(await page.locator("#ctx-hostile a[href^=javascript]").count(), 0);
    const fits = await page.locator("#ctx-hostile").evaluate((e) => e.scrollWidth <= e.clientWidth + 1);
    assert.ok(fits, "a long label wraps");
    assert.deepEqual(problems, []);
    await close();
  });

  for (const [label, options] of [
    ["dark at 1440", {}],
    ["light at 1440", { light: true }],
    ["dark on a phone", { viewport: PHONE }],
    ["light on a phone", { light: true, viewport: PHONE }],
  ]) {
    test(`axe finds nothing in the context menus, shut and open (${label})`, async () => {
      const { page, close } = await open(PATH, options);
      assert.deepEqual(await axe(page, "#contexts"), []);
      await rightClick(page);
      await page.keyboard.press("ArrowDown");
      assert.deepEqual(await axe(page, "#contexts"), []);
      await close();
    });
  }

  // ---- Menu bar -----------------------------------------------------------

  const buttons = (page, bar = "bar") => page.locator(`#${bar} [data-slot=menubar-trigger]`);

  test("a menu bar is one tab stop, and Tab leaves it", async () => {
    const { page, close } = await open(PATH);
    const stops = await buttons(page).evaluateAll((all) => all.filter((b) => b.tabIndex === 0).length);
    assert.equal(stops, 1);
    assert.equal(await page.locator("#bar").getAttribute("role"), "menubar");
    assert.equal(await buttons(page).first().getAttribute("role"), "menuitem");
    await buttons(page).first().focus();
    await page.keyboard.press("Tab");
    assert.equal(await page.evaluate(() => !!document.activeElement.closest("#bar")), false);
    await page.keyboard.press("Shift+Tab");
    assert.equal((await focused(page)).id, "bar-m1-trigger", "back to the bar's one stop");
    await close();
  });

  test("Left and Right move between the buttons with wrap, Home and End go to the ends, and the stop follows", async () => {
    const { page, close } = await open(PATH);
    await buttons(page).first().focus();
    const ids = [];
    for (let n = 0; n < 5; n++) {
      await page.keyboard.press("ArrowRight");
      ids.push((await focused(page)).text);
    }
    assert.deepEqual(ids, ["View", "Reports", "Admin", "Help", "File"], "a disabled button is reached too");
    await page.keyboard.press("ArrowLeft");
    assert.equal((await focused(page)).text, "Help", "wraps backwards");
    await page.keyboard.press("Home");
    assert.equal((await focused(page)).text, "File");
    await page.keyboard.press("End");
    assert.equal((await focused(page)).text, "Help");
    const stops = await buttons(page).evaluateAll((all) => all.filter((b) => b.tabIndex === 0).map((b) => b.textContent.trim()));
    assert.deepEqual(stops, ["Help"]);
    await close();
  });

  test("Down opens a menu on its first item, Up on its last, and Enter and Space open it too", async () => {
    const { page, close } = await open(PATH);
    await buttons(page).first().focus();
    await page.keyboard.press("ArrowDown");
    assert.equal(await expanded(page, "#bar-m1-trigger"), "true");
    assert.equal((await focused(page)).text, "New invoice");
    await page.keyboard.press("Escape");
    assert.equal(await expanded(page, "#bar-m1-trigger"), "false");
    assert.equal((await focused(page)).id, "bar-m1-trigger", "Escape returns to the button");
    await page.keyboard.press("ArrowUp");
    assert.equal((await focused(page)).text, "Export", "the last item");
    await page.keyboard.press("Escape");
    // Enter and Space press the button, and the browser opens the menu: focus follows.
    await page.keyboard.press("Enter");
    await focusOn(page, "New invoice");
    assert.equal((await focused(page)).text, "New invoice");
    await page.keyboard.press("Escape");
    await page.keyboard.press(" ");
    await focusOn(page, "New invoice");
    assert.equal((await focused(page)).text, "New invoice");
    await close();
  });

  test("with a menu open, Right and Left go to the next and previous button and open its menu", async () => {
    const { page, close } = await open(PATH);
    await buttons(page).first().focus();
    await page.keyboard.press("ArrowDown");
    await page.keyboard.press("ArrowRight");
    assert.equal(await expanded(page, "#bar-m1-trigger"), "false");
    assert.equal(await expanded(page, "#bar-m2-trigger"), "true");
    assert.equal((await focused(page)).text, "Show the grid");
    await page.keyboard.press("ArrowRight");
    assert.equal(await expanded(page, "#bar-m3-trigger"), "true");
    await page.keyboard.press("ArrowRight");
    assert.equal(await expanded(page, "#bar-m3-trigger"), "false", "Admin has no menu: the open one closes");
    assert.equal((await focused(page)).text, "Admin");
    await page.keyboard.press("ArrowLeft");
    assert.equal((await focused(page)).text, "Reports");
    assert.equal(await expanded(page, "#bar-m3-trigger"), "false", "with no menu open, an arrow only moves focus");
    await page.keyboard.press("ArrowDown");
    await page.keyboard.press("ArrowLeft");
    await page.keyboard.press("ArrowLeft");
    assert.equal(await expanded(page, "#bar-m1-trigger"), "true", "with a menu open, Left opens the one before");
    await close();
  });

  test("in a submenu Left closes the submenu only, then goes to the previous button", async () => {
    const { page, close } = await open(PATH);
    await buttons(page).first().focus();
    await page.keyboard.press("ArrowUp");
    assert.equal((await focused(page)).text, "Export");
    await page.keyboard.press("ArrowRight");
    assert.equal((await focused(page)).text, "PDF");
    await page.keyboard.press("ArrowLeft");
    assert.equal((await focused(page)).text, "Export", "only the submenu closed");
    assert.equal(await expanded(page, "#bar-m1-trigger"), "true");
    await page.keyboard.press("ArrowRight");
    await page.keyboard.press("ArrowRight");
    assert.equal(await expanded(page, "#bar-m2-trigger"), "true", "from inside a submenu, Right goes to the next button");
    await close();
  });

  test("with a menu open, the pointer moving to another button opens that menu", async () => {
    const { page, close } = await open(PATH);
    await buttons(page).first().click();
    assert.equal(await expanded(page, "#bar-m1-trigger"), "true");
    await buttons(page).nth(2).hover();
    await page.waitForFunction(() => document.getElementById("bar-m3-trigger").getAttribute("aria-expanded") === "true");
    assert.equal(await expanded(page, "#bar-m1-trigger"), "false");
    await close();
  });

  test("checkbox and radio items choose with Space, and a disabled menu opens nothing", async () => {
    const { page, close } = await open(PATH);
    await buttons(page).nth(1).focus();
    await page.keyboard.press("ArrowDown");
    assert.equal((await focused(page)).text, "Show the grid");
    await page.keyboard.press(" ");
    assert.ok(!(await page.locator("#bar input[name=grid]").isChecked()), "Space chose it");
    assert.ok(!(await isOpen(page, "#bar-m2")), "and closed the menu");
    await page.keyboard.press("ArrowDown");
    await page.keyboard.press("ArrowDown");
    await page.keyboard.press("ArrowDown");
    assert.equal((await focused(page)).text, "125%");
    await page.keyboard.press(" ");
    assert.ok(await page.locator("#bar input[name=zoom][value='125']").isChecked());
    await buttons(page).nth(3).focus();
    await page.keyboard.press("Enter");
    await page.keyboard.press("ArrowDown");
    assert.equal(await page.locator("#bar [popover]:popover-open").count(), 0);
    assert.equal(await buttons(page).nth(3).getAttribute("aria-disabled"), "true");
    await close();
  });

  test("in a right-to-left page Left is the next button", async () => {
    const { page, close } = await open(PATH);
    await buttons(page, "bar-rtl").first().focus();
    await page.keyboard.press("ArrowLeft");
    assert.equal((await focused(page)).id, "bar-rtl-m2-trigger");
    await page.keyboard.press("ArrowDown");
    await page.keyboard.press("ArrowLeft");
    assert.equal(await expanded(page, "#bar-rtl-m3-trigger"), "true");
    const first = await box(page, "#bar-rtl-m1-trigger");
    const second = await box(page, "#bar-rtl-m2-trigger");
    assert.ok(first.x > second.x, "the first button is at the right");
    await close();
  });

  test("on a phone the bar wraps and the page has no sideways scroll", async () => {
    const { page, close } = await open(PATH, { viewport: PHONE });
    assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), "no sideways scroll");
    await buttons(page).first().click();
    const menu = await box(page, "#bar-m1");
    assert.ok(menu.x >= 0 && menu.x + menu.width <= 390, JSON.stringify(menu));
    await shot(page, `${browserName}-menubar-390`);
    await close();
  });

  test("with JavaScript off every button opens its menu, every button is a tab stop, and the bar closes with Escape", async () => {
    const { page, close } = await open(PATH, { js: false });
    await buttons(page).first().click();
    assert.ok(await isOpen(page, "#bar-m1"));
    await page.keyboard.press("Escape");
    assert.ok(!(await isOpen(page, "#bar-m1")));
    const tabbable = await buttons(page).evaluateAll((all) => all.filter((b) => b.tabIndex === 0).length);
    assert.equal(tabbable, 5);
    await close();
  });

  for (const [label, options] of [
    ["dark at 1440", {}],
    ["light at 1440", { light: true }],
    ["dark on a phone", { viewport: PHONE }],
    ["light on a phone", { light: true, viewport: PHONE }],
  ]) {
    test(`axe finds nothing in the menu bars, shut and with a menu open (${label})`, async () => {
      const { page, close } = await open(PATH, options);
      assert.deepEqual(await axe(page, "#menubars"), []);
      await buttons(page).first().focus();
      await page.keyboard.press("ArrowDown");
      await page.keyboard.press("ArrowUp");
      assert.deepEqual(await axe(page, "#menubars"), []);
      await close();
    });
  }

  // ---- Navigation menu ----------------------------------------------------

  const tops = (page, nav = "site-nav") => page.locator(`#${nav} [data-slot=navigation-menu-trigger]`);

  test("the navigation menu is a named nav of links and disclosure buttons, not a menu", async () => {
    const { page, close } = await open(PATH);
    assert.equal(await page.locator("#site-nav").evaluate((e) => e.tagName.toLowerCase()), "nav");
    assert.equal(await page.locator("#site-nav").getAttribute("aria-label"), "Main");
    assert.equal(await page.locator("#site-nav [role=menu], #site-nav [role=menuitem], #site-nav [aria-haspopup]").count(), 0);
    assert.equal(await expanded(page, "#site-nav-p1-trigger"), "false", "the module writes the state when it runs");
    assert.equal(await page.locator("#site-nav-p1-trigger").getAttribute("aria-controls"), "site-nav-p1");
    assert.equal(await page.locator("#site-nav a[aria-current=page]").count(), 1);
    await close();
  });

  test("a button shows and hides its panel, Escape hides it and returns to the button", async () => {
    // Room under the menu for its panel: with none, the overlay opens a panel above its button.
    const { page, close } = await open(PATH, { viewport: { width: 1440, height: 1100 } });
    await page.locator("#site-nav-p1-trigger").focus();
    await page.keyboard.press("Enter");
    assert.ok(await isOpen(page, "#site-nav-p1"));
    assert.equal(await expanded(page, "#site-nav-p1-trigger"), "true");
    await placed(page, "#site-nav-p1");
    await settled(page, "#site-nav-p1");
    const trigger = await box(page, "#site-nav-p1-trigger");
    const panel = await box(page, "#site-nav-p1");
    assert.ok(panel.y >= trigger.y + trigger.height - 1 && Math.abs(panel.x - trigger.x) <= 2, "under its button");
    await shot(page, `${browserName}-navigation-menu`);
    await page.keyboard.press("Escape");
    assert.ok(!(await isOpen(page, "#site-nav-p1")));
    assert.equal(await expanded(page, "#site-nav-p1-trigger"), "false");
    assert.equal((await focused(page)).id, "site-nav-p1-trigger");
    // Enter and Space open it from the keyboard, and again close it.
    await page.keyboard.press("Enter");
    assert.ok(await isOpen(page, "#site-nav-p1"));
    await page.keyboard.press("Enter");
    assert.ok(!(await isOpen(page, "#site-nav-p1")));
    await close();
  });

  test("Tab walks through the panel's links, and leaving the item closes the panel", async () => {
    const { page, close } = await open(PATH);
    await page.locator("#site-nav-p1-trigger").focus();
    await page.keyboard.press("Enter");
    await tab(page, browserName);
    assert.equal((await focused(page)).text.startsWith("Invoices"), true);
    await tab(page, browserName);
    await tab(page, browserName);
    assert.equal((await focused(page)).text, "Payroll");
    await tab(page, browserName);
    assert.equal((await focused(page)).text.startsWith("Company"), true, "on to the next top-level item");
    assert.ok(!(await isOpen(page, "#site-nav-p1")), "its panel closed behind");
    await page.keyboard.press("Shift+Tab");
    assert.equal((await focused(page)).id, "site-nav-p1-trigger", "back to the first button, its panel still shut");
    assert.ok(!(await isOpen(page, "#site-nav-p1")));
    await close();
  });

  test("Left, Right, Home and End move between the top-level items and close an open panel", async () => {
    const { page, close } = await open(PATH);
    await page.locator("#site-nav-p1-trigger").focus();
    await page.keyboard.press("Enter");
    await page.keyboard.press("ArrowRight");
    assert.ok(!(await isOpen(page, "#site-nav-p1")));
    const names = [];
    for (let n = 0; n < 4; n++) {
      names.push((await focused(page)).text.replace(/\s+/g, " "));
      await page.keyboard.press("ArrowRight");
    }
    assert.deepEqual(names, ["Company", "Pricing", "Docs", "Products"], "the ends wrap");
    await page.keyboard.press("End");
    assert.equal((await focused(page)).text, "Docs");
    await page.keyboard.press("Home");
    assert.equal((await focused(page)).text, "Products");
    await close();
  });

  test("hovering a button opens its panel after a pause, moving to another switches at once, and leaving closes", async () => {
    const { page, close } = await open(PATH);
    await tops(page).first().hover();
    await page.waitForFunction(() => document.getElementById("site-nav-p1").matches(":popover-open"), null, { timeout: 2000 });
    await tops(page).nth(1).hover();
    await page.waitForFunction(() => document.getElementById("site-nav-p2").matches(":popover-open"), null, { timeout: 2000 });
    assert.ok(!(await isOpen(page, "#site-nav-p1")));
    // Into the panel keeps it.
    await page.locator("#site-nav-p2 a").first().hover();
    await page.waitForTimeout(500);
    assert.ok(await isOpen(page, "#site-nav-p2"));
    await page.mouse.move(5, 5);
    await page.waitForFunction(() => !document.getElementById("site-nav-p2").matches(":popover-open"), null, { timeout: 2000 });
    // A click on a button the pointer has just opened keeps the panel open.
    await tops(page).first().hover();
    await page.waitForFunction(() => document.getElementById("site-nav-p1").matches(":popover-open"));
    await tops(page).first().click();
    assert.ok(await isOpen(page, "#site-nav-p1"));
    await close();
  });

  test("a link of a panel goes to its page, and the page marks the current one", async () => {
    const { page, close } = await open(PATH);
    await page.locator("#site-nav-p1-trigger").click();
    const current = page.locator("#site-nav-p1 a[aria-current=page]");
    assert.equal((await current.innerText()).split("\n")[0], "Payments");
    assert.equal(await page.locator("#site-nav-p1-trigger").getAttribute("data-active"), "");
    await page.locator("#site-nav-p1").getByText("Invoices", { exact: true }).click();
    await page.waitForURL(/to=invoices/);
    await close();
  });

  test("on a phone the items stack, and a panel stays inside the window", async () => {
    const { page, close } = await open(PATH, { viewport: PHONE });
    const xs = await tops(page).evaluateAll((all) => all.map((e) => Math.round(e.getBoundingClientRect().x)));
    assert.equal(new Set(xs).size, 1, `stacked: ${xs}`);
    await tops(page).nth(1).click();
    await placed(page, "#site-nav-p2");
    await settled(page, "#site-nav-p2");
    const panel = await box(page, "#site-nav-p2");
    assert.ok(panel.x >= 0 && panel.x + panel.width <= 390, JSON.stringify(panel));
    assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth));
    await shot(page, `${browserName}-navigation-menu-390`);
    await close();
  });

  test("in a right-to-left page Left is the next item and the panel lines up with its button", async () => {
    const { page, close } = await open(PATH);
    await page.locator("#site-nav-rtl-p1-trigger").focus();
    await page.keyboard.press("ArrowLeft");
    assert.equal((await focused(page)).text, "الأسعار");
    await page.keyboard.press("ArrowRight");
    await page.keyboard.press("Enter");
    await placed(page, "#site-nav-rtl-p1");
    await settled(page, "#site-nav-rtl-p1");
    const trigger = await box(page, "#site-nav-rtl-p1-trigger");
    const panel = await box(page, "#site-nav-rtl-p1");
    assert.ok(Math.abs(panel.x + panel.width - (trigger.x + trigger.width)) <= 2, `aligned at the end: ${JSON.stringify([trigger, panel])}`);
    await close();
  });

  test("with JavaScript off a button opens its panel and the links work", async () => {
    const { page, close } = await open(PATH, { js: false });
    await page.locator("#site-nav-p1-trigger").click();
    assert.ok(await isOpen(page, "#site-nav-p1"));
    assert.equal(await page.locator("#site-nav-p1-trigger").getAttribute("aria-expanded"), null, "no stale state without the module");
    await page.keyboard.press("Escape");
    assert.ok(!(await isOpen(page, "#site-nav-p1")));
    await page.locator("#site-nav-p1-trigger").click();
    await page.locator("#site-nav-p1").getByText("Invoices", { exact: true }).click();
    await page.waitForURL(/to=invoices/);
    await close();
  });

  for (const [label, options] of [
    ["dark at 1440", {}],
    ["light at 1440", { light: true }],
    ["dark on a phone", { viewport: PHONE }],
    ["light on a phone", { light: true, viewport: PHONE }],
  ]) {
    test(`axe finds nothing in the navigation menus, shut and with a panel open (${label})`, async () => {
      const { page, close } = await open(PATH, options);
      assert.deepEqual(await axe(page, "#navigation"), []);
      await page.locator("#site-nav-p1-trigger").click();
      assert.deepEqual(await axe(page, "#navigation"), []);
      await close();
    });
  }
});
