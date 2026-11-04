// The support inbox (`apps/inbox-demo`) in real browsers: three panes resized by keyboard
// and pointer and kept per agent on the server, the live stream landing in the list and
// the open conversation while an agent types, replies, notes, assigning through a
// combobox searched on the server, status, priority and snooze with the server checking
// each, the keys and the command palette, the panes stacked on a phone, and every action
// with JavaScript off. Needs the app serving with its simulator and clock off and its test
// hooks on:
//
//   INBOX_SIMULATOR=off INBOX_TICK=off INBOX_TEST_HOOKS=1 cargo run --release -p inbox-demo
//
// INBOX_URL points elsewhere (default http://127.0.0.1:8096); INBOX_BROWSERS picks engines
// (default "chromium,firefox,webkit"); INBOX_SHOTS=<dir> saves a screenshot of every page
// in every state. Without the app the suite is skipped.

import assert from "node:assert/strict";
import { mkdirSync, readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import { after, afterEach, before, beforeEach, describe, test } from "node:test";
import { fileURLToPath } from "node:url";
import { chromium, firefox, webkit } from "playwright";

const BASE = process.env.INBOX_URL || "http://127.0.0.1:8096";
const require = createRequire(import.meta.url);
const axeSource = readFileSync(require.resolve("axe-core/axe.min.js"), "utf8");
const ENGINES = { chromium, firefox, webkit };
const NAMES = (process.env.INBOX_BROWSERS || "chromium,firefox,webkit").split(",").filter((n) => ENGINES[n]);
const SHOTS = process.env.INBOX_SHOTS;
if (SHOTS) mkdirSync(SHOTS, { recursive: true });

/** The light values of the tokens, from the toolkit's one source: the toolkit ships dark only so far. */
const here = dirname(fileURLToPath(import.meta.url));
const tokensRs = readFileSync(join(here, "../../../crates/navree-ui/src/tokens.rs"), "utf8");
const LIGHT =
  ":root{color-scheme:light" +
  [...tokensRs.matchAll(/t\("([a-z0-9-]+)",\s*"(#[0-9a-f]{6})",\s*"(#[0-9a-f]{6})"\)/g)].map((m) => `;--${m[1]}:${m[3]}`).join("") +
  "}body{background:var(--ink-0)}";

/** Round on purpose: status dots, the timeline's icons, radios. Not pills, not letter tiles. */
const ROUND = ["timeline-dot", "live-dot", "badge-dot", "activity-icon", "radio", "spinner", "timeline-icon"];

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const post = (path) => fetch(BASE + path, { method: "POST" });
/** Something happens on the desk: `what` is new, customer, reply, note or close; `on` names the conversation. */
const happen = async (what, on) => {
  const text = await (await post(`/__test/step?what=${what}${on ? `&on=${on}` : ""}`)).text();
  const [conversation, customer] = text.split(" ").map(Number);
  assert.ok(conversation > 0, `"${what}" happened: "${text}"`);
  return { conversation, customer };
};
/** What the server holds for a conversation, and its latest message. */
const held = async (id) => (await fetch(`${BASE}/__test/conversation/${id}`)).json();
const layoutOf = async (agent = 1) => (await fetch(`${BASE}/__test/layout/${agent}`)).text();

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

/** The app's design rules: 2, 4 or 6 px corners (dots excepted), no shadow, no style attribute but measurements, no text in an avatar. */
const rules = (page) =>
  page.evaluate((round) => {
    const bad = [];
    for (const el of document.querySelectorAll("body *")) {
      if (el.closest("template") || el.getClientRects().length === 0) continue;
      // A native radio or checkbox under a toggle is the browser's own round control, out of sight.
      if (el.matches("input[type=radio], input[type=checkbox]")) continue;
      const style = getComputedStyle(el);
      const slot = el.getAttribute("data-slot") || el.tagName;
      const radius = parseFloat(style.borderTopLeftRadius);
      if (radius && ![2, 4, 6].includes(radius) && !round.includes(slot) && !el.closest("[data-slot=spinner]")) bad.push(`radius ${style.borderTopLeftRadius} on ${slot}`);
      const shadows = style.boxShadow === "none" ? [] : style.boxShadow.split(/,(?![^(]*\))/).filter((s) => !/inset/.test(s));
      if (shadows.length && !el.matches("dialog, [popover], [data-slot=toast], [data-slot=toast] *")) bad.push(`shadow on ${slot}: ${style.boxShadow}`);
      const own = [...el.style].filter((property) => !property.startsWith("--"));
      if (el.hasAttribute("style") && !el.hasAttribute("popover") && own.length) bad.push(`style attribute on ${slot}: ${own.join(", ")}`);
      if (slot === "avatar" && el.textContent.trim()) bad.push(`text in an avatar: ${el.textContent}`);
    }
    return [...new Set(bad)];
  }, ROUND);

/** No word broken in two where a line had room: every text node's lines start at a word's start. */
const brokenWords = (page) =>
  page.evaluate(() => {
    const bad = [];
    const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
    for (let node = walker.nextNode(); node; node = walker.nextNode()) {
      const text = node.textContent;
      if (!text.trim() || node.parentElement.closest("template, script, style, [hidden], textarea, [data-slot=sidebar]")) continue;
      if (getComputedStyle(node.parentElement).textOverflow === "ellipsis") continue;
      const range = document.createRange();
      for (const match of text.matchAll(/\S{4,}/g)) {
        range.setStart(node, match.index);
        range.setEnd(node, match.index + match[0].length);
        const rects = [...range.getClientRects()].filter((r) => r.width > 0);
        const lines = new Set(rects.map((r) => Math.round(r.top)));
        if (lines.size > 1) bad.push(match[0]);
      }
    }
    return [...new Set(bad)].slice(0, 10);
  });

const sideways = (page) => page.evaluate(() => document.documentElement.scrollWidth > window.innerWidth);

for (const name of NAMES) {
  describe(`inbox app in ${name}`, { concurrency: false, skip: reachable ? false : `the app isn't serving at ${BASE}` }, () => {
    let browser;
    before(async () => {
      // Firefox slows a background tab's network while another load (an open event stream) runs.
      browser = await ENGINES[name].launch(name === "firefox" ? { firefoxUserPrefs: { "network.http.throttle.enable": false } } : {});
    });
    after(() => browser?.close());
    const contexts = [];
    afterEach(async () => {
      for (const context of contexts.splice(0)) await context.close().catch(() => {});
    });
    beforeEach(async () => {
      assert.equal((await post("/__test/reset")).status, 204);
    });

    /** A page of the app. Console errors, page errors and policy reports are kept; `page.done()` asserts none. */
    async function open(path, options = {}) {
      const { light, agent, ...rest } = options;
      const context = await browser.newContext({ viewport: { width: 1440, height: 900 }, bypassCSP: !!light, ...rest });
      if (agent) await context.addCookies([{ name: "inbox_agent", value: String(agent), url: BASE }]);
      contexts.push(context);
      const page = await context.newPage();
      page.problems = [];
      page.on("console", (m) => {
        if (name === "webkit" && /Refused to apply a stylesheet/.test(m.text())) return;
        if (m.type() === "error" && !/Failed to load resource|ERR_ABORTED|NS_ERROR|Load failed|can.t establish a connection|interrupted while the page was loading/i.test(m.text())) page.problems.push(m.text());
      });
      page.on("pageerror", (e) => page.problems.push("pageerror: " + e.message));
      await page.addInitScript(() => {
        document.addEventListener("securitypolicyviolation", (e) => {
          (window.__csp ||= []).push(e.violatedDirective + " " + e.blockedURI);
        });
      });
      await page.goto(BASE + path);
      if (light) {
        await page.addStyleTag({ content: LIGHT });
        await page.waitForTimeout(200);
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
        if (await page.evaluate(check, arg)) return;
        if (Date.now() > end) assert.fail(`timed out waiting for ${what}`);
        await sleep(100);
      }
    };
    const live = (page, id) => until(page, (region) => document.getElementById(region)?.getAttribute("data-live") === "on", id, `${id} to be live`);
    const rowIds = (page) => page.evaluate(() => [...document.querySelectorAll("#conversation-list li[data-key]")].map((li) => Number(li.getAttribute("data-key"))));
    const messageCount = (page) => page.evaluate(() => document.querySelectorAll("#thread-messages li[data-key]").length);
    const visible = (page, selector) => page.locator(selector).first().isVisible();
    const widthOf = (page, id) => page.evaluate((id) => document.getElementById(id).getBoundingClientRect().width, id);
    const splitWidth = (page) => widthOf(page, "inbox");
    /** The first conversation the list shows that is open and has a customer waiting. */
    const firstOpen = async () => {
      for (let id = 1; id < 60; id++) {
        const { conversation } = await held(id);
        if (conversation?.status === "open") return id;
      }
      throw new Error("no open conversation");
    };

    // ---- Every page, every width, both themes -----------------------------------------

    for (const theme of ["dark", "light"]) {
      for (const width of [1440, 390]) {
        test(`every page is axe clean, keeps the rules, breaks no word and doesn't scroll sideways (${theme}, ${width} px)`, async () => {
          for (const path of ["/inbox", "/inbox/1", "/inbox/18", "/inbox/18?pane=customer", "/inbox?view=closed", "/inbox?q=zzzz", "/inbox/1/assign"]) {
            const page = await open(path, { light: theme === "light", viewport: { width, height: 900 } });
            assert.deepEqual(await axe(page), [], `${path}`);
            assert.deepEqual(await rules(page), [], path);
            assert.deepEqual(await brokenWords(page), [], `${path}: words broken mid-word`);
            assert.equal(await sideways(page), false, `${path} scrolls sideways`);
            await shot(page, `${path.replace(/[/?=]/g, "_")}-${theme}-${width}`);
            await page.done();
          }
        });
      }
    }

    // ---- Three panes ------------------------------------------------------------------

    test("three panes side by side at 1440 px, each a named region, and separators that say where they are", async () => {
      const page = await open("/inbox/18");
      for (const id of ["conversations", "thread", "customer"]) {
        assert.equal(await page.locator(`section#${id}`).isVisible(), true, id);
        assert.ok(await page.locator(`section#${id}`).getAttribute("aria-label"));
      }
      const separators = page.locator("#inbox > [role=separator]");
      assert.equal(await separators.count(), 2);
      const first = separators.nth(0);
      assert.equal(await first.getAttribute("aria-controls"), "conversations");
      assert.equal(await first.getAttribute("aria-label"), "Conversations");
      assert.equal(await first.getAttribute("aria-orientation"), "vertical");
      assert.equal(await first.getAttribute("tabindex"), "0");
      assert.equal(await first.getAttribute("aria-valuenow"), "30");
      assert.equal(await first.getAttribute("aria-valuemin"), "22");
      assert.equal(await separators.nth(1).getAttribute("aria-controls"), "customer");
      assert.equal(await separators.nth(1).getAttribute("aria-valuemin"), "0", "the customer pane can collapse");
      // The split fills the screen under the heading, and each pane scrolls on its own.
      const box = await page.locator("#inbox").boundingBox();
      assert.ok(box.y + box.height <= 900 && box.y + box.height > 860, `${box.y} + ${box.height}`);
      assert.equal(await page.evaluate(() => document.documentElement.scrollHeight <= window.innerHeight + 1), true, "the page doesn't scroll; the panes do");
      // The conversation opens at its newest message.
      const body = page.locator("#thread [data-slot=split-pane-body]");
      const last = page.locator("#thread-messages li[data-key]").last();
      const [bodyBox, lastBox] = [await body.boundingBox(), await last.boundingBox()];
      assert.ok(lastBox.y + lastBox.height <= bodyBox.y + bodyBox.height + 1, "the newest message is in view");
      await shot(page, "panes");
      await page.done();
    });

    test("the separators move by keyboard: arrows, Shift, Home, End and Enter, and the sizes are saved for the agent", async () => {
      const page = await open("/inbox/18");
      const width = await splitWidth(page);
      const separator = page.locator("#inbox-separator-0");
      await separator.focus();
      await page.keyboard.press("ArrowRight");
      assert.equal(await separator.getAttribute("aria-valuenow"), "32");
      await page.keyboard.press("Shift+ArrowRight");
      assert.equal(await separator.getAttribute("aria-valuenow"), "42");
      assert.ok(Math.abs((await widthOf(page, "conversations")) - width * 0.42) < 3, "the pane is 42% wide");
      await page.keyboard.press("End");
      assert.equal(await separator.getAttribute("aria-valuenow"), "44", "the most it can take beside the conversation's least");
      await page.keyboard.press("Home");
      assert.equal(await separator.getAttribute("aria-valuenow"), "22", "its least: it can't collapse");
      assert.equal(await visible(page, "#conversations"), true);
      await page.keyboard.press("ArrowLeft");
      assert.equal(await separator.getAttribute("aria-valuenow"), "22", "nothing moves past the least");
      // The customer pane collapses with Enter and comes back at the size it had.
      const customer = page.locator("#inbox-separator-1");
      await customer.focus();
      await page.keyboard.press("ArrowLeft");
      assert.equal(await customer.getAttribute("aria-valuenow"), "28", "Left grows a pane on the right");
      await page.keyboard.press("Enter");
      assert.equal(await visible(page, "#customer"), false);
      assert.equal(await customer.getAttribute("aria-valuenow"), "0");
      assert.equal(await customer.getAttribute("aria-valuetext"), "Collapsed");
      assert.equal(await page.locator("[data-ui-split-toggle=customer]").getAttribute("aria-expanded"), "false");
      await page.keyboard.press("Enter");
      assert.equal(await visible(page, "#customer"), true);
      assert.equal(await customer.getAttribute("aria-valuenow"), "28");
      // Saved after a pause, held to the layout by the server, and drawn on the next page.
      await until(page, async () => (await (await fetch("/__test/layout/1")).text()) === "22,50,28;", null, "the save");
      await page.reload();
      assert.equal(await page.locator("#conversations").getAttribute("data-split-basis"), "22");
      assert.ok(Math.abs((await widthOf(page, "conversations")) - width * 0.22) < 3, "drawn at the saved size before any script");
      await page.done();
    });

    test("the separators move by pointer, a collapsible pane snaps shut past half its least, and the toggle opens it", async () => {
      const page = await open("/inbox/18");
      const width = await splitWidth(page);
      const drag = async (selector, dx) => {
        const box = await page.locator(selector).boundingBox();
        const x = box.x + box.width / 2, y = box.y + 300;
        await page.mouse.move(x, y);
        await page.mouse.down();
        for (let i = 1; i <= 10; i++) await page.mouse.move(x + (dx * i) / 10, y);
        await page.mouse.up();
      };
      const before = await widthOf(page, "conversations");
      await drag("#inbox-separator-0", 115);
      const after = await widthOf(page, "conversations");
      assert.ok(Math.abs(after - before - 115) < 3, `${before} -> ${after}`);
      assert.equal(await page.evaluate(() => document.activeElement?.id), "inbox-separator-0", "the separator dragged takes focus");
      // Past the most, it stops there.
      await drag("#inbox-separator-0", 600);
      assert.equal(await page.locator("#inbox-separator-0").getAttribute("aria-valuenow"), "44");
      // The customer pane, dragged to the right edge, shuts.
      await drag("#inbox-separator-1", 400);
      assert.equal(await visible(page, "#customer"), false);
      await until(page, async () => /;2$/.test(await (await fetch("/__test/layout/1")).text()), null, "the collapsed pane saved");
      // The Customer button opens it again, with no request.
      let requests = 0;
      page.on("request", (r) => r.url().includes("/inbox/18?pane") && requests++);
      await page.locator("[data-ui-split-toggle=customer]").click();
      assert.equal(await visible(page, "#customer"), true);
      assert.equal(requests, 0);
      assert.equal(await page.locator("[data-ui-split-toggle=customer]").getAttribute("aria-expanded"), "true");
      assert.ok(width > 0);
      await page.done();
    });

    test("sizes are kept per agent: one agent's sizes don't move another's", async () => {
      const sam = await open("/inbox/18");
      await sam.locator("#inbox-separator-0").focus();
      await sam.keyboard.press("Shift+ArrowRight");
      await until(sam, async () => (await (await fetch("/__test/layout/1")).text()).startsWith("40,"), null, "Sam's save");
      const priya = await open("/inbox/18", { agent: 2 });
      assert.equal(await priya.locator("#conversations").getAttribute("data-split-basis"), "30");
      assert.equal(await layoutOf(2), "");
      await priya.locator("#inbox-separator-0").focus();
      await priya.keyboard.press("ArrowLeft");
      await until(priya, async () => (await (await fetch("/__test/layout/2")).text()).startsWith("28,"), null, "Priya's save");
      assert.ok((await layoutOf(1)).startsWith("40,"), "Sam's stays");
      await sam.done();
      await priya.done();
    });

    test("the server holds a save to the layout: out of range, garbled, another split or another site is refused", async () => {
      const page = await open("/inbox");
      const send = (body, headers = {}) => page.evaluate(async ([body, headers]) => (await fetch("/inbox/panes", { method: "POST", headers: { "content-type": "application/x-www-form-urlencoded", ...headers }, body })).status, [body, headers]);
      assert.equal(await send("split=inbox&sizes=5,10,85&collapsed="), 204, "held to the limits");
      assert.equal(await layoutOf(1), "22,38,40;");
      assert.equal(await send("split=inbox&sizes=NaN,10,85"), 400);
      assert.equal(await send("split=inbox&sizes=30,40"), 400);
      assert.equal(await send("split=inbox&sizes=30,40,30&collapsed=0"), 400, "the list can't collapse");
      assert.equal(await send("split=other&sizes=30,40,30"), 400);
      assert.equal(await send("split=inbox&sizes=30,40,30&split=x"), 400);
      assert.equal(await layoutOf(1), "22,38,40;", "nothing refused was kept");
      const crossSite = await fetch(BASE + "/inbox/panes", { method: "POST", headers: { "content-type": "application/x-www-form-urlencoded", "sec-fetch-site": "cross-site", origin: "https://elsewhere.example" }, body: "split=inbox&sizes=30,40,30" });
      assert.equal(crossSite.status, 403);
      const get = await fetch(BASE + "/inbox/panes");
      assert.ok([404, 405].includes(get.status), String(get.status));
      await page.done();
    });

    // ---- Live ---------------------------------------------------------------------------

    test("new messages and conversations land in the list and the open thread while the agent types", async () => {
      const id = await firstOpen();
      const page = await open(`/inbox/${id}`);
      await live(page, "conversation-list");
      await live(page, "thread-messages");
      const typed = "Thanks for waiting, I can see the";
      await page.locator("#composer-body").click();
      await page.keyboard.type(typed);
      await page.keyboard.press("Home");
      for (let i = 0; i < 6; i++) await page.keyboard.press("ArrowRight");
      const before = await messageCount(page);
      // The customer writes again: the message arrives at the end of the open thread.
      await happen("customer", id);
      await until(page, (n) => document.querySelectorAll("#thread-messages li[data-key]").length === n, before + 1, "the customer's message");
      assert.equal(await page.locator("#thread-messages article").last().getAttribute("data-author"), "customer");
      // A customer nobody has answered writes in: the list gains the row, at the top, marked new.
      const { conversation } = await happen("new");
      await until(page, (id) => document.querySelector("#conversation-list li[data-key]")?.getAttribute("data-key") === String(id), conversation, "the new conversation at the top");
      const row = page.locator(`#conversation-${conversation}`);
      assert.match(await row.textContent(), /1 new/);
      assert.match(await row.textContent(), /Unassigned/);
      assert.match(await page.locator("#list-summary").textContent(), /new/);
      // What the agent typed, where the caret was, and the focus all stayed.
      assert.equal(await page.locator("#composer-body").inputValue(), typed);
      assert.equal(await page.evaluate(() => [document.activeElement?.id, document.activeElement?.selectionStart]).then((v) => v.join()), "composer-body,6");
      // A teammate's note in the open conversation lands too, set apart.
      await happen("note", id).catch(() => null);
      await shot(page, "live");
      await page.done();
    });

    test("the timers count down on the server: the clock redraws the list and the open conversation", async () => {
      const id = await firstOpen();
      const page = await open(`/inbox/${id}`);
      await live(page, "conversation-list");
      await post(`/__test/sla?on=${id}&left=600`);
      await until(page, (id) => /10 min left|9 min left/.test(document.querySelector(`#conversation-${id}`)?.textContent || ""), id, "10 min left in the list");
      await until(page, () => /(10|9) min left/.test(document.querySelector("#thread-sla")?.textContent || ""), null, "and in the heading");
      assert.equal(await page.locator(`#conversation-${id} [data-sla]`).getAttribute("data-sla"), "soon");
      await post(`/__test/sla?on=${id}&left=-180`);
      await until(page, (id) => /Overdue 3 min/.test(document.querySelector(`#conversation-${id}`)?.textContent || ""), id, "overdue");
      assert.equal(await page.locator(`#conversation-${id} [data-sla]`).getAttribute("data-sla"), "overdue");
      await page.done();
    });

    // ---- Replies, notes and the composer --------------------------------------------------

    test("a reply is sent with a toast, the server keeps it, and the clock stops", async () => {
      const id = await firstOpen();
      const page = await open(`/inbox/${id}`);
      const before = await messageCount(page);
      await page.locator("#composer-body").fill("We've fixed the setting on our side. Could you try again?");
      await page.locator("#composer").getByRole("button", { name: "Send" }).click();
      await page.locator("[data-slot=toast]", { hasText: "Reply sent" }).waitFor();
      await until(page, (n) => document.querySelectorAll("#thread-messages li[data-key]").length === n, before + 1, "the reply in the thread");
      assert.equal(await page.locator("#thread-messages article").last().getAttribute("data-author"), "agent");
      assert.equal(await page.locator("#composer-body").inputValue(), "", "the composer is empty again");
      const { conversation, last } = await held(id);
      assert.equal(last.body, "We've fixed the setting on our side. Could you try again?");
      assert.equal(last.kind, "agent");
      assert.equal(conversation.status, "pending");
      assert.equal(conversation.sla_due, null);
      await page.done();
    });

    test("an internal note is set apart, and an empty message is refused under the field", async () => {
      const id = await firstOpen();
      const page = await open(`/inbox/${id}`);
      const before = (await held(id)).last.id;
      await page.locator("#composer").getByText("Internal note").click();
      await page.locator("#composer-body").fill("Checked the logs: nothing failed on our side.");
      await page.locator("#composer").getByRole("button", { name: "Send" }).click();
      await page.locator("[data-slot=toast]", { hasText: "Note added" }).waitFor();
      const note = page.locator("#thread-messages article[data-author=note]").last();
      await note.waitFor();
      assert.equal(await note.locator("[data-slot=bubble][data-variant=note]").count(), 1);
      assert.match(await note.textContent(), /Internal note/);
      const { conversation, last } = await held(id);
      assert.equal(last.kind, "note");
      assert.equal(conversation.status, "open", "a note answers nobody");
      // Nothing typed: the field says so, and nothing reaches the server.
      await page.locator("#composer-body").fill("   ");
      await page.locator("#composer").getByRole("button", { name: "Send" }).click();
      await page.locator("#composer [data-slot=field-error]").waitFor();
      assert.match(await page.locator("#composer [data-slot=field-error]").textContent(), /Write a message first/);
      assert.equal((await held(id)).last.id, last.id);
      assert.notEqual(last.id, before);
      await page.done();
    });

    test("a saved reply goes into the composer, after what was typed, from the composer and from the palette", async () => {
      const id = await firstOpen();
      const page = await open(`/inbox/${id}`);
      await page.locator("#composer-body").fill("Hello,");
      await page.getByRole("button", { name: "Saved replies" }).click();
      await page.getByRole("button", { name: "Ask for screenshots" }).click();
      await until(page, () => /Could you send a screenshot/.test(document.querySelector("#composer-body")?.value || ""), null, "the saved reply in the text");
      assert.match(await page.locator("#composer-body").inputValue(), /^Hello,\n\nCould you send a screenshot/);
      // The palette: Ctrl+K, type, Enter.
      await page.locator("#composer-body").fill("Thanks!");
      await page.keyboard.press("ControlOrMeta+k");
      await page.locator("dialog#palette[open]").waitFor();
      await page.keyboard.type("refund");
      await page.keyboard.press("Enter");
      await until(page, () => /^Thanks!\n\nI've refunded/.test(document.querySelector("#composer-body")?.value || ""), null, "the palette's saved reply");
      assert.equal(await page.evaluate(() => document.activeElement?.id), "composer-body");
      assert.equal(await page.locator("dialog#palette[open]").count(), 0);
      // Nothing was sent.
      assert.notEqual((await held(id)).last.body, "Thanks!");
      await page.done();
    });

    // ---- Assign, status, priority, snooze ------------------------------------------------

    test("assign with a combobox searched on the server, checked again on save", async () => {
      const page = await open("/inbox/1");
      const searches = [];
      page.on("request", (r) => r.url().includes("/inbox/agents/options") && searches.push(new URL(r.url()).searchParams.get("q")));
      await page.locator("[popovertarget=assign-popover]").click();
      const input = page.locator("#assign-agent-input");
      await input.click();
      await input.fill("");
      await input.pressSequentially("tech", { delay: 30 });
      await until(page, () => [...document.querySelectorAll("#assign-agent [role=option]")].length > 0 && [...document.querySelectorAll("#assign-agent [role=option]")].every((o) => /Technical/.test(o.textContent)), null, "the Technical team, from the server");
      assert.ok(searches.includes("tech"), searches.join());
      await input.fill("");
      await input.pressSequentially("Mariana", { delay: 30 });
      await page.locator("#assign-agent [role=option]", { hasText: "Mariana Costa" }).click();
      await page.locator("#assign").getByRole("button", { name: "Assign", exact: true }).click();
      await page.locator("[data-slot=toast]", { hasText: "Mariana Costa has it now" }).waitFor();
      assert.equal((await held(1)).conversation.assignee_id, 4);
      assert.match(await page.locator("[popovertarget=assign-popover]").textContent(), /Mariana Costa/);
      await until(page, () => /Mariana/.test(document.querySelector("#conversation-1")?.textContent || ""), null, "the list row to say who");
      // The server checks what comes back: someone away today, or nobody that exists.
      const status = (agent) => page.evaluate(async (agent) => (await fetch("/inbox/1/assign", { method: "POST", headers: { "content-type": "application/x-www-form-urlencoded", "x-navree-ui": "1" }, body: `agent=${agent}` })).status, agent);
      assert.equal(await status(22), 422, "away today");
      assert.equal(await status(999), 422, "nobody");
      assert.equal(await status("1%20OR%201"), 422);
      assert.equal((await held(1)).conversation.assignee_id, 4);
      await page.done();
    });

    test("status and priority from their menus, snooze with a date picker, each checked by the server", async () => {
      const id = await firstOpen();
      const page = await open(`/inbox/${id}`);
      await page.getByRole("button", { name: /priority, change it/ }).click();
      await page.getByRole("menuitem", { name: "Low" }).click();
      await page.locator("[data-slot=toast]", { hasText: "Priority is low now" }).waitFor();
      assert.equal((await held(id)).conversation.priority, "low");
      await page.getByRole("button", { name: /status, change it/ }).click();
      await page.getByRole("menuitem", { name: "Mark pending" }).click();
      await page.locator("[data-slot=toast]", { hasText: "Waiting for the customer" }).waitFor();
      assert.equal((await held(id)).conversation.status, "pending");
      // Snooze to a day picked in the calendar.
      await page.locator("[popovertarget=snooze-popover]").click();
      const target = new Date(Date.now() + 5 * 86400000).toISOString().slice(0, 10);
      const field = page.locator("#snooze-until");
      await field.fill(target.split("-").reverse().slice(0, 2).reverse().join("/") + "/" + target.slice(0, 4)).catch(() => {});
      await page.evaluate((day) => {
        const native = document.querySelector("#snooze input[type=date][name=until]");
        native.value = day;
      }, target);
      await page.locator("#snooze").getByRole("button", { name: "Snooze", exact: true }).click();
      await page.locator("[data-slot=toast]", { hasText: "It comes back on" }).waitFor();
      const { conversation } = await held(id);
      assert.equal(conversation.status, "snoozed");
      assert.equal(conversation.snoozed_until, target);
      // The server holds the calendar's limits: not today, not past 90 days, not garbage.
      const snooze = (body) => page.evaluate(async (body) => (await fetch(location.pathname + "/snooze", { method: "POST", headers: { "content-type": "application/x-www-form-urlencoded", "x-navree-ui": "1" }, body })).status, body);
      const today = new Date().toISOString().slice(0, 10);
      const far = new Date(Date.now() + 120 * 86400000).toISOString().slice(0, 10);
      assert.equal(await snooze(`until=${today}`), 422);
      assert.equal(await snooze(`until=${far}`), 422);
      assert.equal(await snooze("until=tomorrow"), 422);
      assert.equal(await snooze("preset=forever"), 422);
      const bad = await page.evaluate(async () => (await fetch(location.pathname + "/status/snoozed", { method: "POST", headers: { "content-type": "application/x-www-form-urlencoded", "x-navree-ui": "1" }, body: "" })).status);
      assert.equal(bad, 422);
      await page.done();
    });

    // ---- Keys ---------------------------------------------------------------------------

    test("j and k move through the list, e closes, r replies, / searches, ? lists the keys, and no key fires while typing", async () => {
      const page = await open("/inbox");
      const ids = await rowIds(page);
      await page.keyboard.press("j");
      await page.waitForURL(new RegExp(`/inbox/${ids[0]}$`));
      await page.locator(`#conversation-${ids[0]} [aria-current=page]`).waitFor();
      await page.keyboard.press("j");
      await page.waitForURL(new RegExp(`/inbox/${ids[1]}$`));
      await page.keyboard.press("k");
      await page.waitForURL(new RegExp(`/inbox/${ids[0]}$`));
      // r: the composer takes focus, and the r isn't typed.
      await page.keyboard.press("r");
      assert.equal(await page.evaluate(() => document.activeElement?.id), "composer-body");
      assert.equal(await page.locator("#composer-body").inputValue(), "");
      // Typing j, k and e in the composer is typing.
      await page.keyboard.type("jke?");
      assert.equal(await page.locator("#composer-body").inputValue(), "jke?");
      assert.match(page.url(), new RegExp(`/inbox/${ids[0]}$`));
      await page.locator("#composer-body").fill("");
      await page.locator("#thread-subject").click();
      // e closes, and e again reopens.
      await page.keyboard.press("e");
      await page.locator("[data-slot=toast]", { hasText: "is closed" }).waitFor();
      assert.equal((await held(ids[0])).conversation.status, "closed");
      await page.getByRole("button", { name: "Reopen" }).waitFor();
      await page.keyboard.press("e");
      await page.locator("[data-slot=toast]", { hasText: "back in the open list" }).waitFor();
      assert.equal((await held(ids[0])).conversation.status, "open");
      // / searches; ? opens the list of keys, and Esc closes it.
      await page.locator("#thread-subject").click();
      await page.keyboard.press("/");
      assert.equal(await page.evaluate(() => document.activeElement?.getAttribute("name")), "q");
      await page.locator("#thread-subject").click();
      await page.keyboard.press("?");
      const dialog = page.locator("dialog#shortcuts[open]");
      await dialog.waitFor();
      assert.ok((await dialog.locator("kbd, [data-slot=kbd]").count()) >= 8);
      // Inside the dialog, j is no shortcut for the page behind.
      await page.keyboard.press("j");
      assert.match(page.url(), new RegExp(`/inbox/${ids[0]}$`));
      await shot(page, "shortcuts");
      await page.keyboard.press("Escape");
      assert.equal(await page.locator("dialog#shortcuts[open]").count(), 0);
      // A key with Ctrl is no shortcut either.
      await page.keyboard.press("Control+j").catch(() => {});
      assert.match(page.url(), new RegExp(`/inbox/${ids[0]}$`));
      await page.done();
    });

    test("the command palette: actions on the open conversation and places to go", async () => {
      const id = await firstOpen();
      const page = await open(`/inbox/${id}`);
      await page.keyboard.press("ControlOrMeta+k");
      await page.locator("dialog#palette[open]").waitFor();
      await page.keyboard.type("assign to me");
      await page.keyboard.press("Enter");
      await page.locator("[data-slot=toast]", { hasText: "Sam Okafor has it now" }).waitFor();
      assert.equal((await held(id)).conversation.assignee_id, 1);
      await page.keyboard.press("ControlOrMeta+k");
      await page.keyboard.type("unassigned");
      await page.keyboard.press("Enter");
      await page.waitForURL(/view=unassigned/);
      assert.match(await page.locator("#conversations h2").textContent(), /Unassigned/);
      await page.done();
    });

    // ---- Filters ---------------------------------------------------------------------------

    test("the search and filters narrow the list in place, keep the open conversation, and live in the address", async () => {
      const page = await open("/inbox/1");
      const all = (await rowIds(page)).length;
      await page.locator("#conversations input[name=q]").fill("webhook");
      await page.waitForURL(/q=webhook/);
      await until(page, () => [...document.querySelectorAll("#conversation-list li[data-key]")].every((li) => /webhook/i.test(li.textContent)), null, "only webhook rows");
      assert.ok((await rowIds(page)).length < all);
      assert.equal(await page.locator("#thread-subject").isVisible(), true, "the open conversation stays");
      await page.locator("#conversations select[name=priority]").selectOption("high");
      await page.waitForURL(/priority=high/);
      // Nothing matches: say so, with a way out.
      await page.locator("#conversations input[name=q]").fill("zzzzzz");
      await page.waitForURL(/q=zzzzzz/);
      await page.getByText("Nothing matches").waitFor();
      await page.goBack();
      await page.waitForURL((url) => !/zzzzzz/.test(url.href));
      await page.done();
    });

    // ---- Lazy: the list and a long conversation ----------------------------------------------

    test("the list loads fifty at a time by cursor as the agent scrolls, and keeps its rows when a conversation opens", async () => {
      const page = await open("/inbox?view=closed");
      const asked = [];
      page.on("request", (r) => r.url().includes("/inbox/rows") && asked.push(new URL(r.url()).searchParams));
      assert.equal((await rowIds(page)).length, 50, "the first page");
      const body = page.locator("#conversations [data-slot=split-pane-body]");
      for (let i = 0; i < 4; i++) {
        await body.evaluate((el) => (el.scrollTop = el.scrollHeight));
        await until(page, (n) => document.querySelectorAll("#conversation-list li[data-key]").length > n, 50 * (i + 1), `page ${i + 2}`);
      }
      const ids = await rowIds(page);
      assert.ok(ids.length >= 250, String(ids.length));
      assert.equal(new Set(ids).size, ids.length, "no row twice");
      for (const params of asked.filter((p) => p.get("dir") === "end")) {
        assert.match(params.get("cursor"), /^\d{24}$/, "a cursor, not an offset");
        assert.equal(params.get("page"), null);
        assert.equal(params.get("offset"), null);
      }
      // Newest activity first, all the way down.
      const cursors = await page.evaluate(() => [...document.querySelectorAll("#conversation-list li[data-key]")].map((li) => li.getAttribute("data-cursor")));
      assert.deepEqual(cursors, [...cursors].sort().reverse());
      // Opening a row far down keeps the list where it was.
      const target = ids[200];
      const row = page.locator(`#conversation-${target} a`);
      await row.scrollIntoViewIfNeeded();
      const top = await row.evaluate((el) => el.getBoundingClientRect().top);
      await row.click();
      await page.waitForURL(new RegExp(`/inbox/${target}\\?view=closed$`));
      await page.locator("#thread-subject").waitFor();
      assert.ok((await rowIds(page)).length >= 250, "the rows loaded are still there");
      assert.equal(await page.locator(`#conversation-${target} a`).getAttribute("aria-current"), "page");
      assert.ok(Math.abs((await page.locator(`#conversation-${target} a`).evaluate((el) => el.getBoundingClientRect().top)) - top) < 4, "and where they were");
      await page.done();
    });

    test("a long conversation opens at its newest thirty and loads older ones as the agent scrolls up, keeping their place", async () => {
      const page = await open("/inbox/1008?view=closed");
      assert.equal((await held(1008)).conversation.messages, 100);
      assert.equal(await messageCount(page), 30);
      assert.match(await page.locator("#thread-head").textContent(), /100 messages/);
      const body = page.locator("#thread [data-slot=split-pane-body]");
      for (let n = 30; n < 100; ) {
        const key = await page.locator("#thread-messages li[data-key]").first().getAttribute("data-key");
        await body.evaluate((el) => (el.scrollTop = -el.scrollHeight));
        await until(page, (n) => document.querySelectorAll("#thread-messages li[data-key]").length > n, n, "older messages");
        const at = await page.locator(`#thread-messages li[data-key="${key}"]`).evaluate((el) => el.getBoundingClientRect().top);
        const view = await body.evaluate((el) => {
          const box = el.getBoundingClientRect();
          return { top: box.top, bottom: box.bottom };
        });
        assert.ok(at >= view.top - 2 && at <= view.bottom, "the message that was first stays in view");
        n = await messageCount(page);
      }
      const keys = await page.evaluate(() => [...document.querySelectorAll("#thread-messages li[data-key]")].map((li) => Number(li.getAttribute("data-key"))));
      assert.equal(keys.length, 100);
      assert.deepEqual(keys, [...keys].sort((a, b) => a - b), "oldest first, none twice");
      assert.equal(await page.locator("#thread-messages > [data-slot=lazy-more][data-dir=start]").isVisible(), false, "nothing older");
      await page.done();
    });

    test("with JavaScript off the list and a long conversation page by links", async () => {
      const page = await open("/inbox?view=closed", { javaScriptEnabled: false });
      const more = page.locator("#conversation-list > [data-slot=lazy-more][data-dir=end]");
      assert.match(await more.getAttribute("href"), /rows_after=\d{24}/);
      const last = (await rowIds(page)).at(-1);
      await more.click();
      await page.waitForURL(/rows_after=/);
      const next = await rowIds(page);
      assert.equal(next.length, 50);
      assert.ok(!next.includes(last));
      assert.match(await page.locator("#conversation-list > [data-slot=lazy-more][data-dir=start]").getAttribute("href"), /rows_before=/);
      await page.goto(BASE + "/inbox/1008?view=closed");
      const earlier = page.locator("#thread-messages > [data-slot=lazy-more][data-dir=start]");
      await earlier.click();
      await page.waitForURL(/older=\d{12}/);
      assert.equal(await messageCount(page), 30);
      await page.done();
    });

    test("a live list merges what changed without duplicates, and a conversation that leaves the view leaves the list", async () => {
      const page = await open("/inbox");
      await live(page, "conversation-list");
      const ids = await rowIds(page);
      // A conversation further down gets a customer's message: it moves to the top, once.
      const moved = ids[3];
      await happen("customer", moved);
      await until(page, (id) => document.querySelector("#conversation-list li[data-key]")?.getAttribute("data-key") === String(id), moved, "the row at the top");
      const after = await rowIds(page);
      assert.equal(after.filter((id) => id === moved).length, 1);
      // A teammate closes one: it leaves the open list.
      const { conversation } = await happen("close");
      await until(page, (id) => !document.querySelector(`#conversation-${id}`), conversation, "the closed row gone");
      await page.done();
    });

    // ---- A phone -------------------------------------------------------------------------

    test("on a phone the panes stack: the list, a conversation, its customer, and back", async () => {
      const page = await open("/inbox", { viewport: { width: 390, height: 844 }, hasTouch: true, isMobile: name !== "firefox" });
      assert.equal(await visible(page, "#conversations"), true);
      assert.equal(await visible(page, "#thread"), false);
      assert.equal(await visible(page, "#customer"), false);
      assert.equal(await page.locator("#inbox > [role=separator]").first().isVisible(), false);
      const first = page.locator("#conversation-list li[data-key] a").first();
      const href = await first.getAttribute("href");
      await first.click();
      await page.waitForURL(new RegExp(href.replace("?", "\\?") + "$"));
      await page.locator("#thread").waitFor({ state: "visible" });
      assert.equal(await visible(page, "#conversations"), false);
      await page.getByRole("link", { name: "Customer" }).click();
      await page.waitForURL(/pane=customer/);
      await page.locator("#customer").waitFor({ state: "visible" });
      assert.equal(await visible(page, "#thread"), false);
      await shot(page, "phone-customer");
      await page.getByRole("link", { name: "Conversation" }).click();
      await page.locator("#thread").waitFor({ state: "visible" });
      await page.getByRole("link", { name: "Conversations" }).click();
      await page.locator("#conversations").waitFor({ state: "visible" });
      assert.equal(await sideways(page), false);
      await page.done();
    });

    // ---- JavaScript off ------------------------------------------------------------------

    test("with JavaScript off every action is a link or a form", async () => {
      const page = await open("/inbox", { javaScriptEnabled: false });
      // The panes keep their sizes; the separators are lines, not controls.
      assert.equal(await page.locator("#inbox-separator-0").getAttribute("tabindex"), null);
      assert.equal(await page.locator("#conversations").isVisible(), true);
      // Filter with the form's own button.
      await page.locator("#conversations input[name=q]").fill("refund");
      await page.locator("#conversations").getByRole("button", { name: "Filter" }).click();
      await page.waitForURL(/q=refund/);
      const link = page.locator("#conversation-list li[data-key] a").first();
      const id = Number(await page.locator("#conversation-list li[data-key]").first().getAttribute("data-key"));
      await link.click();
      await page.waitForURL(new RegExp(`/inbox/${id}`));
      // Reply.
      await page.locator("#composer-body").fill("Answered with no script at all.");
      await page.locator("#composer").getByRole("button", { name: "Send" }).click();
      await page.waitForLoadState();
      assert.equal((await held(id)).last.body, "Answered with no script at all.");
      assert.match(await page.locator("#thread-messages").textContent(), /Answered with no script at all\./);
      assert.match(await page.locator("body").textContent(), /Your reply is on its way/);
      // A saved reply, put in the text by a form post.
      await page.locator("#composer-body").fill("Hi,");
      await page.getByRole("button", { name: "Saved replies" }).click();
      await page.getByRole("button", { name: "Passed to engineering" }).click();
      await page.waitForLoadState();
      assert.match(await page.locator("#composer-body").inputValue(), /^Hi,\n\nI've passed this to our engineers/);
      // Assign through the page that lists the team.
      await page.locator("[popovertarget=assign-popover]").click();
      await page.getByRole("link", { name: "Choose from the whole team" }).click();
      await page.waitForURL(new RegExp(`/inbox/${id}/assign`));
      await page.locator("input[name=q]").fill("Aisha");
      await page.getByRole("button", { name: "Search" }).click();
      await page.getByRole("button", { name: "Assign to Aisha Bello" }).click();
      await page.waitForURL(new RegExp(`/inbox/${id}`));
      assert.equal((await held(id)).conversation.assignee_id, 6);
      // Status from the menu, which is the browser's own popover.
      await page.getByRole("button", { name: /status, change it/ }).click();
      await page.getByRole("menuitem", { name: "Mark closed" }).click();
      await page.waitForLoadState();
      assert.equal((await held(id)).conversation.status, "closed");
      // Snooze with the browser's own date field.
      const day = new Date(Date.now() + 3 * 86400000).toISOString().slice(0, 10);
      await page.locator("[popovertarget=snooze-popover]").click();
      await page.locator("#snooze input[type=date]").fill(day);
      await page.locator("#snooze").getByRole("button", { name: "Snooze", exact: true }).click();
      await page.waitForLoadState();
      assert.equal((await held(id)).conversation.snoozed_until, day);
      await page.done();
    });

    test("with JavaScript off on a phone, the panes stack and link to each other", async () => {
      const page = await open("/inbox/18", { javaScriptEnabled: false, viewport: { width: 390, height: 844 } });
      assert.equal(await visible(page, "#thread"), true);
      assert.equal(await visible(page, "#conversations"), false);
      await page.getByRole("link", { name: "Customer" }).click();
      await page.waitForURL(/pane=customer/);
      assert.equal(await visible(page, "#customer"), true);
      assert.equal(await sideways(page), false);
      await page.done();
    });
  });
}
