// The support inbox at full size: 100,000 conversations of 10 to 100 messages each,
// about 5.5 million messages, read lazily. It measures what a reader waits for and
// what the page costs, and holds the page to a flat size after scrolling through
// 2,000 conversations. Needs the app serving at that size in a release build:
//
//   INBOX_SIMULATOR=off INBOX_TICK=off INBOX_TEST_HOOKS=1 cargo run --release -p inbox-demo
//
// INBOX_SCALE_URL points at it (the suite is skipped without it); INBOX_NUMBERS=<file>
// writes every number measured as JSON. Chromium only: it reads the JS heap through the
// DevTools protocol, which the other engines don't offer.

import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { after, before, describe, test } from "node:test";
import { chromium } from "playwright";

const BASE = process.env.INBOX_SCALE_URL;
const NUMBERS = process.env.INBOX_NUMBERS;
const numbers = {};
const RUNS = 5;

const median = (values) => {
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.floor(sorted.length / 2)];
};
const percentile = (values, p) => {
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.min(sorted.length - 1, Math.ceil((p / 100) * sorted.length) - 1)];
};
const round = (n) => Math.round(n * 10) / 10;

let size = null;
if (BASE) {
  try {
    size = (await (await fetch(`${BASE}/__test/conversation/101000`)).json()).conversation;
  } catch {}
}

describe("inbox at full size", { concurrency: false, skip: size ? false : "INBOX_SCALE_URL isn't serving 100,000 conversations" }, () => {
  let browser;
  before(async () => {
    browser = await chromium.launch({ args: ["--enable-precise-memory-info"] });
  });
  after(async () => {
    await browser?.close();
    if (NUMBERS) writeFileSync(NUMBERS, JSON.stringify(numbers, null, 2));
  });

  /** One cold load of `path`: time to first byte and first paint, the largest paint, and the bytes sent. */
  async function load(path) {
    const context = await browser.newContext({ viewport: { width: 1440, height: 900 } });
    const page = await context.newPage();
    const bytes = { document: 0, script: 0, stylesheet: 0, font: 0, other: 0 };
    const client = await context.newCDPSession(page);
    await client.send("Network.enable");
    const kinds = new Map();
    client.on("Network.responseReceived", (e) => kinds.set(e.requestId, e.type));
    client.on("Network.loadingFinished", (e) => {
      const kind = (kinds.get(e.requestId) || "other").toLowerCase();
      bytes[kind in bytes ? kind : "other"] += e.encodedDataLength;
    });
    await page.goto(BASE + path, { waitUntil: "load" });
    await page.waitForTimeout(400);
    const timing = await page.evaluate(
      () =>
        new Promise((done) => {
          const nav = performance.getEntriesByType("navigation")[0];
          const fcp = performance.getEntriesByName("first-contentful-paint")[0]?.startTime ?? 0;
          let lcp = 0;
          new PerformanceObserver((list) => {
            for (const entry of list.getEntries()) lcp = Math.max(lcp, entry.startTime);
          }).observe({ type: "largest-contentful-paint", buffered: true });
          setTimeout(() => done({ ttfb: nav.responseStart - nav.startTime, server: nav.responseStart - nav.requestStart, fcp, lcp, dom: nav.domContentLoadedEventEnd - nav.startTime, nodes: document.getElementsByTagName("*").length }), 100);
        }),
    );
    await context.close();
    return { ...timing, bytes };
  }

  async function measure(label, path) {
    const runs = [];
    for (let i = 0; i < RUNS; i++) runs.push(await load(path));
    const result = {
      ttfb_ms: round(median(runs.map((r) => r.ttfb))),
      server_ms: round(median(runs.map((r) => r.server))),
      first_paint_ms: round(median(runs.map((r) => r.fcp))),
      largest_paint_ms: round(median(runs.map((r) => r.lcp))),
      dom_ready_ms: round(median(runs.map((r) => r.dom))),
      dom_nodes: runs[0].nodes,
      bytes: Object.fromEntries(Object.entries(runs[0].bytes).map(([k, v]) => [k, v])),
      bytes_total: Object.values(runs[0].bytes).reduce((a, b) => a + b, 0),
      runs: RUNS,
    };
    numbers[label] = result;
    return result;
  }

  test("the inbox's first screen: one page of fifty, whatever the desk holds", async () => {
    const result = await measure("inbox first screen", "/inbox");
    assert.ok(result.largest_paint_ms < 1500, JSON.stringify(result));
    assert.ok(result.bytes.document < 60 * 1024, "the page is small: " + result.bytes.document);
  });

  test("opening a conversation of 100 messages draws the newest thirty", async () => {
    const id = 1008;
    const held = (await (await fetch(`${BASE}/__test/conversation/${id}`)).json()).conversation;
    assert.equal(held.messages, 100);
    const result = await measure("open a 100-message conversation", `/inbox/${id}?view=closed`);
    assert.ok(result.largest_paint_ms < 1500, JSON.stringify(result));
  });

  test("scrolling to the oldest of 100 messages", async () => {
    const context = await browser.newContext({ viewport: { width: 1440, height: 900 } });
    const page = await context.newPage();
    let asked = 0, bytes = 0;
    page.on("response", async (r) => {
      if (r.url().includes("/messages?")) {
        asked++;
        bytes += (await r.body().catch(() => Buffer.alloc(0))).length;
      }
    });
    await page.goto(`${BASE}/inbox/1008?view=closed`);
    const started = Date.now();
    const body = page.locator("#thread [data-slot=split-pane-body]");
    for (;;) {
      const n = await page.evaluate(() => document.querySelectorAll("#thread-messages li[data-key]").length);
      if (n >= 100) break;
      await body.evaluate((el) => (el.scrollTop = -el.scrollHeight));
      await page.waitForFunction((n) => document.querySelectorAll("#thread-messages li[data-key]").length > n, n, { timeout: 10000 });
    }
    numbers["scroll to the oldest of 100 messages"] = { ms: Date.now() - started, fetches: asked, bytes_uncompressed: bytes };
    assert.equal(asked, 3, "three pages of thirty, after the first");
    await context.close();
  });

  test("after scrolling through 2,000 conversations the page holds the same few hundred rows and the heap stays flat", async () => {
    const context = await browser.newContext({ viewport: { width: 1440, height: 900 } });
    const page = await context.newPage();
    const client = await context.newCDPSession(page);
    await page.goto(`${BASE}/inbox?view=closed`);
    const seen = new Set();
    const samples = [];
    const body = page.locator("#conversations [data-slot=split-pane-body]");
    const heap = async () => {
      await client.send("HeapProfiler.collectGarbage");
      return (await client.send("Runtime.getHeapUsage")).usedSize;
    };
    let next = 250;
    while (seen.size < 2000) {
      const keys = await page.evaluate(() => [...document.querySelectorAll("#conversation-list li[data-key]")].map((li) => li.getAttribute("data-key")));
      keys.forEach((key) => seen.add(key));
      if (seen.size >= next) {
        samples.push({ seen: seen.size, rows: keys.length, nodes: await page.evaluate(() => document.getElementsByTagName("*").length), heap_kb: Math.round((await heap()) / 1024) });
        next += 250;
      }
      const before = keys.at(-1);
      await body.evaluate((el) => (el.scrollTop = el.scrollHeight));
      await page.waitForFunction((last) => document.querySelector("#conversation-list li[data-key]:last-child")?.getAttribute("data-key") !== last, before, { timeout: 10000 });
    }
    numbers["scroll through 2,000 conversations"] = samples;
    const nodes = samples.map((s) => s.nodes);
    const heaps = samples.map((s) => s.heap_kb);
    assert.ok(Math.max(...samples.map((s) => s.rows)) <= 300 + 50, "the list keeps at most its cap and a page");
    assert.ok(Math.max(...nodes) <= nodes[1] * 1.25, `nodes stay flat: ${nodes}`);
    assert.ok(Math.max(...heaps) <= heaps[1] * 1.5, `the heap stays flat: ${heaps}`);
    await context.close();
  });

  test("every query reads by an index: list, thread, search, live", async () => {
    const plans = await (await fetch(`${BASE}/__test/explain`)).text();
    numbers["query plans"] = plans;
    const sections = plans.split("\n## ").map((part) => part.replace(/^## /, ""));
    for (const section of sections) {
      const [name] = section.split("\n");
      if (!name.trim()) continue;
      const plan = section.split("\n").slice(2).join("\n");
      assert.ok(/USING (COVERING )?INDEX|USING INTEGER PRIMARY KEY|VIRTUAL TABLE INDEX/.test(plan), `${name}: ${plan}`);
      assert.ok(!/SCAN conversations\b(?! USING)|SCAN messages\b(?! USING)/.test(plan), `${name} scans a table: ${plan}`);
      if (!name.startsWith("count")) assert.ok(!/USE TEMP B-TREE FOR ORDER BY/.test(plan) || name.startsWith("search"), `${name} sorts: ${plan}`);
    }
  });

  test("server times at full size, 200 requests each, four at a time", async () => {
    const page = (await (await fetch(`${BASE}/inbox/rows?view=closed`)).text()).match(/data-cursor="(\d{24})"/g) || [];
    const cursors = page.map((m) => m.slice(13, 37));
    const deep = [];
    // Cursors spread across the closed list: one page after another, 40 deep.
    let cursor = cursors.at(-1);
    for (let i = 0; i < 40 && cursor; i++) {
      const html = await (await fetch(`${BASE}/inbox/rows?view=closed&dir=end&cursor=${cursor}`)).text();
      const found = html.match(/data-cursor="(\d{24})"/g) || [];
      cursor = found.at(-1)?.slice(13, 37);
      if (cursor) deep.push(cursor);
    }
    const kinds = {
      "list: the first page": () => `/inbox/rows`,
      "list: a page deep in the closed list": (i) => `/inbox/rows?view=closed&dir=end&cursor=${deep[i % deep.length]}`,
      "search": (i) => `/inbox/rows?view=closed&q=${["booking", "invoice", "webhook", "refund", "calendar"][i % 5]}`,
      "thread: the newest thirty": (i) => `/inbox/${1001 + ((i * 37) % 99000)}/messages`,
      "thread: thirty older": (i) => `/inbox/${1001 + ((i * 37) % 99000)}/messages?dir=start&cursor=999999999999`,
      "page: the inbox": () => `/inbox`,
      "page: a conversation": (i) => `/inbox/${1001 + ((i * 53) % 99000)}?view=closed`,
    };
    const out = {};
    for (const [label, path] of Object.entries(kinds)) {
      const times = [];
      let next = 0;
      const worker = async () => {
        while (next < 200) {
          const i = next++;
          const started = performance.now();
          const response = await fetch(BASE + path(i));
          await response.arrayBuffer();
          assert.equal(response.status, 200, path(i));
          times.push(performance.now() - started);
        }
      };
      await Promise.all([worker(), worker(), worker(), worker()]);
      out[label] = { p50_ms: round(percentile(times, 50)), p95_ms: round(percentile(times, 95)), p99_ms: round(percentile(times, 99)), requests: times.length };
    }
    numbers["server times"] = out;
    for (const [label, result] of Object.entries(out)) assert.ok(result.p95_ms < 250, `${label}: ${JSON.stringify(result)}`);
  });
});
