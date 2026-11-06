// The assistant of the example app (apps/agent-demo) in real browsers, with the model
// scripted: a question answered with a chart drawn from the reader's own data, the
// answer reaching a second tab live, a refund waiting in the page's own place until
// Approve streams the rest, Reject with a reason, staff who are offered no refund and
// see an order without buttons, a card's Refund asking in a dialog the page draws, Enter
// to send, JavaScript off from the first message to the answer, and axe at 390 and
// 1440 px in light and dark. Needs the app serving with its model scripted:
//
//   AGENT_TEST_HOOKS=1 cargo run --release -p agent-demo
//
// AGENT_URL points elsewhere (default http://127.0.0.1:8098); UI_BROWSERS picks engines
// (default "chromium"; "chromium,firefox,webkit" for all three). Without the app the
// suite is skipped.
//
// The shop is seeded once, at the app's first start. Every test that changes an order
// finds one that is still paid, so tests never depend on each other's writes.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { after, before, describe, test } from "node:test";
import { chromium, firefox, webkit } from "playwright";
import { launch } from "./support/launch.mjs";

const BASE = process.env.AGENT_URL || "http://127.0.0.1:8098";
const require = createRequire(import.meta.url);
const axeSource = readFileSync(require.resolve("axe-core/axe.min.js"), "utf8");
const ENGINES = { chromium, firefox, webkit };
const NAMES = (process.env.UI_BROWSERS || "chromium").split(",").filter((n) => ENGINES[n]);
const MANAGER = ["simran.kaur@kesar.example", "demo-password-2026"];
const STAFF = ["arjun.mehta@kesar.example", "demo-password-2026"];

/** Queues what the scripted model answers next. */
export async function script(replies) {
  const res = await fetch(BASE + "/__test/script", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(replies),
  });
  assert.ok(res.status < 300, `the model was scripted: ${res.status}`);
}

let reachable = false;
try {
  reachable = (await fetch(BASE + "/sign-in")).status === 200;
  if (reachable) await script([]);
} catch {
  reachable = false;
}

async function axe(page) {
  await page.evaluate(axeSource);
  return page.evaluate(async () => {
    const out = await window.axe.run(document, { runOnly: ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa", "best-practice"] });
    return out.violations.map((v) => `${v.id}: ${v.nodes.slice(0, 3).map((n) => n.target.join(" ")).join(" | ")}`);
  });
}

for (const name of NAMES) {
  describe(`the assistant in ${name}`, { skip: !reachable && "the agent demo is not serving" }, () => {
    let browser;
    before(async () => {
      browser = await launch(ENGINES[name]);
    });
    after(async () => {
      await browser?.close();
    });

    /** A page of `who` signed in through the form, on the assistant. */
    async function signedIn([email, password], options = {}) {
      const context = await browser.newContext({ viewport: { width: 1440, height: 900 }, ...options });
      const page = await context.newPage();
      page.problems = [];
      page.on("pageerror", (error) => page.problems.push(String(error)));
      await page.goto(BASE + "/sign-in");
      await page.fill("input[name=email]", email);
      await page.fill("input[name=password]", password);
      await page.getByRole("button", { name: "Sign in" }).click();
      await page.waitForURL((url) => url.pathname === "/assistant");
      return page;
    }

    /** Writes `text` in the composer and sends it. */
    async function send(page, text) {
      await page.fill("form[data-genui=composer] textarea[name=message]", text);
      await page.locator("form[data-genui=composer]").getByRole("button", { name: "Send" }).click();
    }

    /** Waits for a finished answer saying `text`. */
    const answered = (page, text) =>
      page
        .locator('[data-genui=message][data-role=assistant][aria-busy="false"]', { hasText: text })
        .first()
        .waitFor({ timeout: 20000 });

    /** The number and id of an order that is still paid, from the orders page. */
    async function paidOrder(page) {
      await page.goto(BASE + "/orders");
      const row = page.locator("tbody tr", { hasText: "Paid" }).first();
      const link = row.locator("a").first();
      const number = (await link.innerText()).trim();
      const id = (await link.getAttribute("href")).split("/").pop();
      await page.goto(BASE + "/assistant");
      return { number, id };
    }

    test("a question is answered with a chart drawn from the reader's own data", async () => {
      const page = await signedIn(MANAGER);
      await script([
        { tool: "show_payments_by_method", arguments: { period: "this_week" } },
        { text: "UPI took the most this week." },
      ]);
      await send(page, "How did UPI do this week?");
      await page.waitForURL((url) => /^\/assistant\/[0-9a-f-]{36}$/.test(url.pathname));
      const surface = page.locator("[data-genui=surface]", { hasText: "Payments this week" });
      await surface.waitFor({ timeout: 20000 });
      await answered(page, "UPI took the most this week.");
      assert.equal(await surface.locator("[data-slot=chart], figure").count() > 0, true, "a chart");
      assert.equal(await surface.locator("form, button[type=submit], input:not([type=checkbox])").count(), 0, "nothing in it posts");
      assert.match(await surface.innerText(), /Shown by the assistant/);
      assert.deepEqual(await axe(page), []);
      assert.deepEqual(page.problems, []);
      await page.context().close();
    });

    test("the answer reaches a second tab live, and the composer keeps what is typed", async () => {
      const page = await signedIn(MANAGER);
      await script([{ text: "Hello Simran." }]);
      await send(page, "Hello");
      await answered(page, "Hello Simran.");
      const other = await page.context().newPage();
      await other.goto(page.url());
      // What the reader types in the second tab stays while the conversation changes.
      await other.fill("form[data-genui=composer] textarea[name=message]", "half a thought");
      await script([{ text: "Still here, as ever." }]);
      await send(page, "Are you there?");
      await answered(other, "Still here, as ever.");
      assert.equal(await other.inputValue("form[data-genui=composer] textarea[name=message]"), "half a thought");
      await page.context().close();
    });

    test("a refund waits in the page's own place, and Approve streams the rest", async () => {
      const page = await signedIn(MANAGER);
      const { number } = await paidOrder(page);
      await script([
        { tool: "refund_order", arguments: { order: number } },
        { text: `Refunded order ${number}.` },
      ]);
      await send(page, `Refund order ${number}`);
      const card = page.locator("#assistant-approval [data-genui=approval]");
      await card.waitFor({ timeout: 20000 });
      assert.match(await card.innerText(), /The assistant asks to: Refund an order/);
      assert.match(await card.innerText(), new RegExp(number));
      assert.equal(await page.locator("[data-genui=conversation] [data-genui=approval]").count(), 0, "never inside the conversation");
      await page.locator("[data-genui=conversation]", { hasText: "Waiting for your answer" }).waitFor();
      assert.deepEqual(await axe(page), [], "with the card");
      await card.getByRole("button", { name: "Approve" }).click();
      await answered(page, `Refunded order ${number}.`);
      await page.locator("#assistant-approval [data-genui=approval]").waitFor({ state: "detached" });
      await page.goto(BASE + "/orders");
      assert.match(await page.locator("tbody tr", { hasText: number }).innerText(), /Refunded/);
      await page.context().close();
    });

    test("Reject with a reason tells the model, and nothing runs", async () => {
      const page = await signedIn(MANAGER);
      const { number } = await paidOrder(page);
      await script([
        { tool: "refund_order", arguments: { order: number } },
        { text: "I left the order as it was." },
      ]);
      await send(page, `Refund order ${number}`);
      const card = page.locator("#assistant-approval [data-genui=approval]");
      await card.waitFor({ timeout: 20000 });
      await card.locator("textarea[name=reason]").fill("The customer changed their mind");
      await card.getByRole("button", { name: "Reject" }).click();
      await answered(page, "I left the order as it was.");
      await page.locator("[data-genui=conversation]", { hasText: "The customer changed their mind" }).waitFor();
      await page.goto(BASE + "/orders");
      assert.match(await page.locator("tbody tr", { hasText: number }).innerText(), /Paid/);
      await page.context().close();
    });

    test("staff see an order without its buttons", async () => {
      const page = await signedIn(STAFF);
      await script([
        { tool: "show_order_card", arguments: { order: "KK-1001" } },
        { text: "Here is the order." },
      ]);
      await send(page, "Show order KK-1001");
      const surface = page.locator("[data-genui=surface]", { hasText: "Order KK-1001" });
      await surface.waitFor({ timeout: 20000 });
      assert.match(await surface.innerText(), /Only a manager can change an order/);
      assert.equal(await surface.locator("[data-genui=surface-actions]").count(), 0);
      await page.context().close();
    });

    test("a card's Refund asks first, in a dialog the page draws", async () => {
      const page = await signedIn(MANAGER);
      const { number } = await paidOrder(page);
      await script([
        { tool: "show_order_card", arguments: { order: number } },
        { text: "Here it is." },
      ]);
      await send(page, `Show order ${number}`);
      const surface = page.locator("[data-genui=surface]", { hasText: `Order ${number}` });
      await surface.waitFor({ timeout: 20000 });
      await surface.getByRole("button", { name: /^Refund ₹/ }).click();
      const dialog = page.getByRole("alertdialog");
      await dialog.waitFor();
      assert.match(await dialog.innerText(), new RegExp(`Refund order ${number}\\?`));
      await dialog.getByRole("button", { name: "Cancel" }).click();
      await dialog.waitFor({ state: "hidden" });
      await surface.getByRole("button", { name: /^Refund ₹/ }).click();
      await dialog.getByRole("button", { name: /^Refund ₹/ }).click();
      await page.locator("[data-slot=toast]", { hasText: `for order ${number}` }).waitFor({ timeout: 20000 });
      await page.locator("[data-genui=conversation]", { hasText: `for order ${number}` }).waitFor();
      await page.context().close();
    });

    test("Enter sends and Shift+Enter starts a new line", async () => {
      const page = await signedIn(MANAGER);
      await script([{ text: "Two lines read." }]);
      const box = page.locator("form[data-genui=composer] textarea[name=message]");
      await box.focus();
      await page.keyboard.type("First line");
      await page.keyboard.press("Shift+Enter");
      await page.keyboard.type("second line");
      assert.equal(await box.inputValue(), "First line\nsecond line");
      await page.keyboard.press("Enter");
      await page.waitForURL((url) => url.pathname.startsWith("/assistant/"));
      await answered(page, "Two lines read.");
      assert.match(await page.locator('[data-genui=message][data-role=user]').last().innerText(), /First line\s+second line/);
      await page.context().close();
    });

    test("with JavaScript off, a message is a form and the answer is there when drawn again", async () => {
      const page = await signedIn(MANAGER, { javaScriptEnabled: false });
      await script([{ text: "Answered with no script." }]);
      await page.fill("form[data-genui=composer] textarea[name=message]", "Anyone there?");
      await page.locator("form[data-genui=composer]").getByRole("button", { name: "Send" }).click();
      await page.waitForURL((url) => url.pathname.startsWith("/assistant/"));
      for (let i = 0; i < 50; i++) {
        if ((await page.content()).includes("Answered with no script.")) break;
        await new Promise((r) => setTimeout(r, 200));
        await page.reload();
      }
      assert.match(await page.content(), /Answered with no script\./);
      await page.context().close();
    });

    for (const width of [390, 1440]) {
      for (const scheme of ["light", "dark"]) {
        test(`a conversation is axe clean at ${width} px in ${scheme}`, async () => {
          const page = await signedIn(MANAGER, { viewport: { width, height: 900 }, colorScheme: scheme });
          await script([
            { tool: "show_payments_by_method", arguments: { period: "this_month" } },
            { text: "This month so far." },
          ]);
          await send(page, "How did this month go?");
          await answered(page, "This month so far.");
          assert.deepEqual(await axe(page), [], `${width} ${scheme}`);
          const sideways = await page.evaluate(() => document.documentElement.scrollWidth > window.innerWidth);
          assert.equal(sideways, false, "no sideways scroll");
          await page.context().close();
        });
      }
    }
  });
}
