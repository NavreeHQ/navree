// Logbook in real browsers, Chromium and Firefox: the shell with the entry types, the
// list and the open entry in two panes on a wide screen and one at a time on a phone,
// an address for every entry, nothing wider than the window at 390px, the colours,
// the states (empty, a bad filter, an entry that isn't there) and the app rules.
//
// Needs a second build of the proof app, with Logbook on (see README.md):
//   UI_PROOF_LOGBOOK=1 PORT=8092 ui-proof serve    # a build with --features logbook
//   UI_LOGBOOK_URL=http://127.0.0.1:8092 npm test

import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { createRequire } from "node:module";
import { test } from "node:test";
import { chromium, firefox } from "playwright";

const LOGBOOK = process.env.UI_LOGBOOK_URL;
const require = createRequire(import.meta.url);
const axeSource = await readFile(require.resolve("axe-core/axe.min.js"), "utf8");

const WIDE = { width: 1440, height: 900 };
const PHONE = { width: 390, height: 844 };

/** A long path, a long query and hostile text: what a phone has to fit. */
const LONG = "a".repeat(180);
const HOSTILE = encodeURIComponent('"><img src=x onerror=alert(1)>');

/** Traffic for the list: pages of the proof app, a missing one, a very long one, a hostile one. */
async function traffic() {
  const paths = ["/plain", "/deals?per=3", "/gallery", "/nope", `/${LONG}`, `/plain?q=${LONG}`, `/plain?x=${HOSTILE}`, "/deals/new"];
  for (const path of paths) await fetch(`${LOGBOOK}${path}`).catch(() => {});
}

/** Runs axe on the page as it stands. Violations come back as short lines. */
async function axe(page) {
  await page.evaluate(axeSource);
  return page.evaluate(async () => {
    const out = await window.axe.run(document, { runOnly: ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa", "best-practice"] });
    return out.violations.map((v) => `${v.id}: ${v.nodes.slice(0, 3).map((n) => n.target.join(" ")).join(" | ")}`);
  });
}

/** Watches a page for console errors and content security policy reports. */
function watch(page) {
  const problems = [];
  page.on("console", (message) => {
    if (message.type() === "error") problems.push(message.text());
  });
  page.on("pageerror", (error) => problems.push(String(error)));
  page.addInitScript(() => {
    document.addEventListener("securitypolicyviolation", (e) => console.error(`CSP: ${e.violatedDirective} ${e.blockedURI}`));
  });
  return problems;
}

/** How far the page reaches sideways, and how wide the window is. */
const reach = (page) => page.evaluate(() => ({ page: document.documentElement.scrollWidth, window: innerWidth }));

/** The visible panes of a two-pane page, left to right. */
const panes = (page) =>
  page.evaluate(() =>
    [...document.querySelectorAll("[data-pane]")].map((pane) => {
      const box = pane.getBoundingClientRect();
      return { visible: getComputedStyle(pane).display !== "none" && box.width > 0, left: box.left, right: box.right, top: box.top, width: box.width };
    }),
  );

for (const [engine, launcher] of [["chromium", chromium], ["firefox", firefox]]) {
  const suite = (name, options, fn) => test(`${engine}: ${name}`, { skip: !LOGBOOK, timeout: 90000, ...options }, fn);
  let browser;
  const withPage = async (viewport, fn, contextOptions = {}) => {
    browser ||= await launcher.launch();
    const context = await browser.newContext({ viewport, reducedMotion: "reduce", ...contextOptions });
    const page = await context.newPage();
    const problems = watch(page);
    try {
      await fn(page, problems);
    } finally {
      await context.close();
    }
    assert.deepEqual(problems, []);
  };

  suite("on a wide screen the list and the entry open side by side", {}, async () => {
    await traffic();
    await withPage(WIDE, async (page) => {
      await page.goto(`${LOGBOOK}/_logbook`);
      let both = await panes(page);
      assert.equal(both.length, 2, "two panes");
      assert.ok(both[0].visible && both[1].visible, "the list, and the place the entry will open");
      assert.ok(both[0].right <= both[1].left + 1, "side by side, not stacked");
      assert.ok(Math.abs(both[0].top - both[1].top) < 2, "from the same line");
      await page.locator("#rows-body [data-slot=item]").first().click();
      await page.waitForURL("**/_logbook/entries/**");
      await page.locator("section#timeline, section#request").first().waitFor();
      both = await panes(page);
      assert.ok(both[0].visible && both[1].visible);
      assert.ok(both[0].right <= both[1].left + 1);
      assert.ok(both[1].width > both[0].width, "the entry gets more room than the list");
      assert.equal(await page.locator("#rows-body [aria-current=true]").count(), 1, "its row is marked");
      const { page: reached, window } = await reach(page);
      assert.ok(reached <= window, `no sideways scroll: ${reached} > ${window}`);
    });
  });

  suite("on a phone the list and the entry take the page in turn", {}, async () => {
    await traffic();
    await withPage(PHONE, async (page) => {
      await page.goto(`${LOGBOOK}/_logbook`);
      let both = await panes(page);
      assert.ok(both[0].visible && !both[1].visible, "the list alone");
      assert.ok(both[0].width <= PHONE.width);
      const first = page.locator("#rows-body [data-slot=item]").first();
      const href = await first.getAttribute("href");
      await first.click();
      await page.waitForURL(`**${href}`);
      both = await panes(page);
      assert.ok(!both[0].visible && both[1].visible, "the entry alone");
      const { page: reached, window } = await reach(page);
      assert.ok(reached <= window, `no sideways scroll: ${reached} > ${window}`);
      await page.goBack();
      await page.waitForURL(`**/_logbook`);
      both = await panes(page);
      assert.ok(both[0].visible && !both[1].visible, "back to the list");
    });
  });

  suite("every entry has an address, and links and the back button work", {}, async () => {
    await traffic();
    await withPage(WIDE, async (page) => {
      await page.goto(`${LOGBOOK}/_logbook?type=request`);
      const rows = page.locator("#rows-body [data-slot=item]");
      const hrefs = [await rows.nth(0).getAttribute("href"), await rows.nth(1).getAttribute("href")];
      assert.ok(hrefs.every((href) => href.includes("/_logbook/entries/") && href.endsWith("?type=request")), `${hrefs}`);
      await rows.nth(0).click();
      await page.waitForURL(`**${hrefs[0]}`);
      const first = await page.locator("h1[data-slot=page-title]").textContent();
      await page.locator("#rows-body [data-slot=item]").nth(1).click();
      await page.waitForURL(`**${hrefs[1]}`);
      const second = await page.locator("h1[data-slot=page-title]").textContent();
      assert.notEqual(first, second, "another entry, another heading");
      await page.goBack();
      await page.waitForURL(`**${hrefs[0]}`);
      assert.equal(await page.locator("h1[data-slot=page-title]").textContent(), first, "back is the first entry");
      await page.goForward();
      await page.waitForURL(`**${hrefs[1]}`);
      assert.equal(await page.locator("h1[data-slot=page-title]").textContent(), second);
      // The address is the page: a new tab opens the same entry with the same list.
      const other = await page.context().newPage();
      await other.goto(`${LOGBOOK}${hrefs[0]}`);
      assert.equal(await other.locator("h1[data-slot=page-title]").textContent(), first);
      assert.equal(await other.locator("#rows-body [aria-current=true]").count(), 1);
      assert.ok(other.url().endsWith("?type=request"));
      await other.close();
    });
  });

  suite("the entry types are in the sidebar with their counts, and collapse to icons", {}, async () => {
    await traffic();
    await withPage(WIDE, async (page) => {
      await page.goto(`${LOGBOOK}/_logbook`);
      const items = page.locator("[data-slot=sidebar-item]");
      assert.equal(await items.count(), 14, "All and the thirteen types");
      const requests = page.locator("[data-slot=sidebar-item][title=Requests]");
      const count = Number(await requests.locator("[data-slot=sidebar-count]").textContent());
      assert.ok(count >= 8, `the requests recorded so far: ${count}`);
      await requests.click();
      await page.waitForURL("**?type=request");
      assert.equal(await page.locator("[data-slot=sidebar-item][aria-current=page]").count(), 1);
      assert.equal(await page.locator("[data-slot=sidebar-item][aria-current=page]").getAttribute("title"), "Requests");
      assert.ok((await page.locator("h1[data-slot=page-title]").textContent()).includes("Requests"));
      // The rail: only the icons, three rem wide, and it stays that way across a reload.
      await page.getByRole("button", { name: "Toggle sidebar" }).click();
      const width = () => page.locator("[data-slot=sidebar]").evaluate((el) => el.getBoundingClientRect().width);
      assert.equal(await width(), 48);
      assert.ok(await page.locator("[data-slot=sidebar-item][title=Queries] svg").isVisible(), "the icon is what is left");
      await page.reload();
      assert.equal(await width(), 48);
    });
  });

  suite("nothing is wider than the window at 390px, whatever was recorded", {}, async () => {
    await traffic();
    await withPage(PHONE, async (page) => {
      const long = `/_logbook?q=${LONG}`;
      const rows = ["/_logbook", "/_logbook?type=request", "/_logbook?q=status%3Anope", "/_logbook?q=nothing-matches-this", long, "/_logbook?type=query"];
      for (const path of rows) {
        await page.goto(`${LOGBOOK}${path}`);
        const { page: reached, window } = await reach(page);
        assert.ok(reached <= window, `${path}: ${reached} > ${window}`);
      }
      // The entries of the very long and the hostile requests.
      const entries = await page.evaluate(async (base) => {
        const response = await fetch(`${base}/_logbook/api/entries?type=request&limit=60`);
        return (await response.json()).data.map((entry) => entry.id);
      }, LOGBOOK);
      assert.ok(entries.length >= 8);
      for (const id of entries.slice(0, 12)) {
        await page.goto(`${LOGBOOK}/_logbook/entries/${id}`);
        const { page: reached, window } = await reach(page);
        assert.ok(reached <= window, `entry ${id}: ${reached} > ${window}`);
      }
      await page.goto(`${LOGBOOK}/_logbook/entries/nope`);
      assert.ok((await reach(page)).page <= PHONE.width);
    });
  });

  suite("the colours are the app's, not the system's", {}, async () => {
    await traffic();
    for (const scheme of ["light", "dark"]) {
      await withPage(WIDE, async (page) => {
        await page.goto(`${LOGBOOK}/_logbook`);
        const seen = await page.evaluate(() => {
          const luminance = (color) => {
            const [r, g, b] = color.match(/\d+(\.\d+)?/g).map(Number);
            return (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255;
          };
          return {
            scheme: getComputedStyle(document.documentElement).colorScheme,
            body: luminance(getComputedStyle(document.body).backgroundColor),
            text: luminance(getComputedStyle(document.body).color),
          };
        });
        assert.equal(seen.scheme, "dark", `${scheme} system`);
        assert.ok(seen.body < 0.15, `a dark page on a ${scheme} system: ${seen.body}`);
        assert.ok(seen.text > 0.6, `with light text: ${seen.text}`);
      }, { colorScheme: scheme });
    }
  });

  suite("the states: empty, a bad filter, an entry that isn't there, after Clear", {}, async () => {
    await traffic();
    await withPage(WIDE, async (page) => {
      await page.goto(`${LOGBOOK}/_logbook?q=nothing-matches-this`);
      await page.locator("[data-slot=empty]").waitFor();
      assert.ok((await page.locator("[data-slot=empty]").textContent()).includes("for this filter"));
      assert.deepEqual(await axe(page), []);
      await page.goto(`${LOGBOOK}/_logbook?q=status%3Anope`);
      await page.locator("[data-slot=alert]").waitFor();
      assert.ok((await page.locator("[data-slot=alert]").textContent()).includes("status:nope"));
      assert.deepEqual(await axe(page), []);
      const missing = await page.goto(`${LOGBOOK}/_logbook/entries/nope`);
      assert.equal(missing.status(), 404);
      assert.ok((await page.locator("[data-slot=empty]").textContent()).includes("no entry with this id"));
      assert.ok((await page.locator("#rows-body [data-slot=item]").count()) > 0, "the list is still there");
      assert.deepEqual(await axe(page), []);
      await page.getByRole("link", { name: "Back to the list" }).click();
      await page.waitForURL("**/_logbook");
    });
  });

  suite("axe finds nothing on the list and the entry, wide and narrow", {}, async () => {
    await traffic();
    for (const viewport of [WIDE, PHONE]) {
      await withPage(viewport, async (page) => {
        await page.goto(`${LOGBOOK}/_logbook`);
        assert.deepEqual(await axe(page), [], `list at ${viewport.width}`);
        await page.locator("#rows-body [data-slot=item]").first().click();
        await page.waitForURL("**/_logbook/entries/**");
        await page.locator("section#request").waitFor();
        assert.deepEqual(await axe(page), [], `entry at ${viewport.width}`);
        assert.equal(await page.locator("[style]").count(), 0, "no style attribute");
      });
    }
  });

  suite("the app rules hold: 4px and 6px corners, no glow, no pills, no letter tiles", {}, async () => {
    await traffic();
    await withPage(WIDE, async (page) => {
      for (const path of ["/_logbook", "/_logbook?type=request", "/_logbook/entries/nope"]) {
        await page.goto(`${LOGBOOK}${path}`);
        if (path === "/_logbook") {
          await page.locator("#rows-body [data-slot=item]").first().click();
          await page.waitForURL("**/_logbook/entries/**");
        }
        const found = await page.evaluate(() => {
          const bad = [];
          for (const el of document.querySelectorAll("body *")) {
            const style = getComputedStyle(el);
            const slot = el.getAttribute("data-slot") || el.tagName;
            const radius = parseFloat(style.borderTopLeftRadius);
            const round = ["radio", "spinner"].includes(slot) || el.closest("[data-slot=spinner]");
            if (radius && ![2, 4, 6].includes(radius) && !round) bad.push(`radius ${radius}px on ${slot}`);
            if (style.boxShadow !== "none") bad.push(`shadow on ${slot}: ${style.boxShadow}`);
            if (style.textShadow !== "none") bad.push(`text shadow on ${slot}`);
            if (el.hasAttribute("style") && !el.hasAttribute("popover")) bad.push(`style attribute on ${slot}`);
            if (slot === "avatar" && el.textContent.trim()) bad.push(`text in an avatar: ${el.textContent}`);
            if (slot === "badge" && radius > 6) bad.push(`pill badge ${radius}px`);
          }
          return [...new Set(bad)];
        });
        assert.deepEqual(found, [], path);
      }
    });
  });

  suite("the filter keeps focus while the list changes, and an open entry stays open", {}, async () => {
    await traffic();
    await withPage(WIDE, async (page) => {
      await page.goto(`${LOGBOOK}/_logbook?type=request`);
      await page.locator("#rows-body [data-slot=item]").first().click();
      await page.waitForURL("**/_logbook/entries/**");
      const entry = new URL(page.url()).pathname;
      await page.locator("input[name=q]").click();
      await page.keyboard.type("status:2xx", { delay: 15 });
      await page.waitForURL("**q=status**");
      assert.equal(await page.evaluate(() => document.activeElement?.name), "q", "the caret stays in the filter");
      assert.equal(new URL(page.url()).pathname, entry, "the entry stays open");
      assert.equal(await page.locator("section#request").count(), 1, "and on the page");
    });
  });

  suite("Clear empties the list and the counts, and Replay records a new entry", {}, async () => {
    await traffic();
    await withPage(WIDE, async (page) => {
      await page.goto(`${LOGBOOK}/_logbook?type=request`);
      await page.locator("#rows-body [data-slot=item]").first().click();
      await page.waitForURL("**/_logbook/entries/**");
      const first = page.url().split("#")[0];
      await page.getByRole("button", { name: "Replay", exact: true }).click();
      await page.waitForURL((url) => url.href.split("#")[0] !== first && url.pathname.includes("/entries/"), { timeout: 15000 });
      await page.locator("[data-slot=toast]").filter({ hasText: "Replayed" }).waitFor({ timeout: 5000 });
      await page.getByRole("button", { name: "Clear" }).click();
      await page.waitForURL("**/_logbook");
      await page.locator("[data-slot=empty]").first().waitFor({ timeout: 15000 });
      assert.equal(await page.locator("[data-slot=sidebar-count]").count(), 0, "no counts left");
    });
  });
}
