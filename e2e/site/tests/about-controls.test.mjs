// The About page's live picture and its controls, in Chromium: pause and play,
// the paused choice surviving a reload without loading the scene, the opening
// played once per visit, JavaScript off showing the still, and leaving the
// page freeing the canvas.
//
// The site is the built export in apps/website/out, served here by
// apps/website/scripts/serve.ts on a free port (tests/support/serve.mjs).
// SITE_URL points at a server that is already running instead. The live picture
// draws with WebGL: headless Chromium gets software WebGL, and
// ABOUT_CHANNEL=chrome ABOUT_HEADED=1 runs the installed Chrome with a window
// and the real GPU, which is where the main-thread measurement is calibrated.

import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { after, before, describe, test } from "node:test";
import { chromium } from "playwright";
import { serve } from "./support/serve.mjs";

const SHOTS = process.env.ABOUT_SHOTS || "";
const PAUSE = 'figure button[aria-label$="the animation"]';
const PICTURE = 'figure [class~="group/picture"]';

describe("the About picture", { concurrency: false }, () => {
  let site;
  let browser;
  let context;
  let page;
  let button;
  const sceneRequests = [];
  let pausedMs;
  let playingMs;

  const live = () => page.evaluate((selector) => document.querySelector(selector)?.hasAttribute("data-live") ?? false, PICTURE);
  const starts = () => page.waitForFunction((selector) => document.querySelector(selector)?.hasAttribute("data-live"), PICTURE, { timeout: 30_000 });
  /** Main-thread work over two seconds, in milliseconds per second. */
  const busy = async (cdp) => {
    const task = async () => (await cdp.send("Performance.getMetrics")).metrics.find((m) => m.name === "TaskDuration").value;
    const start = await task();
    await page.waitForTimeout(2000);
    return Math.round((((await task()) - start) * 1000) / 2 * 10) / 10;
  };

  before(async () => {
    site = await serve();
    site.url = `${site.origin}/about/`;
    const headed = process.env.ABOUT_HEADED === "1";
    browser = await chromium.launch({
      channel: process.env.ABOUT_CHANNEL || undefined,
      headless: !headed,
      args: headed ? ["--start-maximized"] : ["--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--ignore-gpu-blocklist"],
    });
    context = await browser.newContext(headed ? { viewport: null } : { viewport: { width: 1440, height: 900 } });
    page = await context.newPage();
    page.on("request", (r) => r.url().includes("/_next/static/chunks/") && sceneRequests.push(r.url()));
    button = page.locator(PAUSE);
  });

  after(async () => {
    await browser?.close();
    site?.stop();
  });

  test("the live picture starts, hidden from assistive technology, with the still and its description kept in the page", async () => {
    await page.goto(site.url, { waitUntil: "load" });
    await starts();
    assert.equal(await live(), true);
    assert.equal(await page.locator("figure canvas").evaluate((c) => c.parentElement.getAttribute("aria-hidden")), "true");
    assert.ok((await page.locator("figure img[data-about-picture]").getAttribute("alt")).length > 200, "the still has a description");
  });

  test("the opening is marked as seen for this visit and a pause button is offered", async () => {
    await page.waitForTimeout(3500);
    assert.equal(await page.evaluate(() => sessionStorage.getItem("navree:about-opening")), "1");
    assert.equal(await button.getAttribute("aria-label"), "Pause the animation");
  });

  test("pressing the button offers play and the choice is remembered", async () => {
    await button.click();
    assert.equal(await button.getAttribute("aria-label"), "Play the animation");
    assert.equal(await page.evaluate(() => localStorage.getItem("navree:about-motion")), "paused");
  });

  test("main-thread work, paused and playing", async () => {
    const cdp = await context.newCDPSession(page);
    await cdp.send("Performance.enable");
    // The click, the button's re-render and a garbage collection settle within two seconds; then measure.
    await page.waitForTimeout(2000);
    pausedMs = await busy(cdp);
    await button.click();
    playingMs = await busy(cdp);
    await button.click();
    await cdp.detach();
  });

  let chunksBefore = 0;
  let baselineMs = 0;
  test("after a reload a paused visitor gets the still and a play button, and the scene is not loaded", async () => {
    chunksBefore = sceneRequests.length;
    await page.reload({ waitUntil: "load" });
    await page.waitForTimeout(3000);
    assert.equal(await live(), false, "a paused visitor must not get the live picture");
    assert.equal(await button.getAttribute("aria-label"), "Play the animation");
    // The same page with the scene never loaded: what is left is the page's own motion.
    const cdp = await context.newCDPSession(page);
    await cdp.send("Performance.enable");
    baselineMs = await busy(cdp);
    await cdp.detach();
  });

  test("paused, the picture stops drawing", () => {
    assert.ok(
      pausedMs <= baselineMs + 3 && playingMs > pausedMs + 8,
      `main thread ${pausedMs} ms/s paused, ${playingMs} ms/s playing, ${baselineMs} ms/s with no scene loaded`,
    );
  });

  test("play loads and starts the live picture", async () => {
    await button.click();
    await starts();
    assert.equal(await live(), true);
    assert.ok(sceneRequests.length > chunksBefore, "play fetches the scene's chunks");
    await page.evaluate(() => localStorage.removeItem("navree:about-motion"));
  });

  test("navigating away removes the picture", async () => {
    await page.click('a[href="/"]');
    await page.waitForURL((u) => u.pathname === "/");
    assert.equal(await page.locator("figure canvas").count(), 0);
  });

  test("without JavaScript the still shows, with real Navree code on its screen", async () => {
    const noJs = await browser.newContext({ javaScriptEnabled: false, viewport: { width: 1440, height: 900 } });
    const p2 = await noJs.newPage();
    await p2.goto(site.url, { waitUntil: "load" });
    const still = await p2.evaluate(() => {
      const img = document.querySelector("figure img[data-about-picture]");
      const code = [...document.querySelectorAll("figure svg text")].map((t) => t.textContent).join("\n");
      return { loaded: img.complete && img.naturalWidth > 0, code };
    });
    assert.equal(still.loaded, true, "the still loads");
    assert.ok(still.code.includes("#[controller") && still.code.includes("DealsModule"), "the still shows Navree code");
    if (SHOTS) {
      fs.mkdirSync(SHOTS, { recursive: true });
      await p2.locator("figure").first().screenshot({ path: path.join(SHOTS, "about-still-no-javascript-1440.png") });
    }
    await noJs.close();
  });
});
