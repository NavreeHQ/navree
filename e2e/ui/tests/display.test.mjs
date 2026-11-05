// Timeline, ActivityFeed, FactCard, badges and live regions in real browsers: every
// state, light and dark, 390 and 1440 px, keyboard only, JavaScript off, axe, and a
// real server-sent event stream with a dropped connection, refused connections,
// a hidden tab and hostile payloads. Run with the proof app serving:
//
//   cargo run --release -p ui-proof -- serve          # UI_PROOF_URL, default http://127.0.0.1:8090
//   UI_BROWSERS=chromium,firefox,webkit npm test -- tests/display.test.mjs
//
// UI_SHOTS=<dir> also saves a screenshot of every state.

import assert from "node:assert/strict";
import { mkdirSync, readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import { after, afterEach, before, beforeEach, describe, test } from "node:test";
import { fileURLToPath } from "node:url";
import { chromium, firefox, webkit } from "playwright";

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

/** Round on purpose: status dots (the timeline dot, the live dot, the badge dot) and the feed's icon container are not pills and not letter tiles. Every other corner is 2, 4 or 6 px. */
const ROUND = new Set(["timeline-dot", "live-dot", "badge-dot", "activity-icon", "radio", "spinner"]);

async function axe(page) {
  await page.evaluate(axeSource);
  return page.evaluate(async () => {
    const out = await window.axe.run(document, { runOnly: ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa", "best-practice"] });
    return out.violations.map((v) => `${v.id}: ${v.nodes.slice(0, 3).map((n) => n.target.join(" ")).join(" | ")}`);
  });
}

const post = (path) => fetch(BASE + path, { method: "POST" });
const stats = async () => (await fetch(BASE + "/display/test/stats")).json();
const reset = () => post("/display/test/reset");
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
/** Waits for `check` to hold, polling the server. */
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
      // Firefox slows the network of a tab it thinks is in the background, in steps of about 3 s, while
      // another load is under way, and an open event stream is a load that never ends. Playwright's
      // Firefox keeps its first tab in front, so a page under test is "in the background" and its
      // fetches stall for seconds. Measured with a plain Node server too: 4 stalls in 10 runs with the
      // default, 1 with the throttle off. A reader's own tab in front is not throttled.
      browser = await ENGINES[name].launch(name === "firefox" ? { firefoxUserPrefs: { "network.http.throttle.enable": false } } : {});
    });
    after(() => browser.close());
    /** Every context a test opened: closed after it, so a failed test leaves no stream behind. */
    const contexts = [];
    afterEach(async () => {
      for (const context of contexts.splice(0)) await context.close().catch(() => {});
    });
    beforeEach(async () => {
      await reset();
      await until(async () => (await stats()).open === 0, "every stream to close");
    });

    /**
     * A page on the proof app. Console errors, page errors and content security policy
     * reports are kept in `page.problems`; `page.done()` asserts there are none.
     */
    async function open(path, options = {}) {
      const { light, visibility, ...rest } = options;
      const context = await browser.newContext({ viewport: { width: 1440, height: 900 }, bypassCSP: !!light, ...rest });
      contexts.push(context);
      const page = await context.newPage();
      // Poll from here, not from the page: Playwright's own wait in Firefox sometimes missed a
      // condition that was already true (a page that had the text, and a wait that timed out).
      page.waitForFunction = async (fn, arg, options = {}) => {
        const end = Date.now() + (options.timeout ?? 30000);
        for (;;) {
          const got = await page.evaluate(fn, arg);
          if (got) return got;
          if (Date.now() > end) throw new Error(`timed out waiting for ${String(fn).slice(0, 140)}`);
          await sleep(100);
        }
      };
      page.problems = [];
      page.on("console", (m) => {
        // Playwright's WebKit on Windows reports this on every page, a plain 404 too.
        if (name === "webkit" && /Refused to apply a stylesheet/.test(m.text())) return;
        // A stream refused or cut on purpose shows up as a failed resource.
        // Firefox words a refused stream as a console error of its own.
        if (/can.t establish a connection to the server at \S*\/display\/events/.test(m.text())) return;
        if (m.type() === "error" && !/503|ERR_FAILED|ERR_ABORTED|ERR_INCOMPLETE|NS_ERROR|Load failed|Failed to load resource|terminated|could not be loaded/i.test(m.text())) page.problems.push(m.text());
      });
      page.on("pageerror", (e) => page.problems.push("pageerror: " + e.message));
      await page.addInitScript(() => {
        document.addEventListener("securitypolicyviolation", (e) => {
          (window.__csp ||= []).push(e.violatedDirective + " " + e.blockedURI);
        });
      });
      if (visibility) {
        // The runtime reads the page's visibility through the prototype's getter, so the
        // test replaces that getter before the runtime loads and flips it by hand.
        await page.addInitScript(() => {
          window.__hidden = false;
          Object.defineProperty(Document.prototype, "hidden", { configurable: true, get: () => window.__hidden === true });
        });
      }
      await page.goto(BASE + path);
      if (light) {
        await page.addStyleTag({ content: LIGHT });
        // The page's colours ease from dark to light; axe reads them once they have.
        await page.waitForTimeout(300);
      }
      page.done = async () => {
        let csp = rest.javaScriptEnabled === false ? [] : await page.evaluate(() => window.__csp || []);
        // Playwright's WebKit on Windows reports inline styles refused on every page, a bare 404
        // too. Chromium and Firefox check the style directives, and the DOM check below holds
        // everywhere: no `style` attribute on a part of the display module.
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
    const requests = (page) => {
      const list = [];
      page.on("request", (r) => list.push(r.url().replace(BASE, "")));
      return list;
    };
    const rows = (page) => page.locator("#activity-list > li");
    const ids = (page) => page.evaluate(() => [...document.querySelectorAll("#activity-list > li")].map((li) => li.id));
    const live = (page) => page.evaluate(() => document.getElementById("activity-region")?.getAttribute("data-live"));
    const waitLive = (page, state, ms = 8000) => page.waitForFunction((s) => document.getElementById("activity-region")?.getAttribute("data-live") === s, state, { timeout: ms });

    // ---- Static: every component, every state --------------------------------

    describe("static", () => {
      for (const theme of ["dark", "light"]) {
        for (const width of [1440, 390]) {
          test(`the display page: axe clean, nothing spills, the rules hold (${theme}, ${width} px)`, async () => {
            const page = await open("/display", { light: theme === "light", viewport: { width, height: 900 } });
            assert.deepEqual(await axe(page), []);
            assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), "no sideways scroll");
            const broken = await page.evaluate((round) => {
              const bad = [];
              for (const el of document.querySelectorAll("main *")) {
                const style = getComputedStyle(el);
                const slot = el.getAttribute("data-slot") || el.tagName;
                const radius = parseFloat(style.borderTopLeftRadius);
                if (radius && ![2, 4, 6].includes(radius) && !round.includes(slot) && !el.closest("[data-slot=spinner]")) bad.push(`radius ${style.borderTopLeftRadius} on ${slot}`);
                if (style.boxShadow !== "none") bad.push(`shadow on ${slot}`);
                if (el.hasAttribute("style") && !el.hasAttribute("popover")) bad.push(`style attribute on ${slot}`);
              }
              return [...new Set(bad)];
            }, [...ROUND]);
            assert.deepEqual(broken, []);
            await shot(page, `display-${theme}-${width}`);
            await page.done();
          });
        }
      }

      for (const kind of ["empty", "one", "many", "long", "rtl", "hostile", "hidden", "loading", "failed"]) {
        for (const width of [1440, 390]) {
          test(`case ${kind} at ${width} px: axe clean, nothing spills`, async () => {
            const page = await open(`/display?case=${kind}`, { viewport: { width, height: 900 } });
            assert.deepEqual(await axe(page), []);
            assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), "no sideways scroll");
            if (kind === "hostile") {
              assert.equal(await page.evaluate(() => window.__pwned), undefined, "no script ran");
              assert.equal(await page.locator("main img").count(), 0, "no injected image");
              assert.equal(await page.locator("main script").count(), 0);
              assert.ok((await page.locator("main").textContent()).includes("<script>window.__pwned=1</script>"), "the text is shown as text");
            }
            await shot(page, `case-${kind}-${width}`);
            if (width === 1440) {
              const light = await open(`/display?case=${kind}`, { light: true });
              assert.deepEqual(await axe(light), []);
              await shot(light, `case-${kind}-light`);
              await light.done();
            }
            await page.done();
          });
        }
      }

      test("the timeline is a list in a section per day, with time elements and gap chips", async () => {
        const page = await open("/display?case=many");
        const shape = await page.evaluate(() => {
          const root = document.getElementById("many");
          return {
            group: root.getAttribute("role"),
            label: root.getAttribute("aria-label"),
            sections: [...root.querySelectorAll("[data-slot=timeline-day]")].map((s) => ({ labelled: !!document.getElementById(s.getAttribute("aria-labelledby")), heading: s.querySelector("h3")?.textContent, items: s.querySelectorAll("ol > li").length })),
            times: root.querySelectorAll("time[datetime]").length,
            events: root.querySelectorAll("[data-slot=timeline-event]").length,
            gaps: [...root.querySelectorAll("[data-slot=timeline-gap]")].slice(0, 4).map((g) => g.textContent),
          };
        });
        assert.equal(shape.group, "group");
        assert.equal(shape.label, "Forty steps");
        assert.equal(shape.events, 40);
        assert.equal(shape.times, 40);
        assert.ok(shape.sections.length > 1, "several days");
        assert.ok(shape.sections.every((s) => s.labelled && /^[A-Z][a-z]+day, [A-Z][a-z]+ \d+, 2026$/.test(s.heading) && s.items > 0), JSON.stringify(shape.sections));
        assert.equal(shape.sections.reduce((n, s) => n + s.items, 0), 40);
        assert.deepEqual(shape.gaps, ["+11 min 40 s", "+35 min", "+58 min 20 s", "+1 h 21 min"]);
        await page.done();
      });

      test("the clock times follow the zone, and the note says which", async () => {
        const page = await open("/display?case=one");
        assert.equal(await page.locator("#one [data-slot=timeline-zone]").textContent(), "Times in Asia/Kolkata (GMT+05:30)");
        assert.equal(await page.locator("#one time").getAttribute("datetime"), "2026-11-02T09:41:07+05:30");
        assert.equal(await page.locator("#one time").textContent(), "09:41:07");
        assert.equal(await page.locator("#one [data-slot=timeline-day-title]").textContent(), "Monday, November 2, 2026");
        await page.done();
      });

      test("the connector joins the dots, and the last event has none", async () => {
        const page = await open("/display?case=one");
        const line = (sel) => page.evaluate((s) => getComputedStyle(document.querySelector(s), "::before").content, sel);
        const many = await open("/display?case=many");
        assert.equal(await many.evaluate(() => getComputedStyle(document.querySelector("#many [data-slot=timeline-event]"), "::before").content), '""');
        assert.equal(await many.evaluate(() => {
          const all = document.querySelectorAll("#many [data-slot=timeline-list]");
          const last = all[all.length - 1].lastElementChild;
          return getComputedStyle(last, "::before").content;
        }), "none");
        assert.equal(await line("#one [data-slot=timeline-event]"), "none");
        await many.done();
        await page.done();
      });

      test("an RTL timeline puts the time on the right and keeps the clock left to right", async () => {
        const page = await open("/display?case=rtl");
        const where = await page.evaluate(() => {
          const li = document.querySelector("#rtl [data-slot=timeline-event]");
          const time = li.querySelector("time").getBoundingClientRect();
          const dot = li.querySelector("[data-slot=timeline-dot]").getBoundingClientRect();
          const body = li.querySelector("[data-slot=timeline-body]").getBoundingClientRect();
          return { timeRight: time.left > dot.left, bodyLeft: body.right < dot.right, dir: li.querySelector("time").getAttribute("dir") };
        });
        assert.deepEqual(where, { timeRight: true, bodyLeft: true, dir: "ltr" });
        await shot(page, "case-rtl-check");
        await page.done();
      });

      test("fact cards are a grid that is one column on a phone", async () => {
        const wide = await open("/display");
        const cols = (page, sel) => page.evaluate((s) => new Set([...document.querySelectorAll(s)].map((c) => Math.round(c.getBoundingClientRect().left))).size, sel);
        assert.ok((await cols(wide, "[data-slot=fact-grid] > [data-slot=fact-card]")) > 1, "several columns at 1440");
        await wide.done();
        const phone = await open("/display", { viewport: { width: 390, height: 900 } });
        assert.equal(await phone.evaluate(() => new Set([...document.querySelectorAll("[data-slot=fact-card]")].map((c) => Math.round(c.getBoundingClientRect().left))).size), 1);
        await phone.done();
      });

      test("fact cards are a term and descriptions", async () => {
        const page = await open("/display");
        const card = await page.evaluate(() => {
          const c = document.querySelector("[data-slot=fact-card]");
          return { dl: !!c.querySelector("dl"), dt: c.querySelector("dt")?.textContent, dds: c.querySelectorAll("dd").length, icon: c.querySelector("[data-slot=fact-icon]")?.getAttribute("aria-hidden") };
        });
        assert.deepEqual(card, { dl: true, dt: "Outcome", dds: 2, icon: "true" });
        await page.done();
      });

      test("fact card text wraps between words, and breaks a word only when it can't fit", async () => {
        for (const width of [1440, 390]) {
          const page = await open("/display?case=words", { viewport: { width, height: 900 } });
          const found = await page.evaluate(() => {
            const midWord = [];
            for (const card of document.querySelectorAll("[data-slot=fact-card]")) {
              const walker = document.createTreeWalker(card, NodeFilter.SHOW_TEXT);
              for (let node = walker.nextNode(); node; node = walker.nextNode()) {
                let top = null;
                for (let i = 0; i < node.data.length; i++) {
                  const range = document.createRange();
                  range.setStart(node, i);
                  range.setEnd(node, i + 1);
                  const rect = range.getClientRects()[0];
                  if (!rect) continue;
                  // A line that starts inside a word, where a whole word would have fitted.
                  const word = node.data.slice(0, i).split(/\s/).pop() + node.data.slice(i).split(/\s/)[0];
                  const room = node.parentElement.closest("dd, dt").getBoundingClientRect().width;
                  const measure = document.createElement("span");
                  measure.textContent = word;
                  // Set through the CSSOM, which the policy allows: an unwrapped word's width.
                  measure.style.whiteSpace = "nowrap";
                  measure.style.position = "absolute";
                  node.parentElement.append(measure);
                  const needs = measure.getBoundingClientRect().width;
                  measure.remove();
                  if (top !== null && rect.top > top + 2 && /\S/.test(node.data[i - 1]) && /\S/.test(node.data[i]) && needs <= room) {
                    midWord.push(`${node.data.slice(0, i)}|${node.data.slice(i)}`);
                  }
                  top = rect.top;
                }
              }
            }
            const grid = document.querySelector("#words-grid [data-slot=fact-card]");
            return { midWord, overflows: grid.scrollWidth > grid.clientWidth + 1 };
          });
          assert.deepEqual(found.midWord, [], `${width} px: no word broken where it fits`);
          assert.equal(found.overflows, false, `${width} px: an address longer than the card breaks inside the card`);
          assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false, `${width} px: no sideways scroll`);
          await shot(page, `case-words-${width}`);
          await page.done();
        }
      });

      test("outcome chips say yes or no to a screen reader, and a dot or tone never stands alone", async () => {
        const page = await open("/display");
        const chips = await page.evaluate(() => [...document.querySelectorAll("[data-variant=outcome]")].map((c) => c.textContent.trim()));
        assert.deepEqual(chips.slice(0, 5), ["Delivered yes", "Opened yes", "Clicked yes", "Replied no", "Unsubscribed no"].map((t) => t.replace(" ", "")));
        await page.done();
      });
    });

    // ---- The notice for rows left out -------------------------------------------

    describe("hidden items", () => {
      test("ticking the box shows the rows in place, and the words change", async () => {
        const page = await open("/display?case=hidden");
        const visible = () => page.evaluate(() => [...document.querySelectorAll("#hidden [data-filtered]")].filter((e) => e.getBoundingClientRect().height > 0).length);
        assert.equal(await visible(), 0);
        const toggle = page.locator("#hidden [data-slot=hidden-notice-toggle]").first();
        assert.equal((await toggle.innerText()).trim(), "Show them");
        await toggle.locator("input").check();
        assert.ok((await visible()) > 0, "the rows are there");
        assert.equal((await toggle.innerText()).trim(), "Hide them");
        // In place: each revealed row sits between the visible ones, in time order.
        const order = await page.evaluate(() => [...document.querySelectorAll("#hidden time")].filter((t) => t.getBoundingClientRect().height > 0).map((t) => t.textContent));
        assert.deepEqual(order, [...order].sort());
        await shot(page, "hidden-open");
        await toggle.locator("input").uncheck();
        assert.equal(await visible(), 0);
        await page.done();
      });

      test("by keyboard: Tab reaches the box and Space toggles it, with a visible ring", async () => {
        const page = await open("/display?case=hidden");
        const box = page.locator("#hidden [data-slot=reveal]").first();
        await box.focus();
        const ring = await page.evaluate(() => {
          const label = document.activeElement.closest("label");
          const style = getComputedStyle(label);
          return style.outlineStyle !== "none" && parseFloat(style.outlineWidth) >= 2;
        });
        assert.ok(ring, "the label shows the focus ring");
        await page.keyboard.press("Space");
        assert.ok(await box.isChecked());
        assert.equal(await page.evaluate(() => document.activeElement.getAttribute("aria-describedby")), "hidden-hidden-text");
        await page.keyboard.press("Space");
        assert.ok(!(await box.isChecked()));
        await page.done();
      });

      test("dismissing removes the notice and leaves revealed rows revealed", async () => {
        const page = await open("/display?case=hidden");
        await page.locator("#hidden [data-slot=reveal]").first().check();
        const dismiss = page.locator("#hidden [data-slot=hidden-notice-dismiss]").first();
        assert.ok(await dismiss.isVisible(), "shown when the runtime is there");
        await dismiss.click();
        assert.ok(await page.locator("#hidden-hidden").first().isHidden());
        assert.ok((await page.locator("#hidden [data-filtered]").evaluateAll((all) => all.filter((e) => e.getBoundingClientRect().height > 0).length)) > 0);
        await page.done();
      });

      test("with JavaScript off the box still reveals, and nothing offers a dismiss that cannot work", async () => {
        const page = await open("/display?case=hidden", { javaScriptEnabled: false });
        assert.ok(await page.locator("#hidden [data-slot=hidden-notice-dismiss]").first().isHidden());
        await page.locator("#hidden [data-slot=reveal]").first().check();
        assert.ok((await page.locator("#hidden [data-filtered]").evaluateAll((all) => all.filter((e) => e.getBoundingClientRect().height > 0).length)) > 0);
        await page.done();
      });
    });

    // ---- JavaScript off ----------------------------------------------------------

    describe("no script", () => {
      test("the display page ships no script and is complete", async () => {
        const page = await open("/display", { javaScriptEnabled: false });
        assert.equal(await page.locator("main script").count(), 0);
        assert.ok(await page.locator("[data-slot=timeline-event]").count() > 5);
        assert.ok(await page.locator("[data-slot=activity-item]").count() > 5);
        assert.ok(await page.locator("[data-slot=live-state]").first().isHidden(), "no live dot without a stream");
        await shot(page, "display-no-script");
        await page.done();
      });

      test("the live page without script: rows, table and the period form work as plain pages", async () => {
        const page = await open("/display/live?window=5", { javaScriptEnabled: false });
        assert.equal(await page.locator("#activity-list > li").count(), 5);
        assert.ok(await page.locator("[data-slot=live-state]").first().isHidden());
        await page.locator("#activity select[name=window]").selectOption("20");
        await Promise.all([page.waitForURL(/window=20/), page.getByRole("button", { name: "Apply" }).first().click()]);
        assert.equal(await page.locator("#activity-list > li").count(), 14);
        await page.done();
      });
    });

    // ---- Keyboard ----------------------------------------------------------------

    describe("keyboard", () => {
      test("Tab reaches the period select, the notice, the scrolling list and each row link, every stop with a visible ring", async () => {
        const page = await open("/display/live");
        await waitLive(page, "on");
        await page.locator("#activity select[name=window]").focus();
        const stops = [];
        for (let i = 0; i < 9; i++) {
          await page.keyboard.press("Tab");
          stops.push(await page.evaluate(() => {
            const a = document.activeElement;
            const row = a.closest("[data-slot=activity-item]");
            const ringOf = (el) => el && getComputedStyle(el).outlineStyle !== "none" && parseFloat(getComputedStyle(el).outlineWidth) >= 2;
            return { slot: a.getAttribute("data-slot"), ring: ringOf(a) || ringOf(row) || ringOf(a.closest("label")) };
          }));
        }
        const slots = stops.map((s) => s.slot);
        if (name === "webkit") {
          // Safari's Tab skips links unless the user turned that on (Option+Tab reaches them), so the
          // links are focused directly here; the rest of the order is the same.
          assert.deepEqual(slots.slice(0, 3), ["reveal", "hidden-notice-dismiss", "activity-list"], JSON.stringify(slots));
          assert.ok(stops.slice(0, 3).every((s) => s.ring), JSON.stringify(stops));
          await page.locator("#activity-list [data-slot=activity-actor]").first().focus();
          assert.ok(await page.evaluate(() => {
            const row = document.activeElement.closest("[data-slot=activity-item]");
            return getComputedStyle(row).outlineStyle !== "none" && parseFloat(getComputedStyle(row).outlineWidth) >= 2;
          }), "the row shows a ring around the focused link");
        } else {
          assert.deepEqual(slots.slice(0, 4), ["reveal", "hidden-notice-dismiss", "activity-list", "activity-actor"], JSON.stringify(slots));
          assert.ok(slots.slice(3, 7).every((s) => s === "activity-actor"), "each row link is one stop: " + JSON.stringify(slots));
          assert.ok(stops.every((s) => s.ring), "every stop shows a ring: " + JSON.stringify(stops));
        }
        await page.keyboard.press("Enter");
        await page.waitForURL(/\/display\/people\/\d+/);
        await page.done();
      });

      test("a person's page: chips, cards and timeline, axe clean in both themes at both widths", async () => {
        for (const light of [false, true]) {
          for (const width of [1440, 390]) {
            const page = await open("/display/people/0", { light, viewport: { width, height: 900 } });
            assert.deepEqual(await axe(page), []);
            assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth));
            await shot(page, `person-${light ? "light" : "dark"}-${width}`);
            await page.done();
          }
        }
      });
    });

    // ---- Live ---------------------------------------------------------------------

    describe("live", () => {
      test("the live page is axe clean, light and dark, 1440 and 390", async () => {
        for (const light of [false, true]) {
          for (const width of [1440, 390]) {
            const page = await open("/display/live", { light, viewport: { width, height: 900 } });
            await waitLive(page, "on");
            assert.deepEqual(await axe(page), []);
            assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth));
            await shot(page, `live-${light ? "light" : "dark"}-${width}`);
            await page.done();
          }
        }
      });

      test("one connection serves the feed and the table, and the dot says live", async () => {
        const page = await open("/display/live");
        await waitLive(page, "on");
        assert.equal(await page.locator("#activity [data-slot=live-state]").evaluate((e) => getComputedStyle(e).display !== "none"), true);
        assert.equal(await page.locator("#activity [data-for=on]").isVisible(), true);
        assert.equal(await page.locator("#activity [data-for=retry]").isVisible(), false);
        assert.equal(await page.locator("#recipients").getAttribute("data-live"), "on", "the table is on the same stream");
        assert.deepEqual(await stats(), { attempts: 1, open: 1, events: 14 });
        await page.done();
      });

      test("new activity arrives at the top, once, and the reader's place is kept", async () => {
        const page = await open("/display/live");
        await waitLive(page, "on");
        const before = await ids(page);
        await page.locator("#activity select[name=window]").focus();
        const reqs = requests(page);
        await post("/display/test/emit?n=3");
        await page.waitForFunction(() => document.querySelectorAll("#activity-list > li").length === 17);
        const after = await ids(page);
        assert.deepEqual(after.slice(3), before);
        assert.deepEqual(after.slice(0, 3), ["event-17", "event-16", "event-15"]);
        assert.equal(new Set(after).size, after.length, "no id twice");
        assert.equal(await page.evaluate(() => document.activeElement.getAttribute("name")), "window", "focus stayed in the select");
        assert.equal(await page.evaluate(() => document.getElementById("activity-region").getAttribute("data-ui-after")), "17");
        await sleep(600);
        assert.equal(reqs.filter((r) => r.startsWith("/display/fragments/feed")).length, 1, "a burst costs one fetch: " + reqs);
        await page.done();
      });

      test("a row the page already has is not added twice, whatever the cursor says", async () => {
        const page = await open("/display/live");
        await waitLive(page, "on");
        await page.evaluate(() => document.getElementById("activity-region").setAttribute("data-ui-after", "0"));
        await post("/display/test/emit?n=1");
        await page.waitForFunction(() => document.querySelectorAll("#activity-list > li").length === 15);
        const found = await ids(page);
        assert.equal(new Set(found).size, found.length);
        assert.equal(found.length, 15);
        await page.done();
      });

      test("a row is added though its flag draws with the same ids as a flag already on the page", async () => {
        const page = await open("/display/live");
        await waitLive(page, "on");
        // Every copy of a flag names the parts of its drawing with the same ids.
        const drawn = await page.evaluate(() => [...document.querySelectorAll("#activity-list svg[data-slot=flag] [id]")].map((e) => e.id));
        assert.ok(drawn.length > 0, "the page already shows a flag whose drawing has ids");
        // The second of three new rows is Maya Patel's, in Mumbai, under the same flag.
        await post("/display/test/emit?n=3");
        await page.waitForFunction(() => document.querySelectorAll("#activity-list > li").length === 17);
        assert.deepEqual((await ids(page)).slice(0, 3), ["event-17", "event-16", "event-15"]);
        assert.equal(await page.locator("#event-16 svg[data-slot=flag] [id]").count() > 0, true, "its flag is the same drawing");
        await page.done();
      });

      test("the feed keeps its newest rows only", async () => {
        const page = await open("/display/live");
        await waitLive(page, "on");
        await page.evaluate(() => document.getElementById("activity-region").setAttribute("data-ui-feed-max", "16"));
        await post("/display/test/emit?n=5");
        await page.waitForFunction(() => document.getElementById("activity-region").getAttribute("data-ui-after") === "19");
        const found = await ids(page);
        assert.equal(found.length, 16);
        assert.equal(found[0], "event-19");
        await page.done();
      });

      test("new rows are announced politely", async () => {
        const page = await open("/display/live");
        await waitLive(page, "on");
        await post("/display/test/emit?n=2");
        await page.waitForFunction(() => document.querySelector("[data-slot=announcer]")?.textContent === "2 new items");
        await post("/display/test/emit?n=1");
        await page.waitForFunction(() => document.querySelector("[data-slot=announcer]")?.textContent === "1 new item");
        await page.done();
      });

      test("the table on the same stream follows its own named event, the feed the unnamed one", async () => {
        const page = await open("/display/live");
        await waitLive(page, "on");
        const reqs = requests(page);
        await post("/display/test/nudge?kind=table");
        await until(async () => reqs.some((r) => r.startsWith("/display/live")), "the table to refetch");
        await sleep(700);
        assert.equal(reqs.filter((r) => r.startsWith("/display/fragments/feed")).length, 0, "the feed did not hear the table's event");
        reqs.length = 0;
        await post("/display/test/nudge?kind=message");
        await until(async () => reqs.some((r) => r.startsWith("/display/fragments/feed")), "the feed to ask for rows");
        await sleep(700);
        assert.equal(reqs.filter((r) => r.startsWith("/display/live")).length, 0, "the table did not hear the unnamed event");
        await page.done();
      });

      test("a recipient's status changes in the live table when its activity arrives", async () => {
        const page = await open("/display/live");
        await waitLive(page, "on");
        const status = (n) => page.locator("#recipients tbody tr").nth(n).locator("[data-slot=badge]").textContent();
        assert.equal(await status(4), "Delivered");
        await post("/display/test/emit?n=8");
        await until(async () => (await status(4)) === "Opened", "a status to change", 6000);
        await shot(page, "live-after-activity");
        await page.done();
      });

      test("the connection is cut: the dot says reconnecting, then live, and what was missed arrives", async () => {
        const page = await open("/display/live");
        await waitLive(page, "on");
        await post("/display/test/refuse?n=1");
        await post("/display/test/drop");
        await waitLive(page, "retry", 3000);
        assert.equal(await page.locator("#activity [data-for=retry]").isVisible(), true);
        assert.equal(await page.locator("#activity [data-for=on]").isVisible(), false);
        await shot(page, "live-reconnecting");
        // Said while it was down.
        await post("/display/test/emit?n=2");
        await waitLive(page, "on", 9000);
        await page.waitForFunction(() => document.querySelectorAll("#activity-list > li").length === 16, null, { timeout: 5000 });
        const now = await stats();
        assert.equal(now.attempts, 3, "the first, the refused one, the one that held: " + JSON.stringify(now));
        assert.equal(now.open, 1);
        await page.done();
      });

      test("refused connections come back after longer and longer waits", async () => {
        const page = await open("/display/live");
        await waitLive(page, "on");
        const times = [];
        // The runtime's one probe of a failed stream (`x-ui-probe`) is not a retry.
        page.on("request", (r) => r.url().includes("/display/events") && !r.headers()["x-ui-probe"] && times.push(Date.now()));
        await post("/display/test/refuse?n=2");
        const cut = Date.now();
        await post("/display/test/drop");
        await waitLive(page, "retry", 10000);
        await waitLive(page, "on", 20000);
        assert.equal(times.length, 3, "two refused, one that held");
        const gaps = [cut, ...times].slice(1).map((t, i) => t - [cut, ...times][i]);
        // 1 s, 2 s, 4 s, each with up to a quarter more.
        assert.ok(gaps[0] >= 900 && gaps[0] <= 1700, JSON.stringify(gaps));
        assert.ok(gaps[1] >= 1900 && gaps[1] <= 3200, JSON.stringify(gaps));
        assert.ok(gaps[2] >= 3900 && gaps[2] <= 6000, JSON.stringify(gaps));
        await page.done();
      });

      test("the wait doubles to a cap of 30 s and is forgiven once a connection holds", async () => {
        const context = await browser.newContext({ viewport: { width: 1440, height: 900 } });
        contexts.push(context);
        const page = await context.newPage();
        // Fake time that stands still: it moves only when this test runs it.
        await page.clock.install();
        await page.clock.pauseAt(new Date(Date.now() + 60_000));
        await post("/display/test/refuse?n=8");
        await page.goto(BASE + "/display/live");
        const attempts = async () => (await stats()).attempts;
        await until(async () => (await attempts()) >= 1, "the first attempt");
        let seen = await attempts();
        const waits = [];
        for (const base of [1000, 2000, 4000, 8000, 16000, 30000, 30000]) {
          // Just under the base, nothing; by a quarter over it, exactly one more.
          await page.clock.runFor(Math.floor(base * 0.97));
          await sleep(150);
          assert.equal(await attempts(), seen, `no attempt before ${base} ms`);
          await page.clock.runFor(Math.ceil(base * 0.3));
          await until(async () => (await attempts()) === seen + 1, `an attempt by ${Math.round(base * 1.27)} ms`, 4000);
          seen += 1;
          waits.push(base);
          await sleep(150);
        }
        assert.deepEqual(waits, [1000, 2000, 4000, 8000, 16000, 30000, 30000]);
        // The eighth is let through: it holds, and the page says live.
        await page.clock.runFor(40000);
        await until(async () => (await stats()).open === 1, "the connection to hold");
        await page.clock.resume();
        await waitLive(page, "on");
        await context.close();
      });

      test("a failed refetch leaves the rows alone, and the next event tries again", async () => {
        const page = await open("/display/live");
        await waitLive(page, "on");
        await page.route("**/display/fragments/feed*", (route) => route.fulfill({ status: 500, body: "no" }));
        await post("/display/test/emit?n=1");
        await sleep(900);
        assert.equal((await ids(page)).length, 14, "the old rows stay");
        await page.unroute("**/display/fragments/feed*");
        await post("/display/test/emit?n=1");
        await page.waitForFunction(() => document.querySelectorAll("#activity-list > li").length === 16);
        assert.equal(new Set(await ids(page)).size, 16);
        await page.done();
      });

      test("a late answer cannot undo a newer one", async () => {
        const page = await open("/display/live");
        await waitLive(page, "on");
        let first = true;
        await page.route("**/display/fragments/feed*", async (route) => {
          if (first) {
            first = false;
            const res = await route.fetch();
            await sleep(1200);
            await route.fulfill({ response: res });
          } else await route.continue();
        });
        await post("/display/test/emit?n=1");
        await sleep(450);
        await post("/display/test/emit?n=1");
        await page.waitForFunction(() => document.querySelectorAll("#activity-list > li").length === 16, null, { timeout: 8000 });
        await sleep(1500);
        const found = await ids(page);
        assert.equal(found.length, 16);
        assert.equal(new Set(found).size, 16);
        assert.equal(found[0], "event-16");
        await page.done();
      });

      test("a hidden page holds no connection, and when it shows again it catches up with no event", async () => {
        const page = await open("/display/live", { visibility: true });
        await waitLive(page, "on");
        assert.equal((await stats()).open, 1);
        await page.evaluate(() => { window.__hidden = true; document.dispatchEvent(new Event("visibilitychange")); });
        await until(async () => (await stats()).open === 0, "the stream to close");
        assert.equal(await live(page), "paused");
        assert.equal(await page.locator("#activity [data-for=paused]").isVisible(), true);
        await shot(page, "live-paused");
        await post("/display/test/emit?n=2");
        await sleep(500);
        assert.equal((await ids(page)).length, 14, "nothing fetched while hidden");
        await page.evaluate(() => { window.__hidden = false; document.dispatchEvent(new Event("visibilitychange")); });
        await page.waitForFunction(() => document.querySelectorAll("#activity-list > li").length === 16, null, { timeout: 5000 });
        await waitLive(page, "on");
        assert.equal((await stats()).open, 1);
        await page.done();
      });

      test("leaving the page closes its connection, and coming back opens exactly one", async () => {
        const page = await open("/display/live");
        await waitLive(page, "on");
        for (let i = 0; i < 3; i++) {
          await page.getByRole("link", { name: "Display", exact: true }).click();
          await page.waitForURL(/\/display$/);
          await until(async () => (await stats()).open === 0, "the stream to close after leaving");
          await page.goBack();
          await waitLive(page, "on");
          assert.equal((await stats()).open, 1, "one, not one per visit");
        }
        await page.done();
      });

      test("a hostile stream changes nothing and runs nothing", async () => {
        const page = await open("/display/live");
        await waitLive(page, "on");
        await page.evaluate(() => { document.getElementById("activity-region").__mine = 1; window.__html = document.getElementById("activity").outerHTML; });
        const reqs = requests(page);
        await post("/display/test/hostile");
        await sleep(1500);
        assert.equal(await page.evaluate(() => window.__pwned), undefined, "no script ran");
        assert.equal(await page.evaluate(() => ({}).polluted), undefined, "no prototype pollution");
        assert.equal(await page.evaluate(() => document.getElementById("activity-region").__mine), 1, "the region is the same element");
        assert.equal(await page.evaluate(() => document.getElementById("activity").outerHTML === window.__html), true, "the feed is untouched");
        assert.equal(await page.locator("#activity-region b, #activity-region script, #activity-region img").count(), 0);
        assert.equal(await page.locator("p#activity-region").count(), 0);
        assert.equal(reqs.filter((r) => r.startsWith("/display/fragments/feed")).length, 1, "the unnamed ones cost one fetch: " + reqs);
        assert.equal(reqs.filter((r) => !r.startsWith("/display/") && !r.startsWith("/_navree/ui/favicon")).length, 0, "nothing was asked for that the page didn't know: " + reqs);
        assert.equal(await live(page), "on");
        await page.done();
      });

      test("a stream address on another site is never opened", async () => {
        const page = await open("/display/live");
        await waitLive(page, "on");
        const reqs = requests(page);
        await page.evaluate(() => {
          const el = document.createElement("div");
          el.id = "elsewhere";
          el.setAttribute("data-ui-live-src", "http://127.0.0.1:1/stolen");
          document.querySelector("main").append(el);
          document.dispatchEvent(new Event("ui:swap"));
        });
        await sleep(600);
        assert.deepEqual(reqs.filter((r) => r.includes("127.0.0.1:1") || r.includes("stolen")), []);
        assert.equal(await page.locator("#elsewhere").getAttribute("data-live"), null);
        await page.done();
      });

      test("a stream name that is not a name is not listened to", async () => {
        const page = await open("/display/live");
        await waitLive(page, "on");
        await page.evaluate(() => {
          const el = document.createElement("div");
          el.id = "oddly";
          el.setAttribute("data-ui-live-src", "/display/events");
          // A part may listen for several names, each checked: none of these is one.
          el.setAttribute("data-ui-live-event", "a\"b <c>");
          document.querySelector("main").append(el);
          document.dispatchEvent(new Event("ui:swap"));
        });
        await sleep(400);
        assert.equal(await page.locator("#oddly").getAttribute("data-live"), null);
        assert.equal((await stats()).open, 1);
        await page.done();
      });

      test("the period select morphs the region in place and the feed keeps following the new period", async () => {
        const page = await open("/display/live");
        await waitLive(page, "on");
        await page.evaluate(() => { window.kept = true; });
        await page.locator("#activity select[name=window]").selectOption("5");
        await page.waitForFunction(() => document.querySelectorAll("#activity-list > li").length === 5);
        assert.equal(await page.evaluate(() => window.kept), true, "no reload");
        assert.match(await page.evaluate(() => document.getElementById("activity-region").getAttribute("data-ui-src")), /window=5/);
        // A select also sends an input event, which asks again 200 ms later: let that settle.
        await sleep(800);
        await waitLive(page, "on");
        await post("/display/test/emit?n=1");
        await page.waitForFunction(() => document.querySelectorAll("#activity-list > li").length === 6);
        assert.equal((await stats()).open, 1);
        await page.done();
      });

      test("a ticked notice stays ticked when the region morphs in place", async () => {
        const page = await open("/display/live");
        await waitLive(page, "on");
        const box = page.locator("#activity [data-slot=reveal]");
        await box.check();
        const sent = page.waitForResponse((r) => /window=20/.test(r.url()));
        await page.locator("#activity select[name=window]").selectOption("20");
        await sent;
        await sleep(900);
        assert.equal(await page.locator("#activity [data-slot=reveal]").isChecked(), true);
        assert.ok((await page.locator("#activity-list [data-filtered]").evaluateAll((all) => all.filter((e) => e.getBoundingClientRect().height > 0).length)) > 0, "the scanner row is still shown");
        await page.done();
      });

      test("a row left out that arrives on a feed with none opens the notice and counts, and a dismissed one stays dismissed", async () => {
        const page = await open("/display/live?window=5");
        await waitLive(page, "on");
        const notice = page.locator("#activity [data-slot=hidden-notice]");
        assert.equal(await notice.count(), 1, "the notice is in the page with nothing left out");
        assert.equal(await notice.isHidden(), true);
        await post("/display/test/emit?n=1&kind=scanner");
        await page.waitForFunction(() => /1 item was hidden/.test(document.querySelector("#activity [data-slot=hidden-notice-text]").textContent));
        assert.equal(await notice.isVisible(), true);
        assert.equal(await page.locator("#activity-list [data-filtered]").count(), 1);
        assert.equal(await page.locator("#activity-list [data-filtered]").first().isVisible(), false, "left out until asked");
        await post("/display/test/emit?n=2&kind=scanner");
        await page.waitForFunction(() => /3 items were hidden/.test(document.querySelector("#activity [data-slot=hidden-notice-text]").textContent));
        await page.locator("#activity [data-slot=reveal]").check();
        assert.equal(await page.locator("#activity-list [data-filtered]").first().isVisible(), true, "ticking shows them in place");
        await page.locator("#activity [data-slot=hidden-notice-dismiss]").click();
        assert.equal(await notice.isHidden(), true);
        await post("/display/test/emit?n=1&kind=scanner");
        await page.waitForFunction(() => document.querySelectorAll("#activity-list [data-filtered]").length === 4);
        await sleep(300);
        assert.equal(await notice.isHidden(), true, "dismissed means dismissed");
        await page.done();
      });

      test("a missed outcome chip keeps its dashed look whatever tone it is given", async () => {
        const page = await open("/display");
        const looks = await page.evaluate(() => {
          const chip = document.querySelector("[data-slot=badge][data-state=missed]");
          const read = () => ({ border: getComputedStyle(chip).borderTopStyle, background: getComputedStyle(chip).backgroundColor });
          const plain = read();
          chip.setAttribute("data-tone", "success");
          return { plain, toned: read() };
        });
        assert.deepEqual(looks.toned, looks.plain);
        assert.equal(looks.toned.border, "dashed");
        await page.done();
      });

      test("the Simulate activity button adds a row through the stream", async () => {
        const page = await open("/display/live");
        await waitLive(page, "on");
        await page.getByRole("button", { name: "Simulate activity" }).click();
        await page.waitForFunction(() => document.querySelectorAll("#activity-list > li").length === 15);
        await page.done();
      });
    });
  });
}
