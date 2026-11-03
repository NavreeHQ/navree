// The docs' icon and flag galleries filter as someone types (`apps/ui-docs/frame/preview.js`).
// This runs the very documents the website serves, from `apps/website/content/ui/previews`,
// with the toolkit's committed stylesheet, so it needs no other server. What typing does:
// the cells that do not match go away and the count says how many are left; what is typed
// is only ever text, so markup in the box injects nothing and no character breaks the filter.
//
// GALLERY_BROWSERS="chromium firefox webkit" picks browsers.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createServer } from "node:http";
import { dirname, join } from "node:path";
import { after, before, describe, test } from "node:test";
import { fileURLToPath } from "node:url";
import * as playwright from "playwright";

const root = join(dirname(fileURLToPath(import.meta.url)), "../../..");
const previews = join(root, "apps/website/content/ui/previews");
const dist = join(root, "crates/navree-ui/dist");
const font = join(root, "crates/navree-ui/assets/fonts/geist.woff2");

const TYPES = { ".html": "text/html; charset=utf-8", ".css": "text/css", ".js": "text/javascript", ".woff2": "font/woff2" };

/** The preview documents as the site serves them: `../_toolkit/name-<hash>.ext` is `dist/name.ext`. */
function serve() {
  return createServer((request, response) => {
    const url = new URL(request.url, "http://x");
    const path = decodeURIComponent(url.pathname);
    let file = null;
    const toolkit = /^\/previews\/_toolkit\/(.+)-[0-9a-f]{12}\.(css|js|woff2)$/.exec(path);
    if (toolkit) file = toolkit[1] === "geist" ? font : join(dist, `${toolkit[1]}.${toolkit[2]}`);
    else if (path.startsWith("/previews/") && !path.includes("..")) file = join(previews, path.slice("/previews/".length));
    try {
      const body = readFileSync(file);
      const extension = file.slice(file.lastIndexOf("."));
      response.writeHead(200, { "content-type": TYPES[extension] ?? "application/octet-stream" });
      response.end(body);
    } catch {
      response.writeHead(404);
      response.end("not found");
    }
  });
}

const browsers = (process.env.GALLERY_BROWSERS || "chromium firefox").split(" ");

for (const browserName of browsers) {
  describe(browserName, () => {
    let browser;
    let server;
    let base;
    before(async () => {
      browser = await playwright[browserName].launch();
      server = serve();
      await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
      base = `http://127.0.0.1:${server.address().port}`;
    });
    after(async () => {
      await browser?.close();
      server?.close();
    });

    async function open(document, { js = true } = {}) {
      const context = await browser.newContext({ viewport: { width: 1000, height: 700 }, javaScriptEnabled: js });
      const page = await context.newPage();
      const problems = [];
      page.on("pageerror", (error) => problems.push(String(error)));
      page.on("console", (message) => {
        if (browserName === "webkit" && /Refused to apply a stylesheet/.test(message.text())) return;
        if (message.type() === "error" && !/Failed to load resource/.test(message.text())) problems.push(message.text());
      });
      await page.goto(`${base}/previews/${document}`);
      return { page, problems, close: () => context.close() };
    }

    /** Types into the box as a person does, then waits for the frame the filter runs in. */
    async function type(page, text) {
      await page.fill("form[data-preview-search] input", text);
      await page.evaluate(() => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))));
    }

    const visible = (page) => page.locator("[data-preview-cell]:not([hidden])").evaluateAll((cells) => cells.map((cell) => cell.querySelector("figcaption span")?.textContent ?? cell.querySelector("figcaption").textContent));
    const count = (page) => page.locator("[data-preview-count]").innerText();
    const total = (page) => page.locator("[data-preview-cell]").count();

    test("icons: the whole set to begin with, and the count says so", async () => {
      const { page, problems, close } = await open("icons/all.html");
      const all = await total(page);
      assert.ok(all > 2000, `${all}`);
      assert.equal((await visible(page)).length, all);
      assert.equal(await count(page), `${all} icons`);
      assert.equal(await page.locator("[data-preview-empty]").isVisible(), false);
      assert.deepEqual(problems, []);
      await close();
    });

    test("icons: a word keeps what matches a name or a tag and hides the rest", async () => {
      const { page, close } = await open("icons/all.html");
      const all = await total(page);
      await type(page, "mail");
      const shown = await visible(page);
      assert.ok(shown.includes("mail-open") && shown.includes("mail"), shown.slice(0, 10).join());
      assert.ok(!shown.includes("trash"), "trash is not a mail");
      assert.ok(shown.length > 5 && shown.length < 100, `${shown.length}`);
      assert.equal(await count(page), `${shown.length} of ${all} icons`);
      // A tag, not a name.
      await type(page, "rubbish");
      assert.ok((await visible(page)).includes("trash"));
      await type(page, "email");
      assert.ok((await visible(page)).includes("mail-open"));
      // Capitals and several words: every word must match.
      await type(page, "MAIL  Open");
      const both = await visible(page);
      assert.ok(both.includes("mail-open"), both.join());
      assert.ok(!both.includes("mail") && !both.includes("trash"), "every word must match, not any");
      // The constant's spelling finds it too.
      await type(page, "MAIL_OPEN");
      assert.deepEqual(await visible(page), ["mail-open"]);
      await close();
    });

    test("icons: nothing matches, then everything comes back", async () => {
      const { page, close } = await open("icons/all.html");
      const all = await total(page);
      await type(page, "zzzzqqqq");
      assert.deepEqual(await visible(page), []);
      assert.equal(await count(page), `0 of ${all} icons`);
      assert.equal(await page.locator("[data-preview-empty]").isVisible(), true);
      await type(page, "");
      assert.equal((await visible(page)).length, all);
      assert.equal(await count(page), `${all} icons`);
      assert.equal(await page.locator("[data-preview-empty]").isVisible(), false);
      await close();
    });

    test("fast typing ends on the right answer", async () => {
      const { page, close } = await open("icons/all.html");
      await page.locator("form[data-preview-search] input").pressSequentially("trash", { delay: 5 });
      await page.evaluate(() => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))));
      const shown = await visible(page);
      assert.ok(shown.includes("trash"), shown.join());
      assert.ok(!shown.includes("mail-open"));
      assert.equal(await count(page), `${shown.length} of ${await total(page)} icons`);
      await close();
    });

    test("typed markup is text: it injects nothing, and no character breaks the filter", async () => {
      const { page, problems, close } = await open("icons/all.html");
      const before = await page.evaluate(() => ({
        scripts: document.scripts.length,
        images: document.images.length,
        nodes: document.getElementsByTagName("*").length,
        terms: [...document.querySelectorAll("[data-preview-cell]")].map((cell) => cell.dataset.terms).join("|").length,
      }));
      const hostile = [
        `<img src=x onerror="window.__pwn=1">`,
        `"><script>window.__pwn=1</script>`,
        `<svg onload="window.__pwn=1"></svg>`,
        `</figure><script>window.__pwn=1</script>`,
        `&lt;img src=x&gt; &#60;script&#62;`,
        `javascript:window.__pwn=1`,
        `{{7*7}} \${7*7} %s %n`,
        `(`, `[`, `\\`, `.*`, `+`, `?`, `^$`, `(?<x>a)`, `*`,
        `\u0000`, "‮", "😀".repeat(500), "a".repeat(100_000),
      ];
      for (const text of hostile) {
        await type(page, text);
        const after = await page.evaluate(() => ({
          scripts: document.scripts.length,
          images: document.images.length,
          nodes: document.getElementsByTagName("*").length,
          pwn: window.__pwn,
          counted: document.querySelector("[data-preview-count]").textContent,
        }));
        assert.equal(after.pwn, undefined, `${text.slice(0, 30)} ran something`);
        assert.equal(after.scripts, before.scripts, `${text.slice(0, 30)} added a script`);
        assert.equal(after.images, before.images, `${text.slice(0, 30)} added an image`);
        assert.equal(after.nodes, before.nodes, `${text.slice(0, 30)} added an element`);
        assert.match(after.counted, /^(\d+ icons|\d+ of \d+ icons)$/, text.slice(0, 30));
      }
      // The words the markup is made of are only words: a real tag still matches itself.
      // A word that is in a tag is a word like any other.
      await type(page, "script");
      assert.ok((await visible(page)).includes("scroll-text"));
      const terms = await page.evaluate(() => [...document.querySelectorAll("[data-preview-cell]")].map((cell) => cell.dataset.terms).join("|").length);
      assert.equal(terms, before.terms, "typing changed what the cells hold");
      // The markup typed is in the box as typed, and nowhere else.
      await type(page, `<img src=x onerror="window.__pwn=1">`);
      assert.equal(await page.locator("form[data-preview-search] input").inputValue(), `<img src=x onerror="window.__pwn=1">`);
      assert.equal(await page.evaluate(() => document.body.innerHTML.includes("onerror")), false);
      assert.equal(await page.locator("[data-preview-empty]").isVisible(), true);
      assert.deepEqual(problems, []);
      await close();
    });

    test("Enter in the box searches and goes nowhere", async () => {
      const { page, close } = await open("icons/all.html");
      const url = page.url();
      await page.evaluate(() => {
        window.__stay = 1;
      });
      await page.locator("form[data-preview-search] input").fill("mail");
      await page.keyboard.press("Enter");
      await page.evaluate(() => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))));
      assert.equal(page.url(), url);
      assert.equal(await page.evaluate(() => window.__stay), 1, "the page was reloaded");
      await close();
    });

    test("flags: by country, by code and by kind", async () => {
      const { page, problems, close } = await open("flags/all.html");
      const all = await total(page);
      assert.equal(all, 270);
      await type(page, "united");
      assert.deepEqual((await visible(page)).sort(), ["AE", "GB", "UN", "US"]);
      assert.equal(await count(page), "4 of 270 flags");
      await type(page, "scot");
      assert.deepEqual(await visible(page), ["GB-SCT"]);
      await type(page, "gb-");
      assert.deepEqual((await visible(page)).sort(), ["GB-ENG", "GB-NIR", "GB-SCT", "GB-WLS"]);
      await type(page, "region");
      const regions = await visible(page);
      assert.ok(regions.includes("GB-SCT") && regions.includes("ES-CT") && !regions.includes("AE"), regions.join());
      await type(page, "organisation");
      assert.ok((await visible(page)).includes("EU"));
      await type(page, "AE");
      assert.ok((await visible(page)).includes("AE"));
      await type(page, "zzzz");
      assert.equal(await page.locator("[data-preview-empty]").isVisible(), true);
      assert.deepEqual(problems, []);
      await close();
    });

    test("flags: typed markup injects nothing here either", async () => {
      const { page, problems, close } = await open("flags/all.html");
      const scripts = await page.evaluate(() => document.scripts.length);
      for (const text of [`<img src=x onerror="window.__pwn=1">`, `"><script>window.__pwn=1</script>`, `[`, `\\`]) {
        await type(page, text);
        const result = await page.evaluate(() => ({ pwn: window.__pwn, images: document.images.length, scripts: document.scripts.length }));
        assert.equal(result.pwn, undefined);
        assert.equal(result.images, 0);
        assert.equal(result.scripts, scripts, "no script was added");
      }
      assert.deepEqual(problems, []);
      await close();
    });

    test("without JavaScript the whole gallery is there, unfiltered", async () => {
      for (const document of ["icons/all.html", "flags/all.html"]) {
        const { page, close } = await open(document, { js: false });
        const cells = await page.locator("[data-preview-cell]").count();
        const hidden = await page.locator("[data-preview-cell][hidden]").count();
        assert.ok(cells >= 270, `${document}: ${cells}`);
        assert.equal(hidden, 0, document);
        await close();
      }
    });
  });
}
