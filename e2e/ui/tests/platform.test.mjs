// The app `navree new` writes (Navree UI is the default front end), in real browsers: sign-in
// with its wrong-password message, the customers table with Indian names and rupees, search,
// filters and sort through the table's own controls with the one list query in the address, the
// form that adds a customer (the browser's own checks, then the server's, then the toast), a
// second window whose new customer reaches the first window's table by itself, the switch to
// the account's second workspace (its own customers, its name in the sidebar, the first
// workspace's customers not in it), and sign-out.
//
// Needs the app serving, with its demo account and forty demo customers (it makes them when
// it starts outside production). `cargo test -p navree-cli --test generated_kits -- --ignored
// navree_ui_kit_journey_in_a_browser` with NAVREE_TEST_KIT_WEB=1 builds one, starts it on a
// free port and runs this file against it. By hand:
//
//   navree new platform --ui navree && cd platform
//   navree dock up && navree key:generate   # put the key in .env as APP_KEY
//   navree dev                              # http://127.0.0.1:8080
//   PLATFORM_URL=http://127.0.0.1:8080 node --test tests/platform.test.mjs   (in e2e/ui)
//
// PLATFORM_URL points elsewhere (default http://127.0.0.1:8080); PLATFORM_EMAIL and
// PLATFORM_PASSWORD name another account (default: the demo account); PLATFORM_BROWSERS picks
// engines (default "chromium"; "chromium,firefox,webkit" for all three). Without the app the
// suite is skipped.
//
// The app keeps what the tests add, so a test never counts on the number of customers. The
// customers they make live in Patiala, a city the demo data has none in, and have names that
// sort after the demo customers', so the demo customers' pages stay as they were.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { after, afterEach, before, describe, test } from "node:test";
import { chromium, firefox, webkit } from "playwright";
import { launch } from "./support/launch.mjs";

const BASE = process.env.PLATFORM_URL || "http://127.0.0.1:8080";
const EMAIL = process.env.PLATFORM_EMAIL || "harpreet.singh@example.com";
const PASSWORD = process.env.PLATFORM_PASSWORD || "demo-password-2026";
const require = createRequire(import.meta.url);
const axeSource = readFileSync(require.resolve("axe-core/axe.min.js"), "utf8");
const ENGINES = { chromium, firefox, webkit };
const NAMES = (process.env.PLATFORM_BROWSERS || "chromium").split(",").filter((n) => ENGINES[n]);

/** The first ten customers by name, as the demo data sorts them. */
const FIRST_TEN = ["Ajay Rawat", "Aman Trivedi", "Amit Khanna", "Anil Mehra", "Anjali Mishra", "Ankit Srivastava", "Ashok Luthra", "Deepak Chopra", "Divya Bhardwaj", "Gaurav Sood"];

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
/** Something no other run of these tests used. */
const stamp = () => Date.now().toString(36) + Math.floor(Math.random() * 1296).toString(36);

// The suite runs only against this app: its sign-in page is the sign of it.
let reachable = false;
try {
  const response = await fetch(BASE + "/sign-in");
  reachable = response.status === 200 && (await response.text()).includes('id="sign-in-form"');
} catch {}

async function axe(page) {
  await page.evaluate(axeSource);
  return page.evaluate(async () => {
    const out = await window.axe.run(document, { runOnly: ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa", "best-practice"] });
    return out.violations.map((v) => `${v.id}: ${v.nodes.slice(0, 3).map((n) => n.target.join(" ")).join(" | ")}`);
  });
}

for (const name of NAMES) {
  describe(`platform app in ${name}`, { concurrency: false, skip: reachable ? false : `the app isn't serving at ${BASE}` }, () => {
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

    /** A browser context of its own (its own cookies): one visitor. */
    async function visitor(options = {}) {
      const context = await browser.newContext({ viewport: { width: 1440, height: 900 }, ...options });
      contexts.push(context);
      return context;
    }

    /** A page of `context`. Console errors, page errors and policy reports are kept; `page.done()` asserts none. */
    async function pageOf(context) {
      const page = await context.newPage();
      page.problems = [];
      page.on("console", (m) => {
        if (name === "webkit" && /Refused to apply a stylesheet/.test(m.text())) return;
        if (m.type() === "error" && !/Failed to load resource|ERR_ABORTED|NS_ERROR|Load failed|can.t establish a connection|40[1-9]|422/i.test(m.text())) page.problems.push(m.text());
      });
      page.on("pageerror", (e) => page.problems.push("pageerror: " + e.message));
      await page.addInitScript(() => {
        document.addEventListener("securitypolicyviolation", (e) => {
          (window.__csp ||= []).push(e.violatedDirective + " " + e.blockedURI);
        });
      });
      page.done = async () => {
        let csp = await page.evaluate(() => window.__csp || []);
        if (name === "webkit") csp = csp.filter((line) => !/^style-src-(elem|attr) inline$/.test(line));
        assert.deepEqual(csp, [], "no content security policy report");
        assert.deepEqual(page.problems, []);
      };
      return page;
    }

    /** Waits for `check` to hold in the page, asking from here. */
    const until = async (page, check, arg, what, ms = 12000) => {
      const end = Date.now() + ms;
      for (;;) {
        if (await page.evaluate(check, arg)) return;
        if (Date.now() > end) assert.fail(`timed out waiting for ${what}`);
        await sleep(100);
      }
    };
    const rows = (page) => page.locator("#customers tbody tr[data-key]");
    const settled = (page) =>
      until(page, () => !document.getElementById("customers")?.hasAttribute("data-pending") && !document.documentElement.hasAttribute("data-ui-loading"), null, "the table to settle");
    const live = (page, id) => until(page, (region) => document.getElementById(region)?.getAttribute("data-live") === "on", id, `${id} to be live`);
    const toast = (page, text) => page.locator("[data-slot=toast]", { hasText: text });
    const names = (page) => rows(page).locator("td[data-col=name]").allInnerTexts().then((texts) => texts.map((text) => text.trim()));
    const params = (page) => new URL(page.url()).searchParams;

    /** Fills the sign-in form and presses its button. */
    async function fillSignIn(page, email, password) {
      await page.fill("input[name=email]", email);
      await page.fill("input[name=password]", password);
      await page.getByRole("button", { name: "Sign in" }).click();
    }

    /** A page of a visitor who has signed in through the form, on `path`. */
    async function signedIn(path = "/customers", context) {
      context ||= await visitor();
      const page = await pageOf(context);
      await page.goto(BASE + "/sign-in");
      await fillSignIn(page, EMAIL, PASSWORD);
      await page.waitForURL((url) => url.pathname === "/");
      if (path !== "/") await page.goto(BASE + path);
      return page;
    }

    /** Fills the customer form; the plan and the status are the form's own choices. */
    async function fillCustomer(page, { name, email, phone = "+91 98765 43210", city = "Patiala", plan = "growth", status = "trial", monthly = "24,999", since = "2026-11-01", notes = "" }) {
      await page.fill("input[name=name]", name);
      await page.fill("input[name=email]", email);
      await page.fill("input[name=phone]", phone);
      await page.fill("input[name=city]", city);
      await page.selectOption("select[name=plan]", plan);
      await page.selectOption("select[name=status]", status);
      await page.fill("input[name=monthly]", monthly);
      await page.fill("input[name=since]", since);
      await page.check("input[name=autopay]");
      if (notes) await page.fill("textarea[name=notes]", notes);
    }

    // ---- Signing in ----------------------------------------------------------------

    test("a guest is sent to sign in with the page they asked for, and a wrong password says nothing about which part was wrong", async () => {
      const page = await pageOf(await visitor());
      await page.goto(BASE + "/customers?search=ludhiana");
      assert.equal(new URL(page.url()).pathname, "/sign-in");
      assert.equal(params(page).get("next"), "/customers?search=ludhiana");
      assert.deepEqual(await axe(page), [], "the sign-in page");
      assert.equal(await page.locator("h1").innerText(), "Sign in");

      // A right email with a wrong password, and an email nobody has: the same words.
      const messages = [];
      for (const email of [EMAIL, "nobody@example.com"]) {
        await page.fill("input[name=email]", email);
        await page.fill("input[name=password]", "not-the-password");
        const [response] = await Promise.all([
          page.waitForResponse((r) => new URL(r.url()).pathname === "/sign-in" && r.request().method() === "POST"),
          page.getByRole("button", { name: "Sign in" }).click(),
        ]);
        assert.equal(response.status(), 422);
        const error = page.locator("#sign-in-form [data-slot=field-error]");
        await error.first().waitFor();
        messages.push((await error.first().innerText()).trim());
        assert.equal(await page.inputValue("input[name=email]"), email, "what was typed stays");
        assert.equal(new URL(page.url()).pathname, "/sign-in");
      }
      assert.equal(messages[0], "These details don't match an account. Check them and try again.");
      assert.equal(messages[0], messages[1]);

      // The right one goes where the visitor was going.
      await fillSignIn(page, EMAIL, PASSWORD);
      await page.waitForURL((url) => url.pathname === "/customers");
      assert.equal(params(page).get("search"), "ludhiana");
      await rows(page).first().waitFor();
      assert.equal(await rows(page).count(), 5, "the five customers in Ludhiana");
      await page.done();
    });

    test("a next address that leaves the site is not followed", async () => {
      const page = await pageOf(await visitor());
      await page.goto(BASE + "/sign-in?next=" + encodeURIComponent("//evil.example/"));
      await fillSignIn(page, EMAIL, PASSWORD);
      await page.waitForURL((url) => url.origin === new URL(BASE).origin && url.pathname === "/");
      await page.done();
    });

    // ---- The customers ---------------------------------------------------------------

    test("the table lists Indian names and rupees with lakh grouping, ten to a page, with a row link", async () => {
      const page = await signedIn("/customers");
      await rows(page).first().waitFor();
      assert.deepEqual(await names(page), FIRST_TEN);
      const text = await page.locator("#customers").innerText();
      assert.match(text, /₹1,10,000/, "a lakh is written 1,00,000");
      assert.match(text, /₹29,999/);
      assert.equal(await rows(page).count(), 10);
      // A row is a link to the customer.
      await page.locator("#customers a", { hasText: "Ajay Rawat" }).first().click();
      await page.waitForURL((url) => /^\/customers\/[0-9a-f-]{36}$/.test(url.pathname));
      assert.equal(await page.locator("h1").innerText(), "Ajay Rawat");
      assert.match(await page.locator("main").innerText(), /\+91 98765 0\d{4}/, "the phone number as people read it");
      await page.done();
    });

    test("search, a filter and a sort, through the table's controls, and the address says it in the one grammar", async () => {
      const page = await signedIn("/customers");
      await rows(page).first().waitFor();
      await live(page, "customers");

      // Two cities, ticked in the filter's menu: filter[city][in]=...
      await page.locator("#customers-m-city").evaluate((pop) => pop.showPopover());
      await page.locator("#customers-m-city input[value=Ludhiana]").check();
      await page.waitForURL((url) => url.searchParams.get("filter[city][in]") === "Ludhiana");
      await settled(page);
      await page.locator("#customers-m-city input[value=Amritsar]").check();
      await page.waitForURL((url) => new Set((url.searchParams.get("filter[city][in]") || "").split(",")).size === 2);
      await settled(page);
      assert.deepEqual(new Set(params(page).get("filter[city][in]").split(",")), new Set(["Ludhiana", "Amritsar"]));
      await page.keyboard.press("Escape");
      assert.equal(await rows(page).count(), 10, "five customers in each city");

      // Sorted by what they pay, biggest first: sort=-monthly_paise
      const header = page.locator("#customers thead th[data-col=monthly_paise]");
      await header.locator("[data-slot=sort-link]").click();
      await page.waitForURL((url) => url.searchParams.get("sort") === "monthly_paise");
      await settled(page);
      assert.equal(await header.getAttribute("aria-sort"), "ascending");
      await header.locator("[data-slot=sort-link]").click();
      await page.waitForURL((url) => url.searchParams.get("sort") === "-monthly_paise");
      await settled(page);
      assert.equal(await header.getAttribute("aria-sort"), "descending");
      assert.equal((await names(page))[0], "Manoj Sethi", "₹1,50,000 a month");

      // A search in the box above: search=...
      await page.locator("#customers input[name=search]").fill("Amit");
      await page.waitForURL((url) => url.searchParams.get("search") === "Amit");
      await settled(page);
      assert.deepEqual(await names(page), ["Amit Khanna"]);

      // One grammar: none of the names the table used to have.
      const keys = [...params(page).keys()];
      for (const old of ["q", "per", "after"]) assert.ok(!keys.includes(old), `${old} is not in ${page.url()}`);
      assert.ok(!keys.some((key) => key.startsWith("f.")), page.url());
      assert.ok(keys.includes("search") && keys.includes("sort") && keys.includes("filter[city][in]"), keys.join());

      // Emptying the box and removing the filter's chip take them out of the address.
      await page.locator("#customers input[name=search]").fill("");
      await page.waitForURL((url) => !url.searchParams.has("search"));
      await settled(page);
      const chips = page.locator("#customers [data-slot=filter-chip]");
      assert.equal(await chips.count(), 1, "one chip for the one filter");
      assert.match(await chips.first().innerText(), /Ludhiana/);
      await chips.first().click();
      await page.waitForFunction(() => ![...new URLSearchParams(location.search).keys()].some((key) => key.startsWith("filter[")));
      await settled(page);
      assert.equal(await rows(page).count(), 10);
      await page.done();
    });

    test("a link with the grammar in it opens the same table, and the browser's back button goes back", async () => {
      const page = await signedIn("/customers");
      await page.goto(BASE + "/customers?filter%5Bplan%5D=scale&sort=-monthly_paise&per_page=20");
      await rows(page).first().waitFor();
      const scale = await names(page);
      assert.equal(scale[0], "Ashok Luthra", "₹2,50,000 a month");
      assert.equal(scale.length, 8, "eight customers on the Scale plan");
      await page.goBack();
      await page.waitForURL((url) => url.pathname === "/customers" && !url.searchParams.has("filter[plan]"));
      await page.done();
    });

    // ---- A customer ----------------------------------------------------------------------

    test("the form checks in the browser first, then the server, then adds the customer with a toast; the row is there", async () => {
      const page = await signedIn("/customers/new");
      await page.evaluate(() => { window.kept = true; });
      const add = page.getByRole("button", { name: "Add customer" });

      // The browser's own checks: nothing is sent.
      await add.click();
      assert.equal(await page.locator("input[name=name]").evaluate((el) => el.validity.valueMissing), true, "a name is required");
      assert.equal(new URL(page.url()).pathname, "/customers/new");
      await page.fill("input[name=name]", "Test Person");
      await page.fill("input[name=email]", "not-an-email");
      await add.click();
      assert.equal(await page.locator("input[name=email]").evaluate((el) => el.validity.typeMismatch), true, "an email looks like one");
      assert.equal(new URL(page.url()).pathname, "/customers/new");

      // The server's check: an email another customer has. The page stays, with what was typed.
      const id = stamp();
      await fillCustomer(page, { name: `Test Person ${id}`, email: "rajesh.sharma@example.com" });
      await add.click();
      const error = page.locator("#customer-form [data-slot=field-error]");
      await error.first().waitFor();
      assert.equal(await error.count(), 1, "one mistake, one message");
      assert.match(await error.first().innerText(), /A customer with this email already exists\./);
      assert.equal(await page.evaluate(() => window.kept), true, "the page did not reload");
      assert.equal(await page.inputValue("input[name=name]"), `Test Person ${id}`, "what was typed stays");

      // A good one: a toast, and the table.
      await page.fill("input[name=email]", `test.person.${id}@example.com`);
      await add.click();
      await page.waitForURL((url) => url.pathname === "/customers");
      await toast(page, `Test Person ${id} was added.`).first().waitFor({ timeout: 8000 });
      assert.equal(await page.evaluate(() => window.kept), true, "no reload: the shell stayed");
      await page.goto(BASE + `/customers?search=${id}`);
      await rows(page).first().waitFor();
      assert.deepEqual(await names(page), [`Test Person ${id}`]);
      assert.match(await page.locator("#customers").innerText(), /₹24,999/);
      await page.done();
    });

    test("a customer is edited from its page, and saved with a toast", async () => {
      const id = stamp();
      const page = await signedIn("/customers/new");
      await fillCustomer(page, { name: `Test Editor ${id}`, email: `test.editor.${id}@example.com`, notes: "Prefers calls in the morning." });
      await page.getByRole("button", { name: "Add customer" }).click();
      await page.waitForURL((url) => url.pathname === "/customers");
      await page.goto(BASE + `/customers?search=${id}`);
      await page.locator("#customers a", { hasText: `Test Editor ${id}` }).first().click();
      await page.waitForURL((url) => /^\/customers\/[0-9a-f-]{36}$/.test(url.pathname));
      assert.match(await page.locator("main").innerText(), /Prefers calls in the morning\./);
      await page.getByRole("link", { name: "Edit" }).click();
      await page.waitForURL((url) => url.pathname.endsWith("/edit"));
      assert.equal(await page.inputValue("input[name=city]"), "Patiala", "the form is filled in");
      await page.fill("input[name=city]", "Bathinda");
      await page.getByRole("button", { name: "Save changes" }).click();
      await page.waitForURL((url) => /^\/customers\/[0-9a-f-]{36}$/.test(url.pathname));
      await toast(page, `Test Editor ${id} was saved.`).first().waitFor({ timeout: 8000 });
      assert.match(await page.locator("main").innerText(), /Bathinda/);
      await page.done();
    });

    // ---- Live ----------------------------------------------------------------------------

    test("a customer added in a second window reaches the first window's table, by itself", async () => {
      const id = stamp();
      const first = await signedIn(`/customers?search=live.${id}`);
      await live(first, "customers");
      assert.equal(await rows(first).count(), 0, "nothing matches yet");
      await first.evaluate(() => { window.kept = true; });

      // The home page's figures follow the same stream.
      const home = await signedIn("/", await visitor());
      await live(home, "figures");

      const second = await signedIn("/customers/new", await visitor());
      await fillCustomer(second, { name: `Live ${id}`, email: `live.${id}@example.com` });
      await second.getByRole("button", { name: "Add customer" }).click();
      await second.waitForURL((url) => url.pathname === "/customers");

      await until(first, () => document.querySelectorAll("#customers tbody tr[data-key]").length === 1, null, "the new customer in the first window's table");
      assert.deepEqual(await names(first), [`Live ${id}`]);
      assert.equal(await first.evaluate(() => window.kept), true, "the first window never reloaded");
      await first.done();
      await second.done();
    });

    // ---- Workspaces ----------------------------------------------------------------------

    test("the account's second workspace has its own customers, and the switcher moves between them", async () => {
      const page = await signedIn("/customers");
      await rows(page).first().waitFor();
      // The sidebar names the workspace the browser is in: the first one it belongs to.
      assert.match(await page.locator("[data-slot=sidebar-header]").innerText(), /Ludhiana desk/);
      await page.goto(BASE + "/customers?search=Rajesh");
      await rows(page).first().waitFor();
      assert.deepEqual(await names(page), ["Rajesh Sharma"]);

      // Choosing the other workspace in the switcher loads the page again inside it.
      await page.locator("#ui-workspace-switcher-trigger").click();
      await Promise.all([
        page.waitForResponse((r) => r.request().isNavigationRequest() && new URL(r.url()).pathname === "/customers"),
        page.locator("#ui-workspace-switcher").getByRole("button", { name: "Jalandhar desk" }).click(),
      ]);
      await page.waitForLoadState("domcontentloaded");
      assert.match(await page.locator("[data-slot=sidebar-header]").innerText(), /Jalandhar desk/);
      assert.equal(await rows(page).count(), 0, "Rajesh Sharma is a customer of the first workspace only");
      await page.goto(BASE + "/customers?per_page=40");
      await rows(page).first().waitFor();
      assert.equal(await rows(page).count(), 10, "the ten customers of Jalandhar and Amritsar");
      assert.deepEqual(new Set(await rows(page).locator("td[data-col=city]").allInnerTexts().then((cities) => cities.map((city) => city.trim()))), new Set(["Jalandhar", "Amritsar"]));

      // The choice stays with this browser: another window starts in the first workspace.
      const other = await signedIn("/customers?per_page=40", await visitor());
      await rows(other).first().waitFor();
      assert.equal(await rows(other).count(), 40);
      await page.done();
      await other.done();
    });

    // ---- Signing out ---------------------------------------------------------------------

    test("signing out asks for sign-in again, for every page that needs it", async () => {
      const page = await signedIn("/customers");
      await page.locator("[data-slot=sidebar-user]").click();
      await page.getByRole("button", { name: "Sign out" }).click();
      await page.waitForURL((url) => url.pathname === "/sign-in");
      await page.goto(BASE + "/customers");
      assert.equal(new URL(page.url()).pathname, "/sign-in");
      assert.equal(params(page).get("next"), "/customers");
      await page.goto(BASE + "/customers/new");
      assert.equal(new URL(page.url()).pathname, "/sign-in");
      await page.done();
    });

    // ---- Every page ------------------------------------------------------------------------

    for (const width of [1440, 390]) {
      test(`every page is axe clean and does not scroll sideways (${width} px)`, async () => {
        const context = await visitor({ viewport: { width, height: 900 } });
        const guest = await pageOf(context);
        await guest.goto(BASE + "/sign-in");
        assert.deepEqual(await axe(guest), [], "/sign-in");
        assert.equal(await guest.evaluate(() => document.documentElement.scrollWidth > window.innerWidth), false, "/sign-in scrolls sideways");
        await guest.done();

        const page = await signedIn("/", context);
        const paths = ["/", "/customers", "/customers?filter%5Bcity%5D%5Bin%5D=Delhi&sort=-monthly_paise", "/customers/new", "/profile", "/customers/00000000-0000-7000-8000-000000000000"];
        for (const path of paths) {
          await page.goto(BASE + path);
          assert.deepEqual(await axe(page), [], path);
          assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > window.innerWidth), false, `${path} scrolls sideways`);
        }
        // A customer's own page and its form.
        await page.goto(BASE + "/customers");
        const link = await page.locator("#customers tbody a").first().getAttribute("href");
        for (const path of [link, link + "/edit"]) {
          await page.goto(BASE + path);
          assert.deepEqual(await axe(page), [], path);
        }
        await page.done();
      });
    }

    test("the pages work with JavaScript off: sign in, the table, a filter in the address, and sign out", async () => {
      const context = await visitor({ javaScriptEnabled: false });
      const page = await context.newPage();
      await page.goto(BASE + "/customers?filter%5Bplan%5D=scale");
      assert.equal(new URL(page.url()).pathname, "/sign-in");
      await page.fill("input[name=email]", EMAIL);
      await page.fill("input[name=password]", "not-the-password");
      await page.getByRole("button", { name: "Sign in" }).click();
      await page.locator("[data-slot=field-error]").first().waitFor();
      assert.equal(await page.inputValue("input[name=email]"), EMAIL, "the page comes back with what was typed");
      assert.equal(await page.inputValue("input[name=password]"), "", "and never with the password");
      await page.fill("input[name=password]", PASSWORD);
      await page.getByRole("button", { name: "Sign in" }).click();
      await page.waitForURL((url) => url.pathname === "/customers");
      assert.equal(params(page).get("filter[plan]"), "scale");
      assert.equal(await rows(page).count(), 8, "eight customers on the Scale plan");
      await page.goto(BASE + "/customers/new");
      const id = stamp();
      await fillCustomer(page, { name: `Plain ${id}`, email: `plain.${id}@example.com` });
      await page.getByRole("button", { name: "Add customer" }).click();
      await page.waitForURL((url) => url.pathname === "/customers");
      assert.match(await page.locator("[data-slot=toaster] [data-slot=toast]").textContent(), /was added\./, "the toast comes through the redirect");
      // The user menu is a native popover, and its button posts a plain form.
      await page.locator("[data-slot=sidebar-user]").click();
      await page.getByRole("button", { name: "Sign out" }).click();
      await page.waitForURL((url) => url.pathname === "/sign-in");
    });
  });
}
