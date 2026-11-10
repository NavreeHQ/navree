// Islands in real browsers, against the proof app's /islands page: a part of the page
// drawn by a script of the app's own, mounted once with the server's props, kept as it
// drew it through a morph, given new props in place, let go of when it leaves the page;
// with JavaScript off, the server's fallback. Run with the proof app serving:
//
//   cargo run -p ui-proof -- serve          # UI_PROOF_URL, default http://127.0.0.1:8090
//   UI_BROWSERS=chromium,firefox,webkit npm test -- tests/islands.test.mjs

import assert from "node:assert/strict";
import { after, before, describe, test } from "node:test";
import { chromium, firefox, webkit } from "playwright";
import { launch } from "./support/launch.mjs";

const BASE = process.env.UI_PROOF_URL || "http://127.0.0.1:8090";
const ENGINES = { chromium, firefox, webkit };
const NAMES = (process.env.UI_BROWSERS || "chromium,firefox").split(",").filter((n) => ENGINES[n]);

for (const name of NAMES) {
  describe(name, { concurrency: false }, () => {
    let browser;
    before(async () => {
      browser = await launch(ENGINES[name]);
    });
    after(() => browser.close());

    /** A page on the proof app; `close` asserts no console error and no policy report. */
    async function open(path, options = {}) {
      const context = await browser.newContext({ viewport: { width: 1280, height: 800 }, ...options });
      const page = await context.newPage();
      const problems = [];
      const requests = [];
      page.on("console", (m) => m.type() === "error" && problems.push(m.text()));
      page.on("pageerror", (e) => problems.push("pageerror: " + e.message));
      page.on("request", (r) => requests.push(r.url()));
      await page.addInitScript(() => {
        document.addEventListener("securitypolicyviolation", (e) => (window.__csp ||= []).push(`${e.violatedDirective} ${e.blockedURI}`));
      });
      await page.goto(BASE + path);
      const close = async () => {
        const csp = options.javaScriptEnabled === false ? [] : await page.evaluate(() => window.__csp || []);
        await context.close();
        assert.deepEqual(csp, [], "no content security policy report");
        assert.deepEqual(problems, []);
      };
      return { page, requests, close };
    }
    /** What the island's script has done: mounts, updates and leaves. */
    const seen = (page) => page.evaluate(() => ({ mounts: 0, updates: 0, leaves: 0, ...window.__islands }));
    const mounted = (page) => page.locator("#tally[data-ui-mounted]").waitFor();

    test("an island mounts once with the server's props, by its script's hash, and its own button counts on the page", async () => {
      const { page, requests, close } = await open("/islands");
      await mounted(page);
      assert.deepEqual(await seen(page), { mounts: 1, updates: 0, leaves: 0 });
      assert.equal(await page.textContent("#tally [data-tally-city]"), "Parcels packed in Amritsar: ");
      assert.equal(await page.textContent("#tally [data-tally-count]"), "124");
      assert.equal(await page.locator("#tally [data-tally-fallback]").count(), 0, "the script drew over the fallback");
      await page.locator("#tally [data-tally-more]").click();
      assert.equal(await page.textContent("#tally [data-tally-count]"), "125");
      assert.ok(requests.some((url) => /\/_navree\/ui\/island\/tally-[0-9a-f]{12}\.js$/.test(url)), "its script, by its hash");
      assert.ok(requests.some((url) => /\/_navree\/ui\/islands-[0-9a-f]{12}\.js$/.test(url)), "the islands module");
      // Markup that names another site's script: nothing is asked of it, and it stays as drawn.
      assert.ok(!requests.some((url) => url.includes("attacker.example")));
      assert.equal(await page.getAttribute("#forged", "data-ui-mounted"), null);
      assert.equal(await page.getAttribute("#tally", "role"), "region");
      assert.equal(await page.getAttribute("#tally", "aria-label"), "Parcels packed today");
      await close();
    });

    test("a morph leaves the island as its script drew it, and new props reach it in place", async () => {
      const { page, close } = await open("/islands");
      await mounted(page);
      await page.locator("#tally [data-tally-more]").click();
      // The region drawn again by the server, as a live answer draws it: the same props.
      await page.evaluate(
        () =>
          new Promise((resolve) => {
            document.addEventListener("ui:swap", resolve, { once: true });
            document.getElementById("depot").dispatchEvent(new CustomEvent("ui:refresh", { bubbles: true, detail: {} }));
          }),
      );
      assert.equal(await page.textContent("#tally [data-tally-count]"), "125", "what the script drew, and the reader did, stays");
      assert.deepEqual(await seen(page), { mounts: 1, updates: 0, leaves: 0 }, "the same props: nothing to do");
      assert.equal(await page.getAttribute("#tally", "data-ui-mounted"), "", "still mounted after the morph");
      // A link in the region: the server draws the island for another city.
      await page.click("#depot-jalandhar");
      await page.waitForFunction(() => document.querySelector("#tally [data-tally-city]")?.textContent.includes("Jalandhar"));
      assert.deepEqual(await seen(page), { mounts: 1, updates: 1, leaves: 0 });
      assert.equal(await page.textContent("#tally [data-tally-count]"), "125", "the count is the island's own");
      assert.match(page.url(), /\?city=Jalandhar$/);
      await close();
    });

    test("an island that leaves the page is let go of, once", async () => {
      const { page, close } = await open("/islands");
      await mounted(page);
      // A boosted link to a page of the same build with no island: the body is morphed and the island goes.
      await page.click("#away");
      await page.waitForFunction(() => window.__islands?.leaves === 1);
      assert.deepEqual(await seen(page), { mounts: 1, updates: 0, leaves: 1 });
      assert.equal(await page.locator("#tally").count(), 0);
      // Back to the island: a new one, mounted again.
      await page.goBack();
      await page.waitForFunction(() => window.__islands?.mounts === 2);
      await mounted(page);
      await close();
    });

    test("with JavaScript off the island is the server's fallback", async () => {
      const { page, close } = await open("/islands", { javaScriptEnabled: false });
      assert.equal(await page.textContent("#tally [data-tally-fallback]"), "Parcels packed in Amritsar: 124");
      assert.equal(await page.getAttribute("#tally", "data-ui-mounted"), null);
      await close();
    });
  });
}
