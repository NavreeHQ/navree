// The grid charts in real browsers: the calendar and matrix heat maps and the scatter plot, against the
// proof app's /charts/grids. Every case at 390 and 1440 px in dark and light with axe clean (also with a
// tooltip open), no policy report and no sideways scroll of the page; the keys of a grid (arrows by row and
// column, Home and End along a row, the page keys along a column, Escape) with the tooltip and the live
// region saying what the table's row says, and right to left; the pointer; a calendar that scrolls starting
// at the latest week and following the keys; the steps of the colour in both palettes; empty days that are
// not zero; motion only for a reader who allows it; JavaScript off; hostile text; lakh grouping.
// Run with the proof app serving:
//
//   PORT=8132 cargo run --release -p ui-proof -- serve
//   CHARTS_URL=http://127.0.0.1:8132 UI_BROWSERS=chromium,firefox,webkit npm test -- tests/chart-grids.test.mjs
//
// CHARTS_URL is the server (default http://127.0.0.1:8132). UI_BROWSERS picks the engines (default chromium).
// UI_SHOTS=<dir> saves a screenshot of every case at both widths in both themes. Without the server the suite
// is skipped.

import assert from "node:assert/strict";
import { mkdirSync, readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { after, afterEach, before, describe, test } from "node:test";
import { chromium, firefox, webkit } from "playwright";
import { launch } from "./support/launch.mjs";

const BASE = process.env.CHARTS_URL || "http://127.0.0.1:8132";
const ENGINES = { chromium, firefox, webkit };
const NAMES = (process.env.UI_BROWSERS || "chromium").split(",").filter((n) => ENGINES[n]);
const SHOTS = process.env.UI_SHOTS;
if (SHOTS) mkdirSync(SHOTS, { recursive: true });

const require = createRequire(import.meta.url);
const axeSource = readFileSync(require.resolve("axe-core/axe.min.js"), "utf8");
const reachable = await fetch(`${BASE}/charts/grids?case=matrix`).then((r) => r.ok, () => false);

const CASES = ["calendar", "calendar-short", "calendar-year", "matrix", "matrix-full", "relations", "scatter", "scatter-axes", "scatter-big", "locale", "motion", "hostile", "states"];
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

/** Polls `check` until it is true: the wait is for a state, never for a time. */
async function until(check, what, ms = 8000) {
  const end = Date.now() + ms;
  for (;;) {
    const got = await check();
    if (got) return got;
    if (Date.now() > end) assert.fail(`timed out waiting for ${what}`);
    await sleep(25);
  }
}

async function settle(page) {
  await page.evaluate(() => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))));
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

/** What one figure shows now: the tooltip, the live region, the marked cells and the marked headers. */
const view = (page, fig) =>
  page.evaluate((fig) => {
    const f = Document.prototype.getElementById.call(document, fig);
    const body = f.querySelector("[data-slot=chart-body]");
    const tip = body.querySelector("[data-slot=chart-tip]");
    const tb = tip.getBoundingClientRect();
    const bb = body.getBoundingClientRect();
    const open = tip.hasAttribute("data-open");
    return {
      open,
      head: tip.querySelector("[data-tip=head]")?.textContent ?? null,
      rows: [...tip.querySelectorAll("[data-tip=row]")].map((r) => ({ value: r.querySelector("strong").textContent, name: r.lastElementChild.textContent, unit: r.querySelector("em")?.textContent ?? "" })),
      markup: tip.querySelectorAll("img,script,iframe").length,
      say: f.querySelector("[data-slot=chart-announce]").textContent,
      // The tip may stand anywhere in the scrolled content; it must be inside the visible part.
      inside: !open || (tb.left >= bb.left - 0.5 && tb.right <= bb.right + 0.5),
      active: [...body.querySelectorAll("[data-i][data-active]")].map((e) => +e.getAttribute("data-i")),
      heads: [...body.querySelectorAll("[data-hr][data-active],[data-hc][data-active]")].map((e) => e.textContent),
      cell: (() => {
        const c = body.querySelector("[data-i][data-active]");
        return c && { r: +c.dataset.r, c: +c.dataset.c, level: c.dataset.level ?? null };
      })(),
      scroll: [body.scrollLeft, body.scrollWidth, body.clientWidth],
    };
  }, fig);

/** What the table says for row `i`. */
const fromTable = (page, fig, i) =>
  page.evaluate(
    ({ fig, i }) => {
      const f = Document.prototype.getElementById.call(document, fig);
      const table = f.querySelector("[data-slot=chart-table] table");
      const heads = [...table.querySelectorAll("thead th")];
      const cells = [...table.querySelectorAll("tbody tr")[i].querySelectorAll("th,td")];
      const text = (cell) => (cell.querySelector("[aria-hidden=true]") || cell).textContent.trim();
      return { head: text(cells[0]), rows: cells.slice(1).map((c, k) => ({ value: text(c), name: heads[k + 1].textContent.trim() })), count: table.querySelectorAll("tbody tr").length };
    },
    { fig, i },
  );

/** The tooltip and the live region say what the table's row `i` says, and exactly cell `i` is marked. */
async function expectAt(page, fig, i, what = "") {
  const v = await view(page, fig);
  const t = await fromTable(page, fig, i);
  const label = `${fig} at ${i}${what ? ` (${what})` : ""}`;
  assert.equal(v.open, true, `${label}: the tooltip is open`);
  assert.equal(v.markup, 0, `${label}: no markup from the data in the tooltip`);
  assert.equal(v.head, t.head, `${label}: the head is the row's header`);
  assert.deepEqual(v.rows.map(({ value, name }) => ({ value, name })), t.rows, `${label}: the lines are the row's values`);
  assert.equal(v.say, [t.head, ...t.rows.map((r) => `${r.name} ${r.value}`)].join(", "), `${label}: the live region says the same words`);
  assert.equal(v.inside, true, `${label}: the tooltip stays in the visible chart`);
  assert.deepEqual(v.active, [i], `${label}: exactly value ${i} is marked`);
  return v;
}

/** Whether the focused element matches `selector`. */
const focusOn = (page, selector) =>
  page.evaluate((selector) => {
    const active = Object.getOwnPropertyDescriptor(Document.prototype, "activeElement").get.call(document);
    return !!active && Element.prototype.matches.call(active, selector);
  }, selector);

async function tabTo(page, selector, max = 120) {
  for (let i = 0; i <= max; i++) {
    if (await focusOn(page, selector)) return i;
    await page.keyboard.press("Tab");
  }
  assert.fail(`Tab never reached ${selector}`);
}

const body = (fig) => `#${fig} [data-slot=chart-body]`;

describe("the markup the server writes", { skip: reachable ? false : `needs the proof app at ${BASE}` }, () => {
  for (const name of CASES) {
    test(`/charts/grids?case=${name}: no style attribute, no handler, no repeated attribute`, async () => {
      const response = await fetch(`${BASE}/charts/grids?case=${name}`);
      assert.equal(response.status, 200);
      const html = await response.text();
      const figures = html.slice(html.indexOf("<figure"), html.lastIndexOf("</figure>"));
      assert.doesNotMatch(figures, /<(script|style|iframe|object|embed|img)\b/i, "no script, style or image element in a chart");
      for (const tag of figures.matchAll(/<([a-zA-Z][\w-]*)((?:\s+[^\s"'<>/=]+(?:="[^"]*")?)*)\s*\/?>/g)) {
        const names = [...tag[2].matchAll(/\s+([^\s"'<>/=]+)(?:="[^"]*")?/g)].map((a) => a[1].toLowerCase());
        assert.deepEqual(names.filter((n) => n === "style" || n.startsWith("on")), [], `<${tag[1]}> has a style attribute or a handler`);
        assert.deepEqual(names.filter((n, i) => names.indexOf(n) !== i), [], `<${tag[1]}> repeats an attribute`);
      }
    });
  }
  test("a scatter of five thousand points draws fewer than two thousand circles", async () => {
    const html = await (await fetch(`${BASE}/charts/grids?case=scatter-big`)).text();
    const circles = html.match(/<circle /g)?.length ?? 0;
    assert.ok(circles > 1000 && circles <= 2000, `${circles} circles`);
    assert.match(html, /The drawing shows 1,667 of 5,000 points/);
    assert.match(html, /All: 5,000 points\./);
  });
});

for (const name of NAMES) {
  describe(name, { concurrency: false, skip: reachable ? false : `needs the proof app at ${BASE}` }, () => {
    let browser;
    before(async () => {
      browser = await launch(ENGINES[name]);
    });
    after(() => browser?.close());
    const contexts = [];
    afterEach(async () => {
      for (const context of contexts.splice(0)) await context.close().catch(() => {});
    });

    async function open(path, { width = 1440, height = 900, light = false, js = true, reducedMotion = "no-preference" } = {}) {
      const context = await browser.newContext({ viewport: { width, height }, javaScriptEnabled: js, colorScheme: light ? "light" : "dark", reducedMotion });
      contexts.push(context);
      const page = await context.newPage();
      page.problems = [];
      page.on("console", (m) => {
        if (name === "webkit" && /Refused to apply a stylesheet/.test(m.text())) return;
        if (m.type() === "error" && !/Failed to load resource/.test(m.text())) page.problems.push(m.text());
      });
      page.on("pageerror", (e) => page.problems.push("pageerror: " + e.message));
      if (js) await page.addInitScript(() => document.addEventListener("securitypolicyviolation", (e) => (window.__csp ||= []).push(e.violatedDirective + " " + e.blockedURI)));
      await page.goto(BASE + path + (light ? (path.includes("?") ? "&" : "?") + "mode=light" : ""));
      page.done = async () => {
        let csp = js ? await page.evaluate(() => window.__csp || []) : [];
        if (name === "webkit") csp = csp.filter((line) => !/^style-src-(elem|attr) inline$/.test(line));
        assert.deepEqual(csp, [], "no content security policy report");
        assert.deepEqual(page.problems, [], "no console error and no page error");
      };
      return page;
    }

    for (const width of [390, 1440]) {
      for (const light of [false, true]) {
        for (const c of CASES) {
          test(`${c} at ${width}px, ${light ? "light" : "dark"}: axe clean, no sideways scroll, no policy report`, async () => {
            const page = await open(`/charts/grids?case=${c}`, { width, light });
            assert.deepEqual(await axe(page), []);
            const wide = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
            assert.ok(wide <= 0, `the page scrolls sideways by ${wide}px`);
            // With a tooltip open: the first figure's first value, by keys.
            const first = await page.evaluate(() => document.querySelector("[data-ui-chart]")?.id);
            if (first) {
              await page.focus(body(first));
              await page.keyboard.press("ArrowRight");
              assert.deepEqual(await axe(page), [], "axe clean with a tooltip open");
            }
            if (SHOTS) await page.screenshot({ path: `${SHOTS}/grids-${c}-${width}-${light ? "light" : "dark"}-${name}.png`, fullPage: true });
            await page.done();
          });
        }
      }
    }

    describe("the calendar", () => {
      test("the keys move by week and by day, along a row and down a column, and the tooltip says the table's row", async () => {
        const page = await open("/charts/grids?case=calendar");
        await tabTo(page, body("calendar"));
        // Focus alone shows the first day.
        await expectAt(page, "calendar", 0, "focus");
        const rowsOf = await page.evaluate(() => document.querySelectorAll("#calendar [data-slot=chart-table] tbody tr").length);
        assert.equal(rowsOf, 181);
        // 181 days end on 12 November, so the first is Saturday 16 May: the first row of the grid is Sunday
        // here, so it is row 6 of the first column, which holds that day alone.
        const start = (await view(page, "calendar")).cell;
        assert.deepEqual([start.r, start.c], [6, 0]);
        await page.keyboard.press("ArrowDown");
        assert.deepEqual((await view(page, "calendar")).active, [0], "down from the last row stays");
        await page.keyboard.press("ArrowRight");
        let v = await expectAt(page, "calendar", 7, "right: the next week, the same weekday");
        assert.deepEqual([v.cell.r, v.cell.c], [6, 1]);
        await page.keyboard.press("ArrowUp");
        v = await expectAt(page, "calendar", 6, "up: the day before");
        assert.equal(v.cell.r, 5);
        await page.keyboard.press("PageUp");
        v = await expectAt(page, "calendar", 1, "page up: the top of the week");
        assert.equal(v.cell.r, 0);
        await page.keyboard.press("PageDown");
        v = await expectAt(page, "calendar", 7, "page down: the bottom of the week");
        assert.equal(v.cell.r, 6);
        await page.keyboard.press("Home");
        v = await expectAt(page, "calendar", 0, "home: the first week of the row");
        await page.keyboard.press("End");
        v = await view(page, "calendar");
        const last = v.cell;
        assert.equal(last.r, 6);
        assert.equal(last.c, 25, "end: the last Saturday");
        await page.keyboard.press("ArrowLeft");
        assert.equal((await view(page, "calendar")).cell.c, 24);
        await page.keyboard.press("Escape");
        assert.equal((await view(page, "calendar")).open, false, "Escape hides the tooltip");
        // Tab out and in again: the value it was on.
        await page.keyboard.press("Tab");
        await page.keyboard.press("Shift+Tab");
        assert.equal((await view(page, "calendar")).open, true);
        await page.done();
      });

      test("a day with no data says so, and is not a day of zero", async () => {
        const page = await open("/charts/grids?case=calendar");
        const kinds = await page.evaluate(() => {
          const c = [...document.querySelectorAll("#calendar [data-slot=heat-cell]")];
          const pick = (level) => c.find((e) => e.dataset.level === level);
          const style = (e) => e && getComputedStyle(e);
          return Object.fromEntries(["none", "0", "1", "2", "3", "4"].map((l) => [l, { present: !!pick(l), bg: style(pick(l))?.backgroundColor, outline: style(pick(l))?.outlineStyle }]));
        });
        assert.ok(kinds.none.present && kinds["0"].present, "the data has both an empty day and a day of zero");
        assert.equal(kinds.none.outline, "dashed", "no data is hollow and dashed");
        assert.match(kinds.none.bg, /rgba\(0, 0, 0, 0\)|transparent/);
        assert.notEqual(kinds["0"].bg, kinds.none.bg, "zero is filled");
        assert.equal(kinds["0"].outline, "none");
        // Hover an empty day.
        const at = await page.evaluate(() => Number([...document.querySelectorAll("#calendar [data-slot=heat-cell]")].find((e) => e.dataset.level === "none").dataset.i));
        await page.hover(`#calendar [data-i="${at}"]`);
        await until(async () => (await view(page, "calendar")).open, "the tooltip");
        const v = await view(page, "calendar");
        assert.equal(v.rows[0].value, "No data");
        const t = await fromTable(page, "calendar", at);
        assert.equal(t.rows[0].value, "No data");
        await page.done();
      });

      test("the steps run from pale to deep in both palettes", async () => {
        for (const light of [false, true]) {
          const page = await open("/charts/grids?case=calendar", { light });
          const lightness = await page.evaluate(() => {
            const bg = (l) => getComputedStyle([...document.querySelectorAll("#calendar [data-slot=heat-cell]")].find((e) => e.dataset.level === l)).backgroundColor;
            const rgb = (color) => {
              const ctx = document.createElement("canvas").getContext("2d", { willReadFrequently: true });
              ctx.fillStyle = "#000";
              ctx.fillStyle = color;
              ctx.fillRect(0, 0, 1, 1);
              return [...ctx.getImageData(0, 0, 1, 1).data.slice(0, 3)];
            };
            const luma = (c) => {
              const [r, g, b] = rgb(c);
              return 0.2126 * r + 0.7152 * g + 0.0722 * b;
            };
            return ["0", "1", "2", "3", "4"].map((l) => ({ l, bg: bg(l), luma: luma(bg(l)) }));
          });
          const colors = new Set(lightness.map((s) => s.bg));
          assert.equal(colors.size, 5, `five distinct steps (${light ? "light" : "dark"})`);
          const ordered = lightness.map((s) => s.luma);
          // Light: deeper is darker. Dark: deeper is brighter. Either way, one direction.
          const rising = ordered.every((x, i) => i === 0 || x >= ordered[i - 1]);
          const falling = ordered.every((x, i) => i === 0 || x <= ordered[i - 1]);
          assert.ok(light ? falling : rising, `${light ? "light" : "dark"} steps are monotone: ${JSON.stringify(ordered)}`);
          await page.done();
        }
      });

      test("on a phone the weeks scroll and start at the latest, and the keys bring the cell into view", async () => {
        const page = await open("/charts/grids?case=calendar", { width: 390 });
        const [left, whole, shown] = (await view(page, "calendar")).scroll;
        assert.ok(whole > shown + 100, "the weeks are wider than the chart");
        assert.ok(left >= whole - shown - 2, `it starts at the end: ${left} of ${whole - shown}`);
        await tabTo(page, body("calendar"));
        await page.keyboard.press("Home");
        await until(async () => (await view(page, "calendar")).scroll[0] < 30, "scrolling to the first week");
        const v = await view(page, "calendar");
        assert.equal(v.inside, true);
        await page.keyboard.press("End");
        await until(async () => (await view(page, "calendar")).scroll[0] > 100, "scrolling back toward the last week");
        // The cell is on screen.
        const onScreen = await page.evaluate(() => {
          const b = document.querySelector("#calendar [data-slot=chart-body]").getBoundingClientRect();
          const c = document.querySelector("#calendar [data-i][data-active]").getBoundingClientRect();
          return c.left >= b.left - 1 && c.right <= b.right + 1;
        });
        assert.ok(onScreen, "the active cell is inside the chart");
        await page.done();
      });

      test("right to left: the arrows are mirrored", async () => {
        const page = await open("/charts/grids?case=calendar");
        await page.evaluate(() => (document.documentElement.dir = "rtl"));
        await tabTo(page, body("calendar"));
        await page.keyboard.press("ArrowRight");
        // Right is the earlier week in a right-to-left page: from the first week there is none.
        assert.equal((await view(page, "calendar")).cell.c, 0);
        await page.keyboard.press("ArrowLeft");
        assert.equal((await view(page, "calendar")).cell.c, 1);
        await page.done();
      });

      test("motion is only for a reader who allows it, and the cells are in the page either way", async () => {
        for (const reduce of [false, true]) {
          const page = await open("/charts/grids?case=motion", { reducedMotion: reduce ? "reduce" : "no-preference" });
          const names = await page.evaluate(() => ({
            week: getComputedStyle(document.querySelector("#motion-calendar [data-slot=heat-week]")).animationName,
            row: getComputedStyle(document.querySelector("#motion-matrix [data-motion]")).animationName,
            points: getComputedStyle(document.querySelector("#motion-scatter [data-slot=chart-points]")).animationName,
            cells: document.querySelectorAll("#motion-calendar [data-slot=heat-cell]").length,
            circles: document.querySelectorAll("#motion-scatter circle").length,
          }));
          for (const k of ["week", "row", "points"]) assert.equal(names[k] !== "none", !reduce, `${k}: ${names[k]} (reduce ${reduce})`);
          assert.equal(names.cells, 60);
          assert.equal(names.circles, 40);
          await page.done();
        }
      });

      test("lakh grouping in the tooltip and the summary", async () => {
        const page = await open("/charts/grids?case=locale");
        const summary = await page.evaluate(() => document.querySelector("#locale-calendar [data-slot=chart-summary]").textContent);
        assert.match(summary, /₹7,20,250/, summary);
        const last = await page.evaluate(() => document.querySelectorAll("#locale-calendar [data-i]").length - 1);
        await page.hover(`#locale-calendar [data-i="${last}"]`);
        await until(async () => (await view(page, "locale-calendar")).open, "the tooltip");
        const scatter = await page.evaluate(() => document.querySelector("#locale-scatter [data-slot=chart-summary]").textContent);
        assert.match(scatter, /12,50,000|7,25,000|21,00,000/, scatter);
        await page.done();
      });
    });

    describe("the matrix", () => {
      test("the keys move by row and column in the staircase and the headers follow", async () => {
        const page = await open("/charts/grids?case=matrix");
        await tabTo(page, body("matrix"));
        let v = await expectAt(page, "matrix", 0, "focus");
        assert.deepEqual([v.cell.r, v.cell.c], [0, 0]);
        assert.deepEqual(v.heads.sort(), ["Sleep", "Stress"], "the row and the column of the cell are marked");
        assert.match(v.head, /^Stress ↔ Sleep$/, "the tooltip names the pair in full");
        await page.keyboard.press("ArrowDown");
        v = await expectAt(page, "matrix", 1, "down");
        assert.deepEqual([v.cell.r, v.cell.c], [1, 0]);
        await page.keyboard.press("ArrowRight");
        v = await expectAt(page, "matrix", 2, "right");
        assert.deepEqual([v.cell.r, v.cell.c], [1, 1]);
        await page.keyboard.press("ArrowRight");
        assert.deepEqual((await view(page, "matrix")).cell, { r: 1, c: 1, level: null }, "right at the end of a row stays");
        await page.keyboard.press("ArrowUp");
        v = await view(page, "matrix");
        assert.deepEqual([v.cell.r, v.cell.c], [0, 0], "up into a shorter row lands on its nearest cell");
        await page.keyboard.press("PageDown");
        v = await view(page, "matrix");
        assert.deepEqual([v.cell.r, v.cell.c], [4, 0], "page down: the bottom of the column");
        await page.keyboard.press("End");
        v = await expectAt(page, "matrix", 14, "end: the last cell of the row");
        assert.deepEqual([v.cell.r, v.cell.c], [4, 4]);
        await page.keyboard.press("Home");
        assert.deepEqual((await view(page, "matrix")).cell.c, 0);
        await page.done();
      });

      test("the tooltip colours its key by the cell, and a pair not significant or not tested is neutral", async () => {
        const page = await open("/charts/grids?case=matrix");
        const looks = await page.evaluate(() => {
          const cells = [...document.querySelectorAll("#matrix [data-slot=heat-cell]")];
          const by = (result) => cells.find((c) => c.dataset.result === result);
          const bg = (c) => getComputedStyle(c).backgroundColor;
          return { pos: bg(by("pos")), neg: bg(by("neg")), ns: bg(by("ns")), none: bg(by("none")), outline: getComputedStyle(by("none")).outlineStyle };
        });
        assert.equal(new Set([looks.pos, looks.neg, looks.ns]).size, 3, "three different fills");
        assert.match(looks.none, /rgba\(0, 0, 0, 0\)|transparent/, "untested is blank");
        assert.equal(looks.outline, "dotted");
        await page.done();
      });

      test("the full matrix writes values in its cells that read on every fill", async () => {
        for (const light of [false, true]) {
          const page = await open("/charts/grids?case=matrix-full", { light });
          const ratios = await page.evaluate(() => {
            const rgb = (color) => {
              const ctx = document.createElement("canvas").getContext("2d", { willReadFrequently: true });
              ctx.fillStyle = "#000";
              ctx.fillStyle = color;
              ctx.fillRect(0, 0, 1, 1);
              return [...ctx.getImageData(0, 0, 1, 1).data.slice(0, 3)];
            };
            const lum = (c) => {
              const v = rgb(c).map((x) => x / 255).map((x) => (x <= 0.04045 ? x / 12.92 : ((x + 0.055) / 1.055) ** 2.4));
              return 0.2126 * v[0] + 0.7152 * v[1] + 0.0722 * v[2];
            };
            return [...document.querySelectorAll("#matrix-full [data-slot=heat-cell][data-m]")].map((c) => {
              const s = getComputedStyle(c);
              const a = lum(s.color) + 0.05, b = lum(s.backgroundColor) + 0.05;
              return Math.max(a, b) / Math.min(a, b);
            });
          });
          assert.ok(ratios.length > 8);
          assert.ok(Math.min(...ratios) >= 4.5, `${light ? "light" : "dark"}: the least contrast is ${Math.min(...ratios).toFixed(2)}`);
          await page.done();
        }
      });

      test("a long name is cut in its header and whole in the tooltip and the title", async () => {
        const page = await open("/charts/grids?case=hostile");
        const info = await page.evaluate(() => {
          const h = [...document.querySelectorAll("#hostile-matrix [data-slot=heat-rowhead]")].find((e) => e.textContent.startsWith("A measure"));
          return { cut: h.scrollWidth > h.clientWidth, title: h.title, scripts: document.querySelectorAll("#hostile-matrix script,#hostile-matrix img").length, pwned: window.__pwned };
        });
        assert.ok(info.cut, "the header is cut by the style, not in the words");
        assert.match(info.title, /very long name/);
        assert.equal(info.scripts, 0);
        assert.equal(info.pwned, undefined);
        await page.done();
      });
    });

    describe("the scatter", () => {
      test("the keys go point to point and the tooltip says the table's row", async () => {
        const page = await open("/charts/grids?case=scatter");
        await tabTo(page, body("scatter"));
        await expectAt(page, "scatter", 0, "focus");
        for (const [key, to] of [["ArrowRight", 1], ["ArrowRight", 2], ["ArrowDown", 3], ["ArrowLeft", 2], ["ArrowUp", 1], ["End", 249], ["Home", 0], ["PageDown", 25]]) {
          await page.keyboard.press(key);
          const v = await expectAt(page, "scatter", to, key);
          assert.match(v.head, /^Beat \d+$/);
          assert.deepEqual(v.rows.map((r) => r.name), ["This beat (ms)", "Next beat (ms)"]);
        }
        await page.done();
      });

      test("the pointer finds a point, and a hidden axis and the equal line are drawn", async () => {
        const page = await open("/charts/grids?case=scatter");
        const geometry = await page.evaluate(() => {
          const f = document.getElementById("scatter");
          const plot = f.querySelector("[data-slot=chart-plot]").getBoundingClientRect();
          const line = f.querySelector("[data-slot=chart-ref] line");
          return {
            axes: f.querySelectorAll("[data-slot=chart-y-axis],[data-slot=chart-x-axis],[data-slot=chart-grid]").length,
            square: Math.abs(plot.width - (plot.height - 18)) < 24,
            x1: line.getAttribute("x1"), x2: line.getAttribute("x2"),
            ref: f.querySelector("[data-slot=chart-ref] text").textContent,
            opacity: getComputedStyle(f.querySelector("circle")).fillOpacity,
            dashed: getComputedStyle(line).strokeDasharray,
          };
        });
        assert.equal(geometry.axes, 0, "no axes and no grid");
        assert.equal(geometry.ref, "Equal");
        assert.notEqual(geometry.dashed, "none");
        assert.ok(Number(geometry.opacity) < 1, "the points are translucent");
        const at = await page.evaluate(() => {
          const all = [...document.querySelectorAll("#scatter circle")].map((c) => {
            const r = c.getBoundingClientRect();
            return { i: +c.dataset.i, x: r.left + r.width / 2, y: r.top + r.height / 2 };
          });
          const mx = all.reduce((a, p) => a + p.x, 0) / all.length;
          const my = all.reduce((a, p) => a + p.y, 0) / all.length;
          const far = all.reduce((a, p) => (Math.hypot(p.x - mx, p.y - my) > Math.hypot(a.x - mx, a.y - my) ? p : a));
          document.querySelector(`#scatter circle[data-i="${far.i}"]`).scrollIntoView({ block: "center" });
          const r = document.querySelector(`#scatter circle[data-i="${far.i}"]`).getBoundingClientRect();
          return { i: far.i, x: r.left + r.width / 2, y: r.top + r.height / 2 };
        });
        await page.mouse.move(at.x, at.y);
        await until(async () => (await view(page, "scatter")).open, "the tooltip");
        const v = await view(page, "scatter");
        assert.deepEqual(v.active, [at.i]);
        assert.equal(v.head, `Beat ${at.i + 1}`);
        assert.equal(v.inside, true);
        await page.done();
      });

      test("two series: the legend, a series line in the tooltip, and the average", async () => {
        const page = await open("/charts/grids?case=scatter-axes");
        const info = await page.evaluate(() => {
          const f = document.getElementById("scatter-axes");
          return {
            legend: [...f.querySelectorAll("[data-slot=chart-legend-name]")].map((e) => e.textContent),
            groups: f.querySelectorAll("[data-slot=chart-points]").length,
            circles: f.querySelectorAll("circle").length,
            average: f.querySelector("[data-slot=chart-ref] text")?.textContent,
            heads: [...f.querySelectorAll("thead th")].map((t) => t.textContent),
          };
        });
        assert.deepEqual(info.legend, ["Weekdays", "Weekends"]);
        assert.equal(info.groups, 2);
        assert.equal(info.circles, 84);
        assert.match(info.average, /^Average \d/);
        assert.deepEqual(info.heads, ["Point", "Steps", "Resting heart rate (BPM)", "Series"]);
        await tabTo(page, body("scatter-axes"));
        await page.keyboard.press("End");
        const v = await expectAt(page, "scatter-axes", 83, "the last point");
        assert.equal(v.rows.at(-1).value, "Weekends");
        await page.done();
      });

      test("JavaScript off: the points, the table behind its switch and the summary are all in the page", async () => {
        const page = await open("/charts/grids?case=scatter", { js: false });
        const info = await page.evaluate(() => {
          const f = document.getElementById("scatter");
          const table = f.querySelector("[data-slot=chart-table]");
          const hidden = getComputedStyle(table).clipPath;
          f.querySelector("[data-slot=chart-table-switch]").click();
          return { circles: f.querySelectorAll("circle").length, hidden, shown: getComputedStyle(table).clipPath, rows: table.querySelectorAll("tbody tr").length, summary: f.querySelector("[data-slot=chart-summary]").textContent };
        });
        assert.equal(info.circles, 250);
        assert.notEqual(info.hidden, "none");
        assert.equal(info.shown, "none", "the switch shows the table with no script");
        assert.equal(info.rows, 250);
        assert.match(info.summary, /Resting: 250 points\./);
        assert.match(info.summary, /Correlation 0\.\d\d\./);
        await page.close();
        const cal = await open("/charts/grids?case=calendar", { js: false });
        const cells = await cal.evaluate(() => ({ cells: document.querySelectorAll("#calendar [data-slot=heat-cell]").length, months: [...document.querySelectorAll("#calendar [data-slot=heat-month]")].map((m) => m.textContent), legend: document.querySelector("#calendar [data-slot=heat-legend]").textContent }));
        assert.equal(cells.cells, 181);
        assert.deepEqual(cells.months, ["May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov"]);
        assert.match(cells.legend, /Less.*More/);
      });
    });
  });
}
