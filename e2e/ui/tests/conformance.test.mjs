// Accessibility and design rules, in every state the proof pages can reach:
// axe finds no violation, the content security policy reports nothing, and the
// app rules hold (radius 4 or 6px, no glow, no text in an avatar, no inline
// style). Keyboard scripts for each native pattern live here too.

import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { createRequire } from "node:module";
import { after, before, test } from "node:test";
import { chromium, firefox } from "playwright";

const URL_BASE = process.env.UI_PROOF_URL || "http://127.0.0.1:8090";
const require = createRequire(import.meta.url);
const axeSource = await readFile(require.resolve("axe-core/axe.min.js"), "utf8");

let browser;
before(async () => {
  browser = await chromium.launch();
});
after(() => browser.close());

/** Runs axe on the page as it stands. Violations come back as short lines. */
async function axe(page) {
  await page.evaluate(axeSource);
  const result = await page.evaluate(async () => {
    const out = await window.axe.run(document, { runOnly: ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa", "best-practice"] });
    return out.violations.map((v) => `${v.id}: ${v.nodes.slice(0, 3).map((n) => n.target.join(" ")).join(" | ")}`);
  });
  return result;
}

/** Every state: a name, a path, the viewport, and what to do first. */
const STATES = [
  ["plain", "/plain"],
  ["dashboard, lazy region loading then filled", "/", async (page) => page.locator("#revenue [data-slot=bars]").waitFor()],
  ["form, empty", "/deals/new"],
  ["form, with errors", "/deals/new", async (page) => {
    await page.fill("[name=title]", "Acme");
    await page.selectOption("[name=company_id]", { index: 1 });
    await page.fill("[name=amount]", "lots");
    await page.getByRole("button", { name: "Create deal" }).click();
    await page.locator("[data-slot=field-error]").waitFor();
  }],
  ["form, on a phone", "/deals/new", null, { width: 390, height: 800 }],
  ["table, 1,000 rows", "/deals?per=1000"],
  ["table, sorted", "/deals?per=50&sort=amount&dir=desc"],
  ["table, paged", "/deals?per=50&page=3"],
  ["table, empty after a filter", "/deals?q=nothing-matches-this"],
  ["table, on a phone", "/deals?per=50", null, { width: 390, height: 800 }],
  ["gallery", "/gallery"],
  ["gallery, sidebar collapsed", "/gallery", async (page) => page.keyboard.press("Control+b")],
  ["gallery, sheet open on a phone", "/gallery", async (page) => {
    await page.getByRole("button", { name: "Toggle sidebar" }).click();
    await page.getByRole("link", { name: "Gallery" }).waitFor();
  }, { width: 390, height: 800 }],
  ["dialog open", "/gallery", async (page) => {
    await page.getByRole("button", { name: "Open dialog" }).click();
    await page.locator("#g-dialog").waitFor();
  }],
  ["alert dialog open", "/gallery", async (page) => {
    await page.getByRole("button", { name: "Open alert" }).click();
    await page.getByRole("alertdialog").waitFor();
  }],
  ["sheet open", "/gallery", async (page) => {
    await page.getByRole("button", { name: "Open sheet" }).click();
    await page.locator("#g-sheet").waitFor();
  }],
  ["drawer open", "/gallery", async (page) => {
    await page.getByRole("button", { name: "Open drawer" }).click();
    await page.locator("#g-drawer").waitFor();
  }],
  ["popover open", "/gallery", async (page) => {
    await page.getByRole("button", { name: "Open popover" }).click();
    await page.locator("#g-popover").waitFor();
  }],
  ["tooltip open", "/gallery", async (page) => {
    await page.getByRole("button", { name: "Tooltip" }).focus();
    await page.getByRole("tooltip").waitFor();
  }],
  ["user menu open", "/", async (page) => {
    await page.locator("[data-slot=sidebar-user]").click();
    await page.locator("#ui-user-menu").waitFor();
  }],
  ["toast showing", "/deals/new", async (page) => {
    await page.fill("[name=title]", "Acme");
    await page.selectOption("[name=company_id]", { index: 1 });
    await page.fill("[name=amount]", "100");
    await page.getByRole("button", { name: "Create deal" }).click();
    await page.locator("[data-slot=toast]").waitFor();
  }],
];

for (const [name, path, prepare, viewport] of STATES) {
  test(`axe, no CSP report: ${name}`, { timeout: 60000 }, async () => {
    const context = await browser.newContext({ viewport: viewport || { width: 1280, height: 800 }, reducedMotion: "reduce" });
    const page = await context.newPage();
    const problems = [];
    page.on("console", (message) => {
      if (message.type() === "error" && !message.text().includes("422")) problems.push(message.text());
    });
    await page.goto(URL_BASE + path);
    if (prepare) await prepare(page);
    const violations = await axe(page);
    await context.close();
    assert.deepEqual(violations, []);
    assert.deepEqual(problems, []);
  });
}

// A real tool's page: run with `UI_PROOF_LOGBOOK=1 PORT=8092 ui-proof serve`.
const LOGBOOK = process.env.UI_LOGBOOK_URL;

test("Logbook's list: axe, new entries morph in, the filter keeps focus, Clear works", { skip: !LOGBOOK, timeout: 60000 }, async () => {
  const context = await browser.newContext({ viewport: { width: 1280, height: 800 } });
  const page = await context.newPage();
  const problems = [];
  page.on("console", (message) => {
    if (message.type() === "error") problems.push(message.text());
  });
  await fetch(`${LOGBOOK}/plain`);
  await page.goto(`${LOGBOOK}/_logbook`);
  assert.ok((await page.locator("#rows tbody tr").count()) >= 1, "the first request is on the page");
  assert.deepEqual(await axe(page), []);
  await page.evaluate(() => { window.kept = document.querySelector("#rows tbody tr"); });
  const before = await page.locator("#rows tbody tr").count();
  await fetch(`${LOGBOOK}/deals/new`);
  await page.waitForFunction((n) => document.querySelectorAll("#rows tbody tr").length > n, before, { timeout: 15000 });
  assert.equal(await page.evaluate(() => window.kept.isConnected), true, "the old row is the same node");
  await page.locator("input[name=q]").click();
  await page.keyboard.type("status:2xx", { delay: 10 });
  await page.waitForURL("**q=status**");
  assert.equal(await page.evaluate(() => document.activeElement?.name), "q");
  await page.getByRole("link", { name: "Requests" }).first().click();
  await page.waitForURL("**type=**");
  await page.getByRole("button", { name: "Clear" }).click();
  await page.locator("[data-slot=empty]").waitFor({ timeout: 15000 });
  await context.close();
  assert.deepEqual(problems, []);
});

test("the design rules hold on every page", { timeout: 60000 }, async () => {
  const context = await browser.newContext({ viewport: { width: 1280, height: 800 } });
  const page = await context.newPage();
  for (const path of ["/", "/deals?per=50", "/deals/new", "/gallery", "/plain"]) {
    await page.goto(URL_BASE + path);
    const found = await page.evaluate(() => {
      const bad = [];
      for (const el of document.querySelectorAll("body *")) {
        const style = getComputedStyle(el);
        const slot = el.getAttribute("data-slot") || el.tagName;
        const radius = parseFloat(style.borderTopLeftRadius);
        // Radio dots and spinners are the only round things.
        const round = ["radio", "spinner"].includes(slot) || el.closest("[data-slot=spinner]");
        if (radius && ![2, 4, 6].includes(radius) && !round) bad.push(`radius ${radius}px on ${slot}`);
        if (style.boxShadow !== "none") bad.push(`shadow on ${slot}: ${style.boxShadow}`);
        if (el.hasAttribute("style") && !el.hasAttribute("popover")) bad.push(`style attribute on ${slot}`);
        if (slot === "avatar" && el.textContent.trim()) bad.push(`text in an avatar: ${el.textContent}`);
      }
      return [...new Set(bad)];
    });
    assert.deepEqual(found, [], path);
  }
  await context.close();
});

test("the keyboard reaches everything in order, with a visible focus ring", { timeout: 60000 }, async () => {
  const context = await browser.newContext({ viewport: { width: 1280, height: 800 } });
  const page = await context.newPage();
  await page.goto(URL_BASE + "/deals/new");
  const seen = [];
  for (let i = 0; i < 40; i++) {
    await page.keyboard.press("Tab");
    const state = await page.evaluate(() => {
      const el = document.activeElement;
      if (!el || el === document.body) return null;
      // An input in a group shows its ring on the group.
      // A date input's picker button has focus inside the input, and its own ring.
      const picker = el.type === "date" && !el.matches(":focus");
      const style = picker ? getComputedStyle(el, "::-webkit-calendar-picker-indicator") : getComputedStyle(el.closest("[data-slot=input-group]") || el);
      if (picker) return { name: el.getAttribute("name"), ring: true };
      return { name: el.getAttribute("name") || el.textContent.trim() || el.getAttribute("aria-label"), ring: style.outlineStyle !== "none" && parseFloat(style.outlineWidth) > 0 };
    });
    if (!state) break;
    seen.push(state);
  }
  await context.close();
  const names = seen.map((s) => s.name);
  for (const expected of ["Toggle sidebar", "title", "company_id", "amount", "currency", "Create deal"]) {
    assert.ok(names.includes(expected), `${expected} is reachable: ${names}`);
  }
  assert.ok(names.indexOf("title") < names.indexOf("company_id"), String(names));
  assert.deepEqual(seen.filter((s) => !s.ring), [], "every stop shows a ring");
});

test("radio buttons, switches and toggles use the browser's keyboard", { timeout: 60000 }, async () => {
  const context = await browser.newContext({ viewport: { width: 1280, height: 800 } });
  const page = await context.newPage();
  await page.goto(URL_BASE + "/gallery");
  const first = page.locator("[name=radio]").first();
  await first.focus();
  await page.keyboard.press("ArrowDown");
  assert.equal(await page.locator("[name=radio]:checked").getAttribute("value"), "2");
  const toggle = page.locator("[name=bold]");
  await toggle.focus();
  await page.keyboard.press("Space");
  assert.equal(await toggle.isChecked(), true);
  const on = page.locator("[name=s1]");
  assert.equal(await on.getAttribute("role"), "switch");
  await on.focus();
  await page.keyboard.press("Space");
  assert.equal(await on.isChecked(), true);
  await context.close();
});

test("firefox: dialogs, popovers and forms work the same", { timeout: 120000 }, async () => {
  const engine = await firefox.launch();
  const context = await engine.newContext({ viewport: { width: 1280, height: 800 } });
  const page = await context.newPage();
  const problems = [];
  page.on("pageerror", (error) => problems.push(String(error)));
  await page.goto(URL_BASE + "/gallery");
  await page.getByRole("button", { name: "Open dialog" }).click();
  const dialog = page.locator("#g-dialog");
  await dialog.waitFor();
  await page.keyboard.press("Escape");
  await dialog.waitFor({ state: "hidden" });
  const button = page.getByRole("button", { name: "Open popover" });
  await button.click();
  await page.locator("#g-popover").waitFor();
  const [a, p] = [await button.boundingBox(), await page.locator("#g-popover").boundingBox()];
  assert.ok(Math.abs(p.y - (a.y + a.height)) < 8 || Math.abs(a.y - (p.y + p.height)) < 8, JSON.stringify({ a, p }));
  await page.goto(URL_BASE + "/deals?per=1000");
  await page.getByRole("link", { name: "Amount" }).click();
  await page.locator("th[aria-sort=ascending]").waitFor();
  await engine.close();
  assert.deepEqual(problems, []);
});
