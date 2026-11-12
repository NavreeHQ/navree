// Charts in real browsers: every case of the proof app's /charts page in dark and light,
// at 390 and 1440 px, with axe clean (also with the table shown and a tooltip open);
// keyboard only; the pointer; the legend toggles; the table switch; JavaScript off;
// hostile text and named elements; the report page and its URL; a million points.
// Run with the proof app serving:
//
//   PORT=8132 cargo run --release -p ui-proof -- serve
//   CHARTS_URL=http://127.0.0.1:8132 UI_BROWSERS=chromium,firefox,webkit npm test -- tests/charts.test.mjs
//
// CHARTS_URL is the server (default http://127.0.0.1:8132). UI_BROWSERS picks the engines
// (default all three). UI_SHOTS=<dir> saves a screenshot of every case at both widths in
// both themes. Without the server the suite is skipped.

import assert from "node:assert/strict";
import { mkdirSync, readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import { after, afterEach, before, describe, test } from "node:test";
import { fileURLToPath } from "node:url";
import { chromium, firefox, webkit } from "playwright";
import { page as doc, start } from "./support/fixture.mjs";
import { launch } from "./support/launch.mjs";
import { decodePng } from "./support/png.mjs";

const BASE = process.env.CHARTS_URL || "http://127.0.0.1:8132";
const ENGINES = { chromium, firefox, webkit };
const NAMES = (process.env.UI_BROWSERS || "chromium,firefox,webkit").split(",").filter((n) => ENGINES[n]);
const SHOTS = process.env.UI_SHOTS;
if (SHOTS) mkdirSync(SHOTS, { recursive: true });

const here = dirname(fileURLToPath(import.meta.url));
const crate = join(here, "../../../crates/navree-ui");
const require = createRequire(import.meta.url);
const axeSource = readFileSync(require.resolve("axe-core/axe.min.js"), "utf8");
const chartsModule = readFileSync(join(crate, "dist/charts.js"));

const reachable = await fetch(`${BASE}/charts?case=bar`).then((r) => r.ok, () => false);

const NASTY = `<script>window.__pwned=1</script><img src=x onerror="window.__pwned=1"> "quotes" 'single' javascript:alert(1)`;
const GIF = "data:image/gif;base64,R0lGODlhAQABAAAAACw=";

/** Every case of /charts?case=<name>, with the figures it draws. */
const CASES = {
  line: ["line"],
  "line-many": ["line-many"],
  "line-gaps": ["line-gaps"],
  "line-one": ["line-one"],
  area: ["area"],
  "area-stacked": ["area-stacked"],
  bar: ["bar"],
  "bar-grouped": ["bar-grouped"],
  "bar-stacked": ["bar-stacked"],
  "bar-percent": ["bar-percent"],
  "bar-negative": ["bar-negative"],
  "bar-horizontal": ["bar-horizontal"],
  "bar-horizontal-stacked": ["bar-horizontal-stacked"],
  donut: ["donut"],
  pie: ["pie"],
  sparkline: ["spark-1", "spark-2", "spark-3"],
  funnel: ["funnel"],
  annotations: ["annotations"],
  empty: ["empty"],
  loading: ["loading"],
  failed: ["failed"],
  hostile: ["hostile"],
  extremes: ["extremes"],
  rtl: ["rtl"],
  paint: ["paint"],
  steps: ["steps"],
  ring: ["ring-1", "ring-2", "ring-3"],
  rings: ["rings-1", "rings-2"],
  gauge: ["gauge-1", "gauge-2"],
  clock: ["clock"],
  locale: ["locale-de", "locale-in"],
  shown: ["shown"],
};
/** The figures with a tab stop, tooltips and keys. */
const INTERACTIVE = Object.entries(CASES)
  .filter(([name]) => !["sparkline", "empty", "loading", "failed", "ring", "rings", "gauge"].includes(name))
  .flatMap(([name, figures]) => figures.map((fig) => [name, fig]));
/** The figures with a switch for the table. */
const SWITCHED = INTERACTIVE.filter(([name]) => name !== "shown");

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
/** Polls `check` until it returns something true: the wait is for a state, never for a time. */
async function until(check, what, ms = 8000) {
  const end = Date.now() + ms;
  for (;;) {
    const got = await check();
    if (got) return got;
    if (Date.now() > end) assert.fail(`timed out waiting for ${what}`);
    await sleep(25);
  }
}

/** Waits for the page's transitions to end, so a colour is read at its final value. */
async function settle(page) {
  await page.evaluate(() => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))));
  // An animation that never ends (a loading placeholder's pulse) is not waited for.
  await page.evaluate(() => Promise.all(document.getAnimations().filter((a) => Number.isFinite(a.effect?.getComputedTiming().endTime)).map((a) => a.finished.catch(() => {}))));
}

async function axe(page) {
  await settle(page);
  await page.evaluate(axeSource);
  return page.evaluate(async () => {
    const out = await window.axe.run(document, { runOnly: ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa", "best-practice"] });
    return out.violations.map((v) => `${v.id}: ${v.nodes.slice(0, 3).map((n) => n.target.join(" ")).join(" | ")}`);
  });
}

/** The tooltip, the live region, the cursor, the dots and the marked marks of one figure, as a person or a screen reader gets them. */
const view = (page, fig) =>
  page.evaluate((fig) => {
    const f = Document.prototype.getElementById.call(document, fig);
    const body = f.querySelector("[data-slot=chart-body]");
    const tip = body.querySelector("[data-slot=chart-tip]");
    const cursor = body.querySelector("[data-slot=chart-cursor]");
    const plot = body.querySelector("[data-slot=chart-plot]");
    const tb = tip.getBoundingClientRect();
    const bb = body.getBoundingClientRect();
    const pb = plot?.getBoundingClientRect();
    const open = tip.hasAttribute("data-open");
    const marks = {};
    for (const mark of body.querySelectorAll("[data-i]")) marks[mark.getAttribute("data-i")] = (marks[mark.getAttribute("data-i")] || 0) + 1;
    return {
      open,
      head: tip.querySelector("[data-tip=head]")?.textContent ?? null,
      // The server writes a line for every column; the legend's switches hide those of the series they hide.
      rows: [...tip.querySelectorAll("[data-tip=row]")].filter((r) => r.checkVisibility()).map((r) => ({ value: r.querySelector("strong").textContent, name: r.lastElementChild.textContent })),
      markup: tip.querySelectorAll("img,script,iframe").length,
      say: f.querySelector("[data-slot=chart-announce]").textContent,
      inside: !open || (tb.left >= bb.left - 0.5 && tb.right <= bb.right + 0.5 && tb.top >= bb.top - 0.5 && tb.bottom <= bb.bottom + 0.5),
      geometry: open ? { tip: [tb.left, tb.top, tb.right, tb.bottom].map(Math.round), body: [bb.left, bb.top, bb.right, bb.bottom].map(Math.round) } : null,
      active: [...body.querySelectorAll("[data-active]")].map((e) => e.getAttribute("data-i")),
      marks,
      xs: (body.getAttribute("data-xs") || "").split(" ").map(Number),
      mode: body.getAttribute("data-mode"),
      cursor: cursor && { x1: cursor.getAttribute("x1"), x2: cursor.getAttribute("x2"), visibility: cursor.getAttribute("visibility") },
      dots: [...body.querySelectorAll("[data-slot=chart-dot]")].map((d) => {
        const r = d.getBoundingClientRect();
        return {
          s: d.getAttribute("data-s"),
          ys: d.getAttribute("data-ys").split(" "),
          cx: d.getAttribute("cx"),
          cy: d.getAttribute("cy"),
          visibility: d.getAttribute("visibility"),
          at: [r.left + r.width / 2, r.top + r.height / 2],
        };
      }),
      plot: pb && [pb.left, pb.top],
    };
  }, fig);

/** What the table says for row `i`: the row's header and each series' cell, for the series switched on. */
const fromTable = (page, fig, i) =>
  page.evaluate(
    ({ fig, i }) => {
      const f = Document.prototype.getElementById.call(document, fig);
      const table = f.querySelector("[data-slot=chart-table] table");
      const heads = [...table.querySelectorAll("thead th")];
      const cells = [...table.querySelectorAll("tbody tr")[i].querySelectorAll("th,td")];
      // A gap's dash is for the eyes and its words for a screen reader: the tooltip shows the dash.
      const text = (cell) => (cell.querySelector("[aria-hidden=true]") || cell).textContent.trim();
      const kind = f.getAttribute("data-kind");
      const rows = [];
      for (let c = 1; c < cells.length; c++) {
        const toggle = f.querySelector(`[data-slot=chart-toggle][data-s="${c - 1}"]`);
        if (kind !== "donut" && kind !== "funnel" && toggle && !toggle.checked) continue;
        rows.push({ value: text(cells[c]), name: heads[c].textContent.trim() });
      }
      return { head: text(cells[0]), rows, count: table.querySelectorAll("tbody tr").length };
    },
    { fig, i },
  );

/**
 * Checks that the figure shows value `i` everywhere it should: tooltip, cursor, dots, marks, and the
 * live region's words (a key speaks; the pointer does not, so a pointer test passes `speaks: false`).
 */
async function expectAt(page, fig, i, what = "", { speaks = true } = {}) {
  const v = await view(page, fig);
  const t = await fromTable(page, fig, i);
  const label = `${fig} at ${i}${what ? ` (${what})` : ""}`;
  assert.equal(v.open, true, `${label}: the tooltip is open`);
  assert.equal(v.markup, 0, `${label}: the tooltip holds no markup from the data`);
  assert.equal(v.head, t.head, `${label}: the tooltip's head is the table row's header`);
  assert.deepEqual(v.rows, t.rows, `${label}: the tooltip's rows are the table row's values`);
  if (speaks) assert.equal(v.say, [t.head, ...t.rows.map((r) => `${r.name} ${r.value}`)].join(", "), `${label}: the live region says the same words`);
  assert.equal(v.inside, true, `${label}: the tooltip stays inside the chart (${JSON.stringify(v.geometry)})`);
  assert.deepEqual(v.active, Array(v.marks[i] || 0).fill(String(i)), `${label}: exactly the marks of value ${i} are marked`);
  if (v.cursor) {
    assert.equal(v.cursor.visibility, "visible", `${label}: the crosshair shows`);
    assert.equal(v.cursor.x1, `${v.xs[i]}%`, `${label}: the crosshair sits on the value`);
    assert.equal(v.cursor.x2, v.cursor.x1);
    for (const dot of v.dots) {
      const y = dot.ys[i];
      const off = await page.evaluate(({ fig, s }) => Document.prototype.getElementById.call(document, fig).querySelector(`[data-slot=chart-toggle][data-s="${s}"]`)?.checked === false, { fig, s: dot.s });
      if (!y || y === "-" || off) {
        assert.equal(dot.visibility, "hidden", `${label}: series ${dot.s} has no dot (a gap or switched off)`);
      } else {
        assert.equal(dot.visibility, "visible", `${label}: series ${dot.s} has a dot`);
        assert.equal(dot.cx, `${v.xs[i]}%`);
        assert.equal(dot.cy, y, `${label}: the dot sits at the series' y`);
      }
    }
  }
  return v;
}

/** Where the arrow keys, Home, End and the page keys take a person who is on value `at` of `n`. */
function moved(key, at, n, rtl) {
  const step = Math.max(1, Math.round(n / 10));
  const to = { ArrowRight: rtl ? at - 1 : at + 1, ArrowLeft: rtl ? at + 1 : at - 1, ArrowDown: at + 1, ArrowUp: at - 1, Home: 0, End: n - 1, PageDown: at + step, PageUp: at - step }[key];
  return Math.max(0, Math.min(n - 1, to));
}

/** Whether the focused element matches `selector`: read through the prototypes, so a page of named elements can't get in the way. */
const focusOn = (page, selector) =>
  page.evaluate((selector) => {
    const active = Object.getOwnPropertyDescriptor(Document.prototype, "activeElement").get.call(document);
    return !!active && Element.prototype.matches.call(active, selector);
  }, selector);

/** An attribute of the first element matching `selector`, read through the prototypes. */
const attrOf = (page, selector, name) =>
  page.evaluate(({ selector, name }) => Element.prototype.getAttribute.call(Document.prototype.querySelector.call(document, selector), name), { selector, name });

/** The box of the first element matching `selector`, read through the prototypes. */
const boxOf = (page, selector) =>
  page.evaluate((selector) => {
    const r = Element.prototype.getBoundingClientRect.call(Document.prototype.querySelector.call(document, selector));
    return { x: r.left, y: r.top, width: r.width, height: r.height };
  }, selector);

/** Presses Tab until `selector` has focus. A page has a sidebar and a menu before its charts. */
async function tabTo(page, selector, max = 120) {
  for (let i = 0; i <= max; i++) {
    if (await focusOn(page, selector)) return i;
    await page.keyboard.press("Tab");
  }
  assert.fail(`Tab never reached ${selector}`);
}

/** A point on element `selector` (or on something inside it) that hits it, found on a grid: a ring's slice has no middle to point at. */
const hitPoint = (page, selector) =>
  page.evaluate((selector) => {
    const el = Document.prototype.querySelector.call(document, selector);
    el.scrollIntoView({ block: "center", behavior: "instant" });
    const r = el.getBoundingClientRect();
    // Whole pixels: Firefox and WebKit put the pointer on one, and a thin slice has a neighbour's edge a pixel away.
    for (const fy of [0.5, 0.4, 0.6, 0.3, 0.7, 0.2, 0.8, 0.1, 0.9]) {
      for (let fx = 0.05; fx < 1; fx += 0.05) {
        const x = Math.round(r.left + r.width * fx);
        const y = Math.round(r.top + r.height * fy);
        const hit = document.elementFromPoint(x, y);
        if (hit && (hit === el || el.contains(hit))) return { x, y };
      }
    }
    return null;
  }, selector);

/** Every tag of `html` with its attribute names, in order. */
function tagsOf(html) {
  const out = [];
  for (const tag of html.matchAll(/<([a-zA-Z][\w-]*)((?:\s+[^\s"'<>/=]+(?:="[^"]*")?)*)\s*\/?>/g)) {
    out.push({ tag: tag[1].toLowerCase(), names: [...tag[2].matchAll(/\s+([^\s"'<>/=]+)(?:="[^"]*")?/g)].map((a) => a[1].toLowerCase()) });
  }
  return out;
}

/** The tags of `html` that write the same attribute twice: a browser keeps the first and drops the rest without a word. */
const repeatedAttributes = (html) => tagsOf(html).flatMap(({ tag, names }) => names.filter((n, i) => names.indexOf(n) !== i).map((n) => `<${tag}> writes ${n} twice`));

describe("the markup the server writes", { skip: reachable ? false : `needs the proof app at ${BASE}` }, () => {
  for (const path of [...Object.keys(CASES).map((c) => `/charts?case=${c}`), "/charts", "/charts/report", "/charts/big?n=50"]) {
    test(`${path}: no tag writes an attribute twice, and no inline style or script is written`, async () => {
      const response = await fetch(BASE + path);
      assert.equal(response.status, 200);
      const html = await response.text();
      assert.deepEqual(repeatedAttributes(html), []);
      const figures = html.slice(html.indexOf("<figure"), html.lastIndexOf("</figure>"));
      assert.doesNotMatch(figures, /<(script|style|iframe|object|embed|img)\b/i, "no script, style or image element in a chart: the data is text");
      const tags = tagsOf(figures);
      assert.ok(tags.length > 5, "the charts were found");
      assert.deepEqual(tags.flatMap(({ tag, names }) => names.filter((n) => n === "style" || n.startsWith("on")).map((n) => `<${tag} ${n}>`)), [], "no style attribute and no event handler in a chart");
    });
  }
});

for (const name of NAMES) {
  describe(name, { concurrency: false, skip: reachable ? false : `needs the proof app at ${BASE}` }, () => {
    let browser;
    before(async () => {
      browser = await launch(ENGINES[name]);
    });
    after(() => browser?.close());
    /** Every context a test opened: closed after it. */
    const contexts = [];
    afterEach(async () => {
      for (const context of contexts.splice(0)) await context.close().catch(() => {});
    });

    /**
     * A page on the proof app. Console errors, page errors and content security policy reports
     * are kept; `page.done()` asserts there are none. `light` asks for the light palette with
     * `?mode=light`, as a reader's choice would; `theme` asks for a built-in theme by its id.
     */
    async function open(path, { width = 1440, height = 900, light = false, js = true, theme = null, reducedMotion = "no-preference" } = {}) {
      const context = await browser.newContext({ viewport: { width, height }, javaScriptEnabled: js, colorScheme: light ? "light" : "dark", reducedMotion });
      contexts.push(context);
      const page = await context.newPage();
      page.problems = [];
      page.on("console", (m) => {
        // Playwright's WebKit on Windows reports this on every page, a plain 404 too.
        if (name === "webkit" && /Refused to apply a stylesheet/.test(m.text())) return;
        if (m.type() === "error" && !/Failed to load resource/.test(m.text())) page.problems.push(m.text());
      });
      page.on("pageerror", (e) => page.problems.push("pageerror: " + e.message));
      if (js) {
        await page.addInitScript(() => {
          document.addEventListener("securitypolicyviolation", (e) => {
            (window.__csp ||= []).push(e.violatedDirective + " " + e.blockedURI);
          });
        });
      }
      const asked = [light && "mode=light", theme && `theme=${theme}`].filter(Boolean).join("&");
      await page.goto(BASE + path + (asked ? (path.includes("?") ? "&" : "?") + asked : ""));
      page.done = async () => {
        let csp = js ? await page.evaluate(() => window.__csp || []) : [];
        // Playwright's WebKit on Windows reports inline styles refused on every page, a bare 404
        // too. The other engines check the style directives, and no chart writes a style attribute.
        if (name === "webkit") csp = csp.filter((line) => !/^style-src-(elem|attr) inline$/.test(line));
        assert.deepEqual(csp, [], "no content security policy report");
        assert.deepEqual(page.problems, [], "no console error");
      };
      return page;
    }

    // ---- 1 and 2: every case loads clean, fits, is axe clean --------------------------------

    describe("every case", () => {
      for (const [kase, figures] of Object.entries(CASES)) {
        for (const width of [1440, 390]) {
          test(`${kase} at ${width} px: no error, no policy report, no sideways scroll, axe clean in dark and light`, { timeout: 120000 }, async () => {
            for (const theme of ["dark", "light"]) {
              const page = await open(`/charts?case=${kase}`, { width, light: theme === "light" });
              const label = `${kase} ${theme} ${width}`;
              const ground = await page.evaluate(() => getComputedStyle(document.body).backgroundColor);
              assert.equal(ground === "rgb(255, 255, 255)", theme === "light", `${label}: the page is ${theme} (${ground})`);
              assert.equal(await page.locator(`section[data-case=${kase}]`).count(), 1, `${label}: the case is drawn`);
              for (const fig of figures) assert.equal(await page.locator(`figure#${fig}`).count(), 1, `${label}: ${fig} is drawn`);
              const spill = await page.evaluate(() => {
                const wide = document.documentElement.scrollWidth - window.innerWidth;
                const out = [];
                for (const el of document.querySelectorAll("[data-case] *")) {
                  if (el.closest("[data-sr]") || el.closest("[data-slot=chart-tip]")) continue;
                  // What is inside an SVG is measured by the SVG (Firefox reports a nested SVG's content, drawn or not, at its unscaled size).
                  if (el instanceof SVGElement && el.ownerSVGElement) continue;
                  // The table behind its switch is one clipped pixel: what it holds is not on the screen.
                  const held = el.closest("[data-slot=chart-table]");
                  if (held && held.getBoundingClientRect().width <= 1) continue;
                  const r = el.getBoundingClientRect();
                  if (r.width && (r.right > window.innerWidth + 1 || r.left < -1)) out.push(`${el.tagName}:${el.getAttribute("data-slot") || ""} ${Math.round(r.left)}..${Math.round(r.right)}`);
                }
                return { wide, out: out.slice(0, 5) };
              });
              assert.ok(spill.wide <= 0, `${label}: the page scrolls sideways by ${spill.wide} px`);
              assert.deepEqual(spill.out, [], `${label}: nothing sticks out of the screen`);
              assert.deepEqual(await axe(page), [], `${label}: axe`);
              if (SHOTS) {
                await settle(page);
                await page.locator(`section[data-case=${kase}]`).screenshot({ path: join(SHOTS, `${name}-${kase}-${width}-${theme}.png`) });
              }
              // The table shown, on every figure that has a switch.
              const labels = page.locator("[data-slot=chart-table-label]");
              const n = await labels.count();
              for (let k = 0; k < n; k++) await labels.nth(k).click();
              if (n) assert.deepEqual(await axe(page), [], `${label}: axe with the table shown`);
              // A tooltip open on each figure that has one, one figure at a time.
              const bodies = page.locator("[data-slot=chart-body][tabindex]");
              const b = await bodies.count();
              for (let k = 0; k < b; k++) {
                await bodies.nth(k).focus();
                assert.equal(await page.locator("[data-slot=chart-tip][data-open]").count(), 1, `${label}: a tooltip opens on focus`);
                assert.deepEqual(await axe(page), [], `${label}: axe with a tooltip open on figure ${k}`);
              }
              await page.done();
              await page.context().close();
            }
          });
        }
      }

      test("the states say what they are: empty, loading, failed, and a sparkline is a picture with a name", async () => {
        const empty = await open("/charts?case=empty");
        assert.equal(await empty.locator("#empty [data-state=empty]").count(), 1);
        assert.match(await empty.locator("#empty [data-state=empty]").innerText(), /No data/);
        assert.equal(await empty.locator("#empty svg[data-slot=chart-plot]").count(), 0, "an empty chart draws no axes");
        await empty.done();
        const loading = await open("/charts?case=loading");
        assert.equal(await loading.locator("#loading").getAttribute("aria-busy"), "true");
        assert.equal(await loading.locator("#loading [data-state=loading]").getAttribute("role"), "status");
        assert.match(await loading.locator("#loading [data-state=loading]").textContent(), /Loading Opens by hour/);
        await loading.done();
        const failed = await open("/charts?case=failed");
        assert.equal(await failed.locator("#failed [data-state=failed]").getAttribute("role"), "alert");
        assert.match(await failed.locator("#failed [data-state=failed]").innerText(), /could not be loaded/);
        assert.match(await failed.locator("#failed [data-state=failed]").innerText(), /did not answer in time/);
        const retry = failed.getByRole("link", { name: "Try again" });
        assert.equal(await retry.getAttribute("href"), "/charts?case=failed");
        // No zeros stand in for the missing data.
        assert.equal(await failed.locator("#failed table, #failed svg[data-slot=chart-plot]").count(), 0);
        await failed.done();
        const spark = await open("/charts?case=sparkline");
        for (const id of ["spark-1", "spark-2", "spark-3"]) {
          const body = spark.locator(`#${id} [data-slot=chart-body]`);
          assert.equal(await body.getAttribute("role"), "img", `${id} is a picture`);
          assert.equal(await body.getAttribute("tabindex"), null, `${id} is not a tab stop`);
          assert.match(await body.getAttribute("aria-label"), /last 24 hours/);
          assert.equal(await spark.locator(`#${id}[data-ui-chart]`).count(), 0, `${id} loads no tooltip`);
          assert.equal(await spark.locator(`#${id} [data-slot=chart-table-switch]`).count(), 0, `${id} has no switch`);
        }
        assert.equal(await spark.getByRole("table", { name: /last 24 hours/ }).count(), 3, "the three tables are there for a screen reader");
        await spark.done();
      });

      test("a state is as tall as the chart it stands in for, so nothing moves when the data comes", async () => {
        for (const kase of ["empty", "loading", "failed"]) {
          const page = await open(`/charts?case=${kase}`);
          const state = page.locator(`#${kase} [data-slot=chart-state]`);
          const want = Number(await state.getAttribute("data-h"));
          const got = (await state.boundingBox()).height;
          assert.ok(want > 100, `${kase}: a drawing's height (${want})`);
          assert.ok(Math.abs(got - want) < 0.5, `${kase}: ${got} px against ${want}`);
          await page.done();
        }
        // The line case draws the same chart: its drawing is the height the states keep.
        const line = await open("/charts?case=line");
        const drawn = (await line.locator("#line [data-slot=chart-frame]").boundingBox()).height;
        assert.ok(Math.abs(drawn - 232) < 0.5, `the drawing is ${drawn} px`);
        await line.done();
      });
    });

    // ---- 3: keyboard only -------------------------------------------------------------------

    describe("keyboard", () => {
      /** Tab to the chart, then walk every key, checking tooltip, table, words, marks and bounds at each stop. */
      async function walk(page, fig, { rtl = false } = {}) {
        const body = `#${fig} [data-slot=chart-body]`;
        assert.equal(await page.locator(body).getAttribute("tabindex"), "0", `${fig}: the module makes the chart a tab stop`);
        const described = (await page.locator(body).getAttribute("aria-describedby")).split(" ");
        assert.ok(described.includes(`${fig}-summary`) && described.includes(`${fig}-keys`), `${fig}: described by its summary and the keys: ${described}`);
        assert.match(await page.locator(`#${fig}-keys`).textContent(), /arrow keys/);
        const tabs = await tabTo(page, body);
        assert.ok(tabs < 80, `${fig}: reached in ${tabs} tabs`);
        const ring = await page.locator(body).evaluate((el) => {
          const s = getComputedStyle(el);
          return { style: s.outlineStyle, width: parseFloat(s.outlineWidth), color: s.outlineColor, focusVisible: el.matches(":focus-visible") };
        });
        assert.equal(ring.focusVisible, true, `${fig}: Tab gives a focus-visible ring`);
        assert.equal(ring.style, "solid", `${fig}: the ring is drawn`);
        assert.ok(ring.width >= 2, `${fig}: the ring is at least 2 px`);
        assert.notEqual(ring.color, "rgba(0, 0, 0, 0)", `${fig}: the ring has a colour`);
        const n = (await fromTable(page, fig, 0)).count;
        // The first value shows on focus.
        await expectAt(page, fig, 0, "on focus");
        let at = 0;
        const keys = ["ArrowRight", "ArrowRight", "ArrowLeft", "End", "ArrowRight", "Home", "ArrowLeft", "PageDown", "PageDown", "PageUp", "ArrowDown", "ArrowUp", "End", "PageDown", "Home"];
        for (const key of keys) {
          await page.keyboard.press(key);
          at = moved(key, at, n, rtl);
          await expectAt(page, fig, at, key);
        }
        // Escape hides the tooltip and the crosshair and keeps focus; the next key brings it back.
        await page.keyboard.press("ArrowRight");
        at = moved("ArrowRight", at, n, rtl);
        await page.keyboard.press("Escape");
        let v = await view(page, fig);
        assert.equal(v.open, false, `${fig}: Escape hides the tooltip`);
        assert.deepEqual(v.active, [], `${fig}: Escape unmarks the marks`);
        if (v.cursor) assert.equal(v.cursor.visibility, "hidden", `${fig}: Escape hides the crosshair`);
        for (const dot of v.dots) assert.equal(dot.visibility, "hidden");
        assert.equal(await focusOn(page, body), true, `${fig}: focus stays on the chart`);
        await page.keyboard.press("ArrowRight");
        at = moved("ArrowRight", at, n, rtl);
        await expectAt(page, fig, at, "after Escape");
        // Focus leaving hides it.
        await page.keyboard.press("Tab");
        // A chart that is the last stop on its page has nowhere for Tab to go (Firefox leaves the page and keeps
        // the element active): focus is taken off by script then.
        if (await focusOn(page, body)) await page.evaluate(() => Object.getOwnPropertyDescriptor(Document.prototype, "activeElement").get.call(document).blur());
        assert.equal(await focusOn(page, body), false, `${fig}: Tab leaves the chart`);
        v = await view(page, fig);
        assert.equal(v.open, false, `${fig}: leaving hides the tooltip`);
        assert.deepEqual(v.active, []);
        if (v.cursor) assert.equal(v.cursor.visibility, "hidden");
        // Coming back puts the tooltip where it was.
        await page.locator(body).focus();
        await expectAt(page, fig, at, "back on the chart");
        return n;
      }

      for (const [kase, fig] of INTERACTIVE) {
        test(`${kase} (${fig}): Tab, the arrow keys, Home, End, the page keys, Escape and leaving`, { timeout: 90000 }, async () => {
          const page = await open(`/charts?case=${kase}`);
          const n = await walk(page, fig, { rtl: kase === "rtl" });
          assert.ok(n >= 1);
          await page.done();
        });
      }

      test("rtl: ArrowLeft moves forward and ArrowRight back", async () => {
        const page = await open("/charts?case=rtl");
        const body = "#rtl [data-slot=chart-body]";
        await tabTo(page, body);
        await expectAt(page, "rtl", 0);
        await page.keyboard.press("ArrowLeft");
        await expectAt(page, "rtl", 1, "ArrowLeft");
        await page.keyboard.press("ArrowLeft");
        await expectAt(page, "rtl", 2, "ArrowLeft");
        await page.keyboard.press("ArrowRight");
        await expectAt(page, "rtl", 1, "ArrowRight");
        await page.keyboard.press("ArrowRight");
        await page.keyboard.press("ArrowRight");
        await expectAt(page, "rtl", 0, "ArrowRight at the start stays");
        // The words are the language's: Arabic month names and series names.
        const v = await view(page, "rtl");
        assert.match(v.head, /\p{Script=Arabic}/u, "the head is written in Arabic");
        assert.deepEqual(v.rows.map((r) => r.name), ["أُرسلت", "فُتحت"]);
        await page.done();
      });

      test("bars mark the focused band's rects, and a donut, a pie and a funnel mark their parts", async () => {
        for (const [kase, fig, mark] of [["bar-grouped", "bar-grouped", "rect"], ["bar-stacked", "bar-stacked", "rect"], ["bar-horizontal", "bar-horizontal", "div,rect"], ["donut", "donut", "path,li"], ["pie", "pie", "path,li"], ["funnel", "funnel", "li,rect"]]) {
          const page = await open(`/charts?case=${kase}`);
          await tabTo(page, `#${fig} [data-slot=chart-body]`);
          await page.keyboard.press("ArrowRight");
          const marked = await page.evaluate((fig) => [...document.querySelectorAll(`#${fig} [data-active]`)].map((e) => `${e.tagName.toLowerCase()}:${e.getAttribute("data-i")}`), fig);
          assert.ok(marked.length >= 2, `${kase}: more than one mark is marked: ${marked}`);
          assert.ok(marked.every((m) => m.endsWith(":1")), `${kase}: only value 1: ${marked}`);
          assert.ok(mark.split(",").every((tag) => marked.some((m) => m.startsWith(tag + ":"))), `${kase}: marks of kinds ${mark} are marked: ${marked}`);
          // Marking changes how a mark looks: the same marks look different once Escape unmarks them.
          const look = () =>
            page.evaluate((fig) => {
              const sig = (el) => [el, ...el.querySelectorAll("*")].map((e) => { const s = getComputedStyle(e); return s.fill + s.opacity + s.color; }).join("|");
              return [...document.querySelectorAll(`#${fig} [data-i="1"]`)].map(sig);
            }, fig);
          await settle(page);
          const marked1 = await look();
          await page.keyboard.press("Escape");
          await settle(page);
          const plain = await look();
          assert.ok(marked1.some((m, k) => m !== plain[k]), `${kase}: a marked mark looks different from the same mark unmarked`);
          await page.done();
        }
      });

      test("a big tooltip shows the value large with its unit, and a value's band colours its line", async () => {
        const page = await open("/charts?case=paint");
        await tabTo(page, "#paint [data-slot=chart-body]");
        const big = await page.evaluate(() => {
          const row = document.querySelector("#paint [data-slot=chart-tip][data-open] [data-tip=head] + [data-tip=row]");
          const strong = row?.querySelector("strong");
          return { value: strong?.textContent, unit: row?.querySelector("em")?.textContent, size: strong && parseFloat(getComputedStyle(strong).fontSize) };
        });
        assert.ok(big.value && /\d/.test(big.value), `a value: ${JSON.stringify(big)}`);
        assert.equal(big.unit, "BPM");
        assert.ok(big.size >= 18, `large: ${big.size}`);
        await page.done();
        const steps = await open("/charts?case=steps");
        await tabTo(steps, "#steps [data-slot=chart-body]");
        await steps.keyboard.press("End");
        const line = await steps.evaluate(() => {
          const row = document.querySelector("#steps [data-slot=chart-tip][data-open] [data-tip=head] + [data-tip=row]");
          const detail = [...document.querySelectorAll("#steps [data-slot=chart-tip] [data-tip=row]")].at(-1);
          return { value: row?.querySelector("strong")?.textContent, color: row?.getAttribute("data-color"), detail: detail?.querySelector("strong")?.textContent };
        });
        assert.deepEqual(line, { value: "Awake", color: "4", detail: "5 min" });
        assert.match(await steps.locator("#steps [data-slot=chart-announce]").textContent(), /Awake/);
        await steps.done();
      });

      test("marks come in only for a reader who has not asked for less motion", async () => {
        for (const reduce of [false, true]) {
          const page = await open("/charts?case=paint", { reducedMotion: reduce ? "reduce" : "no-preference" });
          const running = await page.evaluate(() => document.getAnimations().length);
          if (reduce) assert.equal(running, 0, "nothing moves");
          else assert.ok(running > 0, "the marks come in");
          await page.done();
        }
      });

      test("the focus ring is painted outside the chart, in light and dark", async () => {
        for (const light of [false, true]) {
          for (const [kase, fig] of [["line", "line"], ["bar", "bar"], ["donut", "donut"], ["funnel", "funnel"], ["bar-horizontal", "bar-horizontal"]]) {
            const page = await open(`/charts?case=${kase}`, { light });
            const body = `#${fig} [data-slot=chart-body]`;
            await tabTo(page, body);
            await settle(page);
            const after = decodePng(await page.screenshot());
            const r = await page.locator(body).evaluate((el) => {
              const b = el.getBoundingClientRect();
              return { left: b.left, top: b.top, right: b.right };
            });
            // The same screen with the focus gone: the page has not scrolled in between.
            await page.evaluate(() => document.activeElement.blur());
            await settle(page);
            const before = decodePng(await page.screenshot());
            // A strip just above the chart: the ring sits 4 px out and is 2 px wide.
            const y0 = Math.floor(r.top) - 7;
            const y1 = Math.floor(r.top) - 3;
            let changed = 0;
            let columns = 0;
            for (let x = Math.ceil(r.left) + 12; x < Math.floor(r.right) - 12; x++) {
              columns++;
              for (let y = y0; y <= y1; y++) {
                const a = (y * before.width + x) * before.channels;
                const diff = Math.abs(before.data[a] - after.data[a]) + Math.abs(before.data[a + 1] - after.data[a + 1]) + Math.abs(before.data[a + 2] - after.data[a + 2]);
                if (diff > 60) {
                  changed++;
                  break;
                }
              }
            }
            assert.ok(changed >= columns * 0.9, `${kase} ${light ? "light" : "dark"}: the ring shows along the top of the chart (${changed} of ${columns} columns)`);
            await page.context().close();
          }
        }
      });

      for (const kase of ["line", "line-many", "bar-grouped", "bar-horizontal-stacked", "donut", "funnel", "hostile", "locale", "extremes"]) {
        test(`${kase} at 390 px: every value's tooltip stays inside the chart`, { timeout: 90000 }, async () => {
          const page = await open(`/charts?case=${kase}`, { width: 390 });
          for (const fig of CASES[kase]) {
            const body = `#${fig} [data-slot=chart-body]`;
            await tabTo(page, body);
            const n = (await fromTable(page, fig, 0)).count;
            for (let at = 0; at < n; at++) {
              await expectAt(page, fig, at, "390 px");
              if (at < n - 1) await page.keyboard.press("ArrowRight");
            }
          }
          await page.done();
        });
      }
    });

    // ---- 4: pointer -------------------------------------------------------------------------

    describe("pointer", () => {
      /** The index of the value nearest to `pct` of the plot's width. */
      const nearest = (xs, pct) => xs.reduce((best, at, i) => (Math.abs(at - pct) < Math.abs(xs[best] - pct) ? i : best), 0);

      async function plotBox(page, fig) {
        const plot = page.locator(`#${fig} [data-slot=chart-plot]`);
        await plot.scrollIntoViewIfNeeded();
        return plot.boundingBox();
      }

      for (const [kase, fig] of [["line", "line"], ["area-stacked", "area-stacked"], ["line-many", "line-many"], ["line-gaps", "line-gaps"], ["area", "area"], ["annotations", "annotations"]]) {
        test(`${kase}: the crosshair snaps to the nearest value, dots sit at each series' y, leaving hides them`, async () => {
          const page = await open(`/charts?case=${kase}`);
          const box = await plotBox(page, fig);
          const xs = (await view(page, fig)).xs;
          for (const f of [0.02, 0.31, 0.5, 0.77, 0.98]) {
            const pct = f * 100;
            const expected = nearest(xs, pct);
            await page.mouse.move(box.x + box.width * f, box.y + box.height / 2);
            const v = await until(async () => {
              const v = await view(page, fig);
              return v.open && v.cursor.x1 === `${xs[expected]}%` && v;
            }, `${kase}: the crosshair at ${expected}`);
            assert.equal(v.cursor.visibility, "visible");
            await expectAt(page, fig, expected, `pointer at ${Math.round(pct)}%`, { speaks: false });
            // Where the dots are on screen: on the crosshair, at the series' y.
            for (const dot of v.dots) {
              if (dot.visibility !== "visible") continue;
              assert.ok(Math.abs(dot.at[0] - (v.plot[0] + (box.width * xs[expected]) / 100)) <= 1.5, `${kase}: a dot is on the crosshair (${dot.at[0]})`);
              assert.ok(Math.abs(dot.at[1] - (v.plot[1] + Number(dot.cy))) <= 1.5, `${kase}: a dot is at its y (${dot.at[1]})`);
            }
            // A dot for every series that has a value here (expectAt checked each one); none where all have a gap.
            const valued = v.dots.filter((d) => d.ys[expected] && d.ys[expected] !== "-");
            assert.equal(v.dots.filter((d) => d.visibility === "visible").length, valued.length, `${kase}: a dot for each series with a value`);
          }
          await page.mouse.move(2, 2);
          const v = await until(async () => {
            const v = await view(page, fig);
            return !v.open && v;
          }, `${kase}: the tooltip to close`);
          assert.equal(v.cursor.visibility, "hidden", `${kase}: the crosshair hides`);
          for (const dot of v.dots) assert.equal(dot.visibility, "hidden", `${kase}: the dots hide`);
          await page.done();
        });
      }

      test("a gap: the pointer on a day with nothing measured shows a dash and no dot", async () => {
        const page = await open("/charts?case=line-gaps");
        const box = await plotBox(page, "line-gaps");
        const v0 = await view(page, "line-gaps");
        // Values 8 to 11 are gaps.
        const f = v0.xs[9] / 100;
        await page.mouse.move(box.x + box.width * f, box.y + box.height / 2);
        const v = await until(async () => {
          const v = await view(page, "line-gaps");
          return v.open && v.cursor.x1 === `${v0.xs[9]}%` && v;
        }, "the tooltip on the gap");
        assert.deepEqual(v.rows.map((r) => r.value), ["–"]);
        assert.ok(v.dots.every((d) => d.visibility === "hidden"), "no dot on a gap");
        await expectAt(page, "line-gaps", 9, "a gap", { speaks: false });
        await page.done();
      });

      test("a bar: the pointer on a bar shows that band's tooltip and marks its rects", async () => {
        for (const [kase, fig, picks] of [["bar", "bar", [0, 3, 6]], ["bar-grouped", "bar-grouped", [1, 4]], ["bar-stacked", "bar-stacked", [0, 2, 4]], ["bar-negative", "bar-negative", [1, 3]], ["rtl", "rtl", [0, 5]]]) {
          const page = await open(`/charts?case=${kase}`);
          for (const i of picks) {
            const at = await hitPoint(page, `#${fig} [data-slot=chart-bar][data-i="${i}"]`);
            assert.ok(at, `${kase}: a bar ${i} can be hit`);
            await page.mouse.move(at.x, at.y);
            await until(async () => (await view(page, fig)).open && (await view(page, fig)).active.length > 0 && (await view(page, fig)).active[0] === String(i), `${kase}: the tooltip for ${i}`);
            await expectAt(page, fig, i, "pointer on a bar", { speaks: false });
          }
          // Hovering moves nothing for a screen reader: the live region is for keys.
          assert.equal((await view(page, fig)).say, "", `${kase}: the pointer does not speak`);
          await page.mouse.move(2, 2);
          await until(async () => !(await view(page, fig)).open, `${kase}: the tooltip to close`);
          assert.deepEqual((await view(page, fig)).active, []);
          await page.done();
        }
      });

      test("a row of horizontal bars, a stage of a funnel and a slice of a donut or a pie show their tooltip", async () => {
        for (const [kase, fig, selector, picks] of [
          ["bar-horizontal", "bar-horizontal", "[data-slot=chart-row]", [0, 2, 5]],
          ["bar-horizontal-stacked", "bar-horizontal-stacked", "[data-slot=chart-row]", [0, 1, 2]],
          ["funnel", "funnel", "[data-slot=chart-stage]", [0, 1, 2, 3]],
          ["donut", "donut", "[data-slot=chart-slice]", [0, 1]],
          ["pie", "pie", "[data-slot=chart-slice]", [0, 1, 2]],
          ["donut", "donut", "[data-slot=chart-legend-item]", [0, 2]],
        ]) {
          const page = await open(`/charts?case=${kase}`);
          for (const i of picks) {
            const at = await hitPoint(page, `#${fig} ${selector}[data-i="${i}"]`);
            assert.ok(at, `${kase}: ${selector} ${i} can be hit`);
            await page.mouse.move(at.x, at.y);
            await until(async () => {
              const v = await view(page, fig);
              return v.open && v.active.length > 0 && v.active.every((a) => a === String(i));
            }, `${kase}: the tooltip for ${i}`);
            await expectAt(page, fig, i, `pointer on ${selector}`, { speaks: false });
          }
          // Between the marks (the donut's hole, the space between rows) the tooltip goes.
          await page.mouse.move(2, 2);
          await until(async () => !(await view(page, fig)).open, `${kase}: the tooltip to close`);
          await page.done();
        }
        // In the middle of a donut, in the body but on no slice, there is no tooltip.
        const page = await open("/charts?case=donut");
        const slice = await hitPoint(page, "#donut [data-slot=chart-slice][data-i='0']");
        await page.mouse.move(slice.x, slice.y);
        await until(async () => (await view(page, "donut")).open, "the donut's tooltip");
        const hole = await page.locator("#donut [data-slot=chart-donut-plot]").boundingBox();
        await page.mouse.move(hole.x + hole.width / 2, hole.y + hole.height / 2);
        await until(async () => !(await view(page, "donut")).open, "the tooltip to close over the hole");
        await page.done();
      });

      test("moves that arrive faster than frames end on the last position", async () => {
        const page = await open("/charts?case=line");
        const box = await plotBox(page, "line");
        const xs = (await view(page, "line")).xs;
        const y = box.y + box.height / 2;
        // Two pointer events in one frame, the way a flick over the chart arrives.
        await page.evaluate(({ x1, x2, y }) => {
          const el = document.elementFromPoint(x1, y);
          for (const x of [x1, x2]) el.dispatchEvent(new PointerEvent("pointermove", { bubbles: true, clientX: x, clientY: y }));
        }, { x1: box.x + box.width * 0.1, x2: box.x + box.width * 0.9, y });
        const last = nearest(xs, 90);
        await until(async () => (await view(page, "line")).cursor.x1 === `${xs[last]}%`, "the tooltip to follow the last move");
        await expectAt(page, "line", last, "after a burst", { speaks: false });
        // The same over bars.
        const bars = await open("/charts?case=bar");
        const a = await hitPoint(bars, "#bar [data-slot=chart-bar][data-i='0']");
        const z = await hitPoint(bars, "#bar [data-slot=chart-bar][data-i='6']");
        await bars.evaluate(({ a, z }) => {
          const el = document.elementFromPoint(a.x, a.y);
          for (const p of [a, z]) el.dispatchEvent(new PointerEvent("pointermove", { bubbles: true, clientX: p.x, clientY: p.y }));
        }, { a, z });
        await until(async () => (await view(bars, "bar")).active[0] === "6", "the bars to follow the last move");
        await page.done();
        await bars.done();
      });

      test("a move and a leave in one frame leave the tooltip hidden", async () => {
        const page = await open("/charts?case=line");
        const box = await plotBox(page, "line");
        const y = box.y + box.height / 2;
        await page.evaluate(({ x, y }) => {
          const el = document.elementFromPoint(x, y);
          el.dispatchEvent(new PointerEvent("pointermove", { bubbles: true, clientX: x, clientY: y }));
          // Out of the chart, to the page, before the frame that would draw the move.
          el.dispatchEvent(new PointerEvent("pointerout", { bubbles: true, clientX: 1, clientY: 1, relatedTarget: document.body }));
        }, { x: box.x + box.width / 2, y });
        // Two frames: the one the move asked for, and one more.
        await page.evaluate(() => new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done))));
        const after = await view(page, "line");
        assert.equal(after.open, false, "no tooltip after the pointer left");
        assert.equal(after.cursor.visibility, "hidden", "no crosshair either");
        await page.done();
      });

      test("a tooltip taller than its chart grows the chart while it shows, and no longer", async () => {
        const page = await open("/charts?case=bar-horizontal-stacked");
        const body = page.locator("#bar-horizontal-stacked [data-slot=chart-body]");
        const natural = (await body.boundingBox()).height;
        await body.focus();
        await until(async () => (await view(page, "bar-horizontal-stacked")).open, "the tooltip");
        const tip = await page.locator("#bar-horizontal-stacked [data-slot=chart-tip]").boundingBox();
        const grown = (await body.boundingBox()).height;
        assert.ok(grown >= Math.min(tip.height, natural) && grown >= natural, `${grown} holds the tooltip`);
        await page.keyboard.press("Escape");
        await until(async () => !(await view(page, "bar-horizontal-stacked")).open, "the tooltip to hide");
        assert.equal(await body.evaluate((el) => el.style.minHeight), "", "the height it was given is taken back");
        assert.ok(Math.abs((await body.boundingBox()).height - natural) < 1, "the chart is its own height again");
        await page.done();
      });

      test("the tooltip never leaves the chart's box, anywhere the pointer goes, at 390 and 1440 px", async () => {
        for (const width of [390, 1440]) {
          for (const [kase, fig] of [["line-many", "line-many"], ["bar-grouped", "bar-grouped"], ["bar", "bar"], ["hostile", "hostile"], ["area-stacked", "area-stacked"]]) {
            const page = await open(`/charts?case=${kase}`, { width });
            const box = await plotBox(page, fig);
            const xs = (await view(page, fig)).xs;
            for (let k = 0; k <= 10; k++) {
              // On the value itself, not between two: the nearest is then not in doubt.
              const at = Math.round((k / 10) * (xs.length - 1));
              const f = xs[at] / 100;
              const expected = (await fromTable(page, fig, at)).head;
              for (const g of [0.05, 0.5, 0.95]) {
                await page.mouse.move(box.x + box.width * f, box.y + box.height * g);
                const shown = await until(async () => {
                  const v = await view(page, fig);
                  return v.open && v.head === expected && v;
                }, `${kase} ${width}px: the tooltip at value ${at}`);
                assert.equal(shown.inside, true, `${kase} ${width}px at value ${at}, ${g}: ${JSON.stringify(shown.geometry)}`);
              }
            }
            await page.done();
          }
        }
      });
    });

    // ---- 5: legend toggles ------------------------------------------------------------------

    describe("legend toggles", () => {
      for (const js of [true, false]) {
        for (const [kase, fig] of [["line-many", "line-many"], ["bar-grouped", "bar-grouped"]]) {
          test(`${kase}, JavaScript ${js ? "on" : "off"}: switching a series off hides its marks, and back`, async () => {
            const page = await open(`/charts?case=${kase}`, { js });
            const toggles = page.locator(`#${fig} [data-slot=chart-toggle]`);
            const n = await toggles.count();
            assert.ok(n >= 3, `${kase}: a toggle per series`);
            const marks = (s) => page.locator(`#${fig} [data-s="${s}"]:is(path,rect,circle)`);
            const displays = (s) => marks(s).evaluateAll((els) => els.map((e) => getComputedStyle(e).display));
            const names = await page.locator(`#${fig} [data-slot=chart-legend-name]`).allTextContents();
            for (let s = 0; s < n; s++) {
              assert.ok((await marks(s).count()) > 0, `${kase}: series ${s} has marks`);
              assert.ok((await displays(s)).every((d) => d !== "none"), `${kase}: series ${s} shows`);
            }
            // Off: by the label, like a click on its name.
            const pick = 1;
            await page.locator(`#${fig} label[for="${fig}-s${pick}"]`).click();
            assert.equal(await toggles.nth(pick).isChecked(), false);
            assert.ok((await displays(pick)).every((d) => d === "none"), `${kase}: series ${pick} is hidden`);
            for (let s = 0; s < n; s++) if (s !== pick) assert.ok((await displays(s)).every((d) => d !== "none"), `${kase}: series ${s} stays`);
            if (js) {
              // The tooltip leaves the series out: by key, and by pointer.
              const body = `#${fig} [data-slot=chart-body]`;
              await page.locator(body).focus();
              await page.keyboard.press("Home");
              let v = await expectAt(page, fig, 0, "a series off");
              assert.ok(!v.rows.some((r) => r.name === names[pick]), `${kase}: the tooltip leaves out ${names[pick]}`);
              assert.equal(v.rows.length, n - 1);
              assert.ok(v.dots.every((d) => d.s !== String(pick) || d.visibility === "hidden"), `${kase}: no dot for the series that is off`);
              await page.keyboard.press("Escape");
              // And a second series, by keyboard: Space on the focused checkbox.
              await toggles.nth(0).focus();
              await page.keyboard.press("Space");
              assert.equal(await toggles.nth(0).isChecked(), false, `${kase}: Space switches a series off`);
              assert.ok((await displays(0)).every((d) => d === "none"));
              await page.locator(body).focus();
              await page.keyboard.press("End");
              const last = (await fromTable(page, fig, 0)).count - 1;
              v = await expectAt(page, fig, last, "two series off");
              assert.ok(!v.rows.some((r) => r.name === names[0] || r.name === names[pick]), `${kase}: both are left out`);
              assert.equal(v.rows.length, n - 2);
              // Back on: both return.
              await page.locator(`#${fig} label[for="${fig}-s${pick}"]`).click();
              await page.locator(`#${fig} label[for="${fig}-s0"]`).click();
              for (let s = 0; s < n; s++) assert.ok((await displays(s)).every((d) => d !== "none"), `${kase}: series ${s} is back`);
              await page.locator(body).focus();
              await page.keyboard.press("Home");
              v = await expectAt(page, fig, 0, "all back");
              assert.equal(v.rows.length, n);
              // The table still lists every series: switching one off is for the eyes only.
              assert.equal((await page.locator(`#${fig} [data-slot=chart-table] thead th`).count()) - 1 >= n, true);
              // Pointer: with one off, hovering shows only the others.
              await page.locator(`#${fig} label[for="${fig}-s${pick}"]`).click();
              const plot = await page.locator(`#${fig} [data-slot=chart-plot]`).boundingBox();
              await page.mouse.move(plot.x + plot.width * 0.5, plot.y + plot.height / 2);
              v = await until(async () => {
                const v = await view(page, fig);
                return v.open && v;
              }, "the tooltip on hover");
              assert.ok(!v.rows.some((r) => r.name === names[pick]), `${kase}: hovering leaves out ${names[pick]}`);
              assert.equal(v.rows.length, n - 1);
              await page.done();
            } else {
              await page.locator(`#${fig} label[for="${fig}-s${pick}"]`).click();
              assert.ok((await displays(pick)).every((d) => d !== "none"), `${kase}: back on without script`);
              // Keyboard without script: Tab to a checkbox and Space.
              await toggles.nth(2).focus();
              await page.keyboard.press("Space");
              assert.ok((await displays(2)).every((d) => d === "none"), `${kase}: Space works without script`);
              assert.equal(await page.locator(`#${fig} [data-slot=chart-body]`).getAttribute("tabindex"), null);
            }
          });
        }
      }

      test("the legend's keys: a line for a line, a block for a block, and a tone's icon so colour is never alone", async () => {
        const page = await open("/charts?case=bar-stacked");
        const keys = await page.locator("#bar-stacked [data-slot=chart-legend-item]").evaluateAll((items) =>
          items.map((li) => ({ text: li.textContent.trim(), key: li.querySelector("[data-slot=chart-key]").getAttribute("data-key"), icon: !!li.querySelector("[data-slot=icon]") })),
        );
        assert.deepEqual(keys, [
          { text: "Reported", key: "block", icon: true },
          { text: "Ignored", key: "block", icon: false },
          { text: "Clicked", key: "block", icon: true },
        ]);
        const lines = await open("/charts?case=line");
        // The legend's keys: the tooltip the server writes has a key for each series too.
        assert.deepEqual(await lines.locator("#line [data-slot=chart-legend-item] [data-slot=chart-key]").evaluateAll((els) => els.map((e) => e.getAttribute("data-key"))), ["line", "line"]);
        await page.done();
        await lines.done();
      });
    });

    // ---- 6: the table switch ----------------------------------------------------------------

    describe("table switch", () => {
      for (const js of [true, false]) {
        for (const [kase, fig] of SWITCHED) {
          test(`${kase} (${fig}), JavaScript ${js ? "on" : "off"}: the table is hidden from sight and there for a screen reader, shown by its switch`, async () => {
            const page = await open(`/charts?case=${kase}`, { js });
            const table = page.locator(`#${fig} [data-slot=chart-table]`);
            const real = page.locator(`#${fig} [data-slot=chart-table] table`);
            const state = () =>
              table.evaluate((el) => {
                const s = getComputedStyle(el);
                const r = el.getBoundingClientRect();
                const t = el.querySelector("table");
                const chain = [];
                for (let e = el; e && e !== document.body; e = e.parentElement) {
                  const cs = getComputedStyle(e);
                  if (cs.display === "none" || cs.visibility === "hidden" || e.hasAttribute("hidden") || e.getAttribute("aria-hidden") === "true" || e.inert) chain.push(e.tagName + (e.getAttribute("data-slot") || ""));
                }
                return { position: s.position, width: r.width, height: r.height, clip: s.clipPath, overflow: s.overflow, notInTree: chain, rows: t.querySelectorAll("tbody tr").length };
              });
            const before = await state();
            assert.equal(before.position, "absolute", `${fig}: out of the flow`);
            assert.ok(before.width <= 1 && before.height <= 1, `${fig}: one pixel, ${before.width}x${before.height}`);
            assert.equal(before.clip, "inset(50%)", `${fig}: clipped away`);
            assert.equal(before.overflow, "hidden");
            assert.deepEqual(before.notInTree, [], `${fig}: no display none, visibility, hidden, aria-hidden or inert on the way up`);
            // In the accessibility tree: found by role, with its rows and cells, and in the snapshot.
            const named = page.locator(`#${fig}`).getByRole("table");
            assert.equal(await named.count(), 1, `${fig}: a table by role`);
            assert.equal(await named.getByRole("row").count(), before.rows + 1, `${fig}: its rows by role`);
            assert.equal(await named.getByRole("columnheader").count() >= 2, true);
            const snapshot = await page.locator(`#${fig}`).ariaSnapshot();
            assert.match(snapshot, /table/, `${fig}: the snapshot has the table`);
            const first = await fromTable(page, fig, 0);
            assert.ok(snapshot.includes(first.rows[0].value.replace(/[–]/g, "")), `${fig}: the snapshot has the first value ${first.rows[0].value}`);
            const toggle = page.locator(`#${fig} [data-slot=chart-table-switch]`);
            assert.equal(await toggle.isChecked(), false);
            // The label shows it, and shows it again to hide.
            await page.locator(`#${fig} [data-slot=chart-table-label]`).click();
            assert.equal(await toggle.isChecked(), true);
            const shown = await state();
            assert.equal(shown.position, "static", `${fig}: back in the flow`);
            assert.ok(shown.width > 200 && shown.height > 40, `${fig}: the table is drawn ${shown.width}x${shown.height}`);
            assert.equal(shown.clip, "none");
            assert.equal(await real.isVisible(), true);
            const box = await real.boundingBox();
            const view = page.viewportSize();
            assert.ok(box.x >= 0 && box.x + box.width <= view.width + 0.5, `${fig}: the table fits the width: ${box.x}..${box.x + box.width}`);
            assert.equal(await named.getByRole("row").count(), before.rows + 1);
            await page.locator(`#${fig} [data-slot=chart-table-label]`).click();
            assert.equal(await toggle.isChecked(), false);
            assert.ok((await state()).width <= 1, `${fig}: hidden again`);
            await page.done();
            // The keyboard, on a page the pointer has not touched (a click on a label moves focus in some engines):
            // Tab reaches the switch, its label shows a ring, and Space turns the table on and off.
            const keys = await open(`/charts?case=${kase}`, { js });
            const swtch = `#${fig} [data-slot=chart-table-switch]`;
            await tabTo(keys, swtch);
            assert.equal(await keys.evaluate((s) => Element.prototype.matches.call(Document.prototype.querySelector.call(document, s), ":focus-visible"), swtch), true, `${fig}: Tab gives the switch a focus-visible ring`);
            const outline = await keys.locator(`#${fig} [data-slot=chart-table-label]`).evaluate((el) => getComputedStyle(el).outlineStyle);
            assert.equal(outline, "solid", `${fig}: the label shows the focus`);
            await keys.keyboard.press("Space");
            assert.equal(await keys.locator(swtch).isChecked(), true, `${fig}: Space shows the table`);
            assert.ok((await keys.locator(`#${fig} [data-slot=chart-table]`).boundingBox()).width > 200);
            await keys.keyboard.press("Space");
            assert.equal(await keys.locator(swtch).isChecked(), false);
            assert.ok((await keys.locator(`#${fig} [data-slot=chart-table]`).boundingBox()).width <= 1);
            await keys.done();
          });
        }
      }

      test("the switch is the next tab stop after the chart, and the table is a real table with a caption, headers and a gap's words", async () => {
        const page = await open("/charts?case=line-gaps");
        await tabTo(page, "#line-gaps [data-slot=chart-body]");
        await page.keyboard.press("Tab");
        assert.equal(await page.evaluate(() => document.activeElement.getAttribute("data-slot")), "chart-table-switch");
        assert.equal(await page.evaluate(() => document.activeElement.matches(":focus-visible")), true);
        const t = page.locator("#line-gaps table");
        assert.equal(await t.locator("caption").textContent(), "Response time");
        assert.equal(await t.locator("thead th[scope=col]").count(), 2);
        assert.equal(await t.locator("tbody th[scope=row]").count(), 30);
        // A gap is a dash for the eyes and "No value" for a screen reader.
        const gap = t.locator("tbody tr").nth(9).locator("td");
        assert.equal(await gap.locator("[aria-hidden=true]").textContent(), "–");
        assert.equal(await gap.locator("[data-sr]").textContent(), "No value");
        assert.equal(await page.getByRole("cell", { name: "No value" }).count() > 0, true);
        await page.done();
      });

      test("a chart whose table is always shown has no switch, and its table is drawn", async () => {
        const page = await open("/charts?case=shown");
        assert.equal(await page.locator("#shown [data-slot=chart-table-switch]").count(), 0);
        const box = await page.locator("#shown table").boundingBox();
        assert.ok(box.width > 200 && box.height > 80);
        assert.equal(await page.getByRole("table", { name: "Seats by plan" }).count(), 1);
        await page.done();
      });
    });

    // ---- 7: JavaScript off ------------------------------------------------------------------

    describe("JavaScript off", () => {
      for (const [kase, figures] of Object.entries(CASES)) {
        test(`${kase}: every figure still draws its SVG and its table, and no chart is a tab stop`, async () => {
          const page = await open(`/charts?case=${kase}`, { js: false });
          for (const fig of figures) {
            const f = page.locator(`#${fig}`);
            assert.equal(await f.count(), 1);
            const state = await f.evaluate((el) => el.querySelector("[data-slot=chart-state]")?.getAttribute("data-state") ?? null);
            if (state) {
              // Empty, loading and failed say so in text.
              assert.ok((await f.innerText()).trim().length > 5, `${fig}: ${state} says something`);
              continue;
            }
            assert.ok((await f.locator("[data-slot=chart-body] svg").count()) > 0, `${fig}: an SVG is drawn`);
            const rows = await f.locator("[data-slot=chart-table] tbody tr").count();
            assert.ok(rows > 0, `${fig}: its table has rows`);
            assert.equal(await f.locator("[data-slot=chart-body]").getAttribute("tabindex"), null, `${fig}: no tab stop without script`);
            const described = await f.locator("[data-slot=chart-body]").getAttribute("aria-describedby");
            assert.equal(described, `${fig}-summary`, `${fig}: described by the summary alone`);
            assert.ok((await f.locator("[data-slot=chart-summary]").textContent()).length > 10, `${fig}: a summary is written`);
            // The marks are all there: as many as the drawing needs.
            const drawn = await f.locator("[data-slot=chart-body] svg :is(path,rect,line,circle)").count();
            assert.ok(drawn > 0, `${fig}: marks are drawn`);
            const box = await f.locator("[data-slot=chart-body]").boundingBox();
            assert.ok(box.width > 50 && box.height > 10, `${fig}: the drawing has a size: ${box.width}x${box.height}`);
          }
          await page.done();
        });
      }

      test("no script runs: nothing is added to a chart that the server did not write", async () => {
        const page = await open("/charts?case=bar-grouped", { js: false });
        assert.equal(await page.locator("#bar-grouped [data-slot=chart-tip]").getAttribute("data-open"), null);
        assert.equal(await page.locator("#bar-grouped [data-slot=chart-announce]").textContent(), "");
        assert.equal(await page.locator("#bar-grouped [data-slot=chart-tip]").isVisible(), false, "the tooltip is not shown");
        // The pointer does nothing but hover colours.
        const at = await hitPoint(page, "#bar-grouped [data-slot=chart-bar][data-i='2']");
        await page.mouse.move(at.x, at.y);
        assert.equal(await page.locator("#bar-grouped [data-slot=chart-tip]").isVisible(), false);
        assert.equal(await page.locator("#bar-grouped [data-active]").count(), 0);
        await page.done();
      });

      test("a ring, rings, a gauge and the day clock are drawn finished: the arcs, the number in the middle, the total", async () => {
        const page = await open("/charts?case=ring", { js: false });
        assert.equal(await page.locator("#ring-1 [data-slot=count-up]").textContent(), "82");
        assert.equal(await page.locator("#ring-3 [data-slot=count-up]").textContent(), "12,450");
        for (const fig of ["ring-1", "ring-2", "ring-3"]) assert.equal(await page.locator(`#${fig} [data-slot=radial-arc]`).count(), 1, `${fig}: its arc`);
        assert.equal(await page.locator("#ring-3 [data-slot=radial-lap]").count(), 1, "past the goal, a second lap");
        await page.done();
        const clock = await open("/charts?case=clock", { js: false });
        assert.equal(await clock.locator("#clock [data-slot=clock-wedge]").count(), 24);
        assert.equal(await clock.locator("#clock [data-slot=radial-center] [data-slot=count-up]").textContent(), "10,090");
        await clock.done();
      });
    });

    // ---- 7b: radial charts ------------------------------------------------------------------

    describe("radial charts", () => {
      test("a ring, rings and a gauge are pictures named by their title and described by their summary; tiny rings in a pill are one image", async () => {
        for (const [kase, figures] of [["ring", ["ring-1", "ring-2", "ring-3"]], ["rings", ["rings-1", "rings-2"]], ["gauge", ["gauge-1", "gauge-2"]]]) {
          const page = await open(`/charts?case=${kase}`);
          for (const fig of figures) {
            const body = page.locator(`#${fig} [data-slot=chart-body]`);
            assert.equal(await body.getAttribute("role"), "img", `${fig} is a picture`);
            assert.equal(await body.getAttribute("tabindex"), null, `${fig} is not a tab stop`);
            assert.equal(await page.locator(`#${fig}[data-ui-chart]`).count(), 0, `${fig} loads no tooltip`);
            assert.ok((await page.locator(`#${fig}-summary`).textContent()).includes(" of ") || fig === "gauge-2", `${fig}: a summary`);
            assert.ok((await page.locator(`#${fig} table tbody tr`).count()) >= 1, `${fig}: a table`);
          }
          if (kase === "rings") {
            const pill = page.locator("[data-slot=radial-inline]");
            assert.equal(await pill.getAttribute("role"), "img");
            assert.match(await pill.getAttribute("aria-label"), /^Movement: 35 of 100, 35%\./);
            assert.equal(await pill.locator("table, figure").count(), 0, "no table inside a button");
          }
          if (kase === "gauge") assert.match(await page.locator("#gauge-2-summary").textContent(), /36\.7°C, on a scale from 34°C to 39°C/);
          await page.done();
        }
      });

      test("the middle counts up to the server's number, and a ring takes its wider size on a wider screen", async () => {
        const wide = await open("/charts?case=ring");
        await settle(wide);
        assert.equal(await wide.locator("#ring-2 [data-slot=count-up]").textContent(), "87");
        assert.equal(Math.round((await wide.locator("#ring-2 [data-slot=radial]").boundingBox()).width), 160);
        await wide.done();
        const narrow = await open("/charts?case=ring", { width: 390 });
        assert.equal(Math.round((await narrow.locator("#ring-2 [data-slot=radial]").boundingBox()).width), 140);
        await narrow.done();
      });

      test("rings and the clock come in only for a reader who has not asked for less motion", async () => {
        for (const kase of ["ring", "rings", "clock"]) {
          for (const reduce of [false, true]) {
            const page = await open(`/charts?case=${kase}`, { reducedMotion: reduce ? "reduce" : "no-preference" });
            const running = await page.evaluate(() => document.getAnimations().length);
            if (reduce) assert.equal(running, 0, `${kase}: nothing moves`);
            else assert.ok(running > 0, `${kase}: the marks come in`);
            await page.done();
          }
        }
      });

      test("the day clock's tooltip comes up beside the hour, on the side of the clock the hour is on", async () => {
        const page = await open("/charts?case=clock");
        await tabTo(page, "#clock [data-slot=chart-body]");
        for (const [key, hour] of [["Home", 0], ["PageDown", 2], ["PageDown", 4], ["PageDown", 6], ["PageDown", 8], ["End", 23]]) {
          await page.keyboard.press(key);
          await expectAt(page, "clock", hour, key);
          const near = await page.evaluate((hour) => {
            const f = Document.prototype.getElementById.call(document, "clock");
            const anchor = f.querySelector(`[data-slot=clock-anchor][data-i="${hour}"]`).getBoundingClientRect();
            const tip = f.querySelector("[data-slot=chart-tip]").getBoundingClientRect();
            const x = anchor.left + anchor.width / 2, y = anchor.top + anchor.height / 2;
            const dx = Math.max(tip.left - x, 0, x - tip.right), dy = Math.max(tip.top - y, 0, y - tip.bottom);
            return Math.hypot(dx, dy);
          }, hour);
          assert.ok(near < 24, `hour ${hour}: the tooltip is beside its hour (${near} px away)`);
        }
        assert.match(await page.locator("#clock [data-slot=chart-announce]").textContent(), /11:00 pm/i);
        await page.done();
      });

      test("the pointer on a wedge or a moment shows that hour", async () => {
        const page = await open("/charts?case=clock");
        const at = await hitPoint(page, "#clock [data-slot=clock-wedge][data-i='7']");
        await page.mouse.move(at.x, at.y);
        await expectAt(page, "clock", 7, "pointer", { speaks: false });
        const moment = await hitPoint(page, "#clock [data-slot=clock-moment][data-i='13']");
        await page.mouse.move(moment.x, moment.y);
        const v = await expectAt(page, "clock", 13, "pointer on a moment", { speaks: false });
        assert.equal(v.markup, 0, "a hostile moment's name is text");
        await page.done();
      });
    });

    // ---- 8: hostile -------------------------------------------------------------------------

    describe("hostile", () => {
      test("hostile text in a title, a description, a category, a series and a target stays text", async () => {
        const page = await open("/charts?case=hostile");
        assert.equal(await page.evaluate(() => window.__pwned), undefined, "no script ran");
        assert.equal(await page.locator("#hostile img, #hostile script, #hostile iframe").count(), 0, "no element made from the text");
        assert.equal(await page.locator("main img").count(), 0);
        assert.equal(await page.locator("#hostile [onerror], #hostile [onclick]").count(), 0);
        assert.equal(await page.locator("#hostile [data-slot=chart-title]").textContent(), NASTY);
        assert.equal(await page.locator("#hostile [data-slot=chart-description]").textContent(), NASTY);
        assert.equal(await page.locator("#hostile table caption").textContent(), NASTY);
        assert.equal(await page.locator("#hostile tbody th").first().textContent(), NASTY);
        assert.equal(await page.locator("#hostile [data-slot=chart-legend-name]").first().textContent(), NASTY);
        assert.equal(await page.locator("#hostile [data-slot=chart-body]").getAttribute("aria-label"), `${NASTY}, chart`);
        assert.ok((await page.locator("#hostile [data-slot=chart-target] text").textContent()).includes("<script>"), "the target's label is text");
        // The tooltip and the live region show the string as text.
        await tabTo(page, "#hostile [data-slot=chart-body]");
        const v = await expectAt(page, "hostile", 0);
        assert.equal(v.head, NASTY);
        assert.equal(v.rows[0].name, NASTY);
        assert.equal(v.rows[0].value, "3");
        assert.equal(v.rows[1].value, "–", "a gap is a dash");
        assert.ok(v.say.startsWith(NASTY), "the live region has the string as text");
        assert.deepEqual(await page.locator("#hostile [data-slot=chart-tip] *").evaluateAll((els) => els.map((e) => e.tagName).filter((t) => !["DIV", "SPAN", "STRONG"].includes(t))), []);
        await page.keyboard.press("ArrowRight");
        await expectAt(page, "hostile", 1);
        // By pointer too.
        const at = await hitPoint(page, "#hostile [data-slot=chart-bar][data-i='0']");
        await page.mouse.move(at.x, at.y);
        await until(async () => (await view(page, "hostile")).head === NASTY, "the hostile string in the tooltip");
        assert.equal(await page.evaluate(() => window.__pwned), undefined);
        assert.equal(await page.locator("#hostile img, #hostile script").count(), 0);
        await page.done();
      });

      describe("the module on a page full of named elements", () => {
        let f;
        let figure;
        before(async () => {
          f = await start();
          f.routes.set("/_navree/ui/charts.js", (req, res) => {
            res.writeHead(200, { "content-type": "text/javascript" });
            res.end(chartsModule);
          });
          // The chart's own markup, as the proof app writes it, in a page that serves the committed assets.
          const text = await (await fetch(`${BASE}/charts?case=bar`)).text();
          const from = text.indexOf("<figure");
          figure = text.slice(from, text.indexOf("</figure>", from) + "</figure>".length);
          assert.ok(figure.includes("data-ui-chart") && figure.includes("chart-table"), "the figure was cut out whole");
        });
        after(() => f?.close());

        // A heading above the chart is where the pointer goes to leave it.
        const html = (body) => doc(body.replace("<main>", `<h1 id="away">A page with a chart</h1><main>`), { css: true, head: `<script type="module" src="/_navree/ui/charts.js"></script>` });
        const serve = (path, body) => f.routes.set(path, (req, res) => f.html(res, html(body)));
        /** A browser page on the fixture, with its errors collected. */
        async function visit(path) {
          const context = await browser.newContext({ viewport: { width: 1000, height: 800 } });
          contexts.push(context);
          const p = await context.newPage();
          p.errors = [];
          p.on("pageerror", (e) => p.errors.push(String(e)));
          // The fixture serves the stylesheet and not the font it names.
          p.on("console", (m) => m.type() === "error" && !/Failed to load resource|Refused to apply a stylesheet|downloadable font/.test(m.text()) && p.errors.push(m.text()));
          await p.addInitScript(() => {
            document.addEventListener("securitypolicyviolation", (e) => (window.__csp ||= []).push(e.violatedDirective + " " + e.blockedURI));
          });
          await p.goto(f.origin + path);
          return p;
        }
        /** The names, of `names`, that the document still answers with its own: run in the page. */
        const unshadowed = (names) =>
          names.filter((n) => {
            const d = [Document.prototype, Node.prototype, EventTarget.prototype].map((proto) => Object.getOwnPropertyDescriptor(proto, n)).find(Boolean);
            const own = d.get ? d.get.call(document) : d.value;
            return document[n] === own;
          });
        const csp = async (p) => (await p.evaluate(() => window.__csp || [])).filter((line) => name !== "webkit" || !/^style-src-(elem|attr) inline$/.test(line));

        /** Keys, a tooltip, the live region and a pointer on the bar chart of the fixture page. */
        async function works(p) {
          assert.equal(await attrOf(p, "[data-slot=chart-body]", "tabindex"), "0", "the module made the chart a tab stop");
          const n = 7;
          await tabTo(p, "#bar [data-slot=chart-body]");
          await expectAt(p, "bar", 0, "clobbered");
          let at = 0;
          for (const key of ["ArrowRight", "ArrowRight", "End", "Home", "PageDown", "ArrowLeft"]) {
            await p.keyboard.press(key);
            at = moved(key, at, n, false);
            await expectAt(p, "bar", at, `clobbered ${key}`);
          }
          await p.keyboard.press("Escape");
          assert.equal((await view(p, "bar")).open, false);
          // Focus off the chart (a focused chart keeps its tooltip when the pointer leaves), then the pointer.
          await p.keyboard.press("Tab");
          assert.equal(await focusOn(p, "#bar [data-slot=chart-body]"), false);
          const hit = await hitPoint(p, "#bar [data-slot=chart-bar][data-i='5']");
          await p.mouse.move(hit.x, hit.y);
          await until(async () => (await view(p, "bar")).active[0] === "5", "the tooltip by pointer");
          await expectAt(p, "bar", 5, "clobbered pointer", { speaks: false });
          const away = await boxOf(p, "#away");
          await p.mouse.move(away.x + 4, away.y + 4);
          await until(async () => !(await view(p, "bar")).open, "the tooltip to close");
          // The switch works: focus is on it after the Tab above, and Space shows the table.
          assert.equal(await focusOn(p, "#bar [data-slot=chart-table-switch]"), true);
          await p.keyboard.press("Space");
          const height = await p.evaluate(() => Element.prototype.getBoundingClientRect.call(Document.prototype.querySelector.call(document, "#bar table")).height);
          assert.ok(height > 40, `the table is drawn: ${height}`);
        }

        test("the figure from the proof app works on the fixture page with no names (the control)", async () => {
          serve("/plain", `<main>${figure}</main>`);
          const p = await visit("/plain");
          await works(p);
          assert.deepEqual(p.errors, []);
          assert.deepEqual(await csp(p), []);
        });

        test("images named after what the module reads, and a form named querySelector around the chart", async () => {
          const names = ["cookie", "getElementById", "querySelector", "querySelectorAll", "body", "head", "documentElement", "activeElement", "title", "createElement", "createTextNode", "addEventListener", "dispatchEvent", "getBoundingClientRect", "parentElement"];
          const clobber = names.map((n) => `<img name="${n}" src="${GIF}" alt=""><img id="${n}" src="${GIF}" alt="">`).join("");
          serve(
            "/clobbered",
            `${clobber}<form name="querySelector" id="addEventListener"><input name="closest" value="x"><input name="matches" value="x"><input name="getBoundingClientRect" value="x"><input name="parentElement" value="x"><input name="querySelectorAll" value="x"><input name="getAttribute" value="x"><input name="children" value="x"><input name="ownerDocument" value="x"><main>${figure}</main></form>`,
          );
          const p = await visit("/clobbered");
          // The premise: the page's own names really do shadow what the document has.
          assert.deepEqual(await p.evaluate(unshadowed, names.filter((n) => n !== "getBoundingClientRect")), [], "every name shadows the document's own");
          await works(p);
          assert.deepEqual(p.errors, [], "nothing threw");
          assert.deepEqual(await csp(p), []);
        });

        for (const clobbered of ["cookie", "getElementById", "querySelector", "querySelectorAll", "body", "head", "documentElement", "activeElement", "title", "createElement", "createTextNode", "addEventListener"]) {
          test(`an element named or with the id ${clobbered} leaves keys and tooltips working`, async () => {
            serve(`/named-${clobbered}`, `<img name="${clobbered}" src="${GIF}" alt=""><div id="${clobbered}"></div><form name="${clobbered}"><input name="${clobbered}"></form><main>${figure}</main>`);
            const p = await visit(`/named-${clobbered}`);
            assert.deepEqual(await p.evaluate(unshadowed, [clobbered]), [], `document.${clobbered} is the page's, not the browser's`);
            await works(p);
            assert.deepEqual(p.errors, [], `${clobbered}: nothing threw`);
          });
        }

        test("a chart with hostile text on the fixture page: no script, no element, the text in the tooltip", async () => {
          const text = await (await fetch(`${BASE}/charts?case=hostile`)).text();
          const from = text.indexOf("<figure");
          const hostile = text.slice(from, text.indexOf("</figure>", from) + "</figure>".length);
          serve("/hostile-figure", `<img name="activeElement" src="${GIF}" alt=""><form name="querySelector"><main>${hostile}</main></form>`);
          const p = await visit("/hostile-figure");
          await tabTo(p, "#hostile [data-slot=chart-body]");
          const v = await expectAt(p, "hostile", 0);
          assert.equal(v.head, NASTY);
          assert.equal(await p.evaluate(() => window.__pwned), undefined);
          assert.equal(await p.locator("#hostile img, #hostile script").count(), 0);
          assert.deepEqual(p.errors, []);
          assert.deepEqual(await csp(p), []);
        });
      });
    });

    // ---- 9: the report ----------------------------------------------------------------------

    describe("report", () => {
      const rowsOf = (page) => page.locator("#report-bars tbody tr").count();
      const stamp = (page) => page.evaluate(() => ({ from: document.querySelector("input[name=from]").value, to: document.querySelector("input[name=to]").value, grain: document.querySelector("[data-slot=chart-grains] [aria-current]")?.textContent }));

      test("the default window is the last 30 days by day, and the charts, the numbers and the table agree", async () => {
        const page = await open("/charts/report");
        assert.equal(await rowsOf(page), 30);
        assert.deepEqual(await stamp(page), { from: "2026-10-05", to: "2026-11-03", grain: "Day" });
        // The area chart and the bar chart cover the same buckets.
        assert.equal(await page.locator("#report-area tbody tr").count(), 30);
        // The numbers add up to the table under them.
        const reported = await page.locator("[data-slot=stat]:has([data-slot=stat-label]:text-is('Reported')) [data-slot=stat-value]").textContent();
        const sum = (await page.locator("#report-bars tbody td").allTextContents()).reduce((a, t) => a + Number(t.replace(/,/g, "")), 0);
        assert.equal(sum, Number(reported), "the bars add up to the Reported figure");
        assert.deepEqual(await axe(page), []);
        await page.done();
      });

      test("a window in the URL sets the bucket count, and the grain links change it", async () => {
        const page = await open("/charts/report?from=2026-10-28&to=2026-11-03");
        assert.equal(await rowsOf(page), 7);
        assert.equal(await page.locator("#report-area tbody tr").count(), 7);
        assert.deepEqual(await stamp(page), { from: "2026-10-28", to: "2026-11-03", grain: "Day" });
        const week = await open("/charts/report?from=2026-10-05&to=2026-11-03&grain=week");
        assert.equal(await rowsOf(week), 5);
        assert.equal((await stamp(week)).grain, "Week");
        // A year offers the longer grains.
        const year = await open("/charts/report?from=2025-11-01&to=2026-11-03");
        const links = await year.locator("[data-slot=chart-grains] a").allTextContents();
        assert.deepEqual(links.sort(), ["Day", "Month", "Quarter", "Week"]);
        assert.ok((await rowsOf(year)) <= 80, "a year is drawn from a few dozen weekly buckets");
        await page.done();
        await week.done();
        await year.done();
      });

      test("a refused window shows the warning and the default", async () => {
        for (const bad of ["from=2026-10-01&to=2026-12-01", "from=2024-01-01&to=2024-02-01"]) {
          const page = await open(`/charts/report?${bad}`);
          const alert = page.locator("[data-slot=alert]");
          assert.equal(await alert.count(), 1, bad);
          assert.match(await alert.innerText(), /That window can.t be shown/);
          assert.match(await alert.innerText(), /Showing the last 30 days/);
          assert.doesNotMatch(await alert.innerText(), /\.\./, "one full stop, not two");
          assert.equal(await rowsOf(page), 30, `${bad}: the default window is drawn`);
          assert.deepEqual(await stamp(page), { from: "2026-10-05", to: "2026-11-03", grain: "Day" });
          assert.deepEqual(await axe(page), [], bad);
          await page.done();
        }
        // Junk is the default without a warning; a reversed window is put in order.
        const junk = await open("/charts/report?from=junk&to=2026-11-03");
        assert.equal(await rowsOf(junk), 30);
        const reversed = await open("/charts/report?from=2026-11-03&to=2026-10-01");
        assert.equal((await stamp(reversed)).from, "2026-10-01");
        assert.equal(await rowsOf(reversed), 34);
        const none = await open("/charts/report");
        assert.equal(await none.locator("[data-slot=alert]").count(), 0, "no warning for the default");
      });

      test("a preset in the range picker and Apply change the window; back and forward restore it", async () => {
        const page = await open("/charts/report");
        await page.locator("#report-window [data-ui-act=open]").click();
        await until(() => page.locator("#report-window-pop").evaluate((el) => el.matches(":popover-open")), "the picker to open");
        await page.locator("#report-window-pop [data-ui-preset=last_7_days]").click();
        await until(async () => (await stamp(page)).from === "2026-10-28", "the preset to fill the form");
        await page.locator("form[data-slot=report-filters] button[type=submit]").click();
        await page.waitForURL(/from=2026-10-28&to=2026-11-03/);
        await until(async () => (await rowsOf(page)) === 7, "seven buckets");
        assert.deepEqual(await stamp(page), { from: "2026-10-28", to: "2026-11-03", grain: "Day" });
        assert.equal(await page.locator("#report-area tbody tr").count(), 7);
        assert.match(await page.locator("#report-window [data-ui-label]").textContent(), /Oct 28/);
        // A grain link keeps the window.
        await page.getByRole("link", { name: "Hour" }).click();
        await page.waitForURL(/grain=hour/);
        await until(async () => (await rowsOf(page)) === 84 && (await stamp(page)).grain === "Hour", "hours");
        assert.equal(await page.locator("#report-area tbody tr").count(), 168, "the area chart keeps every hour of 7 days");
        // Back: the day grain; back again: the default window.
        await page.goBack();
        await until(async () => (await rowsOf(page)) === 7, "back to seven days");
        assert.deepEqual(await stamp(page), { from: "2026-10-28", to: "2026-11-03", grain: "Day" });
        await page.goBack();
        await until(async () => (await rowsOf(page)) === 30, "back to the default window");
        assert.deepEqual(await stamp(page), { from: "2026-10-05", to: "2026-11-03", grain: "Day" });
        assert.match(await page.locator("#report-window [data-ui-label]").textContent(), /Oct 5/, "the picker shows the restored window");
        await page.goForward();
        await until(async () => (await rowsOf(page)) === 7, "forward to seven days");
        await page.goForward();
        await until(async () => (await rowsOf(page)) === 84, "forward to hours");
        assert.equal((await stamp(page)).grain, "Hour");
        // The charts still work after all that moving.
        await tabTo(page, "#report-area [data-slot=chart-body]");
        await expectAt(page, "report-area", 0, "after history");
        await page.done();
      });

      test("with JavaScript off: the native dates and Apply change the window, and the grain links work", async () => {
        const page = await open("/charts/report", { js: false });
        assert.equal(await page.locator("input[name=from]").isVisible(), true, "the date boxes are there");
        assert.equal(await page.locator("#report-window [data-ui-act=open]").isVisible(), false);
        assert.equal(await rowsOf(page), 30);
        await page.locator("input[name=from]").fill("2026-10-28");
        await page.locator("input[name=to]").fill("2026-11-03");
        await page.locator("form[data-slot=report-filters] button[type=submit]").click();
        await page.waitForURL(/from=2026-10-28&to=2026-11-03/);
        assert.equal(await rowsOf(page), 7);
        assert.deepEqual(await stamp(page), { from: "2026-10-28", to: "2026-11-03", grain: "Day" });
        await page.getByRole("link", { name: "Hour" }).click();
        await page.waitForURL(/grain=hour/);
        assert.equal(await rowsOf(page), 84);
        assert.equal((await stamp(page)).grain, "Hour");
        // Apply with a grain chosen keeps it: the form carries it.
        await page.locator("input[name=from]").fill("2026-11-01");
        await page.locator("form[data-slot=report-filters] button[type=submit]").click();
        await page.waitForURL(/from=2026-11-01/);
        assert.equal(new URL(page.url()).searchParams.get("grain"), "hour", "Apply keeps the grain");
        assert.equal(await rowsOf(page), 72, "three days by hour");
        await page.goBack();
        await page.waitForURL(/from=2026-10-28/);
        assert.equal(await rowsOf(page), 84);
        // The grain links of a year, without script.
        const year = await open("/charts/report?from=2025-11-01&to=2026-11-03", { js: false });
        await year.getByRole("link", { name: "Month" }).click();
        await year.waitForURL(/grain=month/);
        assert.equal(await rowsOf(year), 13);
        assert.equal((await stamp(year)).grain, "Month");
        await year.getByRole("link", { name: "Quarter" }).click();
        await year.waitForURL(/grain=quarter/);
        assert.equal(await rowsOf(year), 5);
        await year.getByRole("link", { name: "Day" }).click();
        await year.waitForURL(/grain=day/);
        assert.equal(await year.locator("#report-area tbody tr").count(), 368, "every day of the year in the area chart");
        const bars = await rowsOf(year);
        assert.ok(bars > 50 && bars < 368, `the bars merge their neighbours past the limit: ${bars}`);
        await page.done();
        await year.done();
      });

      test("the report at 390 px: axe clean, nothing spills, in dark and light", async () => {
        for (const light of [false, true]) {
          const page = await open("/charts/report?from=2026-10-05&to=2026-11-03&grain=week", { width: 390, light });
          assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), "no sideways scroll");
          assert.deepEqual(await axe(page), []);
          if (SHOTS) await page.screenshot({ path: join(SHOTS, `${name}-report-390-${light ? "light" : "dark"}.png`), fullPage: true });
          await page.done();
          await page.context().close();
        }
      });
    });

    // ---- 10: a million points ---------------------------------------------------------------

    describe("a large series", () => {
      test("a million points are built in reasonable time and drawn from at most 640", async () => {
        const started = Date.now();
        const page = await open("/charts/big");
        const elapsed = Date.now() - started;
        const built = Number(await page.locator("[data-built-ms]").getAttribute("data-built-ms"));
        assert.ok(Number.isFinite(built) && built < 1000, `the chart took ${built} ms to build`);
        assert.ok(elapsed < 20000, `the page loaded in ${elapsed} ms`);
        const xs = (await page.locator("#big [data-slot=chart-body]").getAttribute("data-xs")).split(" ");
        assert.ok(xs.length <= 640 && xs.length > 100, `the plot has ${xs.length} points`);
        assert.equal(Number(await page.locator("#big [data-slot=chart-body]").getAttribute("data-rows")), xs.length);
        const rows = await page.locator("#big tbody tr").count();
        assert.equal(rows, xs.length, "the table lists the points drawn");
        assert.match(await page.locator("#big [data-slot=chart-note]").first().textContent(), /shows \d+ of 1,000,000 points/);
        const pathLength = await page.locator("#big [data-slot=chart-line]").evaluate((el) => el.getAttribute("d").split(/[ML]/).length);
        assert.ok(pathLength <= 700, `the line has ${pathLength} segments`);
        // The size on the wire: a million points do not reach the page.
        const html = await (await fetch(`${BASE}/charts/big`)).text();
        assert.ok(html.length < 200_000, `the page is ${html.length} bytes`);
        // It still works: keys reach both ends, and the end is the last point.
        await tabTo(page, "#big [data-slot=chart-body]");
        await expectAt(page, "big", 0);
        await page.keyboard.press("End");
        const v = await expectAt(page, "big", xs.length - 1, "End");
        assert.equal(v.cursor.x1, "100%");
        const box = await page.locator("#big [data-slot=chart-plot]").boundingBox();
        await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
        await until(async () => {
          const v = await view(page, "big");
          return v.open && Math.abs(Number(v.cursor.x1.replace("%", "")) - 50) < 1;
        }, "the tooltip in the middle");
        assert.deepEqual(await axe(page), []);
        await page.done();
      });

      test("a smaller n, a single point and a long series are drawn as asked", async () => {
        for (const [n, expected] of [[1, 1], [2, 2], [500, 500], [640, 640], [641, 640], [5000, 640]]) {
          const page = await open(`/charts/big?n=${n}`);
          const rows = Number(await page.locator("#big [data-slot=chart-body]").getAttribute("data-rows"));
          assert.equal(rows, expected, `n=${n}`);
          assert.equal(await page.locator("#big tbody tr").count(), expected);
          await page.done();
        }
      });
    });
  });
}
