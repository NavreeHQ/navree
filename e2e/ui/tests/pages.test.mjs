// Error pages, sign-in and the states inside a page, in real browsers: every status as a
// page, in the shell and out of it, the same pages with JavaScript off, boosted navigation
// to an error, regions that get each status, actions that fail, sign-in and the way back
// (and an open redirect that must not work), locked rows in a table, keyboard only, axe,
// 390 and 1440 px, dark and light. Run with the proof app serving:
//
//   cargo run --release -p ui-proof -- serve          # UI_PROOF_URL, default http://127.0.0.1:8090
//   UI_BROWSERS=chromium,firefox,webkit npm test -- tests/pages.test.mjs
//
// UI_SHOTS=<dir> also saves a screenshot of every state.

import assert from "node:assert/strict";
import { mkdirSync, readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import { after, afterEach, before, describe, test } from "node:test";
import { fileURLToPath } from "node:url";
import { chromium, firefox, webkit } from "playwright";
import { BUILD, CSP, page as fixturePage, start } from "./support/fixture.mjs";

const BASE = process.env.UI_PROOF_URL || "http://127.0.0.1:8090";
const require = createRequire(import.meta.url);
const axeSource = readFileSync(require.resolve("axe-core/axe.min.js"), "utf8");
const ENGINES = { chromium, firefox, webkit };
const NAMES = (process.env.UI_BROWSERS || "chromium,firefox").split(",").filter((n) => ENGINES[n]);
const SHOTS = process.env.UI_SHOTS;
if (SHOTS) mkdirSync(SHOTS, { recursive: true });

/** The light values of the tokens, read from the toolkit's one source, for a page that ships only dark. */
const here = dirname(fileURLToPath(import.meta.url));
const tokensRs = readFileSync(join(here, "../../../crates/navree-ui/src/tokens.rs"), "utf8");
const LIGHT =
  ":root{color-scheme:light" +
  [...tokensRs.matchAll(/t\("([a-z0-9-]+)",\s*"(#[0-9a-f]{6})",\s*"(#[0-9a-f]{6})"\)/g)].map((m) => `;--${m[1]}:${m[3]}`).join("") +
  "}body{background:var(--ink-0)}";

/** Round on purpose: the toolkit's icon containers and status dots are not pills and not letter tiles. */
const ROUND = new Set(["timeline-dot", "live-dot", "badge-dot", "activity-icon", "radio", "spinner"]);

/** What the proof app's failures carry inside them. None of it may reach a screen. */
const SECRETS = ["hunter2", "SELECT * FROM", "token='abc'", "/home/ubuntu", "db.rs", "42:7"];

/** The statuses a page can end in, with the short title of each. */
const STATUSES = [
  [401, "Sign in required", "sign-in"],
  [403, "No access", "forbidden"],
  [404, "Page not found", "not-found"],
  [405, "Not allowed", "method-not-allowed"],
  [409, "Conflict", "conflict"],
  [410, "Gone", "gone"],
  [419, "Page expired", "expired"],
  [422, "Couldn't use that", "invalid"],
  [429, "Too many requests", "rate-limited"],
  [500, "Something went wrong", "failed"],
  [503, "Unavailable", "unavailable"],
];

const BROWSER_ACCEPT = "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8";
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function axe(page) {
  await page.evaluate(axeSource);
  return page.evaluate(async () => {
    const out = await window.axe.run(document, { runOnly: ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa", "best-practice"] });
    return out.violations.map((v) => `${v.id}: ${v.nodes.slice(0, 3).map((n) => n.target.join(" ")).join(" | ")}`);
  });
}

const hits = async () => (await fetch(BASE + "/pages/hits")).json();
const reset = () => fetch(BASE + "/pages/reset", { method: "POST" });
/** Waits for `check` to hold, polling from here. */
async function until(check, what, ms = 8000) {
  const end = Date.now() + ms;
  for (;;) {
    const got = await check();
    if (got) return got;
    if (Date.now() > end) assert.fail(`timed out waiting for ${what}`);
    await sleep(50);
  }
}

for (const name of NAMES) {
  describe(name, { concurrency: false }, () => {
    let browser;
    before(async () => {
      browser = await ENGINES[name].launch(name === "firefox" ? { firefoxUserPrefs: { "network.http.throttle.enable": false } } : {});
    });
    after(() => browser.close());
    const contexts = [];
    afterEach(async () => {
      for (const context of contexts.splice(0)) await context.close().catch(() => {});
    });

    /**
     * A page on the proof app. Console errors, page errors and content security policy
     * reports are kept in `page.problems`; `page.done()` asserts there are none.
     * `user: true` signs the context in; `light` swaps in the light tokens.
     */
    async function open(path, options = {}) {
      const { light, user, expect: expected, ...rest } = options;
      const context = await browser.newContext({ viewport: { width: 1440, height: 900 }, bypassCSP: !!light, ...rest });
      contexts.push(context);
      if (user) await context.addCookies([{ name: "proof_user", value: "ada", url: BASE }]);
      const page = await context.newPage();
      page.problems = [];
      page.requests = [];
      page.on("request", (r) => page.requests.push(r.method() + " " + r.url().replace(BASE, "")));
      page.on("console", (m) => {
        if (name === "webkit" && /Refused to apply a stylesheet/.test(m.text())) return;
        // A page that answers 4xx or 5xx is logged by the browser itself, and so is a failed fetch.
        if (m.type() === "error" && !/Failed to load resource|ERR_|NS_ERROR|Load failed|status of \d{3}|could not be loaded/i.test(m.text())) page.problems.push(m.text());
      });
      page.on("pageerror", (e) => page.problems.push("pageerror: " + e.message));
      await page.addInitScript(() => {
        document.addEventListener("securitypolicyviolation", (e) => {
          (window.__csp ||= []).push(e.violatedDirective + " " + e.blockedURI);
        });
      });
      const response = await page.goto(BASE + path);
      if (expected !== undefined) assert.equal(response.status(), expected, `${path} answers ${expected}`);
      page.response = response;
      if (light) {
        await page.addStyleTag({ content: LIGHT });
        await page.waitForTimeout(300);
      }
      page.done = async () => {
        let csp = rest.javaScriptEnabled === false ? [] : await page.evaluate(() => window.__csp || []);
        if (name === "webkit") csp = csp.filter((line) => !/^style-src-(elem|attr) inline$/.test(line));
        await context.close();
        assert.deepEqual(csp, [], "no content security policy report");
        assert.deepEqual(page.problems, []);
      };
      return page;
    }

    const shot = async (page, label) => {
      if (SHOTS) await page.screenshot({ path: join(SHOTS, `${name}-${label}.png`), fullPage: true });
    };
    const noSideways = (page) => page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth);
    /** The app's rules on everything in the main region: corners of 4 or 6 px (or round where it is allowed), no shadow, no inline style. */
    const rules = (page, scope = "main *") =>
      page.evaluate(([round, scope]) => {
        const bad = [];
        for (const el of document.querySelectorAll(scope)) {
          const style = getComputedStyle(el);
          const slot = el.getAttribute("data-slot") || el.tagName;
          const radius = parseFloat(style.borderTopLeftRadius);
          if (radius && ![2, 4, 6].includes(radius) && !round.includes(slot) && !el.closest("[data-slot=spinner]")) bad.push(`radius ${style.borderTopLeftRadius} on ${slot}`);
          if (style.boxShadow !== "none") bad.push(`shadow on ${slot}`);
          if (el.hasAttribute("style") && !el.hasAttribute("popover")) bad.push(`style attribute on ${slot}`);
        }
        return [...new Set(bad)];
      }, [[...ROUND], scope]);
    /**
     * Moves focus to the next control, as Tab does. WebKit's Tab leaves links out unless the
     * reader turns that on, so there the page's own order is read and focus is moved along it
     * after a key press, which is what a keyboard user's focus ring needs.
     */
    const next = async (page) => {
      if (name !== "webkit") return page.keyboard.press("Tab");
      await page.keyboard.press("Shift");
      await page.evaluate(() => {
        const list = [...document.querySelectorAll("a[href],button:not([disabled]),input:not([disabled]):not([type=hidden]),select,textarea,summary,[tabindex]:not([tabindex='-1'])")].filter((el) => el.getClientRects().length && getComputedStyle(el).visibility !== "hidden");
        const at = list.indexOf(document.activeElement);
        list[at + 1]?.focus();
      });
    };
    const text = (page) => page.locator("body").innerText();
    const announced = (page) => page.evaluate(() => document.querySelector("[data-slot=announcer]")?.textContent ?? "");

    // ---- Every status, as a page ------------------------------------------------

    describe("a page that ends in an error", () => {
      for (const [code, title, kind] of STATUSES) {
        test(`${code}: a guest gets a page on its own with the right status, title, heading and reference`, async () => {
          const page = await open(`/pages/status/${code}`, { expect: code });
          assert.equal(await page.title(), title);
          assert.equal(await page.locator("h1").count(), 1);
          assert.equal(await page.locator("[data-slot=sidebar]").count(), 0, "no shell for a guest");
          assert.equal(await page.locator(`[data-slot=state][data-kind=${kind}]`).count(), 1);
          assert.equal(await page.locator("[data-slot=state-code]").textContent(), String(code));
          const reference = await page.locator("[data-slot=state-reference] code").textContent();
          assert.equal(reference, page.response.headers()["x-request-id"], "the reference is the request id");
          assert.equal(await page.locator("script").count(), 0, "a page of an error runs nothing");
          assert.equal(page.response.headers()["cache-control"], "no-store");
          assert.ok(page.response.headers()["content-type"].startsWith("text/html"));
          assert.ok(await page.locator("[data-slot=state-actions] a, [data-slot=state-actions] button").first().isVisible(), "there is something to do next");
          await shot(page, `status-${code}`);
          await page.done();
        });
      }

      for (const theme of ["dark", "light"]) {
        for (const width of [1440, 390]) {
          test(`every status is axe clean, spills nothing and keeps the rules (${theme}, ${width} px)`, async () => {
            for (const [code] of STATUSES) {
              for (const user of [false, true]) {
                const page = await open(`/pages/status/${code}`, { expect: code, user, light: theme === "light", viewport: { width, height: 800 } });
                assert.deepEqual(await axe(page), [], `${code} ${user ? "signed in" : "guest"}`);
                assert.ok(await noSideways(page), `${code}: no sideways scroll`);
                assert.deepEqual(await rules(page), [], `${code}`);
                if (code === 404 || code === 500) await shot(page, `status-${code}-${user ? "shell" : "plain"}-${theme}-${width}`);
                await page.done();
              }
            }
          });
        }
      }

      test("a signed-in user gets the error inside the shell: sidebar, top bar, the short title as the heading", async () => {
        for (const code of [403, 404, 409, 410, 419, 422, 500]) {
          const page = await open(`/pages/status/${code}`, { expect: code, user: true });
          assert.equal(await page.locator("[data-slot=sidebar]").count(), 1, `${code}`);
          assert.equal(await page.locator("h1").count(), 1);
          assert.equal(await page.locator("h1").textContent(), STATUSES.find(([c]) => c === code)[1]);
          assert.equal(await page.locator("main [data-slot=state]").count(), 1);
          assert.ok(await page.locator("[data-slot=sidebar-item]").first().isVisible());
          assert.equal(await page.locator("[data-slot=state-reference] [data-slot=button]").count(), 1, "the shell page can copy the reference");
          await page.done();
        }
        // The ones that must work when much else does not, and a guest's, stand alone.
        for (const code of [401, 429, 503]) {
          const page = await open(`/pages/status/${code}`, { expect: code, user: true });
          assert.equal(await page.locator("[data-slot=sidebar]").count(), 0, `${code}`);
          await page.done();
        }
      });

      test("a missing page is a 404 page, in the shell for a user and out of it for a guest", async () => {
        const guest = await open("/no/such/page", { expect: 404 });
        assert.equal(await guest.title(), "Page not found");
        assert.equal(await guest.locator("[data-slot=sidebar]").count(), 0);
        await guest.done();
        const user = await open("/no/such/page", { expect: 404, user: true });
        assert.equal(await user.locator("[data-slot=sidebar]").count(), 1);
        await shot(user, "missing-in-shell");
        await user.done();
      });

      test("a page that needs a user sends a guest to sign in with the way back, and shows the page to a user", async () => {
        const guest = await open("/pages/user?x=1", { expect: 401 });
        const href = await guest.locator("[data-slot=state-actions] a").first().getAttribute("href");
        assert.equal(href, "/sign-in?next=%2Fpages%2Fuser%3Fx%3D1");
        await guest.done();
        const user = await open("/pages/user", { expect: 200, user: true });
        assert.ok((await text(user)).includes("You are on /pages/user."));
        await user.done();
      });

      test("the page of a failure shows nothing of what was inside it", async () => {
        for (const path of ["/pages/leak", "/pages/panic"]) {
          for (const user of [false, true]) {
            const page = await open(path, { expect: 500, user });
            const body = await page.content();
            const headers = JSON.stringify(page.response.headers());
            for (const secret of SECRETS) {
              assert.ok(!body.includes(secret), `${path}: ${secret} in the page`);
              assert.ok(!headers.includes(secret), `${path}: ${secret} in the headers`);
            }
            assert.ok(!body.includes("Development only"));
            assert.ok(body.includes("Something went wrong on our side"));
            await page.done();
          }
        }
      });

      test("a route that is data keeps its JSON error, and so does the API, for a browser too", async () => {
        const data = await fetch(BASE + "/pages/data", { headers: { accept: BROWSER_ACCEPT } });
        assert.equal(data.status, 403);
        assert.equal(data.headers.get("content-type"), "application/json");
        assert.equal((await data.json()).code, "forbidden");
        const api = await fetch(BASE + "/api/nothing", { headers: { accept: BROWSER_ACCEPT } });
        assert.equal(api.status, 404);
        assert.equal(api.headers.get("content-type"), "application/json");
        const script = await fetch(BASE + "/pages/status/403", { headers: { accept: "application/json" } });
        assert.equal(script.headers.get("content-type"), "application/json");
        const page = await fetch(BASE + "/pages/status/403", { headers: { accept: BROWSER_ACCEPT } });
        assert.ok(page.headers.get("content-type").startsWith("text/html"));
        assert.ok((page.headers.get("vary") || "").toLowerCase().includes("accept"));
      });
    });

    // ---- JavaScript off -----------------------------------------------------------

    describe("with JavaScript off", () => {
      test("an error page, its sign-in link and a failed post all work", async () => {
        const guest = await open("/pages/user", { expect: 401, javaScriptEnabled: false });
        await guest.locator("[data-slot=state-actions] a").first().click();
        await guest.waitForURL(/\/sign-in\?next=/);
        assert.ok((await text(guest)).includes("You've been signed out"), "the notice for a user sent here");
        await guest.locator("#sign-in button[type=submit]").click();
        // Back where the guest was sent from, now signed in.
        await guest.waitForURL(BASE + "/pages/user");
        assert.ok((await text(guest)).includes("You are on /pages/user."));
        await guest.done();

        const post = await open("/pages/actions", { user: true, javaScriptEnabled: false });
        const response = await Promise.all([post.waitForResponse((r) => r.url().endsWith("/pages/act/403")), post.locator("#act-403 button").click()]);
        assert.equal(response[0].status(), 403);
        await until(() => post.locator("[data-kind=forbidden]").count(), "the page");
        assert.equal(await post.locator("[data-kind=forbidden]").count(), 1, "a page, not a blob");
        assert.equal(await post.locator("[data-slot=sidebar]").count(), 1);
        await post.done();
      });

      test("the page for every status is axe clean with no script", async () => {
        for (const [code] of STATUSES) {
          const page = await open(`/pages/status/${code}`, { expect: code, javaScriptEnabled: false });
          assert.equal(await page.locator("script").count(), 0);
          await page.done();
        }
      });
    });

    // ---- Keyboard ---------------------------------------------------------------

    describe("by keyboard", () => {
      test("Tab reaches the main action first on a page of its own, and Enter takes it", async () => {
        const page = await open("/pages/status/404", { expect: 404 });
        await next(page);
        const first = await page.evaluate(() => ({ slot: document.activeElement.getAttribute("data-slot"), text: document.activeElement.textContent.trim() }));
        assert.deepEqual(first, { slot: "button", text: "Go to home" });
        const ring = await page.evaluate(() => {
          const style = getComputedStyle(document.activeElement);
          return document.activeElement.matches(":focus-visible") && (style.outlineStyle !== "none" || style.boxShadow !== "none");
        });
        assert.ok(ring, "a visible focus ring");
        await Promise.all([page.waitForURL(BASE + "/"), page.keyboard.press("Enter")]);
        await page.done();
      });

      test("in the shell, the state's action is reachable after the sidebar and Enter follows the sign-in link", async () => {
        const page = await open("/pages/status/500", { expect: 500, user: true });
        const reached = [];
        for (let n = 0; n < 40; n++) {
          await next(page);
          const where = await page.evaluate(() => document.activeElement.closest("[data-slot=state]") ? document.activeElement.textContent.trim() : "");
          if (where) reached.push(where);
          if (reached.length >= 2) break;
        }
        assert.deepEqual(reached.slice(0, 2), ["Try again", "Go to home"]);
        await page.done();
      });

      test("a locked row in a table takes no tab stop of its own", async () => {
        const page = await open("/pages/rows", { user: true });
        const stops = await page.evaluate(() => [...document.querySelectorAll("[data-slot=state-row] a, [data-slot=state-row] button, [data-slot=state-row] input")].length);
        assert.equal(stops, 0);
        await page.done();
      });
    });

    // ---- The gallery: every state in every size ---------------------------------

    describe("the states", () => {
      for (const theme of ["dark", "light"]) {
        for (const width of [1440, 390]) {
          test(`the gallery is axe clean, spills nothing and keeps the rules (${theme}, ${width} px)`, async () => {
            const page = await open("/pages", { user: true, light: theme === "light", viewport: { width, height: 900 } });
            assert.deepEqual(await axe(page), []);
            assert.ok(await noSideways(page));
            assert.deepEqual(await rules(page, "main [data-slot=state], main [data-slot=state] *, main [data-slot=state-row] *"), []);
            await shot(page, `gallery-${theme}-${width}`);
            await page.done();
          });
        }
      }

      test("a forbidden or missing thing, a sign-in and an empty list each say their one thing", async () => {
        const page = await open("/pages", { user: true });
        const say = (id) => page.locator(`#${id}`).innerText();
        assert.match(await say("card-forbidden"), /You don't have access to this report\./);
        assert.match(await say("card-missing"), /This deal doesn't exist, or it was removed\./);
        assert.match(await say("card-sign-in"), /Sign in to see your deals\./);
        assert.equal(await page.locator("#card-sign-in a").getAttribute("href"), "/sign-in?next=%2Fpages%3Ftab%3Dnotes");
        assert.equal(await page.locator("#card-access a").getAttribute("href"), "/pages?request=1");
        assert.match(await say("empty-nothing"), /No deals yet/);
        assert.match(await say("empty-none"), /No results/);
        assert.match(await say("empty-done"), /You're all caught up/);
        assert.equal(await page.locator("[data-slot=state-code]").count() > 0, true);
        assert.equal(await page.locator("#empty-nothing [data-slot=state-code]").count(), 0, "an empty list has no status");
        await page.done();
      });

      test("a tab can hold a forbidden or a missing state", async () => {
        const page = await open("/pages", { user: true });
        await page.getByRole("tab", { name: "Notes" }).click();
        assert.equal(await page.locator("#tab-forbidden").isVisible(), true);
        await page.getByRole("tab", { name: "Files" }).click();
        assert.equal(await page.locator("#tab-missing").isVisible(), true);
        assert.deepEqual(await axe(page), []);
        await page.done();
      });

      test("in a table, a locked, a missing and an empty row span the table and keep its header and order", async () => {
        const page = await open("/pages/rows", { user: true });
        const shape = await page.evaluate(() => {
          const table = document.querySelector("table");
          const width = table.getBoundingClientRect().width;
          const rows = [...table.tBodies[0].rows].map((row) => ({ kind: row.getAttribute("data-kind"), cells: row.cells.length, span: row.cells[0].colSpan, full: Math.abs(row.cells[0].getBoundingClientRect().width - width) < 2 }));
          return { head: [...table.tHead.rows[0].cells].map((c) => c.textContent.trim()), rows };
        });
        assert.deepEqual(shape.head, ["Name", "Amount"]);
        assert.deepEqual(shape.rows.map((r) => r.kind), [null, "forbidden", null, "not-found", "no-results", null]);
        for (const row of shape.rows.filter((r) => r.kind)) {
          assert.equal(row.cells, 1);
          assert.equal(row.span, 2);
          assert.ok(row.full, "the cell spans the table");
        }
        assert.deepEqual(await axe(page), []);
        await shot(page, "rows");
        await page.done();
      });

      test("right to left: the state reads from the right and keeps its parts", async () => {
        const page = await open("/pages", { user: true });
        const where = await page.evaluate(() => {
          const state = document.getElementById("rtl");
          const text = state.querySelector("[data-slot=state-title]");
          return { dir: getComputedStyle(state).direction, title: text.textContent, align: getComputedStyle(text).textAlign };
        });
        assert.equal(where.dir, "rtl");
        assert.equal(where.title, "لا توجد عناصر بعد");
        await page.done();
      });
    });

    // ---- Boosted navigation to an error ---------------------------------------------

    describe("boosted navigation", () => {
      for (const [code, title] of [[404, "Page not found"], [403, "No access"], [500, "Something went wrong"], [409, "Conflict"]]) {
        test(`a link to a page that answers ${code} shows it in place: the shell stays, the title and address change, it is announced, back returns`, async () => {
          const page = await open("/pages", { user: true });
          await page.evaluate(() => {
            window.__sidebar = document.querySelector("[data-slot=sidebar]");
            window.__kept = "still here";
          });
          const before = page.requests.length;
          await page.locator(`#links a[href="/pages/status/${code}"]`).click();
          await page.waitForURL(new RegExp(`/pages/status/${code}$`));
          await until(async () => (await page.title()) === title, "the title to change");
          assert.equal(await page.evaluate(() => window.__sidebar === document.querySelector("[data-slot=sidebar]")), true, "the same shell, not a new page");
          assert.equal(await page.evaluate(() => window.__kept), "still here", "no page load");
          assert.equal(await page.locator(`main [data-slot=state][data-status="${code}"]`).count(), 1);
          assert.equal(await page.locator("h1").textContent(), title);
          assert.equal(await announced(page), title, "announced");
          assert.equal(await page.evaluate(() => document.activeElement?.id), "main", "focus goes to the main region");
          assert.ok(!(await text(page)).includes("{\"statusCode\""), "not a JSON blob");
          assert.equal(page.requests.slice(before).filter((r) => r.startsWith("GET /pages/status")).length, 1, "one request");
          await shot(page, `boosted-${code}`);
          await page.goBack();
          await page.waitForURL(BASE + "/pages");
          await until(async () => (await page.title()) === "States", "the gallery to return");
          assert.equal(await page.evaluate(() => window.__kept), "still here", "back is not a page load either");
          await page.done();
        });
      }
    });

    // ---- Regions --------------------------------------------------------------------

    describe("regions", () => {
      test("a region that gets a refusal shows the state where it was, and announces it once", async () => {
        await reset();
        const page = await open("/pages/regions", { user: true });
        for (const [code, kind] of [[401, "sign-in"], [403, "forbidden"], [404, "not-found"], [410, "gone"], [429, "rate-limited"], [500, "failed"], [503, "unavailable"]]) {
          await until(() => page.locator(`#r-${code} [data-slot=state][data-kind=${kind}]`).count(), `region ${code}`);
          const region = page.locator(`#r-${code}`);
          assert.equal(await region.evaluate((el) => el.hasAttribute("aria-busy")), false, `${code} is no longer busy`);
          assert.equal(await region.locator("[data-slot=skeleton]").count(), 0);
          assert.ok(!(await region.innerText()).includes("statusCode"), "not a JSON blob");
          // In place: still inside its card.
          assert.equal(await region.evaluate((el) => !!el.closest("[data-slot=card]")), true);
        }
        assert.deepEqual(await axe(page), []);
        assert.ok(await noSideways(page));
        await shot(page, "regions");
        await page.done();
      });

      test("a 401 in a region leads to sign-in and back to the page, not to the fragment", async () => {
        const page = await open("/pages/regions");
        const href = await until(() => page.locator("#r-401 [data-slot=state] a").first().getAttribute("href").catch(() => null), "the sign-in link");
        assert.equal(href, "/sign-in?next=%2Fpages%2Fregions");
        await page.done();
      });

      test("Try again fetches the region again, and a flaky region recovers", async () => {
        await reset();
        const page = await open("/pages/regions");
        const flaky = page.locator("#r-flaky");
        await until(() => flaky.locator("[data-kind=unavailable]").count(), "the first failure");
        assert.match(await flaky.innerText(), /Not just yet\./);
        for (let n = 0; n < 2; n++) {
          const before = (await hits()).flaky;
          await flaky.getByRole("button", { name: "Try again" }).click();
          await until(async () => (await hits()).flaky > before, "another fetch");
          if (n === 0) await until(() => flaky.locator("[data-kind=unavailable]").count(), "the second failure");
        }
        await until(() => flaky.locator("[data-slot=alert]").count(), "the region to recover");
        assert.match(await flaky.innerText(), /It worked on the third try\./);
        assert.equal(await flaky.locator("[data-slot=state]").count(), 0, "the state is gone");
        await page.done();
      });

      test("a region that polls a page that is gone stops asking", async () => {
        await reset();
        const page = await open("/pages/regions");
        await until(() => page.locator("#r-poll [data-kind=not-found]").count(), "the region to say so");
        const first = (await hits())["404"];
        await sleep(1500);
        const later = (await hits())["404"];
        // The lazy region asked once too; the poller, every 400 ms, would have asked three or four more times.
        assert.ok(later <= first + 1, `asked ${first} times, then ${later}`);
        await page.done();
      });

      test("a region that works is untouched", async () => {
        await reset();
        const page = await open("/pages/regions");
        await until(() => page.locator("#r-ok [data-slot=alert]").count(), "the region to load");
        assert.equal(await page.locator("#r-ok [data-slot=state]").count(), 0);
        await page.done();
      });
    });

    // ---- Actions --------------------------------------------------------------------

    describe("actions", () => {
      const cases = [
        [403, "danger", /You don't have permission to do that\./],
        [404, "danger", /That no longer exists\./],
        [409, "warning", /Someone else renamed this deal a minute ago\./],
        [419, "warning", /This page expired\. Reload it and try again\./],
        [429, "warning", /Try again in 12 seconds\./],
        [500, "danger", /Something went wrong on our side\. Try again in a moment\. Reference [0-9a-f]+\./],
        [503, "danger", /Back at noon\./],
      ];
      for (const [code, tone, words] of cases) {
        test(`a post that ends in ${code} is one ${tone} toast, and the form keeps what was typed`, async () => {
          const page = await open("/pages/actions", { user: true });
          await page.locator(`#act-${code} input`).fill("my typed words");
          const [response] = await Promise.all([page.waitForResponse((r) => r.url().endsWith(`/pages/act/${code}`)), page.locator(`#act-${code} button`).click()]);
          assert.equal(response.status(), code);
          assert.equal(response.headers()["content-type"], "application/vnd.navree.ops+json");
          const toast = page.locator(`[data-slot=toast][data-tone=${tone}]`);
          await until(() => toast.count(), "a toast");
          assert.equal(await toast.count(), 1);
          assert.match(await toast.first().innerText(), words);
          assert.equal(await page.locator(`#act-${code} input`).inputValue(), "my typed words");
          assert.equal(await page.locator("[data-slot=sidebar]").count(), 1);
          assert.equal(new URL(page.url()).pathname, "/pages/actions", "still on the page");
          for (const secret of SECRETS) assert.ok(!(await page.content()).includes(secret), secret);
          await shot(page, `action-${code}`);
          await page.done();
        });
      }

      test("a guest's post leads to sign in, shows the notice, and signing in comes back to the page", async () => {
        const page = await open("/pages/actions");
        await page.locator("#act-401 button").click();
        await page.waitForURL(/\/sign-in\?next=%2Fpages%2Factions$/);
        await until(() => page.locator("[data-notice], [data-slot=alert]").count(), "the notice");
        assert.match(await text(page), /You've been signed out/);
        await page.locator("#sign-in button[type=submit]").click();
        await page.waitForURL(BASE + "/pages/actions");
        await page.done();
      });
    });

    // ---- Sign in and the way back ---------------------------------------------------

    describe("the way back after signing in", () => {
      test("an address on this site is followed, query and all", async () => {
        const page = await open("/sign-in?next=%2Fpages%2Fstatus%2F404%3Fa%3D1");
        await page.locator("#sign-in button[type=submit]").click();
        await page.waitForURL(`${BASE}/pages/status/404?a=1`);
        await page.done();
      });

      test("an address that would leave the site is dropped: the user lands on the home page and nothing is asked of another origin", async () => {
        for (const next of ["//evil.example/x", "/\\evil.example/x", "https://evil.example/x", "/%2f/evil.example/x", "javascript:alert(1)", "/%09/evil.example", "/a/../../evil.example"]) {
          const page = await open(`/sign-in?next=${encodeURIComponent(next)}`);
          const asked = [];
          await page.route("**/*", (route) => {
            const url = route.request().url();
            if (!url.startsWith(BASE)) asked.push(url);
            return route.continue();
          });
          await page.locator("#sign-in button[type=submit]").click();
          await page.waitForURL((url) => !url.pathname.startsWith("/sign-in"));
          await page.waitForLoadState();
          assert.equal(new URL(page.url()).origin, new URL(BASE).origin, `${next} stayed on this site`);
          assert.deepEqual(asked, [], `${next}: nothing was asked of another origin`);
          assert.equal(new URL(page.url()).pathname === "/" || new URL(page.url()).pathname.startsWith("/a/"), true, `${next} landed at ${page.url()}`);
          await page.done();
        }
      });

      test("a hidden next in the form is checked again by the server", async () => {
        const page = await open("/sign-in");
        await page.evaluate(() => {
          document.querySelector("#sign-in input[name=next]").value = "//evil.example/x";
        });
        await page.locator("#sign-in button[type=submit]").click();
        await page.waitForURL((url) => !url.pathname.startsWith("/sign-in"));
        assert.equal(new URL(page.url()).origin, new URL(BASE).origin);
        assert.equal(new URL(page.url()).pathname, "/");
        await page.done();
      });

      test("the sign-in page after a lapsed session shows its notice and no more than that above the form", async () => {
        const page = await open("/sign-in?next=%2Fpages", { viewport: { width: 390, height: 700 } });
        assert.ok(await page.locator("#sign-in button[type=submit]").isVisible(), "the form is on the screen");
        assert.ok(await page.locator("[data-slot=alert]").isVisible());
        assert.deepEqual(await axe(page), []);
        await shot(page, "sign-in-notice-390");
        await page.done();
      });
    });

    // ---- The runtime against a hostile server ------------------------------------------

    describe("a hostile state", () => {
      test("an answer that says it is a state is shown as markup the page owns, and runs nothing", async () => {
        const server = await start();
        try {
          server.routes.set("/", (req, res) => {
            server.html(res, fixturePage(`<main><div id="box" data-ui-region data-ui-src="/frag" data-ui-lazy><img name="querySelector"><form name="addEventListener"></form></div></main>`));
          });
          server.routes.set("/frag", (req, res) => {
            res.writeHead(403, { "content-type": "text/html", "x-navree-state": "403" });
            res.end(
              `<!doctype html><html><body><div data-slot="state" data-kind="forbidden"><p data-slot="state-title">You don't have access to this</p>` +
                `<script>document.documentElement.setAttribute("data-ran","1")</script>` +
                `<img src="${server.evilOrigin}/steal" onerror="document.documentElement.setAttribute('data-ran','2')">` +
                `<meta http-equiv="refresh" content="0;url=${server.evilOrigin}/"><base href="${server.evilOrigin}/"></div></body></html>`,
            );
          });
          const context = await browser.newContext();
          contexts.push(context);
          const page = await context.newPage();
          await page.goto(server.origin + "/");
          await until(() => page.locator("#box [data-slot=state]").count(), "the state");
          assert.equal(await page.evaluate(() => document.documentElement.hasAttribute("data-ran")), false, "no script ran");
          assert.equal(await page.locator("#box base, #box meta").count(), 0);
          assert.equal(new URL(page.url()).origin, server.origin, "no navigation");
          assert.deepEqual(server.evil.filter((r) => !/favicon/.test(r.url)), [], "nothing was asked of the other origin");
          assert.ok(server.log.some((entry) => entry.path === "/frag" && entry.headers["x-navree-region"] === "box" && entry.headers["x-navree-from"] === "/"), "the request names the region and the page");
        } finally {
          server.close();
        }
      });

      test("a 403 without the mark is not shown as a state: the region keeps what it had and says it failed", async () => {
        const server = await start();
        try {
          server.routes.set("/", (req, res) => {
            server.html(res, fixturePage(`<main><div id="box" data-ui-region data-ui-src="/frag" data-ui-lazy><p id="old">Old content</p></div></main>`));
          });
          server.routes.set("/frag", (req, res) => {
            res.writeHead(403, { "content-type": "text/html" });
            res.end(`<!doctype html><html><body><p id="intruder">not a state</p></body></html>`);
          });
          const context = await browser.newContext();
          contexts.push(context);
          const page = await context.newPage();
          await page.goto(server.origin + "/");
          await until(() => page.locator("#box[data-ui-failed]").count(), "the region to be marked failed");
          assert.equal(await page.locator("#intruder").count(), 0);
          assert.equal(await page.locator("#old").count(), 1);
        } finally {
          server.close();
        }
      });
    });
  });
}
