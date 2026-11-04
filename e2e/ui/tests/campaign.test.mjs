// The example app in real browsers: an email campaign's overview with its feed live
// over server-sent events, the recipients table, a recipient in a sheet, and the
// contact form. Needs the app serving with its simulator off and its test hooks on:
//
//   CAMPAIGN_SIMULATOR=off CAMPAIGN_TEST_HOOKS=1 cargo run --release -p campaign-demo
//
// CAMPAIGN_URL points elsewhere (default http://127.0.0.1:8095); CAMPAIGN_BROWSERS
// picks engines (default "chromium,firefox"; add webkit); CAMPAIGN_SHOTS=<dir> saves
// a screenshot of every page in every state. Without the app the suite is skipped.

import assert from "node:assert/strict";
import { mkdirSync, readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import { after, afterEach, before, beforeEach, describe, test } from "node:test";
import { fileURLToPath } from "node:url";
import { chromium, firefox, webkit } from "playwright";

const BASE = process.env.CAMPAIGN_URL || "http://127.0.0.1:8095";
const require = createRequire(import.meta.url);
const axeSource = readFileSync(require.resolve("axe-core/axe.min.js"), "utf8");
const ENGINES = { chromium, firefox, webkit };
const NAMES = (process.env.CAMPAIGN_BROWSERS || "chromium,firefox").split(",").filter((n) => ENGINES[n]);
const SHOTS = process.env.CAMPAIGN_SHOTS;
if (SHOTS) mkdirSync(SHOTS, { recursive: true });

/** The light values of the tokens, from the toolkit's one source: the toolkit ships dark only so far. */
const here = dirname(fileURLToPath(import.meta.url));
const tokensRs = readFileSync(join(here, "../../../crates/navree-ui/src/tokens.rs"), "utf8");
const LIGHT =
  ":root{color-scheme:light" +
  [...tokensRs.matchAll(/t\("([a-z0-9-]+)",\s*"(#[0-9a-f]{6})",\s*"(#[0-9a-f]{6})"\)/g)].map((m) => `;--${m[1]}:${m[3]}`).join("") +
  "}body{background:var(--ink-0)}";

/** Round on purpose: status dots, the feed's icons and radios. Not pills, not letter tiles. */
const ROUND = ["timeline-dot", "live-dot", "badge-dot", "activity-icon", "radio", "spinner"];

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const post = (path) => fetch(BASE + path, { method: "POST" });
const step = async (kind) => {
  const text = await (await post(`/__test/step?kind=${kind}`)).text();
  const [event, recipient] = text.split(" ").map(Number);
  assert.ok(event > 0, `a ${kind} happened: "${text}"`);
  return { event, recipient };
};

let reachable = false;
try {
  reachable = (await fetch(BASE + "/__test/reset", { method: "POST" })).status === 204;
} catch {}

async function axe(page) {
  await page.evaluate(axeSource);
  return page.evaluate(async () => {
    const out = await window.axe.run(document, { runOnly: ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa", "best-practice"] });
    return out.violations.map((v) => `${v.id}: ${v.nodes.slice(0, 3).map((n) => n.target.join(" ")).join(" | ")}`);
  });
}

/** The app's design rules: 2, 4 or 6 px corners (dots excepted), no shadow, no style attribute, no text in an avatar. */
const rules = (page) =>
  page.evaluate((round) => {
    const bad = [];
    for (const el of document.querySelectorAll("body *")) {
      // Only what is drawn: a hidden select that posts a picker's value has no box.
      if (el.closest("template") || el.getClientRects().length === 0) continue;
      const style = getComputedStyle(el);
      const slot = el.getAttribute("data-slot") || el.tagName;
      const radius = parseFloat(style.borderTopLeftRadius);
      if (radius && ![2, 4, 6].includes(radius) && !round.includes(slot) && !el.closest("[data-slot=spinner]")) bad.push(`radius ${style.borderTopLeftRadius} on ${slot}`);
      // An inset hairline (a tab list's rule) is a line, not a shadow; overlays keep theirs.
      const shadows = style.boxShadow === "none" ? [] : style.boxShadow.split(/,(?![^(]*\))/).filter((s) => !/inset/.test(s));
      if (shadows.length && !el.matches("dialog, [popover], [data-slot=toast], [data-slot=toast] *")) bad.push(`shadow on ${slot}: ${style.boxShadow}`);
      // The runtime keeps measurements in custom properties through the CSSOM, which the
      // policy allows; anything else in a style attribute would be a style the page wrote.
      const own = [...el.style].filter((property) => !property.startsWith("--"));
      if (el.hasAttribute("style") && !el.hasAttribute("popover") && own.length) bad.push(`style attribute on ${slot}: ${own.join(", ")}`);
      if (slot === "avatar" && el.textContent.trim()) bad.push(`text in an avatar: ${el.textContent}`);
    }
    return [...new Set(bad)];
  }, ROUND);

const sideways = (page) => page.evaluate(() => document.documentElement.scrollWidth > window.innerWidth);

for (const name of NAMES) {
  describe(`campaign app in ${name}`, { concurrency: false, skip: reachable ? false : `the app isn't serving at ${BASE}` }, () => {
    let browser;
    before(async () => {
      // Firefox slows a background tab's network while another load (an open event stream) runs.
      browser = await ENGINES[name].launch(name === "firefox" ? { firefoxUserPrefs: { "network.http.throttle.enable": false } } : {});
    });
    after(() => browser?.close());
    const contexts = [];
    afterEach(async () => {
      for (const context of contexts.splice(0)) await context.close().catch(() => {});
    });
    beforeEach(async () => {
      assert.equal((await post("/__test/reset")).status, 204);
    });

    /** A page of the app. Console errors, page errors and policy reports are kept; `page.done()` asserts none. */
    async function open(path, options = {}) {
      const { light, ...rest } = options;
      const context = await browser.newContext({ viewport: { width: 1440, height: 900 }, bypassCSP: !!light, ...rest });
      contexts.push(context);
      const page = await context.newPage();
      page.problems = [];
      page.on("console", (m) => {
        if (name === "webkit" && /Refused to apply a stylesheet/.test(m.text())) return;
        if (m.type() === "error" && !/Failed to load resource|ERR_ABORTED|NS_ERROR|Load failed|can.t establish a connection/i.test(m.text())) page.problems.push(m.text());
      });
      page.on("pageerror", (e) => page.problems.push("pageerror: " + e.message));
      await page.addInitScript(() => {
        document.addEventListener("securitypolicyviolation", (e) => {
          (window.__csp ||= []).push(e.violatedDirective + " " + e.blockedURI);
        });
      });
      await page.goto(BASE + path);
      if (light) {
        await page.addStyleTag({ content: LIGHT });
        await page.waitForTimeout(300);
      }
      page.done = async () => {
        let csp = rest.javaScriptEnabled === false ? [] : await page.evaluate(() => window.__csp || []);
        if (name === "webkit") csp = csp.filter((line) => !/^style-src-(elem|attr) inline$/.test(line));
        assert.deepEqual(csp, [], "no content security policy report");
        assert.deepEqual(page.problems, []);
      };
      return page;
    }

    const shot = async (page, label, fullPage = true) => {
      if (SHOTS) await page.screenshot({ path: join(SHOTS, `${name}-${label}.png`), fullPage });
    };
    const live = (page, id) => page.waitForFunction((region) => document.getElementById(region)?.getAttribute("data-live") === "on", id, { timeout: 10000 });
    const feedIds = (page) => page.evaluate(() => [...document.querySelectorAll("#activity-list > li")].map((li) => li.id));
    const stat = (page, label) =>
      page
        .locator("#figures [data-slot=stat]")
        .filter({ has: page.locator("[data-slot=stat-label]", { hasText: new RegExp(`^${label}$`) }) })
        .locator("[data-slot=stat-value]")
        .textContent();

    // ---- Every page, every width, both themes -----------------------------------

    for (const theme of ["dark", "light"]) {
      for (const width of [1440, 390]) {
        test(`every page is axe clean, keeps the rules and doesn't scroll sideways (${theme}, ${width} px)`, async () => {
          for (const path of ["/", "/recipients", "/recipients/7", "/recipients/7/edit"]) {
            const page = await open(path, { light: theme === "light", viewport: { width, height: 900 } });
            assert.deepEqual(await axe(page), [], path);
            assert.deepEqual(await rules(page), [], path);
            assert.equal(await sideways(page), false, `${path} scrolls sideways`);
            await shot(page, `${path.replaceAll("/", "_") || "_"}-${theme}-${width}`);
            await page.done();
          }
        });
      }
    }

    // ---- The overview ---------------------------------------------------------------

    test("the overview: five figures with rates, a chart and a funnel drawn as SVG, and the feed with its notice", async () => {
      const page = await open("/");
      assert.equal(await page.locator("#figures [data-slot=stat]").count(), 5);
      assert.equal(await stat(page, "Sent"), "1,240");
      assert.match(await page.locator("#figures").textContent(), /% open rate/);
      const chart = page.locator("#engagement svg[role=img]");
      assert.match(await chart.getAttribute("aria-label"), /^Opens and clicks in each hour/);
      assert.equal(await chart.locator("polyline").count(), 2, "a line for opens and one for clicks");
      assert.equal(await page.locator("#engagement svg[aria-hidden=true] rect").count(), 4 * 2 + 2, "four bars, each on its track, and the legend");
      // The feed: newest first, every row a link to its recipient.
      const ids = await feedIds(page);
      assert.ok(ids.length > 10, "rows in the last 24 hours");
      const numbers = ids.map((id) => Number(id.replace("event-", "")));
      assert.deepEqual(numbers, [...numbers].sort((a, b) => b - a), "newest first");
      assert.match(await page.locator("#activity-list > li a").first().getAttribute("href"), /^\/recipients\/\d+$/);
      // Opens a proxy made are in the page but out of sight, behind the notice.
      const notice = page.locator("#activity-hidden");
      assert.match(await notice.locator("[data-slot=hidden-notice-text]").textContent(), /^\d+ opens came through a mail privacy proxy\.$/);
      const proxied = page.locator("#activity-list > li[data-filtered]");
      assert.ok((await proxied.count()) > 0);
      assert.equal(await proxied.first().isVisible(), false);
      await notice.locator("[data-slot=hidden-notice-toggle]").click();
      assert.equal(await proxied.first().isVisible(), true, "one tick shows them, with no request");
      await page.done();
    });

    test("the feed, the figures and the chart follow the server-sent event stream", async () => {
      const page = await open("/");
      await live(page, "activity-region");
      const clicked = Number((await stat(page, "Clicked")).replace(",", ""));
      const before = await feedIds(page);
      const { event, recipient } = await step("clicked");
      // The new row arrives at the top, once, and the figure moves.
      await page.locator(`#event-${event}`).waitFor({ timeout: 10000 });
      assert.equal((await feedIds(page))[0], `event-${event}`);
      assert.equal(await page.locator(`#event-${event}`).count(), 1);
      assert.match(await page.locator(`#event-${event} a`).getAttribute("href"), new RegExp(`/recipients/${recipient}$`));
      await page.waitForFunction((n) => document.querySelector("#figures [data-slot=stat]:nth-child(4) [data-slot=stat-value]")?.textContent === String(n).replace(/\B(?=(\d{3})+(?!\d))/g, ","), clicked + 1, { timeout: 10000 });
      assert.deepEqual((await feedIds(page)).slice(1, before.length + 1), before, "the rows already there stay, in order");
      // A proxy's open arrives hidden, and the notice counts it.
      const text = async () => page.locator("#activity-hidden [data-slot=hidden-notice-text]").textContent();
      const hidden = Number((await text()).match(/^\d+/)[0]);
      const proxy = await step("proxy_open");
      await page.locator(`#event-${proxy.event}`).waitFor({ state: "attached", timeout: 10000 });
      assert.equal(await page.locator(`#event-${proxy.event}`).isVisible(), false);
      await page.waitForFunction((n) => document.querySelector("#activity-hidden [data-slot=hidden-notice-text]")?.textContent.startsWith(`${n} opens`), hidden + 1, { timeout: 10000 });
      await shot(page, "overview-live");
      await page.done();
    });

    test("the period select narrows the feed in place", async () => {
      const page = await open("/");
      const all = (await feedIds(page)).length;
      await page.locator("select[name=period]").selectOption("1h");
      await page.waitForFunction(() => /in the last hour/.test(document.querySelector("#activity [data-slot=activity-summary]")?.textContent || ""));
      assert.ok((await feedIds(page)).length < all);
      assert.equal(new URL(page.url()).pathname, "/");
      await page.done();
    });

    // ---- The recipients table -----------------------------------------------------------

    const rows = (page) => page.locator("#recipients tbody tr[data-key]");
    const settled = (page) => page.waitForFunction(() => !document.getElementById("recipients")?.hasAttribute("data-pending") && !document.documentElement.hasAttribute("data-ui-loading"));
    const footer = (page) => page.locator("#recipients").getByText(/^Showing /).textContent();

    test("the table: search, status and date filters, sort and pages, all on the server", async () => {
      const page = await open("/recipients");
      assert.match(await footer(page), /of 1,240 results/);
      assert.equal(await rows(page).count(), 25);
      // Search finds a name or an address.
      await page.locator("#recipients input[name=q]").fill("maya");
      await page.waitForFunction(() => new URL(location.href).searchParams.get("q") === "maya");
      await settled(page);
      const names = await rows(page).locator("button[aria-haspopup=dialog]").allTextContents();
      assert.ok(names.length > 0 && names.every((n) => /maya/i.test(n)), names.join(", "));
      await page.locator("#recipients input[name=q]").fill("");
      await page.waitForFunction(() => !new URL(location.href).searchParams.get("q"));
      await settled(page);
      // The status filter lists each status with its count, and ticks apply at once.
      await page.locator("#recipients [data-filter-trigger]", { hasText: "Status" }).click();
      const bounced = page.locator("#recipients-p-status label", { hasText: "Bounced" });
      const count = Number((await bounced.textContent()).match(/\d+/)[0]);
      await bounced.locator("input").check();
      await page.waitForFunction(() => new URL(location.href).searchParams.getAll("f.status").includes("bounced"));
      await settled(page);
      await page.keyboard.press("Escape");
      assert.match(await footer(page), new RegExp(`of ${count} results`));
      const badges = await rows(page).locator("[data-slot=badge]").allTextContents();
      assert.ok(badges.every((b) => b.trim() === "Bounced"), badges.join(", "));
      assert.equal(await rows(page).first().locator("[data-slot=badge-dot]").count(), 1, "a dot");
      await shot(page, "recipients-bounced", false);
      // A date range, from the URL as a link would carry it.
      const yesterday = new Date(Date.now() - 86400000).toISOString().slice(0, 10);
      await page.goto(BASE + `/recipients?f.last_activity.min=${yesterday}&sort=-opens`);
      await settled(page);
      const days = await rows(page).locator("td:last-child").allTextContents();
      assert.ok(days.length > 0 && days.every((d) => ["Today", "Yesterday"].includes(d.trim())), days.join(", "));
      const opens = (await rows(page).evaluateAll((trs) => trs.map((tr) => Number(tr.querySelector("[data-col=opens]")?.textContent || tr.cells[4]?.textContent))));
      assert.deepEqual(opens, [...opens].sort((a, b) => b - a), "sorted by opens, most first");
      // The next page.
      await page.goto(BASE + "/recipients");
      await page.locator("#recipients a[rel=next], #recipients a[aria-label='Next page']").first().click();
      await page.waitForFunction(() => new URL(location.href).searchParams.get("page") === "2");
      await settled(page);
      assert.match(await footer(page), /26 to 50 of 1,240/);
      await page.done();
    });

    test("the table refreshes when the stream speaks, and the status counts follow", async () => {
      const page = await open("/recipients?sort=-last_activity");
      await page.waitForFunction(() => document.getElementById("recipients")?.getAttribute("data-live") === "on", null, { timeout: 10000 }).catch(() => {});
      const { recipient } = await step("converted");
      await page.waitForFunction((id) => document.querySelector("#recipients tbody tr[data-key]")?.getAttribute("data-key") === String(id), recipient, { timeout: 10000 });
      assert.equal((await rows(page).first().locator("[data-slot=badge]").textContent()).trim(), "Converted");
      await page.done();
    });

    // ---- A recipient in a sheet ---------------------------------------------------------

    test("a row opens the recipient in a sheet: chips, facts, tabs and the timeline", async () => {
      const page = await open("/recipients");
      const first = rows(page).first().locator("button[aria-haspopup=dialog]");
      const who = (await first.textContent()).trim();
      await first.click();
      const sheet = page.locator("dialog#recipient[open]");
      await sheet.waitFor({ timeout: 8000 });
      assert.equal((await sheet.locator("[data-slot=dialog-title]").textContent()).trim(), who);
      assert.equal(await sheet.getAttribute("data-width"), "lg");
      assert.equal(await sheet.locator("[data-slot=badge][data-variant=outcome]").count() >= 4, true, "the outcome chips");
      const facts = await sheet.locator("[data-slot=fact-grid]").first().locator("[data-slot=fact-label]").allTextContents();
      assert.deepEqual(facts.map((f) => f.trim().toLowerCase()), ["device", "location", "engagement speed", "source"]);
      assert.equal(await sheet.locator("[data-slot=fact-grid]").first().locator("svg[data-slot=flag]").count(), 1, "a flag beside the city");
      // The timeline: grouped by day, with the gap between steps.
      const timeline = sheet.locator("#recipient-timeline");
      assert.ok((await timeline.locator("time").count()) >= 2);
      assert.ok((await timeline.getByText(/^\+\d/).count()) >= 1, "a gap chip");
      // Tabs switch in place.
      await sheet.getByRole("tab", { name: "Technical" }).click();
      assert.equal(await sheet.getByRole("tabpanel", { name: "Technical" }).isVisible(), true);
      assert.match(await sheet.getByRole("tabpanel", { name: "Technical" }).textContent(), /spring-launch@larkspur\.example/);
      await shot(page, "sheet", false);
      await page.keyboard.press("Escape");
      await page.waitForFunction(() => !document.querySelector("dialog#recipient")?.open);
      await page.done();
    });

    test("the timeline sets a proxy's opens aside, and one tick shows them", async () => {
      const { recipient } = await step("proxy_open");
      const page = await open(`/recipients/${recipient}`);
      const hidden = page.locator("#recipient-timeline [data-filtered]");
      assert.ok((await hidden.count()) >= 1);
      assert.equal(await hidden.first().isVisible(), false);
      assert.match(await page.locator("#recipient-timeline").textContent(), /opens? came through a mail privacy proxy/);
      await page.locator("#recipient-timeline [data-slot=hidden-notice-toggle]").click();
      assert.equal(await hidden.first().isVisible(), true);
      await page.done();
    });

    test("keyboard only: reach a name, open the sheet, move through the tabs, close it and come back", async () => {
      const page = await open("/recipients");
      const target = await rows(page).first().getAttribute("data-key");
      let reached = false;
      for (let i = 0; i < 80 && !reached; i++) {
        await page.keyboard.press("Tab");
        reached = await page.evaluate((key) => document.activeElement?.closest?.("tr")?.getAttribute("data-key") === key && document.activeElement.matches("button[aria-haspopup=dialog]"), target);
      }
      assert.ok(reached, "Tab reaches the first name");
      const ring = await page.evaluate(() => getComputedStyle(document.activeElement).outlineStyle);
      assert.notEqual(ring, "none", "a visible focus ring");
      await page.keyboard.press("Enter");
      await page.locator("dialog#recipient[open]").waitFor({ timeout: 8000 });
      assert.ok(await page.evaluate(() => document.querySelector("dialog#recipient").contains(document.activeElement)), "focus moves into the sheet");
      // Focus lands on the first thing in the sheet that takes it; Tab on from there.
      let onTab = await page.evaluate(() => document.activeElement?.getAttribute("role") === "tab");
      for (let i = 0; i < 30 && !onTab; i++) {
        await page.keyboard.press("Tab");
        onTab = await page.evaluate(() => document.activeElement?.getAttribute("role") === "tab");
      }
      assert.ok(onTab, "Tab reaches the tabs");
      await page.keyboard.press("ArrowRight");
      assert.equal(await page.evaluate(() => document.activeElement.textContent.trim()), "Device and network");
      assert.equal(await page.locator("dialog#recipient [role=tab][aria-selected=true]").textContent(), "Device and network");
      await page.keyboard.press("Escape");
      await page.waitForFunction(() => !document.querySelector("dialog#recipient")?.open);
      assert.equal(await page.evaluate((key) => document.activeElement?.closest?.("tr")?.getAttribute("data-key"), target), target, "focus comes back to the name");
      await page.done();
    });

    test("with JavaScript off a name leads to the recipient's own page", async () => {
      const page = await open("/recipients", { javaScriptEnabled: false });
      const first = rows(page).first().locator("button[aria-haspopup=dialog]");
      const who = (await first.textContent()).trim();
      await Promise.all([page.waitForURL(/\/recipients\/\d+$/), first.click()]);
      assert.equal((await page.locator("h1").textContent()).trim(), who);
      assert.ok(await page.locator("[data-slot=tabs]").isVisible());
      await page.done();
    });

    // ---- The contact form -----------------------------------------------------------------

    test("the contact form: a phone number formatted as typed, a country found with its flag, a date, and a toast", async () => {
      const page = await open("/recipients/7/edit");
      const phone = page.locator("[data-ui-phone]:has(input[name=mobile])");
      const flags = await page.evaluate(() => document.querySelector("template[data-slot=phone-flags]")?.content.querySelectorAll("img[src^='/flags/']").length ?? 0);
      assert.ok(flags > 200, `the phone input draws the app's flags (${flags})`);
      // Choose the United Kingdom by keyboard, then type a number.
      await phone.locator("[data-slot=phone-country-trigger]").click();
      await page.locator("#mobile-countries [data-slot=phone-search]").fill("United Kingdom");
      await page.keyboard.press("Enter");
      assert.equal(await page.locator("select[name=mobile_country]").inputValue(), "GB");
      const number = page.locator("input[type=tel][name=mobile]");
      await number.fill("");
      await number.pressSequentially("07400123456");
      assert.equal(await number.inputValue(), "07400 123456", "formatted as typed");
      // The country: found by typing, each option with its flag.
      const country = page.locator("#contact-country-picker [data-slot=combobox-input]");
      await country.fill("");
      await country.pressSequentially("portu");
      const option = page.locator("#contact-country-picker-list [role=option]:not([hidden])").first();
      await option.waitFor();
      assert.equal((await option.textContent()).trim(), "Portugal");
      assert.equal(await option.locator("img").getAttribute("src"), "/flags/pt.svg");
      assert.equal((await page.request.get(BASE + "/flags/pt.svg")).headers()["content-type"], "image/svg+xml");
      await page.keyboard.press("Enter");
      assert.equal(await page.locator("select[name=country]").inputValue(), "PT");
      // A date, typed.
      const day = page.locator("#dp-customer_since input[data-ui-text]");
      await day.fill("3/14/2025");
      await day.press("Tab");
      assert.equal(await page.locator("input[type=date][name=customer_since]").inputValue(), "2025-03-14");
      await shot(page, "form-filled", false);
      await page.getByRole("button", { name: "Save contact" }).click();
      const toast = page.locator("[data-slot=toast]", { hasText: "Contact saved" });
      await toast.waitFor({ timeout: 8000 });
      await page.waitForURL(/\/recipients\/7$/);
      await shot(page, "form-saved", false);
      const header = await page.locator("main").textContent();
      assert.match(header, /\+44 7400 123456/, "the number, stored and shown in its country's format");
      assert.match(header, /Portugal/);
      await page.done();
    });

    test("the contact form shows what to fix under the field, without a toast", async () => {
      const page = await open("/recipients/7/edit");
      await page.locator("input[name=name]").fill("");
      await page.locator("input[name=name]").evaluate((input) => input.removeAttribute("required"));
      await page.getByRole("button", { name: "Save contact" }).click();
      await page.getByText("Give the contact a name.").waitFor({ timeout: 8000 });
      assert.equal(await page.locator("input[name=name]").getAttribute("aria-invalid"), "true");
      assert.equal(await page.locator("[data-slot=toast]", { hasText: "Contact saved" }).count(), 0);
      assert.match(page.url(), /\/recipients\/7\/edit$/);
      await page.done();
    });

    test("keyboard only through the form, every stop with a focus ring", async () => {
      const page = await open("/recipients/7/edit");
      await page.locator("input[name=name]").focus();
      const stops = [];
      for (let i = 0; i < 12; i++) {
        const here = await page.evaluate(() => {
          const el = document.activeElement;
          // A field inside a group (the phone input, the combobox, the date box) rings the group.
          const ring = (node) => node && getComputedStyle(node).outlineStyle !== "none";
          return {
            name: el.getAttribute("name") || el.getAttribute("aria-label") || el.closest("[data-slot=combobox], [data-slot=date-picker]")?.getAttribute("data-slot") || el.textContent.trim().slice(0, 20),
            ring: ring(el) || ring(el.parentElement),
          };
        });
        stops.push(here);
        if (/Save contact/.test(here.name)) break;
        await page.keyboard.press("Tab");
      }
      const names = stops.map((s) => s.name);
      // WebKit's Tab skips links, as Safari's does unless asked: Cancel is a link.
      const expected = ["name", "email", "role", "phone country", "mobile", "combobox", "date-picker", "Choose date", "Cancel", "Save contact"].filter((stop) => name !== "webkit" || stop !== "Cancel");
      assert.deepEqual(
        names.map((n) => n.replace(/^Country: .*/, "phone country")),
        expected,
      );
      assert.deepEqual(stops.filter((s) => !s.ring).map((s) => s.name), [], "a ring on every stop");
      await page.done();
    });
  });
}
