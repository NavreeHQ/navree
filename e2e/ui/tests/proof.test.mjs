// The proof pages, end to end in a real browser: what ships, forms, morph,
// boosted links, the sidebar, native overlays, commands. Needs the proof app:
//   cargo run --release -p ui-proof -- serve       (UI_PROOF_URL to point elsewhere)

import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import { chromium } from "playwright";

const URL_BASE = process.env.UI_PROOF_URL || "http://127.0.0.1:8090";

let browser;
before(async () => {
  browser = await chromium.launch();
});
after(() => browser.close());

/** A fresh page that records console errors, CSP violations and requests. */
async function open(path) {
  const context = await browser.newContext({ viewport: { width: 1280, height: 800 } });
  const page = await context.newPage();
  const problems = [];
  const requests = [];
  page.on("console", (message) => {
    if (message.type() === "error") problems.push(message.text());
  });
  page.on("pageerror", (error) => problems.push(String(error)));
  page.on("request", (request) => requests.push(request));
  await page.goto(URL_BASE + path);
  return { page, problems, requests, close: () => context.close() };
}

test("a page with nothing interactive ships no script", async () => {
  const { page, problems, requests, close } = await open("/plain");
  await page.waitForLoadState("networkidle");
  assert.equal(await page.locator("script").count(), 0);
  assert.equal(requests.filter((r) => r.resourceType() === "script").length, 0);
  assert.deepEqual(problems, []);
  await close();
});

test("the shell page loads the core and sidebar modules and nothing inline", async () => {
  const { page, problems, close } = await open("/");
  const scripts = await page.locator("script").evaluateAll((all) => all.map((s) => [s.type, s.getAttribute("src")]));
  assert.ok(scripts.every(([type, src]) => type === "module" && src.startsWith("/_navree/ui/")), JSON.stringify(scripts));
  assert.equal(await page.locator("[style]").count(), 0);
  assert.deepEqual(problems, []);
  await close();
});

test("a lazy region fills in after paint", async () => {
  const { page, problems, close } = await open("/");
  await page.locator("#revenue [data-slot=bars]").waitFor();
  assert.equal(await page.locator("#revenue").getAttribute("aria-busy"), null);
  assert.deepEqual(problems, []);
  await close();
});

test("a failed submit shows errors under the fields, with no reload", async () => {
  const { page, problems, close } = await open("/deals/new");
  await page.evaluate(() => { window.kept = true; });
  await page.fill("[name=title]", "Acme renewal");
  await page.selectOption("[name=company_id]", { index: 1 });
  await page.fill("[name=amount]", "lots");
  await page.selectOption("[name=stage]", { index: 0 });
  await page.getByRole("button", { name: "Create deal" }).click();
  const error = page.locator("#new-deal [data-slot=field-error]");
  await error.waitFor();
  assert.match(await error.textContent(), /12,500/);
  assert.equal(await page.evaluate(() => window.kept), true, "the page did not reload");
  assert.equal(await page.inputValue("[name=title]"), "Acme renewal", "what was typed stays");
  const amount = page.locator("[name=amount]");
  assert.equal(await amount.getAttribute("aria-invalid"), "true");
  assert.equal(await page.evaluate(() => document.activeElement?.getAttribute("name")), "amount", "focus goes to the first error");
  assert.ok((await amount.getAttribute("aria-describedby")).includes(await error.getAttribute("id")));
  // The 422 is what the browser logs for a rejected form, and nothing else.
  assert.deepEqual(problems.filter((p) => !p.includes("422")), []);
  await close();
});

test("the pending state shows within a frame of the submit", async () => {
  const { page, close } = await open("/deals/new");
  await page.fill("[name=title]", "Acme");
  await page.selectOption("[name=company_id]", { index: 1 });
  await page.fill("[name=amount]", "100");
  await page.route("**/deals", async (route) => {
    await new Promise((resolve) => setTimeout(resolve, 300));
    await route.continue();
  });
  const seen = await page.evaluate(async () => {
    const form = document.getElementById("new-deal");
    const button = form.querySelector("[type=submit]");
    button.click();
    const sync = button.hasAttribute("data-pending") && form.getAttribute("aria-busy") === "true";
    await new Promise(requestAnimationFrame);
    return { sync, frame: button.hasAttribute("data-pending") };
  });
  assert.deepEqual(seen, { sync: true, frame: true });
  await close();
});

test("a good submit toasts and goes on, keeping the shell", async () => {
  const { page, problems, close } = await open("/deals/new");
  const sidebar = await page.evaluateHandle(() => document.querySelector("[data-slot=sidebar]"));
  await page.evaluate(() => { window.kept = true; });
  await page.fill("[name=title]", "Acme renewal");
  await page.selectOption("[name=company_id]", { index: 1 });
  await page.fill("[name=amount]", "12,500.50");
  await page.getByRole("button", { name: "Create deal" }).click();
  await page.waitForURL("**/deals?per=1000");
  await page.locator("[data-slot=toast]", { hasText: "Acme renewal created" }).waitFor();
  assert.equal(await page.evaluate(() => window.kept), true, "no reload");
  assert.equal(await sidebar.evaluate((el) => el.isConnected), true, "the same sidebar");
  assert.equal(await page.title(), "Deals");
  assert.deepEqual(problems, []);
  await close();
});

test("the form works with JavaScript off", async () => {
  const context = await browser.newContext({ javaScriptEnabled: false });
  const page = await context.newPage();
  await page.goto(URL_BASE + "/deals/new");
  await page.fill("[name=title]", "Acme");
  await page.selectOption("[name=company_id]", { index: 1 });
  await page.fill("[name=amount]", "lots");
  await page.getByRole("button", { name: "Create deal" }).click();
  await page.locator("[data-slot=field-error]").waitFor();
  assert.equal(await page.inputValue("[name=title]"), "Acme", "the page comes back with what was typed");
  await page.fill("[name=amount]", "100");
  await page.getByRole("button", { name: "Create deal" }).click();
  await page.waitForURL("**/deals?per=1000");
  assert.equal(await page.locator("tbody tr").count(), 1000);
  assert.match(await page.locator("[data-slot=alert]").textContent(), /Acme created/, "the toast comes through the redirect");
  await page.reload();
  assert.equal(await page.locator("[data-slot=alert]").count(), 0, "and only once");
  // Sorting is a plain link.
  await page.getByRole("link", { name: "Amount" }).click();
  await page.waitForURL("**sort=amount**");
  assert.equal(await page.locator("th[aria-sort=ascending]").count(), 1);
  await context.close();
});

test("sorting a 1,000-row table morphs it in place and keeps the URL", async () => {
  const { page, problems, close } = await open("/deals?per=1000");
  assert.equal(await page.locator("tbody tr").count(), 1000);
  const table = await page.evaluateHandle(() => document.querySelector("[data-slot=table]"));
  const row = await page.evaluateHandle(() => document.querySelector('tr[data-key="7"]'));
  await page.getByRole("link", { name: "Amount" }).click();
  await page.waitForURL("**sort=amount**");
  await page.locator("th[aria-sort=ascending]").waitFor();
  assert.equal(await table.evaluate((el) => el.isConnected), true, "the same table");
  assert.equal(await row.evaluate((el) => el.isConnected && el.dataset.key), "7", "the same row, moved");
  assert.equal(await page.locator("tbody tr").first().getAttribute("data-key"), "1");
  await page.getByRole("link", { name: "Amount" }).click();
  await page.locator("th[aria-sort=descending]").waitFor();
  // The rows move at once; the address moves when the server confirms.
  await page.waitForURL("**sort=-amount**");
  await page.goBack();
  await page.locator("th[aria-sort=ascending]").waitFor();
  assert.deepEqual(problems, []);
  await close();
});

test("filtering as you type keeps focus and caret", async () => {
  const { page, problems, close } = await open("/deals?per=1000");
  const search = page.locator("input[name=q]");
  await search.click();
  await page.keyboard.type("acme", { delay: 20 });
  await page.waitForURL("**q=acme**");
  await page.locator("text=of 167").waitFor();
  const state = await search.evaluate((el) => ({ focused: document.activeElement === el, value: el.value, caret: el.selectionStart }));
  assert.deepEqual(state, { focused: true, value: "acme", caret: 4 });
  await page.selectOption("select[name=stage]", "won");
  await page.waitForURL("**stage=won**");
  assert.equal(await page.inputValue("input[name=q]"), "acme");
  assert.deepEqual(problems, []);
  await close();
});

test("a boosted link keeps the page alive and moves focus to the content", async () => {
  const { page, problems, close } = await open("/");
  await page.evaluate(() => { window.kept = true; });
  await page.getByRole("link", { name: "Gallery" }).click();
  await page.waitForURL("**/gallery");
  await page.locator("[data-gallery=Buttons]").waitFor();
  assert.equal(await page.evaluate(() => window.kept), true);
  assert.equal(await page.title(), "Gallery");
  assert.equal(await page.evaluate(() => document.activeElement?.tagName), "MAIN");
  assert.equal(await page.locator('[data-slot=sidebar-item][aria-current=page]').textContent(), "Gallery");
  await page.goBack();
  await page.waitForURL(URL_BASE + "/");
  await page.waitForFunction(() => document.title === "Overview");
  assert.equal(await page.evaluate(() => window.kept), true);
  assert.deepEqual(problems, []);
  await close();
});

test("the sidebar collapses, remembers, and paints right the first time", async () => {
  const { page, close } = await open("/");
  const wrapper = page.locator("[data-slot=sidebar-wrapper]");
  assert.equal(await wrapper.getAttribute("data-collapsed"), null);
  await page.keyboard.press("Control+b");
  assert.equal(await wrapper.getAttribute("data-collapsed"), "");
  await page.waitForFunction(() => document.querySelector("[data-slot=sidebar]").getBoundingClientRect().width <= 49);
  // A boosted navigation keeps it.
  await page.getByRole("link", { name: "Gallery" }).click();
  await page.waitForURL("**/gallery");
  assert.equal(await wrapper.getAttribute("data-collapsed"), "");
  const html = await (await page.context().request.get(URL_BASE + "/")).text();
  assert.match(html, /data-slot="sidebar-wrapper"[^>]*data-collapsed/);
  await page.getByRole("button", { name: "Toggle sidebar" }).click();
  assert.equal(await wrapper.getAttribute("data-collapsed"), null);
  await close();
});

test("the sidebar is a sheet on phones", async () => {
  const context = await browser.newContext({ viewport: { width: 390, height: 800 } });
  const page = await context.newPage();
  await page.goto(URL_BASE + "/");
  const sidebar = page.locator("[data-slot=sidebar]");
  assert.equal(await sidebar.isVisible(), false);
  await page.getByRole("button", { name: "Toggle sidebar" }).click();
  await page.getByRole("link", { name: "Gallery" }).waitFor();
  assert.equal(await sidebar.isVisible(), true);
  await page.keyboard.press("Escape");
  await page.waitForFunction(() => !document.querySelector("[data-mobile-open]"));
  await context.close();
});

test("dialogs open natively, trap focus, close on Esc and give focus back", async () => {
  const { page, requests, close } = await open("/gallery");
  const before = requests.length;
  const opener = page.getByRole("button", { name: "Open dialog" });
  await opener.focus();
  await page.keyboard.press("Enter");
  const dialog = page.locator("#g-dialog");
  await dialog.waitFor();
  assert.equal(await dialog.evaluate((el) => el.matches(":modal")), true);
  assert.ok(await page.evaluate(() => document.getElementById("g-dialog").contains(document.activeElement)), "focus moved in");
  await page.keyboard.press("Escape");
  await dialog.waitFor({ state: "hidden" });
  assert.equal(await opener.evaluate((el) => document.activeElement === el), true, "focus returned");
  assert.equal(requests.length, before, "no network");
  await close();
});

test("a dialog's first field takes focus, not its close button", async () => {
  const { page, close } = await open("/gallery");
  await page.getByRole("button", { name: "Open dialog" }).click();
  await page.locator("#g-dialog").waitFor();
  assert.equal(await page.evaluate(() => document.activeElement?.getAttribute("name")), "name");
  await close();
});

test("Esc cancels an alert dialog", async () => {
  const { page, close } = await open("/gallery");
  await page.getByRole("button", { name: "Open alert" }).click();
  const alert = page.getByRole("alertdialog");
  await alert.waitFor();
  await page.keyboard.press("Escape");
  await alert.waitFor({ state: "hidden" });
  await close();
});

test("a popover takes focus and gives it back on Esc", async () => {
  const { page, close } = await open("/gallery");
  const button = page.getByRole("button", { name: "Open popover" });
  await button.focus();
  await page.keyboard.press("Enter");
  await page.locator("#g-popover").waitFor();
  assert.equal(await page.evaluate(() => document.activeElement?.id), "g-popover");
  await page.keyboard.press("Escape");
  await page.locator("#g-popover").waitFor({ state: "hidden" });
  assert.equal(await button.evaluate((el) => document.activeElement === el), true);
  await close();
});

test("an alert dialog focuses Cancel and ignores a click outside", async () => {
  const { page, close } = await open("/gallery");
  await page.getByRole("button", { name: "Open alert" }).click();
  const alert = page.getByRole("alertdialog");
  await alert.waitFor();
  assert.equal(await page.evaluate(() => document.activeElement?.textContent), "Cancel");
  await page.mouse.click(5, 5);
  assert.equal(await alert.isVisible(), true);
  await page.keyboard.press("Tab");
  await page.getByRole("button", { name: "Cancel" }).click();
  await alert.waitFor({ state: "hidden" });
  await close();
});

test("a popover sits by its button and dismisses on a click outside", async () => {
  const { page, problems, close } = await open("/gallery");
  const button = page.getByRole("button", { name: "Open popover" });
  await button.scrollIntoViewIfNeeded();
  await button.click();
  const popover = page.locator("#g-popover");
  await popover.waitFor();
  // The browser's toggle event places it, a task after it shows.
  await page.waitForFunction(() => document.getElementById("g-popover").style.top !== "");
  const [a, p] = [await button.boundingBox(), await popover.boundingBox()];
  assert.ok(Math.abs(p.y - (a.y + a.height)) < 8 || Math.abs(a.y - (p.y + p.height)) < 8, JSON.stringify({ a, p }));
  await page.mouse.click(5, 5);
  await popover.waitFor({ state: "hidden" });
  assert.deepEqual(problems, []);
  await close();
});

test("a tooltip shows on focus and hover, with its role, and Esc hides it", async () => {
  const { page, close } = await open("/gallery");
  const button = page.getByRole("button", { name: "Tooltip" });
  await button.focus();
  const tip = page.getByRole("tooltip");
  await tip.waitFor();
  assert.equal(await tip.textContent(), "Says more");
  await page.keyboard.press("Escape");
  await tip.waitFor({ state: "hidden" });
  await close();
});

test("an accordion is one-at-a-time, by the browser", async () => {
  const { page, close } = await open("/gallery");
  const summaries = page.locator("[data-slot=accordion-trigger]");
  await summaries.nth(1).focus();
  await page.keyboard.press("Enter");
  const states = await page.locator("[data-slot=accordion-item]").evaluateAll((all) => all.map((el) => el.open));
  assert.deepEqual(states, [false, true]);
  await close();
});

test("client-only commands make no network request", async () => {
  const { page, requests, close } = await open("/gallery");
  await page.waitForLoadState("networkidle");
  const before = requests.length;
  await page.locator("#g-toggle-button").click();
  assert.equal(await page.locator("#g-toggled").isVisible(), false);
  await page.locator("#g-toggle-button").click();
  assert.equal(await page.locator("#g-toggled").isVisible(), true);
  assert.equal(requests.length, before);
  await close();
});

test("text, a counter and a row's details need no request either", async () => {
  const { page, requests, problems, close } = await open("/gallery");
  await page.waitForLoadState("networkidle");
  const before = requests.length;
  await page.locator("#g-yearly").click();
  assert.equal(await page.textContent("#g-price"), "$120 a year");
  await page.locator("#g-monthly").click();
  assert.equal(await page.textContent("#g-price"), "$12 a month");
  await page.getByLabel("Bio").pressSequentially("Hello \u{1F44B}");
  assert.equal(await page.textContent("#g-bio-count"), "7/160");
  await page.locator("#g-fill-8").click();
  await page.locator("#g-fill[open]").waitFor();
  assert.deepEqual(await page.evaluate(() => [document.getElementById("g-fill-name").textContent, document.getElementById("g-fill-sum").textContent, document.getElementById("g-fill-key").value]), ["Globex expansion", "48,000", "8"]);
  await page.keyboard.press("Escape");
  assert.equal(requests.length, before, "no network");
  assert.deepEqual(problems, []);
  await close();
});

test("assets are immutable, precompressed and negotiated", async () => {
  const { page, close } = await open("/");
  const src = await page.locator("script").first().getAttribute("src");
  const get = (encoding) => fetch(URL_BASE + src, { headers: { "accept-encoding": encoding } });
  const br = await get("br");
  assert.equal(br.headers.get("content-encoding"), "br");
  assert.match(br.headers.get("cache-control"), /immutable/);
  assert.equal(br.headers.get("vary"), "accept-encoding");
  assert.equal((await get("gzip")).headers.get("content-encoding"), "gzip");
  const stale = await fetch(URL_BASE + "/_navree/ui/core-000000000000.js");
  assert.equal(stale.status, 200);
  assert.equal(stale.headers.get("cache-control"), "no-cache");
  assert.equal((await fetch(URL_BASE + "/_navree/ui/nothing-000000000000.js")).status, 404);
  await close();
});
