// The live-region runtime (`live.js` and the feed code in `nav.js`) against servers that
// misbehave: streams that end, refuse or flood; slow answers; hostile rows; a changed
// session. A small server per test (tests/support/stream.mjs), the committed dist/ under
// the crate's exact content security policy. In Chromium, Firefox and WebKit.
//
//   UI_BROWSERS=chromium,firefox,webkit node --test tests/live.test.mjs

import assert from "node:assert/strict";
import { after, before, describe, test } from "node:test";
import { chromium, firefox, webkit } from "playwright";
import { gaps, page as shell, sleep, start, until } from "./support/stream.mjs";

const ENGINES = { chromium, firefox, webkit };
const NAMES = (process.env.UI_BROWSERS || "chromium,firefox").split(",").filter((n) => ENGINES[n]);

const region = (extra = "") => `<div id="r" data-ui-region data-ui-src="/frag" data-ui-live-src="/sse"${extra}>v0</div>`;
const live = (page, id = "r") => page.evaluate((i) => document.getElementById(i)?.getAttribute("data-live"), id);
const waitLive = (page, state, id = "r", timeout = 8000) => page.waitForFunction(([i, s]) => document.getElementById(i)?.getAttribute("data-live") === s, [id, state], { timeout });

/**
 * The runtime reopens a failed stream after 1 s, then 2 s, 4 s, ... (each plus up to a
 * quarter). `times` are the moments the connections arrived: the n-th gap is at least the n-th
 * delay, whatever else the page does meanwhile, and no more than that delay plus the quarter
 * plus a margin for a slow machine (the timers of a page in a busy browser run late).
 */
function followsTheBackoff(times, gapsExpected) {
  const seen = gaps(times);
  assert.ok(seen.length >= gapsExpected, `${times.length} connections, ${seen.length} gaps`);
  for (let i = 0; i < gapsExpected; i++) {
    const delay = 1000 * 2 ** i;
    assert.ok(seen[i] >= delay * 0.9, `connection ${i + 2} came ${seen[i]} ms after connection ${i + 1}: the backoff is ${delay} ms`);
    assert.ok(seen[i] <= delay * 1.25 + 5000, `connection ${i + 2} came ${seen[i]} ms after connection ${i + 1}: the backoff is ${delay} ms`);
  }
}

const feed = (max = 5, extra = "") => `<div id="feed" data-ui-region data-ui-src="/rows" data-ui-live-src="/sse" data-ui-feed="list" data-ui-after="0" data-ui-feed-max="${max}" data-ui-feed-say="{n} new items" data-ui-feed-say-one="1 new item"${extra}><ol id="list"><li id="row-1"><a href="#a1">one</a></li></ol></div>
<div id="later" data-ui-region data-ui-src="/later"><button type="button" data-ui-refresh>refresh later</button><span id="later-text">original</span></div>`;

for (const name of NAMES) {
  describe(name, { concurrency: false }, () => {
    let browser;
    before(async () => {
      // Firefox slows the network of a tab it thinks is in the background while a stream is open.
      browser = await ENGINES[name].launch(name === "firefox" ? { firefoxUserPrefs: { "network.http.throttle.enable": false } } : {});
    });
    after(() => browser.close());

    /** A server, a context and a page; everything is closed after `run`, and page errors fail it. */
    async function scenario(run, { visibility = false } = {}) {
      const srv = await start();
      const context = await browser.newContext();
      if (visibility) {
        await context.addInitScript(() => {
          window.__hidden = false;
          Object.defineProperty(Document.prototype, "hidden", { configurable: true, get: () => window.__hidden });
        });
      }
      const page = await context.newPage();
      const errors = [];
      page.on("pageerror", (e) => errors.push(String(e)));
      try {
        await run({ srv, page, context });
        assert.deepEqual(errors, []);
      } finally {
        await context.close();
        srv.close();
      }
    }

    // ---- The backoff cannot be bypassed ---------------------------------------------------

    test("H1 a stream that says one thing and ends is reopened on the backoff schedule", async () => {
      await scenario(async ({ srv, page }) => {
        srv.routes.set("/p", (req, res) => srv.html(res, shell(region())));
        srv.routes.set("/frag", (req, res) => setTimeout(() => srv.html(res, "v1"), 40));
        srv.routes.set("/sse", (req, res) => {
          if (!("x-ui-probe" in req.headers)) {
            srv.sse.attempts++;
            srv.sse.times.push(Date.now());
          }
          res.writeHead(200, { "content-type": "text/event-stream", "cache-control": "no-store" });
          res.end(": ready\n\ndata: x\n\n");
        });
        await page.goto(srv.origin + "/p");
        // The third connection comes at about 3 s (1 s, then 2 s). Wait for it, not for a time.
        await until(() => srv.sse.attempts >= 3, "the third connection");
        // Bypassed, the backoff would have opened a hundred by now.
        assert.equal(srv.sse.attempts, 3, `attempts when the third came: ${srv.sse.attempts}`);
        followsTheBackoff(srv.sse.times, 2);
        // Each connection owes one catch-up, and the event it says may land while that is out: two a connection.
        assert.ok(srv.count("/frag") <= 2 * srv.sse.attempts, `fetches ${srv.count("/frag")}`);
      });
    });

    test("H1 a refused stream is not reopened by another region's poll or by typing", async () => {
      await scenario(async ({ srv, page }) => {
        srv.routes.set("/p", (req, res) => srv.html(res, shell(region() + `<div id="clock" data-ui-region data-ui-src="/clock" data-ui-poll="500">c0</div><form id="f" action="/search" method="get" data-ui-target="results"><input name="q" aria-label="q"></form><div id="results" data-ui-region data-ui-src="/search">r0</div>`)));
        let c = 0;
        srv.routes.set("/clock", (req, res) => srv.html(res, "c" + ++c));
        srv.routes.set("/search", (req, res, url) => srv.html(res, shell(region() + `<div id="clock" data-ui-region data-ui-src="/clock" data-ui-poll="500">c0</div><form id="f" action="/search" method="get" data-ui-target="results"><input name="q" aria-label="q"></form><div id="results" data-ui-region data-ui-src="/search">r ${url.searchParams.get("q")}</div>`)));
        srv.routes.set("/sse", (req, res) => {
          if (!("x-ui-probe" in req.headers)) {
            srv.sse.attempts++;
            srv.sse.times.push(Date.now());
          }
          res.writeHead(503, { "content-type": "text/plain" });
          res.end("down");
        });
        await page.goto(srv.origin + "/p");
        await until(() => srv.sse.attempts >= 1, "the first connection");
        // Type while the stream waits for its next try (the other region polls meanwhile).
        const typingFrom = Date.now();
        await page.locator("#f input").pressSequentially("navree reconnects", { delay: 200 });
        const typingTo = Date.now();
        // Connections come at 0 s, about 1 s, about 3 s, about 7 s: wait for the fourth.
        await until(() => srv.sse.attempts >= 4, "the fourth connection");
        await until(() => srv.count("/clock") >= 8, "eight polls of the other region");
        assert.ok(srv.sse.attempts <= 5, `attempts ${srv.sse.attempts}`);
        followsTheBackoff(srv.sse.times, 3);
        const whileTyping = srv.sse.times.filter((t) => t >= typingFrom && t <= typingTo).length;
        assert.ok(whileTyping <= 2, `attempts while typing ${whileTyping}`);
        assert.equal(await live(page), "retry");
      });
    });

    // ---- One fetch at a time, and a busy stream still updates ------------------------------

    test("H2 a busy stream with a slow answer still updates, one fetch in flight at a time", async () => {
      await scenario(async ({ srv, page }) => {
        srv.routes.set("/p", (req, res) => srv.html(res, shell(region())));
        srv.routes.set("/frag", srv.slow(800, (res, url, n) => srv.html(res, "v" + n)));
        srv.routes.set("/sse", srv.stream());
        await page.goto(srv.origin + "/p");
        await waitLive(page, "on");
        await page.evaluate(() => {
          window.__seen = [];
          new MutationObserver(() => window.__seen.push(document.getElementById("r").textContent)).observe(document.getElementById("r"), { childList: true, subtree: true, characterData: true });
        });
        const timer = setInterval(() => srv.say("data: x\n\n"), 100);
        await sleep(6000);
        clearInterval(timer);
        const seen = await page.evaluate(() => window.__seen.length);
        assert.ok(seen >= 4, `updates in six busy seconds: ${seen}`);
        assert.equal(srv.gauge.most, 1, "never more than one fetch in flight");
        assert.ok(srv.gauge.asked <= 9, `fetches ${srv.gauge.asked}`);
        await sleep(1500);
        assert.equal(srv.gauge.inflight, 0);
      });
    });

    test("H2 a busy stream with a fast answer makes a few fetches a second, not as many as the answers allow", async () => {
      await scenario(async ({ srv, page }) => {
        srv.routes.set("/p", (req, res) => srv.html(res, shell(region())));
        srv.routes.set("/frag", srv.slow(40, (res, url, n) => srv.html(res, "v" + n)));
        srv.routes.set("/sse", srv.stream());
        await page.goto(srv.origin + "/p");
        await waitLive(page, "on");
        const timer = setInterval(() => srv.say("data: x\n\n"), 50);
        await sleep(4000);
        clearInterval(timer);
        // One a quiet time (300 ms): about thirteen in four seconds. Unbounded it was dozens.
        assert.ok(srv.gauge.asked >= 6 && srv.gauge.asked <= 18, `fetches in four busy seconds: ${srv.gauge.asked}`);
        assert.equal(srv.gauge.most, 1);
      });
    });

    test("H2 a burst of events costs one fetch, and events during a fetch cost one more", async () => {
      await scenario(async ({ srv, page }) => {
        srv.routes.set("/p", (req, res) => srv.html(res, shell(region())));
        srv.routes.set("/frag", srv.slow(900, (res, url, n) => srv.html(res, "v" + n)));
        srv.routes.set("/sse", srv.stream());
        await page.goto(srv.origin + "/p");
        await waitLive(page, "on");
        await sleep(300);
        srv.say("data: x\n\n".repeat(300));
        await sleep(2000);
        assert.equal(srv.gauge.asked, 1, "three hundred events at once, one fetch");
        // An event, then five more while that fetch is on its way: one more fetch after it lands.
        srv.say("data: x\n\n");
        await sleep(500);
        for (let i = 0; i < 5; i++) {
          srv.say("data: x\n\n");
          await sleep(60);
        }
        await sleep(2800);
        assert.equal(srv.gauge.asked, 3, `fetches ${srv.gauge.asked}`);
        assert.equal(srv.gauge.most, 1);
        assert.equal(await page.locator("#r").textContent(), "v3");
      });
    });

    // ---- Events are nudges, and a stream's own names are not events ------------------------

    test("M1 events named open and error are not the connection opening or failing", async () => {
      await scenario(async ({ srv, page }) => {
        srv.routes.set("/p", (req, res) => srv.html(res, shell(region())));
        srv.routes.set("/frag", (req, res) => srv.html(res, "v1"));
        let n = 0;
        srv.routes.set("/sse", (req, res) => {
          n++;
          if (n === 1) {
            res.writeHead(503, { "content-type": "text/plain" });
            return res.end("down");
          }
          srv.stream()(req, res);
        });
        await page.goto(srv.origin + "/p");
        await waitLive(page, "on");
        await sleep(1500);
        const base = srv.count("/frag");
        srv.say("event: open\ndata: x\n\n".repeat(300));
        srv.say("event: error\ndata: x\n\n".repeat(50));
        await sleep(2500);
        assert.ok(srv.count("/frag") - base <= 1, `fetches after open and error lines: ${srv.count("/frag") - base}`);
        assert.equal(await live(page), "on");
        assert.equal(srv.sse.open, 1);
        assert.equal(srv.sse.attempts, 1, "one connection after the first was refused");
      });
    });

    test("M1 a region cannot be told to listen for open or error", async () => {
      await scenario(async ({ srv, page }) => {
        srv.routes.set("/p", (req, res) => srv.html(res, shell(`<div id="a" data-ui-region data-ui-src="/frag" data-ui-live-src="/sse" data-ui-live-event="open">a</div><div id="b" data-ui-region data-ui-src="/frag" data-ui-live-src="/sse" data-ui-live-event="error">b</div>`)));
        srv.routes.set("/frag", (req, res) => srv.html(res, "v1"));
        srv.routes.set("/sse", srv.stream());
        await page.goto(srv.origin + "/p");
        await sleep(800);
        assert.equal(await live(page, "a"), null);
        assert.equal(await live(page, "b"), null);
        assert.equal(srv.sse.attempts, 0);
      });
    });

    // ---- A late feed answer is not for the page that asked ---------------------------------

    test("M2 a feed answer that lands after the reader changed the period is dropped", async () => {
      await scenario(async ({ srv, page }) => {
        const pageFor = (w, rows, cursor) => shell(`<form id="period" action="/p" method="get" data-ui-target="feed"><select name="w" aria-label="Period"><option value="60"${w === "60" ? " selected" : ""}>60</option><option value="5"${w === "5" ? " selected" : ""}>5</option></select></form><div id="feed" data-ui-region data-ui-src="/rows?w=${w}" data-ui-live-src="/sse" data-ui-feed="list" data-ui-after="${cursor}"><ol id="list">${rows}</ol></div>`);
        srv.routes.set("/p", (req, res, url) => {
          const w = url.searchParams.get("w") || "60";
          srv.html(res, w === "60" ? pageFor("60", `<li id="e-3">e3 (60)</li>`, "3") : pageFor("5", "", "3"));
        });
        srv.routes.set("/rows", (req, res, url) => {
          if (url.searchParams.get("w") === "60") setTimeout(() => srv.html(res, `<li id="e-4">e4 (60 only)</li>`, { "x-ui-cursor": "4" }), 1200);
          else srv.html(res, "", { "x-ui-cursor": "3" });
        });
        srv.routes.set("/sse", srv.stream());
        await page.goto(srv.origin + "/p?w=60");
        await waitLive(page, "on", "feed");
        srv.say("data: x\n\n");
        await sleep(500);
        await page.locator("#period select").selectOption("5");
        await page.waitForFunction(() => /w=5/.test(document.getElementById("feed").getAttribute("data-ui-src")));
        await sleep(2200);
        assert.deepEqual(await page.locator("#list > li").allTextContents(), [], "the 60-minute answer did not land in the 5-minute list");
        assert.equal(await page.evaluate(() => document.getElementById("feed").getAttribute("data-ui-after")), "3");
      });
    });

    // ---- A changed session closes the old connection ---------------------------------------

    test("L1 a boosted navigation that changes the session reopens the stream", async () => {
      await scenario(async ({ srv, page }) => {
        const body = (who) => `<p>${who}</p><a href="/p2" id="go">next</a>${region()}`;
        srv.routes.set("/p", (req, res) => srv.html(res, shell(body("user A")), { "set-cookie": ["XSRF-TOKEN=aaa; Path=/", "session=A; Path=/; HttpOnly"] }));
        srv.routes.set("/p2", (req, res) => srv.html(res, shell(body("user B")), { "set-cookie": ["XSRF-TOKEN=bbb; Path=/", "session=B; Path=/; HttpOnly"] }));
        srv.routes.set("/frag", (req, res) => srv.html(res, "v1"));
        srv.routes.set("/sse", srv.stream());
        await page.goto(srv.origin + "/p");
        await waitLive(page, "on");
        await page.locator("#go").click();
        await page.waitForFunction(() => document.body.textContent.includes("user B"));
        await page.waitForFunction(() => document.getElementById("r").getAttribute("data-live") === "on");
        await sleep(600);
        assert.equal(srv.sse.attempts, 2, "a second connection for the second user");
        assert.match(srv.sse.cookies[1], /session=B/, "opened as user B");
        assert.equal(srv.sse.open, 1, "the first one is closed");
      });
    });

    // ---- Hostile and heavy feed rows --------------------------------------------------------

    test("L2 a row twice in one answer is added once, and a nested id cannot take another region's", async () => {
      await scenario(async ({ srv, page }) => {
        srv.routes.set("/p", (req, res) => srv.html(res, shell(feed(50))));
        let k = 0;
        srv.routes.set("/rows", (req, res) => {
          k++;
          srv.html(res, k === 1 ? `<li id="dup">a</li><li id="dup">b</li>` : `<li id="r2"><span id="later" data-ui-src="/evil">hijack</span></li>`, { "x-ui-cursor": String(k + 1) });
        });
        srv.routes.set("/later", (req, res) => srv.html(res, `<div id="later" data-ui-region data-ui-src="/later"><button type="button" data-ui-refresh>refresh later</button><span id="later-text">fresh</span></div>`));
        srv.routes.set("/evil", (req, res) => srv.html(res, `<span id="later">evil answer</span>`));
        srv.routes.set("/sse", srv.stream());
        await page.goto(srv.origin + "/p");
        await waitLive(page, "on", "feed");
        srv.say("data: x\n\n");
        await page.waitForFunction(() => document.querySelector("#list > li#dup"));
        assert.equal(await page.evaluate(() => document.querySelectorAll("#list > li#dup").length), 1);
        await sleep(400);
        srv.say("data: x\n\n");
        await sleep(1200);
        await page.locator("#later button").click();
        await sleep(800);
        assert.equal(srv.log.some((l) => l.path === "/evil"), false, "nothing was asked of the nested element");
        assert.equal(srv.log.some((l) => l.path === "/later"), true, "the real region asked its own source");
        assert.equal(await page.locator("#later-text").textContent(), "fresh");
      });
    });

    test("L3 a feed adds elements only, keeps the focused row, and survives a limit of zero or less", async () => {
      await scenario(async ({ srv, page }) => {
        let k = 10;
        srv.routes.set("/p", (req, res, url) => srv.html(res, shell(feed(url.searchParams.get("max") ?? 3))));
        srv.routes.set("/rows", (req, res) => {
          k++;
          srv.html(res, `\n  <!-- row ${k} -->\n  <li id="row-${k}"><a href="#a${k}">${k}</a></li>\n  `, { "x-ui-cursor": String(k) });
        });
        srv.routes.set("/sse", srv.stream());
        await page.goto(srv.origin + "/p");
        await waitLive(page, "on", "feed");
        for (let i = 0; i < 2; i++) {
          srv.say("data: x\n\n");
          await sleep(550);
        }
        await page.locator("#list > li:last-child a").focus();
        const focused = await page.evaluate(() => document.activeElement.getAttribute("href"));
        for (let i = 0; i < 4; i++) {
          srv.say("data: x\n\n");
          await sleep(550);
        }
        assert.equal(await page.evaluate(() => document.activeElement.getAttribute("href")), focused, "the focused row stayed");
        const shape = await page.evaluate(() => ({ nodes: document.getElementById("list").childNodes.length, rows: document.getElementById("list").children.length }));
        assert.equal(shape.nodes, shape.rows, "no whitespace or comment nodes were added");
        assert.ok(shape.rows <= 4, `rows ${shape.rows}`);
      });
      for (const max of ["0", "-1", "abc"]) {
        await scenario(async ({ srv, page }) => {
          srv.routes.set("/p", (req, res) => srv.html(res, shell(feed(max))));
          srv.routes.set("/rows", (req, res) => srv.html(res, `<li id="row-2">2</li>`, { "x-ui-cursor": "2" }));
          srv.routes.set("/sse", srv.stream());
          await page.goto(srv.origin + "/p");
          await waitLive(page, "on", "feed");
          srv.say("data: x\n\n");
          await page.waitForFunction(() => document.getElementById("row-2"), null, { timeout: 5000 });
          const alive = await Promise.race([page.evaluate(() => 1).then(() => "responsive"), sleep(3000).then(() => "hung")]);
          assert.equal(alive, "responsive", `limit ${max}`);
          assert.equal(await page.locator("#list > li").count(), 2, "the default limit of 200 applies");
        });
      }
    });

    test("L5 a busy feed announces once it settles, with the total", async () => {
      await scenario(async ({ srv, page }) => {
        let k = 10;
        srv.routes.set("/p", (req, res) => srv.html(res, shell(feed(200))));
        srv.routes.set("/rows", (req, res) => {
          k++;
          srv.html(res, `<li id="row-${k}">${k}</li>`, { "x-ui-cursor": String(k) });
        });
        srv.routes.set("/sse", srv.stream());
        await page.goto(srv.origin + "/p");
        await waitLive(page, "on", "feed");
        await page.evaluate(() => {
          window.__said = [];
          new MutationObserver(() => {
            const a = document.querySelector("[data-slot=announcer]");
            if (a && !window.__watching) {
              window.__watching = 1;
              if (a.textContent) window.__said.push(a.textContent);
              new MutationObserver(() => window.__said.push(a.textContent)).observe(a, { childList: true, characterData: true, subtree: true });
            }
          }).observe(document.body, { childList: true });
        });
        const timer = setInterval(() => srv.say("data: x\n\n"), 150);
        await sleep(3000);
        clearInterval(timer);
        await sleep(2500);
        const said = await page.evaluate(() => window.__said);
        const rows = (await page.locator("#list > li").count()) - 1;
        assert.ok(said.length >= 1 && said.length <= 2, `announcements ${JSON.stringify(said)}`);
        assert.equal(said.at(-1), `${rows} new items`, "the last one says how many in all");
      });
    });

    // ---- Subscriptions and connections ------------------------------------------------------

    test("L6 a stream closes when its last region is taken out of the page, with no swap", async () => {
      await scenario(async ({ srv, page }) => {
        srv.routes.set("/p", (req, res) => srv.html(res, shell(region())));
        srv.routes.set("/frag", (req, res) => srv.html(res, "v1"));
        srv.routes.set("/sse", srv.stream());
        await page.goto(srv.origin + "/p");
        await waitLive(page, "on");
        await page.evaluate(() => document.getElementById("r").remove());
        await sleep(1500);
        assert.equal(srv.sse.open, 0, "closed without an event or a swap");
      });
    });

    for (const status of [204, 401, 403]) {
      test(`L7 a ${status} answer to the stream ends it: no retry`, async () => {
        await scenario(async ({ srv, page }) => {
          let attempts = 0;
          srv.routes.set("/p", (req, res) => srv.html(res, shell(region())));
          srv.routes.set("/sse", (req, res) => {
            attempts++;
            res.writeHead(status, { "content-type": "text/plain" });
            res.end(status === 204 ? undefined : "no");
          });
          await page.goto(srv.origin + "/p");
          await sleep(6500);
          assert.ok(attempts <= 2, `attempts ${attempts} (the stream and one probe)`);
          assert.equal(await live(page), "closed");
        });
      });
    }

    test("L7 a 503 keeps being retried, on the backoff schedule", async () => {
      await scenario(async ({ srv, page }) => {
        let attempts = 0;
        const times = [];
        srv.routes.set("/p", (req, res) => srv.html(res, shell(region())));
        srv.routes.set("/sse", (req, res) => {
          if (!("x-ui-probe" in req.headers)) {
            attempts++;
            times.push(Date.now());
          }
          res.writeHead(503, { "content-type": "text/plain" });
          res.end("down");
        });
        await page.goto(srv.origin + "/p");
        await until(() => attempts >= 3, "the third connection");
        assert.equal(attempts, 3, `attempts when the third came: ${attempts}`);
        followsTheBackoff(times, 2);
        assert.equal(await live(page), "retry");
      });
    });

    test("L8 a tab shown again opens the stream first, then catches up", async () => {
      await scenario(
        async ({ srv, page }) => {
          srv.routes.set("/p", (req, res) => srv.html(res, shell(region())));
          srv.routes.set("/frag", (req, res) => srv.html(res, "v1"));
          srv.routes.set("/sse", srv.stream());
          await page.goto(srv.origin + "/p");
          await waitLive(page, "on");
          const t = Date.now();
          await page.evaluate(() => { window.__hidden = true; document.dispatchEvent(new Event("visibilitychange")); });
          for (let i = 0; i < 40 && srv.sse.open; i++) await sleep(100);
          assert.equal(srv.sse.open, 0, "no connection while hidden");
          await page.evaluate(() => { window.__hidden = false; document.dispatchEvent(new Event("visibilitychange")); });
          await sleep(1200);
          const order = srv.log.filter((l) => l.t >= t && (l.path === "/frag" || l.path === "/sse")).map((l) => l.path);
          assert.deepEqual(order, ["/sse", "/frag"], "the stream is open before the catch-up asks");
          assert.equal(srv.sse.open, 1);
        },
        { visibility: true },
      );
    });

    test("L10 the dot says it is connecting until the stream opens, never live", async () => {
      await scenario(async ({ srv, page }) => {
        srv.routes.set("/p", (req, res) => srv.html(res, shell(`<div id="r" data-ui-region data-ui-src="/frag" data-ui-live-src="/sse"><span data-slot="live-state"><span data-slot="live-dot"></span><span data-for="connecting">Connecting</span><span data-for="on">Live</span><span data-for="retry">Reconnecting</span><span data-for="paused">Paused</span><span data-for="closed">Not live</span></span></div>`, { css: true })));
        srv.routes.set("/sse", () => {}); // never answers
        await page.goto(srv.origin + "/p");
        await sleep(800);
        assert.equal(await live(page), "connecting");
        const words = await page.evaluate(() => [...document.querySelectorAll("[data-slot=live-state] [data-for]")].filter((e) => getComputedStyle(e).display !== "none").map((e) => e.textContent));
        assert.deepEqual(words, ["Connecting"]);
      });
    });

    // ---- Second pass: a fetch that never ends, a token that never settles, a sign-in in place -------

    test("R1 a refetch that never answers is given up on, and the next event fetches again", async () => {
      await scenario(async ({ srv, page }) => {
        let k = 0;
        let gaveUp = 0;
        let t0 = Date.now();
        srv.routes.set("/p", (req, res) => srv.html(res, shell(region())));
        srv.routes.set("/frag", (req, res) => {
          if (++k === 1) {
            res.on("close", () => (gaveUp ||= Date.now() - t0)); // held for ever, until the page lets go
            return;
          }
          srv.html(res, "v" + k);
        });
        srv.routes.set("/sse", srv.stream());
        await page.goto(srv.origin + "/p");
        await waitLive(page, "on");
        t0 = Date.now();
        for (let i = 0; i < 19; i++) {
          srv.say("data: x\n\n");
          await sleep(1000);
        }
        assert.ok(gaveUp >= 12000 && gaveUp <= 18000, `the page let go of the held fetch after ${gaveUp} ms`);
        assert.ok(k >= 2, `fetches ${k}`);
        assert.match(await page.locator("#r").textContent(), /^v[2-9]/, "the region follows the stream again");
      });
    });

    test("R1 a fetch that cannot be cancelled is superseded by the next event", async () => {
      await scenario(async ({ srv, page, context }) => {
        // The first fetch of the fragment is a promise that never settles, whatever its signal says.
        await context.addInitScript(() => {
          const real = window.fetch.bind(window);
          let first = true;
          window.fetch = (input, init) => {
            const url = String(input instanceof Request ? input.url : input);
            if (first && url.includes("/frag")) {
              first = false;
              return new Promise(() => {});
            }
            return real(input, init);
          };
        });
        srv.routes.set("/p", (req, res) => srv.html(res, shell(region())));
        srv.routes.set("/frag", (req, res) => srv.html(res, "v" + Date.now()));
        srv.routes.set("/sse", srv.stream());
        await page.goto(srv.origin + "/p");
        await waitLive(page, "on");
        const t0 = Date.now();
        let seen = 0;
        for (let i = 0; i < 26 && !seen; i++) {
          srv.say("data: x\n\n");
          await sleep(1000);
          if (srv.count("/frag")) seen = Date.now() - t0;
        }
        assert.ok(seen >= 15000 && seen <= 24000, `the server first saw a fetch of the fragment after ${seen} ms`);
        await page.waitForFunction(() => /^v\d/.test(document.getElementById("r").textContent), null, { timeout: 4000 });
      });
    });

    test("R1 the probe of a refused stream gives up when the stream says nothing", async () => {
      await scenario(async ({ srv, page }) => {
        let probed = 0;
        let gaveUp = 0;
        srv.routes.set("/p", (req, res) => srv.html(res, shell(region())));
        srv.routes.set("/sse", (req, res) => {
          if ("x-ui-probe" in req.headers) {
            probed++;
            // The time is counted from the question, not from the page: loading it takes
            // as long as the engine takes.
            const asked = Date.now();
            res.on("close", () => (gaveUp ||= Date.now() - asked)); // never answers
            return;
          }
          res.writeHead(503, { "content-type": "text/plain" });
          res.end("down");
        });
        await page.goto(srv.origin + "/p");
        // The runtime lets go of the question after ten seconds.
        await until(() => gaveUp > 0, "the probe to be let go of");
        assert.equal(probed, 1, "one probe");
        assert.ok(gaveUp >= 9000 && gaveUp <= 13000, `the probe was let go of after ${gaveUp} ms`);
        assert.equal(await live(page), "retry");
      });
    });

    test("R2 a server that sets a new token on every answer does not make a loop of connections", async () => {
      await scenario(async ({ srv, page }) => {
        let n = 0;
        srv.routes.set("/p", (req, res) => srv.html(res, shell(region()), { "set-cookie": "XSRF-TOKEN=t0; Path=/" }));
        srv.routes.set("/frag", (req, res) => srv.html(res, "v" + ++n, { "set-cookie": `XSRF-TOKEN=t${n}; Path=/` }));
        srv.routes.set("/sse", srv.stream());
        await page.goto(srv.origin + "/p");
        await waitLive(page, "on");
        srv.say("data: x\n\n");
        await sleep(4500);
        // The first change is answered with a new connection; the rest wait for five seconds.
        assert.ok(srv.sse.attempts === 2, `connections in 4.5 s ${srv.sse.attempts}`);
        assert.ok(n <= 4, `fragment fetches ${n}`);
        await sleep(4000);
        // The change after five seconds is still answered: the last connection is the latest reader's.
        assert.ok(srv.sse.attempts <= 3, `connections in 8.5 s ${srv.sse.attempts}`);
        assert.equal(srv.sse.open, 1);
      });
    });

    test("R3 a row whose id is on the page already, anywhere, is not added", async () => {
      await scenario(async ({ srv, page }) => {
        srv.routes.set("/p", (req, res) => srv.html(res, shell(feed(50))));
        srv.routes.set("/rows", (req, res) => srv.html(res, `<li id="later">takes the region's name</li><li id="row-9">nine</li>`, { "x-ui-cursor": "9" }));
        srv.routes.set("/later", (req, res) => srv.html(res, `<div id="later" data-ui-region data-ui-src="/later"><button type="button" data-ui-refresh>refresh later</button><span id="later-text">fresh</span></div>`));
        srv.routes.set("/sse", srv.stream());
        await page.goto(srv.origin + "/p");
        await waitLive(page, "on", "feed");
        srv.say("data: x\n\n");
        await page.waitForFunction(() => document.getElementById("row-9"));
        assert.equal(await page.evaluate(() => document.querySelectorAll("#later").length), 1, "one element has the id");
        assert.equal(await page.locator("#list > li").count(), 2, "the other row of the answer was added");
        await page.locator("#later button").click();
        await page.waitForFunction(() => document.getElementById("later-text")?.textContent === "fresh");
      });
    });

    test("R4 a stream stopped by a 401 starts again when the reader signs in in place", async () => {
      await scenario(async ({ srv, page }) => {
        srv.routes.set("/p", (req, res) => srv.html(res, shell(region() + `<div id="auth" data-ui-region data-ui-src="/auth"><button type="button" data-ui-refresh>sign in</button><span>signed out</span></div>`), { "set-cookie": "XSRF-TOKEN=a; Path=/" }));
        srv.routes.set("/auth", (req, res) => srv.html(res, `<div id="auth" data-ui-region data-ui-src="/auth"><button type="button" data-ui-refresh>sign out</button><span>signed in</span></div>`, { "set-cookie": ["XSRF-TOKEN=b; Path=/", "session=B; Path=/; HttpOnly"] }));
        srv.routes.set("/frag", (req, res) => srv.html(res, "v1"));
        srv.routes.set("/sse", (req, res) => {
          if (!/session=B/.test(req.headers.cookie || "")) {
            res.writeHead(401, { "content-type": "text/plain" });
            return res.end("sign in");
          }
          srv.stream()(req, res);
        });
        await page.goto(srv.origin + "/p");
        await waitLive(page, "closed");
        await page.locator("#auth button").click();
        await waitLive(page, "on");
        assert.equal(srv.sse.open, 1);
        assert.match(srv.sse.cookies.at(-1), /session=B/, "opened as the reader who signed in");
      });
    });

    test("R5 a tab shown while its stream is down is caught up without waiting for the stream", async () => {
      await scenario(
        async ({ srv, page }) => {
          let down = false;
          srv.routes.set("/p", (req, res) => srv.html(res, shell(region())));
          srv.routes.set("/frag", (req, res) => srv.html(res, "v" + Date.now()));
          srv.routes.set("/sse", (req, res) => {
            if (down) {
              res.writeHead(503, { "content-type": "text/plain" });
              return res.end("down");
            }
            srv.stream()(req, res);
          });
          await page.goto(srv.origin + "/p");
          await waitLive(page, "on");
          await page.evaluate(() => { window.__hidden = true; document.dispatchEvent(new Event("visibilitychange")); });
          down = true;
          for (let i = 0; i < 40 && srv.sse.open; i++) await sleep(100);
          const t = Date.now();
          await page.evaluate(() => { window.__hidden = false; document.dispatchEvent(new Event("visibilitychange")); });
          await sleep(4000);
          const caught = srv.log.filter((l) => l.t >= t && l.path === "/frag").length;
          assert.ok(caught >= 1, `fetches after the tab was shown, with the stream down: ${caught}`);
          assert.equal(await live(page), "retry");
        },
        { visibility: true },
      );
    });
  });
}
