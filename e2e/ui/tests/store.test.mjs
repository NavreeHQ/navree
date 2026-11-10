// The store app in real browsers: a dashboard with its sales charts and its feed live
// over server-sent events, 200,000 orders in a table paged by cursor, an order in a drawer over the
// table with refunds, cancels and resends checked on the server, and inventory
// with a restock in each row. Needs the app serving with its simulator off and its
// test hooks on:
//
//   STORE_SIMULATOR=off STORE_TEST_HOOKS=1 cargo run --release -p store-demo
//
// STORE_URL points elsewhere (default http://127.0.0.1:8095); STORE_BROWSERS picks
// engines (default "chromium,firefox"; add webkit); STORE_SHOTS=<dir> saves a
// screenshot of every page in every state. Without the app the suite is skipped.
//
// The shop is seeded once, at the app's start: every test makes the orders it
// changes through the test hook, so tests never depend on each other's writes.

import assert from "node:assert/strict";
import { mkdirSync, readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import { after, afterEach, before, describe, test } from "node:test";
import { fileURLToPath } from "node:url";
import { chromium, firefox, webkit } from "playwright";
import { launch } from "./support/launch.mjs";

const BASE = process.env.STORE_URL || "http://127.0.0.1:8095";
const require = createRequire(import.meta.url);
const axeSource = readFileSync(require.resolve("axe-core/axe.min.js"), "utf8");
const ENGINES = { chromium, firefox, webkit };
const NAMES = (process.env.STORE_BROWSERS || "chromium,firefox").split(",").filter((n) => ENGINES[n]);
const SHOTS = process.env.STORE_SHOTS;
if (SHOTS) mkdirSync(SHOTS, { recursive: true });

/** The light values of the tokens, from the toolkit's one source: the toolkit ships dark only so far. */
const here = dirname(fileURLToPath(import.meta.url));
const tokensRs = readFileSync(join(here, "../../../crates/navree-ui/src/tokens.rs"), "utf8");
const LIGHT =
  ":root{color-scheme:light" +
  [...tokensRs.matchAll(/t\("([a-z0-9-]+)",\s*"(#[0-9a-f]{6})",\s*"(#[0-9a-f]{6})"\)/g)].map((m) => `;--${m[1]}:${m[3]}`).join("") +
  "}body{background:var(--ink-0)}";

/** Round on purpose: status dots, the feed's and timeline's icon containers and radios. Not pills, not letter tiles. */
const ROUND = ["timeline-dot", "timeline-icon", "live-dot", "badge-dot", "activity-icon", "radio", "spinner"];

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const post = (path, body) =>
  fetch(BASE + path, body ? { method: "POST", headers: { "content-type": "application/x-www-form-urlencoded" }, body } : { method: "POST" });
/** One thing happens in the shop: `kind`, to `order` when given. */
const step = async (kind, order) => {
  const text = await (await post(`/__test/step?kind=${kind}${order ? `&order=${order}` : ""}`)).text();
  const [event, orderId, product] = text.split(" ").map(Number);
  assert.ok(event > 0, `a ${kind} happened: "${text}"`);
  return { event, order: orderId, product };
};
/** A new order paid by card, so it can be refunded and cancelled; and its number. */
const freshOrder = async () => {
  for (let i = 0; i < 20; i++) {
    const { order } = await step("placed");
    const html = await (await fetch(`${BASE}/orders/${order}`)).text();
    if (html.includes(">Paid<")) {
      return { id: order, number: `CS-${20480 + order}` };
    }
  }
  assert.fail("no order paid by card in twenty tries");
};

let reachable = false;
try {
  reachable = (await fetch(BASE + "/orders")).status === 200;
} catch {}

async function axe(page) {
  // Once running animations and transitions have ended, so it reads final colours (an animation that never ends, a
  // spinner, is not waited for).
  await page.evaluate(() => Promise.all(document.getAnimations().filter((a) => Number.isFinite(a.effect?.getComputedTiming().endTime ?? 0)).map((a) => a.finished.catch(() => {}))));
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
      if (el.closest("template") || el.getClientRects().length === 0) continue;
      const style = getComputedStyle(el);
      const slot = el.getAttribute("data-slot") || el.tagName;
      const radius = parseFloat(style.borderTopLeftRadius);
      if (radius && ![2, 4, 6].includes(radius) && !round.includes(slot) && !el.closest("[data-slot=spinner]")) bad.push(`radius ${style.borderTopLeftRadius} on ${slot}`);
      const shadows = style.boxShadow === "none" ? [] : style.boxShadow.split(/,(?![^(]*\))/).filter((s) => !/inset/.test(s));
      if (shadows.length && !el.matches("dialog, [popover], [data-slot=toast], [data-slot=toast] *")) bad.push(`shadow on ${slot}: ${style.boxShadow}`);
      const own = [...el.style].filter((property) => !property.startsWith("--"));
      if (el.hasAttribute("style") && !el.hasAttribute("popover") && own.length) bad.push(`style attribute on ${slot}: ${own.join(", ")}`);
      if (slot === "avatar" && el.textContent.trim()) bad.push(`text in an avatar: ${el.textContent}`);
    }
    return [...new Set(bad)];
  }, ROUND);

const sideways = (page) => page.evaluate(() => document.documentElement.scrollWidth > window.innerWidth);

/** Every line break inside a word of a fact card, where the word would have fitted on its own line. */
const midWord = (page, scope) =>
  page.evaluate((scope) => {
    const out = [];
    for (const card of document.querySelectorAll(`${scope} [data-slot=fact-card]`)) {
      const walker = document.createTreeWalker(card, NodeFilter.SHOW_TEXT);
      for (let node = walker.nextNode(); node; node = walker.nextNode()) {
        let top = null;
        for (let i = 0; i < node.data.length; i++) {
          const range = document.createRange();
          range.setStart(node, i);
          range.setEnd(node, i + 1);
          const rect = range.getClientRects()[0];
          if (!rect) continue;
          if (top !== null && rect.top > top + 2 && /\S/.test(node.data[i - 1]) && /\S/.test(node.data[i])) {
            const word = node.data.slice(0, i).split(/\s/).pop() + node.data.slice(i).split(/\s/)[0];
            const measure = document.createElement("span");
            measure.textContent = word;
            measure.style.whiteSpace = "nowrap";
            measure.style.position = "absolute";
            node.parentElement.append(measure);
            const needs = measure.getBoundingClientRect().width;
            measure.remove();
            if (needs <= node.parentElement.closest("dd, dt").getBoundingClientRect().width) out.push(`${node.data.slice(0, i)}|${node.data.slice(i)}`);
          }
          top = rect.top;
        }
      }
    }
    return out;
  }, scope);

for (const name of NAMES) {
  describe(`store app in ${name}`, { concurrency: false, skip: reachable ? false : `the app isn't serving at ${BASE}` }, () => {
    let browser;
    before(async () => {
      // Firefox slows a background tab's network while another load (an open event stream) runs.
      browser = await launch(ENGINES[name], name === "firefox" ? { firefoxUserPrefs: { "network.http.throttle.enable": false } } : {});
    });
    after(() => browser?.close());
    const contexts = [];
    afterEach(async () => {
      for (const context of contexts.splice(0)) await context.close().catch(() => {});
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
        if (m.type() === "error" && !/Failed to load resource|ERR_ABORTED|NS_ERROR|Load failed|can.t establish a connection|409|422/i.test(m.text())) page.problems.push(m.text());
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
    /** Waits for `check` to hold in the page, asking from here. */
    const until = async (page, check, arg, what, ms = 12000) => {
      const end = Date.now() + ms;
      for (;;) {
        if (await page.evaluate(check, arg)) return;
        if (Date.now() > end) assert.fail(`timed out waiting for ${what}`);
        await sleep(100);
      }
    };
    const live = (page, id) => until(page, (region) => document.getElementById(region)?.getAttribute("data-live") === "on", id, `${id} to be live`);
    const rows = (page) => page.locator("#orders tbody tr[data-key]");
    const settled = (page) =>
      until(page, () => !document.getElementById("orders")?.hasAttribute("data-pending") && !document.documentElement.hasAttribute("data-ui-loading"), null, "the table to settle");
    const drawer = (page) => page.locator("dialog#order[open]");
    const toast = (page, text) => page.locator("[data-slot=toast]", { hasText: text });
    /** Opens order `number` in the drawer from the orders table, found by search. */
    async function openDrawer(number, options = {}) {
      const page = await open(`/orders?search=${number}`, options);
      await rows(page).first().waitFor();
      await rows(page).first().locator("button[aria-haspopup=dialog]").click();
      await drawer(page).waitFor({ timeout: 8000 });
      await settledIn(page, "dialog#order");
      return page;
    }
    /** Waits for every animation in `selector` to finish: a drawer fades in, and its colours are read once it has. */
    const settledIn = (page, selector) =>
      page.locator(selector).evaluate((el) => Promise.all(el.getAnimations({ subtree: true }).map((a) => a.finished.catch(() => {}))));

    // ---- Every page, every width, both themes -----------------------------------

    for (const theme of ["dark", "light"]) {
      for (const width of [1440, 390]) {
        test(`every page is axe clean, keeps the rules and doesn't scroll sideways (${theme}, ${width} px)`, async () => {
          for (const path of ["/", "/orders", "/orders/199990", "/inventory"]) {
            const page = await open(path, { light: theme === "light", viewport: { width, height: 900 } });
            assert.deepEqual(await axe(page), [], path);
            assert.deepEqual(await rules(page), [], path);
            assert.equal(await sideways(page), false, `${path} scrolls sideways`);
            assert.deepEqual(await midWord(page, "main"), [], `${path}: a word broken where it fits`);
            await shot(page, `${path.replaceAll("/", "_") || "_"}-${theme}-${width}`);
            await page.done();
          }
        });

        test(`the drawer over its page is axe clean and keeps the rules (${theme}, ${width} px)`, async () => {
          const page = await openDrawer("CS-220470", { light: theme === "light", viewport: { width, height: 900 } });
          await until(page, () => !document.querySelector("#order [aria-busy=true]"), null, "the drawer's parts to load");
          await settledIn(page, "dialog#order");
          assert.deepEqual(await axe(page), []);
          assert.deepEqual(await rules(page), []);
          assert.deepEqual(await midWord(page, "dialog#order"), [], "no word broken where it fits");
          await shot(page, `drawer-${theme}-${width}`, false);
          await page.done();
        });
      }
    }

    // ---- The sales charts on the dashboard ------------------------------------------

    /** A chart's table, row by row: the row's header, then each value as the eye reads it. */
    const chartTable = (page, fig) =>
      page.evaluate((fig) => {
        const table = document.querySelector(`#${fig} [data-slot=chart-table] table`);
        const text = (cell) => (cell.querySelector("[aria-hidden=true]") || cell).textContent.trim();
        return [...table.querySelectorAll("tbody tr")].map((tr) => [...tr.querySelectorAll("th,td")].map(text));
      }, fig);
    /** `1,24,050` as a number. */
    const figure = (text) => Number(text.replace(/[₹,]/g, ""));
    /** Rupees grouped the Indian way: the last three digits, then pairs. */
    const RUPEES = /^₹(?:\d{1,2},(?:\d{2},)*\d{3}|\d{1,3})$/;
    /** Today in India: `4 Nov` for 4 November. */
    const todayInIndia = () => {
      const at = new Date(Date.now() + 5.5 * 3600 * 1000);
      return `${at.getUTCDate()} ${["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"][at.getUTCMonth()]}`;
    };

    test("the dashboard's charts: 30 days on India's clock, rupees in lakh, and how customers pay", async () => {
      const page = await open("/");
      for (const fig of ["sales-revenue", "sales-orders", "sales-methods"]) {
        assert.equal(await page.locator(`#${fig} svg[data-slot=chart-plot], #${fig} svg`).first().isVisible(), true, `${fig} is drawn`);
      }
      // Revenue: a row a day, the last today, each in rupees with Indian groups.
      const revenue = await chartTable(page, "sales-revenue");
      assert.equal(revenue.length, 30, "thirty days");
      assert.ok(revenue.at(-1)[0].startsWith(todayInIndia()), `the last day is today in India: ${revenue.at(-1)[0]}`);
      for (const [day, value] of revenue) assert.match(value, RUPEES, `${day}: ${value}`);
      assert.ok(revenue.some(([, value]) => figure(value) >= 100000), "a day of a lakh or more, written with lakh groups");
      // The axis is written short, in lakh (or crore).
      const ticks = await page.locator("#sales-revenue svg text").allTextContents();
      assert.ok(ticks.some((t) => /^₹\d+(\.\d+)?(L|Cr)$/.test(t.trim())), `the axis in lakh or crore: ${ticks.join(" ")}`);
      // Orders: today's bar is today's figure.
      const orders = await chartTable(page, "sales-orders");
      assert.equal(orders.length, 30);
      assert.deepEqual(orders.map((row) => row[0]), revenue.map((row) => row[0]), "the same days");
      const stat = await page.locator("[data-slot=stat]", { hasText: "Orders today" }).locator("[data-slot=stat-value]").textContent();
      assert.equal(figure(orders.at(-1)[1]), figure(stat), "today's bar is the Orders today figure");
      const earned = await page.locator("[data-slot=stat]", { hasText: "Revenue today" }).locator("[data-slot=stat-value]").textContent();
      assert.equal(figure(revenue.at(-1)[1]), Math.round(figure(earned)), "today's revenue is the Revenue today figure, to the rupee");
      // How customers pay: the three ways, adding up to every order of the 30 days.
      const methods = await chartTable(page, "sales-methods");
      assert.deepEqual(methods.map((row) => row[0]), ["UPI", "Card", "Cash on delivery"]);
      const paid = methods.reduce((sum, row) => sum + figure(row[1]), 0);
      assert.equal(paid, orders.reduce((sum, row) => sum + figure(row[1]), 0), "each order paid one way");
      assert.ok(paid >= 10000, `the 30 days hold tens of thousands of orders: ${paid}`);
      for (const [way, count] of methods) assert.match(count, /^(?:\d{1,2},(?:\d{2},)*\d{3}|\d{1,3})$/, `${way}: ${count} in Indian groups`);
      const centre = await page.locator("#sales-methods [data-slot=chart-donut-center] strong").textContent();
      assert.equal(figure(centre.trim()), paid, `the ring's centre says every order: ${centre}`);
      await shot(page, "dashboard-charts");
      await page.done();
    });

    test("a new order grows today's bar over the stream", async () => {
      const page = await open("/");
      await live(page, "sales");
      const today = () => page.evaluate(() => {
        const rows = document.querySelectorAll("#sales-orders [data-slot=chart-table] tbody tr");
        return Number(rows[rows.length - 1].querySelector("td").textContent.replace(/,/g, ""));
      });
      const before = await today();
      await step("placed");
      await until(
        page,
        (count) => {
          const rows = document.querySelectorAll("#sales-orders [data-slot=chart-table] tbody tr");
          return Number(rows[rows.length - 1].querySelector("td").textContent.replace(/,/g, "")) > count;
        },
        before,
        "today's bar to count the new order",
      );
      assert.ok((await today()) > before);
      await page.done();
    });

    test("the keys read the charts a day at a time, and the tooltip says what the table says", async () => {
      const page = await open("/");
      const body = page.locator("#sales-orders [data-slot=chart-body]");
      await until(page, () => document.querySelector("#sales-orders [data-slot=chart-body]")?.getAttribute("tabindex") === "0", null, "the chart to be a tab stop");
      await body.focus();
      await page.keyboard.press("End");
      const orders = await chartTable(page, "sales-orders");
      const tip = page.locator("#sales-orders [data-slot=chart-tip][data-open]");
      await tip.waitFor();
      assert.equal((await tip.locator("[data-tip=head]").textContent()).trim(), orders.at(-1)[0], "the last day");
      assert.equal((await tip.locator("[data-tip=row] strong").first().textContent()).trim(), orders.at(-1)[1], "today's orders");
      await page.keyboard.press("Home");
      await until(page, (head) => document.querySelector("#sales-orders [data-tip=head]")?.textContent.trim() === head, orders[0][0], "the first day");
      await page.keyboard.press("Escape");
      await until(page, () => !document.querySelector("#sales-orders [data-slot=chart-tip][data-open]"), null, "the tooltip to close");
      await page.done();
    });

    test("with JavaScript off the charts are drawn and their tables open from a switch", async () => {
      const page = await open("/", { javaScriptEnabled: false });
      for (const fig of ["sales-revenue", "sales-orders", "sales-methods"]) {
        assert.ok((await page.locator(`#${fig} svg`).count()) > 0, `${fig} is drawn`);
        const toggle = page.locator(`#${fig} [data-slot=chart-table-switch]`);
        assert.equal(await toggle.isChecked(), false);
        await page.locator(`#${fig} [data-slot=chart-table-label]`).click();
        assert.equal(await toggle.isChecked(), true);
        assert.equal(await page.locator(`#${fig} [data-slot=chart-table] table`).isVisible(), true, `${fig}: the table shows`);
      }
      await page.done();
    });

    // ---- The orders, on the server ---------------------------------------------------

    test("the orders: a cursor's next page, a status filter and a search, all on the server", async () => {
      const page = await open("/orders");
      assert.equal(await rows(page).count(), 25);
      const first = await rows(page).first().getAttribute("data-key");
      // The next page is a keyset cursor in the URL, not a page number.
      await page.locator("#orders a[href*='cursor=']").first().click();
      await until(page, () => new URL(location.href).searchParams.has("cursor"), null, "a cursor in the URL");
      await settled(page);
      assert.notEqual(await rows(page).first().getAttribute("data-key"), first);
      assert.equal(await rows(page).count(), 25);
      // A status filter, from the URL as a link would carry it.
      await page.goto(BASE + "/orders?filter%5Bstatus%5D%5Bin%5D=cancelled");
      await settled(page);
      const badges = await rows(page).locator("td[data-col=status] [data-slot=badge], [data-slot=badge]").allTextContents();
      assert.ok(badges.length > 0);
      const statuses = await rows(page).evaluateAll((trs) => trs.map((tr) => tr.querySelector("[data-slot=badge]")?.textContent.trim()));
      assert.ok(statuses.every((s) => s === "Cancelled"), statuses.join(", "));
      // A search finds an order by its number.
      await page.goto(BASE + "/orders");
      await page.locator("#orders input[name=search]").fill("CS-219999");
      await until(page, () => new URL(location.href).searchParams.get("search") === "CS-219999", null, "the search in the URL");
      await settled(page);
      const numbers = await rows(page).locator("button[aria-haspopup=dialog]").allTextContents();
      assert.deepEqual(numbers.map((n) => n.trim()), ["CS-219999"]);
      await page.done();
    });

    test("every live part of a page listens on the app's live hub, on one connection", async () => {
      for (const [path, region] of [["/", "activity-region"], ["/orders", "orders"], ["/inventory", "products"]]) {
        const page = await open(path);
        await live(page, region);
        const parts = await page.evaluate(() => [...document.querySelectorAll("[data-ui-live-src]")].map((el) => [el.id, el.getAttribute("data-ui-live-src"), el.getAttribute("data-ui-live-event")]));
        assert.ok(parts.length >= 1, path);
        for (const [id, src, event] of parts) {
          assert.equal(src, "/broadcasting/stream?topics=orders,activity,stats,inventory", `${path} #${id}`);
          assert.ok(["orders", "activity", "stats", "inventory"].includes(event), `${path} #${id} listens for ${event}`);
        }
        await page.done();
      }
    });

    test("a new order arrives at the top of the table over the stream", async () => {
      const page = await open("/orders");
      await live(page, "orders");
      const { order } = await step("placed");
      await until(page, (key) => document.querySelector("#orders tbody tr[data-key]")?.getAttribute("data-key") === String(key), order, "the new order at the top");
      await page.done();
    });

    // ---- An order in a drawer -------------------------------------------------------

    test("a row opens the order in a drawer 60% wide over its page, and its history loads after", async () => {
      const page = await openDrawer("CS-220470");
      const box = await drawer(page).boundingBox();
      // About 60% of the screen: of the viewport, less a scrollbar where the system draws one.
      assert.ok(box.width >= 1440 * 0.58 && box.width <= 1440 * 0.61, `about 60% of 1440 px: ${box.width}`);
      assert.ok(box.x > 500, "the page shows on the left");
      assert.equal(await drawer(page).getAttribute("data-width"), "xl");
      assert.ok(await page.locator("#orders").isVisible(), "the table is still drawn behind");
      // Facts, items and the actions come with the drawer; the history and the other orders after it.
      const labels = await drawer(page).locator("[data-slot=fact-label]").allTextContents();
      assert.deepEqual(labels.map((l) => l.trim().toLowerCase()), ["customer", "payment", "ships from", "ships to"]);
      assert.equal(await drawer(page).locator("[data-slot=fact-card] svg[data-slot=flag]").count(), 2, "a flag at each end of the route");
      assert.ok((await drawer(page).locator("table tbody tr").count()) >= 1, "the lines");
      await until(page, () => document.querySelectorAll("#order-steps [data-slot=timeline-event]").length >= 2, null, "the history to load");
      await until(page, () => !document.querySelector("#order-others[aria-busy=true]"), null, "the other orders to load");
      // Focus starts at the top of the drawer, so it opens where a reader reads.
      assert.equal(await page.evaluate(() => document.querySelector("dialog#order").scrollTop), 0);
      await shot(page, "drawer", false);
      await page.keyboard.press("Escape");
      await until(page, () => !document.querySelector("dialog#order")?.open, null, "the drawer to close");
      await page.done();
    });

    test("a partial refund, then the rest, then a refund the server refuses from a page drawn before", async () => {
      const { number } = await freshOrder();
      const refund = async (page, amount) => {
        await drawer(page).getByRole("button", { name: "Refund" }).click();
        const dialog = page.locator("dialog#refund[open]");
        await dialog.waitFor();
        if (amount) await dialog.locator("input[name=amount]").fill(amount);
        await dialog.getByRole("button", { name: "Refund" }).click();
      };
      const page = await openDrawer(number);
      await refund(page, "10.00");
      await toast(page, "back to the customer").waitFor({ timeout: 8000 });
      await until(page, () => /Partly refunded/.test(document.querySelector("dialog#order")?.textContent || ""), null, "the drawer to say partly refunded");
      assert.equal(await page.locator("dialog#refund[open]").count(), 0, "the dialog closed");
      // A second page opens the order now, while there is still money to give back.
      const stale = await openDrawer(number);
      assert.equal(await drawer(stale).getByRole("button", { name: "Refund" }).isDisabled(), false);
      // The first page refunds the rest: the amount offered is what is left.
      await refund(page);
      await toast(page, "refunded in full").waitFor({ timeout: 8000 });
      await until(page, () => document.querySelector("dialog#order button[commandfor=refund]")?.disabled === true, null, "Refund to be disabled");
      // The second page still offers a refund, and the server says no.
      await refund(stale);
      await toast(stale, "Not allowed").waitFor({ timeout: 8000 });
      assert.match(await toast(stale, "Not allowed").textContent(), /refunded in full already/);
      await until(stale, () => document.querySelector("dialog#order button[commandfor=refund]")?.disabled === true, null, "the second page to catch up");
      await stale.done();
      await page.done();
    });

    test("a refund of more than is left is refused, and nothing changes", async () => {
      const { id } = await freshOrder();
      const before = await (await fetch(`${BASE}/orders/${id}`)).text();
      const total = before.match(/Placed [^·]+· (₹[\d,.]+)/)[1];
      const res = await fetch(`${BASE}/orders/${id}/refund`, {
        method: "POST",
        headers: { "content-type": "application/x-www-form-urlencoded", "x-navree-ui": "1" },
        body: "amount=99999.00&reason=goodwill",
      });
      assert.equal(res.status, 409);
      assert.match(await res.text(), new RegExp(`At most ${total.replace(/[.₹]/g, (c) => `\\${c}`)} is left to refund`));
      const after = await (await fetch(`${BASE}/orders/${id}`)).text();
      assert.ok(after.includes(">Paid<") || /Paid/.test(after), "still paid");
      assert.ok(!/Partly refunded|>Refunded</.test(after), "no refund was written");
    });

    test("cancelling: allowed before shipping, refused once shipped even from a page drawn before", async () => {
      // Allowed: the money goes back and the order says cancelled.
      const a = await freshOrder();
      const page = await openDrawer(a.number);
      await drawer(page).getByRole("button", { name: "Cancel order" }).click();
      await page.locator("dialog#cancel[open]").getByRole("button", { name: "Cancel order" }).click();
      await toast(page, "is cancelled").waitFor({ timeout: 8000 });
      await until(page, () => /Cancelled/.test(document.querySelector("dialog#order [data-slot=badge]")?.textContent || ""), null, "the badge to say cancelled");
      await page.done();
      // Refused: the order ships while the drawer is open.
      const b = await freshOrder();
      const stale = await openDrawer(b.number);
      await step("packed", b.id);
      await step("shipped", b.id);
      await drawer(stale).getByRole("button", { name: "Cancel order" }).click();
      await stale.locator("dialog#cancel[open]").getByRole("button", { name: "Cancel order" }).click();
      await toast(stale, "Not allowed").waitFor({ timeout: 8000 });
      assert.match(await toast(stale, "Not allowed").textContent(), /has shipped, so it can't be cancelled/);
      await until(stale, () => /Shipped/.test(document.querySelector("dialog#order")?.textContent || ""), null, "the drawer to show it shipped");
      assert.equal(await drawer(stale).getByRole("button", { name: "Cancel order" }).isDisabled(), true);
      const page2 = await (await fetch(`${BASE}/orders/${b.id}`)).text();
      assert.ok(!/>Cancelled</.test(page2), "the order is not cancelled");
      await stale.done();
    });

    test("a confirmation goes out again at most once a minute", async () => {
      const { id } = await freshOrder();
      const res = await fetch(`${BASE}/orders/${id}/resend`, { method: "POST", headers: { "x-navree-ui": "1", "content-type": "application/x-www-form-urlencoded" }, body: "" });
      assert.equal(res.status, 409, "the confirmation went out when the order was placed");
      assert.match(await res.text(), /went out \d+ s ago/);
    });

    // ---- The dashboard, live --------------------------------------------------------

    test("the order activity follows the real stream, adding only what is new", async () => {
      const page = await open("/");
      await live(page, "activity-region");
      const before = await page.evaluate(() => [...document.querySelectorAll("#activity-list > li")].map((li) => li.id));
      assert.ok(before.length > 0 && before.length <= 30, `a first load of 30 at most: ${before.length}`);
      const { event, order } = await step("placed");
      await until(page, (id) => !!document.getElementById(id), `event-${event}`, "the new row");
      const ids = await page.evaluate(() => [...document.querySelectorAll("#activity-list > li")].map((li) => li.id));
      // A card order is placed and paid in the same second: the payment may sit above it.
      assert.ok(ids.slice(0, 2).includes(`event-${event}`), `at the top: ${ids.slice(0, 3)}`);
      assert.equal(ids.filter((id) => id === `event-${event}`).length, 1, "once");
      assert.deepEqual(ids.slice(ids.indexOf(`event-${event}`) + 1, ids.indexOf(`event-${event}`) + 1 + before.length).slice(0, 5), before.slice(0, 5), "the rows already there stay, in order");
      assert.match(await page.locator(`#event-${event} a`).first().getAttribute("href"), new RegExp(`/orders/${order}$`));
      // The figure for today's orders moves too.
      await until(page, () => /\d/.test(document.querySelector("#figures")?.textContent || ""), null, "the figures");
      await shot(page, "dashboard-live", false);
      await page.done();
    });

    // ---- Inventory ------------------------------------------------------------------

    test("a restock in a row adds the units, and the server refuses a quantity out of range", async () => {
      const page = await open("/inventory");
      await live(page, "products");
      const row = page.locator("#products tbody tr[data-key]").first();
      const key = await row.getAttribute("data-key");
      const before = await page.evaluate((k) => {
        const tr = document.querySelector(`#products tbody tr[data-key="${k}"]`);
        return Number([...tr.cells].find((td) => /Reorder at/.test(td.textContent))?.textContent.match(/^\s*(\d+)/)?.[1]);
      }, key);
      const name = (await row.locator("[data-slot=text]").first().textContent()).trim();
      await row.locator("input[name=quantity]").fill("5");
      await row.getByRole("button", { name: "Restock" }).click();
      await toast(page, "Restocked").waitFor({ timeout: 8000 });
      assert.match(await toast(page, "Restocked").textContent(), new RegExp(`5 units of .+ added\\. ${before + 5} on the shelf now\\.`));
      // The table follows the stream: the product, sorted by stock, shows its new count.
      await page.locator("#products input[name=search]").fill(name);
      await until(
        page,
        ([k, n]) => {
          const tr = document.querySelector(`#products tbody tr[data-key="${k}"]`);
          const cell = tr && [...tr.cells].find((td) => /Reorder at/.test(td.textContent));
          return Number(cell?.textContent.match(/^\s*(\d+)/)?.[1]) === n;
        },
        [key, before + 5],
        "the row to show 5 more",
      );
      for (const quantity of ["0", "501", "many"]) {
        const res = await fetch(`${BASE}/inventory/${key}/restock`, { method: "POST", headers: { "x-navree-ui": "1", "content-type": "application/x-www-form-urlencoded" }, body: `quantity=${quantity}` });
        assert.equal(res.status, 422, quantity);
        assert.match(await res.text(), /Restock between 1 and 500 units/);
      }
      await page.done();
    });

    // ---- Keyboard only --------------------------------------------------------------

    test("keyboard only: reach an order, open the drawer, open Refund, close both and come back", async () => {
      const { number } = await freshOrder();
      const page = await open(`/orders?search=${number}`);
      await rows(page).first().waitFor();
      let reached = false;
      for (let i = 0; i < 60 && !reached; i++) {
        await page.keyboard.press("Tab");
        reached = await page.evaluate(() => document.activeElement?.matches?.("#orders tbody button[aria-haspopup=dialog]"));
      }
      assert.ok(reached, "Tab reaches the order's number");
      assert.notEqual(await page.evaluate(() => getComputedStyle(document.activeElement).outlineStyle), "none", "a visible focus ring");
      await page.keyboard.press("Enter");
      await drawer(page).waitFor({ timeout: 8000 });
      assert.ok(await page.evaluate(() => document.querySelector("dialog#order").contains(document.activeElement)), "focus moves into the drawer");
      let onRefund = false;
      for (let i = 0; i < 10 && !onRefund; i++) {
        onRefund = await page.evaluate(() => document.activeElement?.getAttribute("commandfor") === "refund");
        if (!onRefund) await page.keyboard.press("Tab");
      }
      assert.ok(onRefund, "Tab reaches Refund");
      await page.keyboard.press("Enter");
      await page.locator("dialog#refund[open]").waitFor();
      assert.ok(await page.evaluate(() => document.querySelector("dialog#refund").contains(document.activeElement)), "focus moves into the dialog");
      await page.keyboard.press("Escape");
      await until(page, () => !document.querySelector("dialog#refund")?.open, null, "the dialog to close");
      assert.equal(await page.evaluate(() => document.activeElement?.getAttribute("commandfor")), "refund", "focus comes back to Refund");
      await page.keyboard.press("Escape");
      await until(page, () => !document.querySelector("dialog#order")?.open, null, "the drawer to close");
      assert.ok(await page.evaluate(() => document.activeElement?.matches?.("#orders tbody button[aria-haspopup=dialog]")), "focus comes back to the order");
      await page.done();
    });

    // ---- JavaScript off -------------------------------------------------------------

    test("with JavaScript off an order's number leads to its page, with its history and the other orders drawn in", async () => {
      const page = await open("/orders?search=CS-220470", { javaScriptEnabled: false });
      await Promise.all([page.waitForURL(/\/orders\/\d+$/), rows(page).first().locator("button[aria-haspopup=dialog]").click()]);
      assert.equal((await page.locator("h1").textContent()).trim(), "Order CS-220470");
      assert.ok((await page.locator("#order-page [data-slot=timeline-event]").count()) >= 2, "the history, drawn in");
      assert.equal(await page.locator("#order-page [aria-busy=true]").count(), 0, "nothing waits for a script");
      assert.ok(await page.getByText(/More from /).isVisible());
      await page.done();
    });

    test("with JavaScript off a refund posts as a plain form and the order's page says what happened", async () => {
      const { id } = await freshOrder();
      const page = await open(`/orders/${id}`, { javaScriptEnabled: false });
      const response = await page.request.post(`${BASE}/orders/${id}/refund`, { form: { amount: "3.00", reason: "damaged" }, maxRedirects: 0 });
      assert.equal(response.status(), 303);
      assert.match(response.headers().location, new RegExp(`/orders/${id}$`));
      await page.goto(`${BASE}/orders/${id}`);
      assert.match(await page.locator("body").textContent(), /₹3 back to the customer/);
      assert.match(await page.locator("#order-page").textContent(), /Partly refunded/);
      await page.done();
    });
  });
}
