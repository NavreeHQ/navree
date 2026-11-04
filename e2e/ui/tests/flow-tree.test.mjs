// The tree view in real browsers, against the proof app's /flow: the roles the module adds, every
// key of the pattern, children fetched as a branch opens (and a fetch that fails, is slow or
// answers nothing), tri-state choices, Show more, the post and the server's reading of it,
// JavaScript off, right to left and axe in dark, light and on a phone.
//
//   cargo run --release -p ui-proof -- serve          # UI_PROOF_URL, default http://127.0.0.1:8090
//   UI_BROWSERS=chromium,firefox,webkit node --test tests/flow-tree.test.mjs

import assert from "node:assert/strict";
import { test } from "node:test";
import { BASE, axe, hits, inBrowsers, shot, stops } from "./support/flow.mjs";

const PHONE = { width: 390, height: 844 };

inBrowsers(({ open, browserName }) => {
  /** The treeitem whose own label reads `text`: an item's row. */
  const item = (page, tree, text) => page.locator(`${tree} [role=treeitem]:has([data-slot=tree-text]:text-is("${text}"))`).first();
  const label = item;
  /** The list item a row stands in. */
  const holder = (row) => row.locator("xpath=..");
  const focusedText = (page) => page.evaluate(() => {
    const a = document.activeElement;
    return a?.matches("[role=treeitem]") ? (a.querySelector("[data-slot=tree-text]") || a.querySelector("a:not([data-slot=tree-toggle])") || a)?.textContent.trim() : "(" + (a?.tagName || "none") + ")";
  });
  const focusFirst = async (page, tree) => {
    await page.locator(`${tree} [role=treeitem][tabindex="0"]`).focus();
  };
  const settle = (page) => page.evaluate(() => new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done))));
  const shown = (page, tree) => page.locator(`${tree} [role=treeitem]`).evaluateAll((els) => els.filter((e) => e.getClientRects().length).map((e) => (e.querySelector("[data-slot=tree-text]") || e.querySelector("a:not([data-slot=tree-toggle])") || e)?.textContent.trim()));

  test("the module gives the lists their roles, names, levels and one tab stop", async () => {
    const { page, close } = await open("/flow");
    assert.equal(await page.locator("#t-multi [role=tree]").count(), 1);
    assert.equal(await page.locator("#t-multi [role=tree]").getAttribute("aria-label"), "Folders to share");
    const items = await page.locator("#t-multi [role=treeitem]").evaluateAll((els) =>
      els.slice(0, 5).map((e) => ({
        name: document.getElementById(e.getAttribute("aria-labelledby"))?.textContent.trim(),
        level: e.getAttribute("aria-level"),
        size: e.getAttribute("aria-setsize"),
        pos: e.getAttribute("aria-posinset"),
        expanded: e.getAttribute("aria-expanded"),
        checked: e.getAttribute("aria-checked"),
      })),
    );
    assert.deepEqual(items[0], { name: "Engineering", level: "1", size: "6", pos: "1", expanded: "true", checked: "mixed" });
    assert.deepEqual(items[1], { name: "Web", level: "2", size: "3", pos: "1", expanded: null, checked: "false" });
    assert.equal(items[2].checked, "true", "API is chosen");
    assert.equal(await page.locator("#t-multi [data-slot=tree-group]").first().getAttribute("role"), "group");
    assert.equal(await page.locator("#t-multi > [role=tree] > li").first().getAttribute("role"), "none");
    // The group of a branch belongs to its row.
    assert.equal(await page.locator("#t-multi [role=treeitem]").first().getAttribute("aria-owns"), await page.locator("#t-multi [data-slot=tree-group]").first().getAttribute("id"));
    assert.equal(await stops(page, "#t-multi [role=treeitem]"), 1);
    // The inputs and carets are not tab stops of their own.
    assert.equal(await page.locator("#t-multi [data-slot=tree-row] input[tabindex='0']").count(), 0);
    assert.equal(await page.locator("#t-multi").getAttribute("data-enhanced"), "");
    await close();
  });

  test("Tab enters the tree on one item and leaves it in one more press", async () => {
    const { page, close } = await open("/flow");
    // The slider just before the first tree.
    await page.focus("#sl-lonely");
    await page.keyboard.press("Tab");
    assert.ok(await page.evaluate(() => !!document.activeElement?.closest("#t-links")), "a Tab reaches the tree");
    assert.equal(await focusedText(page), "Documents");
    await page.keyboard.press("Tab");
    assert.ok(await page.evaluate(() => !document.activeElement?.closest("#t-links")), "the next Tab leaves it");
    await page.keyboard.press("Shift+Tab");
    assert.equal(await focusedText(page), "Documents", "and Shift+Tab comes back to the same item");
    await close();
  });

  test("the arrow keys, Home and End move through what is showing, and Right and Left open, enter and close", async () => {
    const { page, close } = await open("/flow");
    await focusFirst(page, "#t-multi");
    assert.equal(await focusedText(page), "Engineering");
    await page.keyboard.press("ArrowDown");
    assert.equal(await focusedText(page), "Web");
    await page.keyboard.press("ArrowDown");
    await page.keyboard.press("ArrowDown");
    assert.equal(await focusedText(page), "Retired");
    await page.keyboard.press("ArrowDown");
    assert.equal(await focusedText(page), "Operations");
    await page.keyboard.press("ArrowUp");
    assert.equal(await focusedText(page), "Retired");
    // Left on a child goes to its parent; Left on an open parent closes it.
    await page.keyboard.press("ArrowLeft");
    assert.equal(await focusedText(page), "Engineering");
    await page.keyboard.press("ArrowLeft");
    assert.equal(await item(page, "#t-multi", "Engineering").getAttribute("aria-expanded"), "false");
    assert.deepEqual((await shown(page, "#t-multi")).slice(0, 3), ["Engineering", "Operations", "Slow folder"]);
    // Right on a closed parent opens it, and on an open one goes to its first child.
    await page.keyboard.press("ArrowRight");
    assert.equal(await item(page, "#t-multi", "Engineering").getAttribute("aria-expanded"), "true");
    assert.equal(await focusedText(page), "Engineering");
    await page.keyboard.press("ArrowRight");
    assert.equal(await focusedText(page), "Web");
    // Right on a leaf does nothing.
    await page.keyboard.press("ArrowRight");
    assert.equal(await focusedText(page), "Web");
    await page.keyboard.press("End");
    assert.equal(await focusedText(page), "Miscellaneous");
    await page.keyboard.press("Home");
    assert.equal(await focusedText(page), "Engineering");
    // One tab stop still.
    assert.equal(await stops(page, "#t-multi [role=treeitem]"), 1);
    await close();
  });

  test("type-ahead finds the next item that starts with what was typed, and * opens the siblings", async () => {
    const { page, close } = await open("/flow");
    await focusFirst(page, "#t-multi");
    await page.keyboard.type("op");
    assert.equal(await focusedText(page), "Operations");
    await page.keyboard.type("s");
    assert.equal(await focusedText(page), "Operations", "typed together, 'ops' is what is looked for, and it is where it is");
    await page.waitForTimeout(650);
    await page.keyboard.type("m");
    assert.equal(await focusedText(page), "Miscellaneous");
    await page.waitForTimeout(650);
    await page.keyboard.type("f");
    assert.equal(await focusedText(page), "Folder that fails");
    await page.waitForTimeout(650);
    // The same letter again goes round.
    await page.keyboard.type("f");
    assert.equal(await focusedText(page), "Folder with nothing");
    // * opens every branch at this level: the ones with children to show.
    await page.keyboard.press("*");
    await settle(page);
    const opened = await page.locator("#t-multi > [role=tree] > li[data-open]").count();
    assert.ok(opened >= 2, `siblings opened: ${opened}`);
    await close();
  });

  test("Space and Enter choose, Shift with the arrows moves and chooses, Ctrl+A chooses all, and a branch carries its children", async () => {
    const { page, close } = await open("/flow");
    await focusFirst(page, "#t-multi");
    await page.keyboard.press("ArrowDown"); // Web
    await page.keyboard.press("Space");
    assert.equal(await item(page, "#t-multi", "Web").getAttribute("aria-checked"), "true");
    // The parent had one of two enabled children: now both, so it is whole.
    assert.equal(await item(page, "#t-multi", "Engineering").getAttribute("aria-checked"), "true");
    await page.keyboard.press("Space");
    assert.equal(await item(page, "#t-multi", "Web").getAttribute("aria-checked"), "false");
    assert.equal(await item(page, "#t-multi", "Engineering").getAttribute("aria-checked"), "mixed");
    assert.equal(await page.locator("#t-multi li[data-slot=tree-item] > [data-slot=tree-row] input").first().evaluate((e) => e.indeterminate), true);
    await page.keyboard.press("Enter");
    assert.equal(await item(page, "#t-multi", "Web").getAttribute("aria-checked"), "true", "Enter on a leaf of a choosing tree chooses");
    // Shift+Down moves to the next item and toggles it.
    await page.keyboard.press("Shift+ArrowDown");
    assert.equal(await focusedText(page), "API");
    assert.equal(await item(page, "#t-multi", "API").getAttribute("aria-checked"), "false", "API was chosen, and Shift+Down toggles it");
    assert.equal(await item(page, "#t-multi", "Engineering").getAttribute("aria-checked"), "mixed");
    // A branch that is half chosen, chosen: all that is under it is chosen, but not what is disabled.
    await page.keyboard.press("Home");
    await page.keyboard.press("Space");
    assert.deepEqual(await page.locator("#t-multi > [role=tree] > li:first-child li [role=treeitem]").evaluateAll((els) => els.map((e) => e.getAttribute("aria-checked"))), ["true", "true", "false"]);
    assert.equal(await item(page, "#t-multi", "Engineering").getAttribute("aria-checked"), "true");
    // And unchosen, it unchooses them.
    await page.keyboard.press("Space");
    assert.deepEqual(await page.locator("#t-multi > [role=tree] > li:first-child li [role=treeitem]").evaluateAll((els) => els.map((e) => e.getAttribute("aria-checked"))), ["false", "false", "false"]);
    // Ctrl+A.
    await page.keyboard.press("Control+a");
    const all = await page.locator("#t-multi [role=treeitem]").evaluateAll((els) => els.filter((e) => e.getClientRects().length && !e.closest("li").hasAttribute("data-disabled")).map((e) => e.getAttribute("aria-checked")));
    assert.ok(all.every((state) => state === "true"), `everything showing is chosen: ${all}`);
    await close();
  });

  test("with one choice, the arrows only move and Space or Enter chooses", async () => {
    const { page, close } = await open("/flow");
    await focusFirst(page, "#t-single");
    await page.keyboard.press("ArrowDown");
    assert.equal(await focusedText(page), "Ada");
    assert.equal(await item(page, "#t-single", "Grace").getAttribute("aria-selected"), "true", "moving chose nothing");
    await page.keyboard.press("Space");
    assert.equal(await item(page, "#t-single", "Ada").getAttribute("aria-selected"), "true");
    assert.equal(await item(page, "#t-single", "Grace").getAttribute("aria-selected"), "false");
    await page.keyboard.press("ArrowDown");
    await page.keyboard.press("Enter");
    assert.equal(await item(page, "#t-single", "Grace").getAttribute("aria-selected"), "true");
    assert.equal(await page.locator("#t-single input:checked").evaluate((e) => e.value), "sales/grace");
    await close();
  });

  test("a branch is opened by a click on its caret, and a link tree follows Enter", async () => {
    const { page, close } = await open("/flow");
    await item(page, "#t-multi", "Engineering").locator("[data-slot=tree-toggle]").click();
    assert.equal(await item(page, "#t-multi", "Engineering").getAttribute("aria-expanded"), "false");
    assert.equal(await item(page, "#t-multi", "Web").isVisible(), false);
    assert.equal(await page.url(), BASE + "/flow", "the caret asked for no page");
    // A link tree: Enter on a link follows it.
    await page.locator("#t-links").scrollIntoViewIfNeeded();
    await page.locator("#t-links [role=treeitem]:has(a[href='/flow?file=license'])").focus();
    await Promise.all([page.waitForURL("**/flow?file=license"), page.keyboard.press("Enter")]);
    await close();
  });

  test("children come from the server once, when a branch first opens, and show at once on the next opening", async () => {
    const { page, close } = await open("/flow");
    const before = await hits(page);
    await focusFirst(page, "#t-multi");
    await page.keyboard.type("op");
    await page.keyboard.press("ArrowRight");
    await item(page, "#t-multi", "Plans").waitFor();
    const kids = await shown(page, "#t-multi");
    assert.ok(kids.includes("Plans") && kids.includes("Archive") && kids.includes("Notes"), `${kids}`);
    const asked = (await hits(page))["lazy-docs"] - (before["lazy-docs"] || 0);
    assert.equal(asked, 1);
    // The children have their roles, levels and positions.
    const plans = label(page, "#t-multi", "Plans");
    assert.equal(await plans.getAttribute("aria-level"), "2");
    assert.equal(await plans.getAttribute("aria-posinset"), "1");
    assert.equal(await plans.getAttribute("aria-setsize"), "3");
    assert.equal(await stops(page, "#t-multi [role=treeitem]"), 1);
    // Close and open again: no new request.
    await page.keyboard.press("ArrowLeft");
    await page.keyboard.press("ArrowRight");
    await settle(page);
    assert.equal((await hits(page))["lazy-docs"] - (before["lazy-docs"] || 0), 1);
    // A branch inside what was loaded loads too.
    await label(page, "#t-multi", "Archive").focus();
    await page.keyboard.press("ArrowRight");
    await item(page, "#t-multi", "2025").waitFor();
    assert.equal(await label(page, "#t-multi", "2025").getAttribute("aria-level"), "3");
    await close();
  });

  test("a branch chosen before its children came chooses them when they arrive", async () => {
    const { page, close } = await open("/flow");
    await item(page, "#t-multi", "Operations").locator("input").click();
    assert.equal(await item(page, "#t-multi", "Operations").getAttribute("aria-checked"), "true");
    await item(page, "#t-multi", "Operations").locator("[data-slot=tree-toggle]").click();
    await item(page, "#t-multi", "Plans").waitFor();
    const states = await holder(item(page, "#t-multi", "Operations")).locator("li [role=treeitem]").evaluateAll((els) => els.map((e) => e.getAttribute("aria-checked")));
    assert.deepEqual(states, ["true", "true", "true"]);
    // Unchoose one and the branch is half chosen.
    await item(page, "#t-multi", "Notes").locator("input").click();
    assert.equal(await item(page, "#t-multi", "Operations").getAttribute("aria-checked"), "mixed");
    await close();
  });

  test("a slow answer is asked for once however often the branch is toggled, and says it is loading", async () => {
    const { page, close } = await open("/flow");
    const before = (await hits(page)).slow || 0;
    const slow = item(page, "#t-multi", "Slow folder");
    await slow.locator("[data-slot=tree-toggle]").click();
    assert.equal(await slow.getAttribute("aria-busy"), "true");
    assert.match(await holder(slow).locator(":scope > [data-slot=tree-group]").innerText(), /Loading/);
    await slow.locator("[data-slot=tree-toggle]").click();
    await slow.locator("[data-slot=tree-toggle]").click();
    await slow.locator("[data-slot=tree-toggle]").click();
    await item(page, "#t-multi", "Slow A").waitFor({ state: "attached", timeout: 8000 });
    assert.equal((await hits(page)).slow - before, 1);
    assert.equal(await slow.getAttribute("aria-busy"), null);
    assert.match(await page.locator("#t-multi [data-slot=tree-status]").textContent(), /2 items loaded/);
    await close();
  });

  test("a branch that fails says so and tries again on Try again; one with nothing in it is just empty", async () => {
    const { page, close } = await open("/flow");
    const before = (await hits(page)).fail || 0;
    const failing = item(page, "#t-multi", "Folder that fails");
    await failing.locator("[data-slot=tree-toggle]").click();
    await holder(failing).locator("[data-slot=tree-retry]").waitFor();
    assert.match(await holder(failing).locator(":scope > [data-slot=tree-group]").innerText(), /Couldn't load this\./);
    assert.equal((await hits(page)).fail - before, 1);
    assert.equal(await failing.getAttribute("aria-busy"), null);
    assert.match(await page.locator("#t-multi [data-slot=tree-status]").textContent(), /Couldn't load this/);
    await holder(failing).locator("[data-slot=tree-retry]").click();
    await page.waitForTimeout(500);
    assert.equal((await hits(page)).fail - before, 2, "Try again asks again");
    // Nothing in it.
    const empty = item(page, "#t-multi", "Folder with nothing");
    await empty.locator("[data-slot=tree-toggle]").click();
    await page.waitForTimeout(400);
    assert.equal(await empty.getAttribute("aria-expanded"), "true");
    assert.equal(await holder(empty).locator(":scope > [data-slot=tree-group] > li").count(), 0);
    assert.equal(await holder(empty).getAttribute("data-lazy"), null, "it is loaded, and empty");
    await close();
  });

  test("a long list comes in pages: Show more fetches the next fifty in place and counts them right", async () => {
    const { page, close } = await open("/flow");
    const before = (await hits(page)).big || 0;
    assert.equal(await page.locator("#t-big li[data-slot=tree-item]").count(), 51, "the branch and fifty children");
    const more = page.locator("#t-big [data-slot=tree-more]");
    assert.equal((await more.innerText()).trim(), "Show more (9950)");
    assert.equal(await more.locator("[role=treeitem]").count(), 1);
    await more.locator("a").click();
    await page.waitForSelector("#t-big [data-slot=tree-text]:text-is('Person 99')");
    assert.equal(await page.locator("#t-big li[data-slot=tree-item]").count(), 101);
    assert.equal((await hits(page)).big - before, 1);
    const p51 = label(page, "#t-big", "Person 50");
    assert.equal(await p51.getAttribute("aria-posinset"), "51");
    assert.equal(await p51.getAttribute("aria-setsize"), "10000");
    assert.equal(await p51.getAttribute("aria-level"), "2");
    assert.equal((await page.locator("#t-big [data-slot=tree-more]").innerText()).trim(), "Show more (9900)");
    assert.equal(await page.locator("#t-big [data-slot=tree-more]").count(), 1, "the old row went");
    assert.equal(await stops(page, "#t-big [role=treeitem]"), 1);
    // By the keys: End goes to the last row, which is the Show more row, and Enter loads.
    await focusFirst(page, "#t-big");
    await page.keyboard.press("End");
    assert.equal(await page.evaluate(() => document.activeElement.closest("li").getAttribute("data-slot")), "tree-more");
    await page.keyboard.press("Enter");
    await page.waitForSelector("#t-big [data-slot=tree-text]:text-is('Person 149')");
    assert.equal(await page.locator("#t-big li[data-slot=tree-item]").count(), 151);
    await close();
  });

  test("the chosen nodes post as the tree's name, and the server reads them back", async () => {
    const { page, close } = await open("/flow");
    await item(page, "#t-multi", "Web").locator("input").click();
    await item(page, "#t-single", "Ada").locator("input").click();
    await Promise.all([page.waitForURL("**/flow/echo"), page.click("#t-post")]);
    const posted = (await page.locator("#posted").innerText()).split("\n");
    for (const line of ["access=eng", "access=eng/api", "access=eng/web", "owner=sales/ada"]) assert.ok(posted.includes(line), `${line} in ${posted}`);
    assert.ok(!posted.includes("access=eng/old"), "a disabled choice posts nothing");
    const judged = await page.locator("#judged").innerText();
    assert.match(judged, /access: 3 chosen: eng \| eng\/web \| eng\/api/);
    assert.match(judged, /owner: 1 chosen: sales\/ada/);
    await close();
  });

  test("hostile names stay text, post exactly, and fetched markup runs nothing", async () => {
    const { page, close } = await open("/flow");
    assert.equal(await page.evaluate(() => window.__pwned), undefined);
    const hostile = page.locator("#t-hostile input").first();
    await hostile.click();
    // A lazy branch named like a DOM method loads children whose names are markup.
    await item(page, "#t-hostile", "toString").locator("[data-slot=tree-toggle]").click();
    await page.waitForSelector("#t-hostile [data-slot=tree-text]:text-is('<script>window.__pwned=1</script>')");
    assert.equal(await page.evaluate(() => window.__pwned), undefined);
    await Promise.all([page.waitForURL("**/flow/echo"), page.click("#t-post")]);
    const judged = await page.locator("#judged").innerText();
    assert.ok(judged.includes('hostile: 1 chosen: "><img src=x onerror=alert(1)>'), judged);
    assert.equal(await page.evaluate(() => window.__pwned), undefined);
    await close();
  });

  test("a morph or a second pass of the module leaves one tab stop and no doubled roles", async () => {
    const { page, close } = await open("/flow");
    await focusFirst(page, "#t-multi");
    await page.keyboard.press("ArrowDown");
    await page.evaluate(() => document.dispatchEvent(new Event("ui:swap")));
    assert.equal(await stops(page, "#t-multi [role=treeitem]"), 1);
    assert.equal(await focusedText(page), "Web");
    assert.equal(await page.locator("#t-multi [role=tree]").count(), 1);
    await close();
  });

  test("two trees on a page keep their own tab stop and their own keys", async () => {
    const { page, close } = await open("/flow");
    await focusFirst(page, "#t-single");
    await page.keyboard.press("End");
    assert.equal(await stops(page, "#t-single [role=treeitem]"), 1);
    assert.equal(await stops(page, "#t-multi [role=treeitem]"), 1);
    assert.equal(await page.evaluate(() => document.activeElement.closest("[data-ui-tree]").id), "t-single");
    await close();
  });

  test("a deep tree opens one level at a time by the keys", async () => {
    const { page, close } = await open("/flow");
    await focusFirst(page, "#t-deep");
    for (let n = 0; n < 70; n++) await page.keyboard.press("ArrowRight");
    assert.equal(await focusedText(page), "The bottom");
    assert.equal(await page.evaluate(() => document.activeElement.getAttribute("aria-level")), "31");
    await close();
  });

  test("right to left: Left opens and Right closes, and the choices cascade", async () => {
    const { page, close } = await open("/flow");
    await focusFirst(page, "#t-rtl-tree");
    assert.equal(await focusedText(page), "الجذر");
    await page.keyboard.press("ArrowRight");
    assert.equal(await page.evaluate(() => document.activeElement.getAttribute("aria-expanded")), "false", "Right closes in a right-to-left page");
    await page.keyboard.press("ArrowLeft");
    assert.equal(await page.evaluate(() => document.activeElement.getAttribute("aria-expanded")), "true", "Left opens");
    await page.keyboard.press("ArrowLeft");
    assert.equal(await focusedText(page), "ورقة");
    await close();
  });

  test("with JavaScript off the tree is a list: every level shows, carets are links, and the inputs post", async () => {
    const { page, close } = await open("/flow", { js: false });
    assert.equal(await page.locator("[data-enhanced]").count(), 0);
    assert.equal(await page.locator("#t-multi [role=tree], #t-multi [role=treeitem]").count(), 0, "no roles promise keys that aren't there");
    assert.ok(await label0(page, "#t-multi", "Web"), "children are showing");
    assert.ok(await label0(page, "#t-big", "Person 49"));
    // A caret of a tree that keeps its open set in the address is a link.
    const caret = page.locator("#t-links a[data-slot=tree-toggle]").first();
    assert.equal(await caret.getAttribute("aria-label"), "Expand Documents");
    await Promise.all([page.waitForURL("**/flow?open=docs"), caret.click()]);
    assert.ok(await label0(page, "#t-links", "Plans"), "the server drew the open branch");
    assert.equal(await page.locator("#t-links a[data-slot=tree-toggle]").first().getAttribute("aria-label"), "Collapse Documents");
    // Choose and post.
    await page.locator("#t-multi input[value='eng/web']").check();
    await Promise.all([page.waitForURL("**/flow/echo"), page.click("#t-post")]);
    assert.match(await page.locator("#judged").innerText(), /access: 2 chosen: eng\/web \| eng\/api/);
    // Show more is a plain link.
    await page.goto(BASE + "/flow");
    const href = await page.locator("#t-big [data-slot=tree-more-link]").getAttribute("href");
    assert.equal(href, "/flow/tree/children?parent=big&after=50");
    await close();
  });

  for (const [name, options] of [
    ["dark at 1440", {}],
    ["light at 1440", { light: true }],
    ["dark on a phone", { viewport: PHONE }],
    ["light on a phone", { viewport: PHONE, light: true }],
  ]) {
    test(`axe finds nothing in the trees as they are and with branches opened and chosen (${name})`, async () => {
      const { page, close } = await open("/flow", options);
      assert.deepEqual(await axe(page, "#trees"), []);
      await focusFirst(page, "#t-multi");
      await page.keyboard.type("op");
      await page.keyboard.press("ArrowRight");
      await page.waitForSelector("#t-multi [data-slot=tree-text]:text-is('Plans')");
      await page.keyboard.press("Space");
      await item(page, "#t-multi", "Notes").locator("input").click();
      await item(page, "#t-multi", "Folder that fails").locator("[data-slot=tree-toggle]").click();
      await page.waitForSelector("#t-multi [data-slot=tree-retry]");
      await settle(page);
      assert.deepEqual(await axe(page, "#trees"), []);
      const width = options.viewport?.width || 1440;
      assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), "no sideways scroll");
      assert.deepEqual(
        await page.locator("#trees").evaluate((e, w) => [...e.querySelectorAll("li, label, a")].filter((el) => el.getBoundingClientRect().right > w + 1).map((el) => el.tagName).slice(0, 3), width),
        [],
      );
      await shot(page, `${browserName}-trees-${name.replaceAll(" ", "-")}`, "#trees");
      await close();
    });
  }
});

/** Whether an item with this text is showing. */
async function label0(page, tree, text) {
  return page.locator(`${tree} [data-slot=tree-text]:text-is("${text}")`).first().isVisible();
}
