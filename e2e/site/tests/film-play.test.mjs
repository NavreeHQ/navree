// The homepage film's play button, pressed before the page's scripts have
// loaded. The press used to do nothing: the button is server-rendered but its
// handler arrives with the scripts. Now an inline script after the player notes
// the press and the player acts on it once it is hydrated.
//
// The test holds every script of the page back, loads the page, presses the
// button, and only then lets the scripts through. The film must then be chosen
// (a source set on the video) or, where this browser can play none of the
// files, the failure must be shown: either way the press was heard. Nothing is
// played, so the test needs no codec.

import assert from "node:assert/strict";
import { after, before, describe, test } from "node:test";
import { chromium } from "playwright";
import { serve } from "./support/serve.mjs";

describe("the homepage film", { concurrency: false }, () => {
  let site;
  let browser;

  before(async () => {
    site = await serve();
    browser = await chromium.launch();
  });

  after(async () => {
    await browser?.close();
    site?.stop();
  });

  /** Loads the page with its scripts held back; returns the page and a function that lets them through. */
  async function loadWithoutScripts(context) {
    const page = await context.newPage();
    const held = [];
    await page.route("**/_next/static/**/*.js", (route) => held.push(route));
    await page.goto(`${site.origin}/`, { waitUntil: "domcontentloaded" });
    return { page, release: () => Promise.all(held.map((route) => route.continue())) };
  }

  test("a press before the scripts load starts the film once they do", async () => {
    const context = await browser.newContext({ viewport: { width: 1440, height: 900 } });
    const { page, release } = await loadWithoutScripts(context);
    const button = page.locator("[data-film-play]");
    await button.waitFor({ state: "visible" });
    assert.equal(await page.locator("video[aria-label]").first().getAttribute("src"), null, "no film is loaded before the press");

    await button.click();
    // Nothing has run yet: the button is still the server's.
    assert.equal(await button.count(), 1);

    await release();
    // The player hydrates, hears the press and either picks a file or reports that none plays.
    await page.waitForFunction(
      () => {
        const video = document.querySelector("video[aria-label]");
        return Boolean(video?.getAttribute("src")) || Boolean(document.querySelector('[role="alert"]'));
      },
      null,
      { timeout: 30_000 },
    );
    const src = await page.locator("video[aria-label]").first().getAttribute("src");
    if (src) assert.match(src, /^\/media\/navree-launch-.+\.(webm|mp4)$/, "the film is one of the launch film's files");
    await context.close();
  });

  test("without a press, nothing of the film is loaded", async () => {
    const context = await browser.newContext({ viewport: { width: 1440, height: 900 } });
    const { page, release } = await loadWithoutScripts(context);
    await page.locator("[data-film-play]").waitFor({ state: "visible" });
    await release();
    await page.waitForLoadState("load");
    await page.waitForTimeout(1500);
    assert.equal(await page.locator("video[aria-label]").first().getAttribute("src"), null);
    assert.equal(await page.locator("[data-film-play]").count(), 1, "the poster and its button are still there");
    await context.close();
  });
});
