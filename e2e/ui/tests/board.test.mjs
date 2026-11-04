// The board example app in real browsers: cards moved by mouse, touch and keys,
// every step announced, refusals put back, limits held, two viewers kept in step
// over a server-sent event stream, columns of thousands loaded a page at a time,
// JavaScript off, 390 and 1440 px, dark and light, axe. Needs the app serving with
// its teammate off and its test hooks on:
//
//   BOARD_SIMULATOR=off BOARD_TEST_HOOKS=1 cargo run --release -p board-demo
//
// BOARD_URL points elsewhere (default http://127.0.0.1:8097); BOARD_BROWSERS picks
// engines (default "chromium,firefox,webkit"); BOARD_SHOTS=<dir> saves screenshots.
// Without the app the suite is skipped.

import assert from "node:assert/strict";
import { mkdirSync, readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import { after, afterEach, before, beforeEach, describe, test } from "node:test";
import { fileURLToPath } from "node:url";
import { chromium, firefox, webkit } from "playwright";

const BASE = process.env.BASE_URL_BOARD || process.env.BOARD_URL || "http://127.0.0.1:8097";
const require = createRequire(import.meta.url);
const axeSource = readFileSync(require.resolve("axe-core/axe.min.js"), "utf8");
const ENGINES = { chromium, firefox, webkit };
const NAMES = (process.env.BOARD_BROWSERS || "chromium,firefox,webkit").split(",").filter((n) => ENGINES[n]);
const SHOTS = process.env.BOARD_SHOTS;
if (SHOTS) mkdirSync(SHOTS, { recursive: true });

const here = dirname(fileURLToPath(import.meta.url));
const tokensRs = readFileSync(join(here, "../../../crates/navree-ui/src/tokens.rs"), "utf8");
const LIGHT =
  ":root{color-scheme:light" +
  [...tokensRs.matchAll(/t\("([a-z0-9-]+)",\s*"(#[0-9a-f]{6})",\s*"(#[0-9a-f]{6})"\)/g)].map((m) => `;--${m[1]}:${m[3]}`).join("") +
  "}body{background:var(--ink-0)}";
const ROUND = ["timeline-dot", "live-dot", "badge-dot", "activity-icon", "radio", "spinner", "avatar"];

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const post = (path) => fetch(BASE + path, { method: "POST" });
/** Where cards are on the server: `{4: ["ready", 0]}`. */
const where = async (...ids) => {
  const text = await (await fetch(`${BASE}/__test/where?ids=${ids.join(",")}`)).text();
  return Object.fromEntries(text.split(" ").map((part) => {
    const [id, list, at] = part.split(":");
    return [id, [list, Number(at)]];
  }));
};

let reachable = false;
try {
  reachable = (await post("/__test/reset")).status === 204;
} catch {}

async function axe(page) {
  await page.evaluate(axeSource);
  return page.evaluate(async () => {
    const out = await window.axe.run(document, { runOnly: ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa", "best-practice"] });
    return out.violations.map((v) => `${v.id}: ${v.nodes.slice(0, 3).map((n) => n.target.join(" ")).join(" | ")}`);
  });
}

/** The app rules: 2, 4 or 6 px corners (dots and icon circles aside), no shadow outside overlays, no style attribute but the runtime's. */
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
      if (shadows.length && !el.matches("dialog, [popover], [data-slot=toast], [data-slot=toast] *")) bad.push(`shadow on ${slot}`);
      const own = [...el.style].filter((p) => !p.startsWith("--"));
      if (el.hasAttribute("style") && !el.hasAttribute("popover") && own.length) bad.push(`style attribute on ${slot}: ${own.join(", ")}`);
      if (slot === "avatar" && el.textContent.trim()) bad.push(`text in an avatar: ${el.textContent}`);
    }
    return [...new Set(bad)];
  }, ROUND);

const sideways = (page) => page.evaluate(() => document.documentElement.scrollWidth > window.innerWidth);

for (const name of NAMES) {
  describe(`board app in ${name}`, { concurrency: false, skip: reachable ? false : `the app isn't serving at ${BASE}` }, () => {
    let browser;
    before(async () => {
      browser = await ENGINES[name].launch(name === "firefox" ? { firefoxUserPrefs: { "network.http.throttle.enable": false } } : {});
    });
    after(() => browser?.close());
    const contexts = [];
    afterEach(async () => {
      for (const context of contexts.splice(0)) await context.close().catch(() => {});
      await post("/__test/delay?ms=0");
    });
    beforeEach(async () => {
      assert.equal((await post("/__test/reset")).status, 204);
    });

    /** A page of the app. Console errors, page errors and policy reports are kept; `page.done()` asserts none. */
    async function open(path = "/", options = {}) {
      const { light, ...rest } = options;
      const context = await browser.newContext({ viewport: { width: 1440, height: 900 }, bypassCSP: !!light, ...rest });
      contexts.push(context);
      const page = await context.newPage();
      page.problems = [];
      page.on("console", (m) => {
        if (name === "webkit" && /Refused to apply a stylesheet/.test(m.text())) return;
        if (m.type() === "error" && !/Failed to load resource|ERR_ABORTED|NS_ERROR|Load failed|can.t establish a connection|status of 409/i.test(m.text())) page.problems.push(m.text());
      });
      page.on("pageerror", (e) => page.problems.push("pageerror: " + e.message));
      await page.addInitScript(() => {
        document.addEventListener("securitypolicyviolation", (e) => (window.__csp ||= []).push(e.violatedDirective + " " + e.blockedURI));
        // Every announcement of the board's live region, in order: a redraw from the server empties it.
        window.__said = [];
        new MutationObserver((records) => {
          for (const record of records) {
            const at = record.target.nodeType === 1 ? record.target : record.target.parentElement;
            // Through the prototype: the suite names form fields like the methods the page calls.
            const status = at instanceof Element ? Element.prototype.closest.call(at, "[data-slot=sortable-status]") : null;
            const text = status?.textContent.trim();
            if (text && window.__said.at(-1) !== text) window.__said.push(text);
          }
        }).observe(document, { subtree: true, childList: true, characterData: true });
      });
      await page.goto(BASE + path);
      if (rest.javaScriptEnabled !== false) await until(page, () => document.documentElement.hasAttribute("data-ui-js"), null, "the runtime");
      if (light) {
        await page.addStyleTag({ content: LIGHT });
        // Controls ease into the new colours: axe reads them once they are there.
        await page.evaluate(() => Promise.all(document.getAnimations().filter((a) => a.effect?.getComputedTiming().endTime !== Infinity).map((a) => a.finished.catch(() => {}))));
      }
      page.done = async () => {
        let csp = rest.javaScriptEnabled === false ? [] : await page.evaluate(() => window.__csp || []);
        if (name === "webkit") csp = csp.filter((line) => !/^style-src-(elem|attr) inline$/.test(line));
        assert.deepEqual(csp, [], "no content security policy report");
        assert.deepEqual(page.problems, []);
      };
      return page;
    }
    const shot = async (page, label, fullPage = false) => {
      if (SHOTS) await page.screenshot({ path: join(SHOTS, `${name}-${label}.png`), fullPage });
    };
    /** Waits for `check` to hold in the page, asking from here. */
    const until = async (page, check, arg, what, ms = 12000) => {
      const end = Date.now() + ms;
      for (;;) {
        if (await page.evaluate(check, arg).catch(() => false)) return;
        if (Date.now() > end) assert.fail(`timed out waiting for ${what}`);
        await sleep(60);
      }
    };
    const listOf = (page, id) => page.evaluate((id) => document.querySelector(`#columns-item-${id}`)?.closest("[data-slot=sortable-list]")?.getAttribute("data-list") ?? null, id);
    const keysIn = (page, list) => page.evaluate((list) => [...document.querySelectorAll(`#columns-list-${list} [data-slot=sortable-item]`)].map((i) => i.dataset.key), list);
    const status = (page) => page.locator("#columns-status").textContent();
    /** Waits until the live region has said something matching `re`, whatever was said after. */
    const said = (page, re, what = re.source) => until(page, (source) => (window.__said || []).some((text) => new RegExp(source).test(text)), re.source, `the board to say ${what}`);
    const settled = (page) => until(page, () => !document.querySelector("[data-slot=sortable-item][data-pending]") && !document.querySelector("[data-ghost]"), null, "the moves to settle");
    const handle = (page, id) => page.locator(`#columns-item-${id} [data-slot=sortable-handle]`);

    /** Drags card `id` with the mouse to the middle top of card `onto` (or the list `list`'s end). */
    async function drag(page, id, { onto, list } = {}) {
      // The cards finish sliding first: a box measured mid-slide is not where the card stays.
      await page.evaluate(() => Promise.all(document.getAnimations().filter((a) => a.effect?.getComputedTiming().endTime !== Infinity).map((a) => a.finished.catch(() => {}))));
      const from = await page.locator(`#columns-item-${id} [data-slot=sortable-body]`).boundingBox();
      const target = onto ? await page.locator(`#columns-item-${onto}`).boundingBox() : await page.locator(`#columns-list-${list} [data-slot=sortable-items]`).boundingBox();
      await page.mouse.move(from.x + 40, from.y + 12);
      await page.mouse.down();
      await page.mouse.move(from.x + 48, from.y + 20, { steps: 2 });
      const y = onto ? target.y + 6 : target.y + target.height + 4;
      await page.mouse.move(target.x + target.width / 2, y, { steps: 10 });
      await until(page, () => !!document.querySelector("[data-ghost]"), null, "the copy that follows the pointer");
      await sleep(80);
      await page.mouse.up();
    }

    // ---- Every page, every width, both themes ----------------------------------

    for (const theme of ["dark", "light"]) {
      for (const width of [1440, 390]) {
        test(`the board and a card are axe clean, keep the rules and don't scroll sideways (${theme}, ${width} px)`, async () => {
          const page = await open("/", { light: theme === "light", viewport: { width, height: 900 } });
          assert.deepEqual(await axe(page), [], "board");
          assert.deepEqual(await rules(page), [], "board");
          assert.equal(await sideways(page), false, "the board scrolls sideways");
          await shot(page, `board-${theme}-${width}`);
          // The drawer over the board, and the card's own page.
          await page.getByRole("button", { name: "Fix the sign-in redirect on Android", exact: true }).click();
          await until(page, () => document.querySelector("dialog#card")?.open, null, "the drawer");
          await page.evaluate(() => Promise.all(document.getAnimations().filter((a) => a.effect?.getComputedTiming().endTime !== Infinity).map((a) => a.finished.catch(() => {}))));
          assert.deepEqual(await axe(page), [], "drawer");
          assert.deepEqual(await rules(page), [], "drawer");
          if (width === 1440) {
            const box = await page.locator("dialog#card").boundingBox();
            assert.ok(box.x > 300, `the board shows beside the drawer (${box.x})`);
          }
          await shot(page, `drawer-${theme}-${width}`);
          await page.done();
          const card = await open("/cards/10", { light: theme === "light", viewport: { width, height: 900 } });
          assert.deepEqual(await axe(card), [], "card page");
          assert.equal(await sideways(card), false, "the card page scrolls sideways");
          await card.done();
        });
      }
    }

    // ---- Pointer and touch -------------------------------------------------------

    test("a card dragged with the mouse lands where it was dropped, on the page and on the server", async () => {
      const page = await open();
      await drag(page, 4, { onto: 6 });
      await settled(page);
      assert.equal(await listOf(page, 4), "ready");
      assert.deepEqual((await keysIn(page, "ready")).slice(0, 2), ["4", "6"]);
      assert.deepEqual((await where(4, 6))["4"], ["ready", 0]);
      await said(page, /Dropped Crash when a swatch photo is over 20 MB at position 1 of 5 in Ready/);
      // The count follows: Ready is full, and says so.
      assert.equal(await page.locator("#columns-list-ready [data-count]").textContent(), "5/5");
      // A click on a title is still a click: the drawer opens.
      await page.getByRole("button", { name: "Write the release notes for 3.2", exact: true }).click();
      await until(page, () => document.querySelector("dialog#card")?.open, null, "the drawer");
      await page.done();
    });

    test("a drop on a full column is refused in the page and sends nothing", async () => {
      const page = await open();
      const posts = [];
      page.on("request", (r) => r.method() === "POST" && posts.push(r.url()));
      await drag(page, 1, { onto: 10 });
      await settled(page);
      assert.equal(await listOf(page, 1), "backlog");
      assert.match(await status(page), /In progress holds at most 3/);
      assert.deepEqual(posts, []);
      // The server holds the limit too: a forged move is refused.
      const forged = await fetch(BASE + "/board/moves", { method: "POST", redirect: "manual", headers: { "content-type": "application/x-www-form-urlencoded" }, body: "item=1&from_list=backlog&to_list=doing" });
      assert.equal(forged.status, 303);
      assert.deepEqual((await where(1))["1"], ["backlog", 0]);
      await page.done();
    });

    if (name === "chromium") {
      test("touch: the handle drags at once, and a press held on the card does too", async () => {
        const page = await open("/", { hasTouch: true, isMobile: false, viewport: { width: 1440, height: 900 } });
        const cdp = await page.context().newCDPSession(page);
        const touch = (type, x, y) => cdp.send("Input.dispatchTouchEvent", { type, touchPoints: type === "touchEnd" ? [] : [{ x, y }] });
        const glide = async (x0, y0, x1, y1, steps = 12) => {
          for (let i = 1; i <= steps; i++) await touch("touchMove", x0 + ((x1 - x0) * i) / steps, y0 + ((y1 - y0) * i) / steps);
        };
        // The handle.
        const grip = await handle(page, 2).boundingBox();
        const onto = await page.locator("#columns-item-7").boundingBox();
        await touch("touchStart", grip.x + 8, grip.y + 8);
        await glide(grip.x + 8, grip.y + 8, onto.x + 60, onto.y + 6);
        await until(page, () => !!document.querySelector("[data-ghost]"), null, "a touch drag");
        await touch("touchEnd");
        await settled(page);
        assert.deepEqual((await where(2))["2"], ["ready", 1]);
        // A press held still on the card body, then moved.
        const body = await page.locator("#columns-item-3 [data-slot=sortable-body]").boundingBox();
        const end = await page.locator("#columns-list-backlog [data-slot=sortable-items]").boundingBox();
        await touch("touchStart", body.x + 30, body.y + 50);
        await sleep(420);
        await glide(body.x + 30, body.y + 50, end.x + 40, end.y + 4, 6);
        await touch("touchEnd");
        await settled(page);
        assert.deepEqual((await where(3))["3"], ["backlog", 0]);
        // A quick swipe on the card is a scroll, not a drag.
        await touch("touchStart", body.x + 30, body.y + 50);
        await glide(body.x + 30, body.y + 50, body.x + 30, body.y - 150, 4);
        assert.equal(await page.evaluate(() => !!document.querySelector("[data-ghost]")), false);
        await touch("touchEnd");
        await page.done();
      });
    }

    // ---- Keys --------------------------------------------------------------------

    test("keys pick a card up, move it, say every step, and drop or cancel it", async () => {
      const page = await open();
      await handle(page, 1).focus();
      await page.keyboard.press("Space");
      // Counts are the column's, drawn or not.
      const backlog = Number(await page.locator("#columns-list-backlog").getAttribute("data-total"));
      assert.equal(await status(page), `Picked up Offline order sheets for the dyeing unit. Position 1 of ${backlog} in Backlog.`);
      await page.keyboard.press("ArrowDown");
      assert.equal(await status(page), `Position 2 of ${backlog} in Backlog.`);
      await page.keyboard.press("ArrowRight");
      // Ready takes it at the same place.
      assert.equal(await status(page), "Position 2 of 5 in Ready.");
      await page.keyboard.press("ArrowRight");
      // In progress is full: it is passed, and the board says why.
      assert.equal(await status(page), "In progress holds at most 3. Position 2 of 2 in Review.");
      await page.keyboard.press("Home");
      assert.equal(await status(page), "Position 1 of 2 in Review.");
      assert.equal(await page.evaluate(() => document.activeElement?.getAttribute("aria-label")), "Move Offline order sheets for the dyeing unit");
      await page.keyboard.press("Space");
      assert.equal(await status(page), "Dropped Offline order sheets for the dyeing unit at position 1 of 2 in Review.");
      await settled(page);
      assert.deepEqual((await where(1))["1"], ["review", 0]);
      // Escape puts it back where the move began.
      await handle(page, 2).focus();
      await page.keyboard.press("Enter");
      await page.keyboard.press("ArrowRight");
      await page.keyboard.press("Escape");
      assert.equal(await status(page), `Move cancelled. Export a shipment as CSV is back at position 1 of ${backlog - 1} in Backlog.`);
      assert.equal(await listOf(page, 2), "backlog");
      // Tabbing away cancels too: nothing moves without a drop.
      await page.keyboard.press("Space");
      await page.keyboard.press("ArrowDown");
      await page.keyboard.press("Tab");
      await until(page, () => !document.querySelector("[data-lifted]"), null, "the move to end");
      assert.deepEqual((await where(2))["2"], ["backlog", 0]);
      await page.done();
    });

    // ---- The server has the last word ----------------------------------------------

    test("a stale move is refused: the card goes back, a toast says why, and the board catches up", async () => {
      const page = await open();
      // Someone moves card 7 and no page hears of it.
      assert.equal(await (await post("/__test/move?item=7&to=backlog&quiet=1")).text(), "moved");
      await handle(page, 7).focus();
      await page.keyboard.press("Space");
      await page.keyboard.press("ArrowUp");
      await page.keyboard.press("Space");
      await until(page, () => /Someone else moved it first/.test(document.querySelector("[data-slot=toast]")?.textContent || ""), null, "the toast");
      await until(page, () => !document.getElementById("columns-item-7")?.closest("#columns-list-ready"), null, "the board to catch up");
      assert.equal((await where(7))["7"][0], "backlog");
      await page.done();
    });

    test("the board is a member's until someone chooses: Done refuses a member's card, and takes the lead's", async () => {
      const page = await open();
      // No one has chosen yet: a member, who can't close a card.
      assert.ok(await page.getByRole("button", { name: /Viewing as Gurpreet Singh/ }).isVisible());
      assert.ok(await page.evaluate(() => !!document.querySelector("#columns-list-done[data-refuse]")), "Done refuses a member");
      await handle(page, 13).focus();
      await page.keyboard.press("Space");
      await page.keyboard.press("ArrowRight");
      assert.match(await status(page), /Only a lead can move cards to Done/);
      await page.keyboard.press("Escape");
      // The server holds to it, with no cookie as with a member's.
      const forged = await fetch(BASE + "/board/moves", { method: "POST", headers: { "content-type": "application/x-www-form-urlencoded", "x-navree-ui": "1" }, body: "item=13&from_list=review&to_list=done" });
      assert.equal(forged.status, 409);
      assert.match(await forged.text(), /Only a lead can move cards to Done/);
      // Choosing who to be is a post from this site: another site's is refused.
      const elsewhere = await fetch(BASE + "/as/1", { method: "POST", redirect: "manual", headers: { "content-type": "application/x-www-form-urlencoded", origin: "https://elsewhere.example" }, body: "" });
      assert.equal(elsewhere.status, 403);
      await page.getByRole("button", { name: /Viewing as Gurpreet Singh/ }).click();
      await page.getByRole("menuitem", { name: "Harleen Kaur (lead)" }).click();
      await until(page, () => !document.querySelector("#columns-list-done[data-refuse]"), null, "the board as the lead");
      await handle(page, 13).focus();
      await page.keyboard.press("Space");
      await page.keyboard.press("ArrowRight");
      await page.keyboard.press("Space");
      await settled(page);
      assert.equal((await where(13))["13"][0], "done");
      await page.done();
    });

    test("a move the network loses is asked about first, then said, and the card is never left waiting", async () => {
      const page = await open();
      await page.route("**/board/moves", (route) => route.abort());
      await handle(page, 6).focus();
      await page.keyboard.press("Space");
      await page.keyboard.press("ArrowDown");
      await page.keyboard.press("Space");
      // The board is drawn again from the server, which never had the move: then, and only then, it is said.
      await until(page, () => /That move wasn.t saved/.test(document.querySelector("[data-slot=toast]")?.textContent || ""), null, "the toast");
      await settled(page);
      assert.deepEqual((await keysIn(page, "ready")).slice(0, 2), ["6", "7"]);
      await said(page, /wasn't moved. It is back at position 1 of 4 in Ready/);
      assert.equal(await page.evaluate(() => /No connection/.test(document.body.textContent)), false, "the runtime leaves the telling to the board");
      await page.done();
    });

    test("a move whose answer is lost, though it landed, says nothing went wrong", async () => {
      const page = await open();
      // The server takes the move; its answer never reaches the page.
      await page.route("**/board/moves", async (route) => {
        await route.fetch();
        await route.abort("timedout");
      });
      await handle(page, 6).focus();
      await page.keyboard.press("Space");
      await page.keyboard.press("ArrowDown");
      await page.keyboard.press("Space");
      await settled(page);
      await said(page, /is at position 2 of 4 in Ready/, "the place once the board is drawn again");
      assert.deepEqual((await where(6))["6"], ["ready", 1]);
      assert.deepEqual((await keysIn(page, "ready")).slice(0, 2), ["7", "6"]);
      assert.equal(await page.evaluate(() => document.querySelectorAll("[data-slot=toast][data-tone=danger]").length), 0, "no toast says it failed");
      await page.done();
    });

    test("a move with no answer, when the board can't be fetched either, goes back and says so", async () => {
      const page = await open();
      await page.route("**/board/moves", (route) => route.abort());
      await page.route("**/fragments/board**", (route) => route.abort());
      await handle(page, 6).focus();
      await page.keyboard.press("Space");
      await page.keyboard.press("ArrowDown");
      await page.keyboard.press("Space");
      await until(page, () => /No connection. That move may not have been saved/.test(document.querySelector("[data-slot=toast]")?.textContent || ""), null, "the toast");
      await settled(page);
      assert.deepEqual((await keysIn(page, "ready")).slice(0, 2), ["6", "7"], "back where the server last had it");
      await said(page, /wasn't moved. It is back at position 1 of 4 in Ready/);
      await page.done();
    });

    test("moves made quickly go one at a time, in order, and the page waits for its answers", async () => {
      const page = await open();
      await post("/__test/delay?ms=800");
      await handle(page, 6).focus();
      await page.keyboard.press("Space");
      await page.keyboard.press("ArrowLeft");
      await page.keyboard.press("Space");
      await until(page, () => !!document.querySelector("#columns-item-6[data-pending]"), null, "the pending card");
      await drag(page, 7, { onto: 6 });
      await post("/__test/delay?ms=0");
      await settled(page);
      const at = await where(6, 7);
      assert.equal(at["6"][0], "backlog");
      assert.equal(at["7"][0], "backlog");
      assert.equal(at["7"][1] + 1, at["6"][1], "7 lands just before 6");
      await page.done();
    });

    test("a refused move whose neighbours moved meanwhile ends where the server has it", async () => {
      const page = await open();
      let first = true;
      await page.route("**/board/moves", async (route) => {
        if (!first) return route.continue();
        first = false;
        await sleep(700);
        await route.abort();
      });
      // [6, 7, 8, 9]: 6 to the end, then 7 to the end while 6's move is out, then 6's fails.
      for (const id of [6, 7]) {
        await handle(page, id).focus();
        await page.keyboard.press("Space");
        await page.keyboard.press("End");
        await page.keyboard.press("Space");
      }
      await settled(page);
      await until(page, () => !document.querySelector("#board[aria-busy]"), null, "the redraw");
      await sleep(400);
      const server = await where(6, 7, 8, 9);
      const order = Object.entries(server).filter(([, [list]]) => list === "ready").sort((a, b) => a[1][1] - b[1][1]).map(([id]) => id);
      assert.deepEqual(await keysIn(page, "ready"), order);
      await page.done();
    });

    test("a refusal that redraws the board waits while a card is lifted, and the drop still posts", async () => {
      const page = await open();
      await post("/__test/delay?ms=700");
      assert.equal(await (await post("/__test/move?item=7&to=backlog&quiet=1")).text(), "moved");
      await handle(page, 7).focus();
      await page.keyboard.press("Space");
      await page.keyboard.press("ArrowDown");
      await page.keyboard.press("Space");
      await handle(page, 2).focus();
      await page.keyboard.press("Space");
      await page.keyboard.press("ArrowDown");
      await until(page, () => /Someone else moved it first/.test(document.querySelector("[data-slot=toast]")?.textContent || ""), null, "the stale refusal");
      assert.ok(await page.evaluate(() => document.querySelector("#columns-item-2")?.hasAttribute("data-lifted")), "still lifted");
      await post("/__test/delay?ms=0");
      await page.keyboard.press("Space");
      await settled(page);
      assert.deepEqual((await where(2))["2"], ["backlog", 2]);
      await page.done();
    });

    test("a click on the handle picks the card up and a second drops it, as a screen reader in browse mode clicks", async () => {
      const page = await open();
      await handle(page, 1).click();
      assert.match(await status(page), /^Picked up Offline order sheets for the dyeing unit/);
      await page.keyboard.press("ArrowDown");
      await handle(page, 1).click();
      assert.match(await status(page), /^Dropped Offline order sheets for the dyeing unit at position 2/);
      await settled(page);
      assert.deepEqual((await where(1))["1"], ["backlog", 1]);
      await page.done();
    });

    test("a drag near the side of a phone's screen scrolls the board to the columns off it", async () => {
      const page = await open("/", { viewport: { width: 390, height: 844 } });
      const from = await page.locator("#columns-item-1 [data-slot=sortable-body]").boundingBox();
      await page.mouse.move(from.x + 20, from.y + 12);
      await page.mouse.down();
      await page.mouse.move(from.x + 30, from.y + 20, { steps: 2 });
      await page.mouse.move(386, from.y + 20, { steps: 6 });
      await until(page, () => document.querySelector("#columns [data-slot=sortable-lists]").scrollLeft > 200, null, "the board to scroll");
      await page.keyboard.press("Escape");
      await page.mouse.up();
      await settled(page);
      assert.deepEqual((await where(1))["1"], ["backlog", 0]);
      await page.done();
    });

    test("a checklist line ticked in the drawer is saved", async () => {
      const page = await open();
      await page.getByRole("button", { name: "Fix the sign-in redirect on Android", exact: true }).click();
      await until(page, () => document.querySelector("dialog#card")?.open, null, "the drawer");
      await page.getByLabel("Ship to the beta group", { exact: true }).check();
      await until(page, () => /3 of 3 done/.test(document.querySelector("#checklist")?.textContent || ""), null, "the tick saved");
      assert.match(await (await fetch(BASE + "/cards/10")).text(), /3 of 3 done/);
      await page.done();
    });

    // ---- Two viewers ------------------------------------------------------------------

    test("a move by one viewer slides into place on another's board", async () => {
      const a = await open();
      const b = await open();
      await until(b, () => document.getElementById("board")?.getAttribute("data-live") === "on", null, "the stream");
      await b.evaluate(() => {
        window.__slid = 0;
        document.addEventListener("ui:swap", () => {
          window.__slid += document.getElementById("columns-item-11")?.getAnimations().length || 0;
        });
      });
      await handle(a, 11).focus();
      await a.keyboard.press("Space");
      await a.keyboard.press("ArrowRight");
      await a.keyboard.press("Space");
      await settled(a);
      await until(b, () => document.getElementById("columns-item-11")?.closest("#columns-list-review"), null, "the move on the other board");
      assert.ok((await b.evaluate(() => window.__slid)) > 0, "the card slid into place");
      await a.done();
      await b.done();
    });

    // ---- Long columns ------------------------------------------------------------------

    test("a long column loads a page at a time as it scrolls, up to its cap, and a drop below the last drawn card lands right after it", async () => {
      const page = await open();
      assert.equal((await keysIn(page, "backlog")).length, 20);
      const total = Number(await page.locator("#columns-list-backlog").getAttribute("data-total"));
      assert.ok(total > 11_000, `${total} cards in the backlog`);
      assert.equal(await page.locator("#columns-list-backlog [data-count]").textContent(), String(total));
      for (let i = 0; i < 40 && (await keysIn(page, "backlog")).length < 200; i++) {
        await page.mouse.wheel(0, 3000);
        await sleep(150);
      }
      const keys = await keysIn(page, "backlog");
      assert.equal(keys.length, 200, "the cap");
      assert.equal(new Set(keys).size, 200, "no card twice");
      assert.match(await page.locator("#columns-list-backlog-more").textContent(), /Showing the first 200/);
      const nodes = await page.evaluate(() => document.getElementsByTagName("*").length);
      assert.ok(nodes < 40_000, `${nodes} nodes`);
      // The region asks for as much as the page holds, so a redraw keeps it.
      assert.match(await page.locator("#board").getAttribute("data-ui-src"), /show\.backlog=200/);
      const last = keys.at(-1);
      const [, lastAt] = (await where(last))[last];
      await handle(page, 6).focus();
      await page.keyboard.press("Space");
      await page.keyboard.press("ArrowLeft");
      await page.keyboard.press("End");
      await page.keyboard.press("Space");
      await settled(page);
      assert.deepEqual((await where(6))["6"], ["backlog", lastAt + 1]);
      await page.done();
    });

    // ---- JavaScript off -----------------------------------------------------------------

    test("with JavaScript off, each card has a form to move it, and a column loads more by a button", async () => {
      const page = await open("/", { javaScriptEnabled: false });
      const card = page.locator("#columns-item-4");
      await card.locator("summary").click();
      await card.locator("select[name=to_list]").selectOption("ready");
      await card.locator("input[name=position]").fill("2");
      await Promise.all([page.waitForEvent("load"), card.getByRole("button", { name: "Move" }).click()]);
      assert.deepEqual((await where(4))["4"], ["ready", 1]);
      assert.deepEqual((await keysIn(page, "ready")).slice(0, 3), ["6", "4", "7"]);
      assert.equal(await page.locator("[data-slot=sortable-handle]:visible").count(), 0);
      // A full column is refused, and the page says why.
      const other = page.locator("#columns-item-1");
      await other.locator("summary").click();
      await other.locator("select[name=to_list]").selectOption({ value: "doing" }, { force: true }).catch(() => {});
      await Promise.all([page.waitForURL(/show\.backlog=40/), page.locator("#columns-list-backlog-more button").click()]);
      await page.waitForLoadState();
      assert.equal((await keysIn(page, "backlog")).length, 40);
      await page.done();
    });

    test("with JavaScript off under a filter, a card's form counts in the whole column and the board keeps the filter", async () => {
      const page = await open("/?label=bug", { javaScriptEnabled: false });
      const keys = await keysIn(page, "backlog");
      assert.ok(keys.length > 3, `${keys.length} bug cards`);
      const id = keys[1];
      const [, at] = (await where(id))[id];
      const card = page.locator(`#columns-item-${id}`);
      assert.equal(await card.locator("input[name=position]").inputValue(), String(at + 1), "its place in the whole column");
      assert.ok(at + 1 !== 2, "the filter leaves cards out before it");
      await card.locator("summary").click();
      await card.locator("input[name=position]").fill(String(at + 3));
      await Promise.all([page.waitForEvent("load"), card.getByRole("button", { name: "Move" }).click()]);
      assert.match(page.url(), /\/\?label=bug$/);
      assert.deepEqual((await where(id))[id], ["backlog", at + 2]);
      await page.done();
    });

    // ---- Hostile names --------------------------------------------------------------------

    test("elements named like the methods the module uses don't change what it does", async () => {
      const page = await open();
      await page.evaluate(() => {
        const add = (html) => document.body.insertAdjacentHTML("beforeend", html);
        add('<img name="elementFromPoint" alt=""><img name="body" alt=""><img name="scrollingElement" alt=""><form name="getElementById"></form>');
        for (const form of document.querySelectorAll("form[data-ui-move]")) {
          for (const name of ["requestSubmit", "elements", "action", "querySelector", "closest"]) {
            const input = document.createElement("input");
            input.type = "hidden";
            input.name = name;
            form.append(input);
          }
        }
      });
      await drag(page, 5, { onto: 6 });
      await settled(page);
      assert.deepEqual((await where(5))["5"], ["ready", 0]);
      await handle(page, 1).focus();
      await page.keyboard.press("Space");
      await page.keyboard.press("ArrowDown");
      await page.keyboard.press("Space");
      await settled(page);
      assert.deepEqual((await where(1))["1"], ["backlog", 1]);
      await page.done();
    });
  });
}
