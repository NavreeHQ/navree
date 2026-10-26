#!/usr/bin/env node
// Builds bench/results/report.html from bench/results/*.json: one ranked bar
// chart per test (Keel highlighted, the rest grey), latency, memory and a
// table with every number.
//   node bench/report.mjs [resultsDir]
import { readdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const BENCH = dirname(fileURLToPath(import.meta.url));
const DIR = process.argv[2] ?? join(BENCH, "results");
const HIGHLIGHT = "keel";

const TESTS = [
  { key: "deals_list", label: "Auth + paginated list", what: "JWT check, user lookup, count + one page of 20 deals with owners" },
  { key: "deals_create", label: "Auth + validated insert", what: "JWT check, user lookup, body validation, INSERT … RETURNING" },
  { key: "db", label: "Single query", what: "one random row by primary key" },
  { key: "queries", label: "20 queries", what: "20 separate random-row queries per request" },
  { key: "json", label: "JSON", what: "serialize a small object: routing and HTTP overhead only" },
];

const results = readdirSync(DIR)
  .filter((f) => f.endsWith(".json") && f !== "env.json")
  .map((f) => JSON.parse(readFileSync(join(DIR, f), "utf8")));
const ok = results.filter((r) => r.ok);
const failed = results.filter((r) => !r.ok);
const fragment = (file) => {
  try {
    return readFileSync(join(DIR, file), "utf8");
  } catch {
    return "";
  }
};
let env = {};
try {
  env = JSON.parse(readFileSync(join(DIR, "env.json"), "utf8"));
} catch {
  // optional
}

const esc = (s) => String(s).replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);
const int = (n) => (n == null ? "–" : Math.round(n).toLocaleString("en-US"));
const ms = (n) => (n == null ? "–" : n < 10 ? n.toFixed(2) : n.toFixed(1));

/** Round axis steps (1, 2, 2.5 or 5 × 10^n), about four of them. */
function niceTicks(v) {
  const rough = (v || 1) / 4;
  const p = 10 ** Math.floor(Math.log10(rough));
  const step = [1, 2, 2.5, 5, 10].map((m) => m * p).find((x) => x >= rough);
  const n = Math.ceil(v / step);
  return Array.from({ length: n + 1 }, (_, i) => i * step);
}

const tickLabel = (n) => (n >= 1e6 ? `${+(n / 1e6).toFixed(2)}M` : n >= 1e3 ? `${+(n / 1e3).toFixed(1)}k` : `${n}`);

/** A ranked horizontal bar chart: rows [{name,label,value,tip}] */
function barChart({ id, rows, unit, lowerIsBetter = false, format = int, size = {} }) {
  rows = [...rows].sort((a, b) => (lowerIsBetter ? a.value - b.value : b.value - a.value));
  const { labelW = 230, valueW = 64, rowH = 22, gap = 6, width = 640 } = size;
  const top = 6;
  const plotW = width - labelW - valueW;
  const ticks = niceTicks(Math.max(...rows.map((r) => r.value)));
  const max = ticks[ticks.length - 1];
  const height = top + rows.length * (rowH + gap) + 26;
  const x = (v) => labelW + (v / max) * plotW;

  let svg = `<svg class="chart" viewBox="0 0 ${width} ${height}" role="img" aria-labelledby="${id}-t">`;
  for (const t of ticks) {
    svg += `<line class="grid" x1="${x(t)}" x2="${x(t)}" y1="${top - 4}" y2="${height - 22}"/>`;
    svg += `<text class="tick" x="${x(t)}" y="${height - 6}" text-anchor="middle">${tickLabel(t)}</text>`;
  }
  rows.forEach((r, i) => {
    const y = top + i * (rowH + gap);
    const hl = r.name === HIGHLIGHT;
    const w = Math.max(1.5, x(r.value) - labelW);
    const bh = rowH - 6;
    const rad = Math.min(4, w / 2, bh / 2);
    const best = i === 0;
    svg += `<g class="row${hl ? " hl" : ""}" data-tip="${esc(r.tip)}">`;
    svg += `<rect class="hit" x="0" y="${y - gap / 2}" width="${width}" height="${rowH + gap}"/>`;
    svg += `<text class="label" x="${labelW - 10}" y="${y + rowH / 2 + 4}" text-anchor="end">${esc(r.label)}</text>`;
    svg += `<path class="bar" d="M${labelW},${y + 3} h${(w - rad).toFixed(1)} a${rad},${rad} 0 0 1 ${rad},${rad} v${(bh - 2 * rad).toFixed(1)} a${rad},${rad} 0 0 1 -${rad},${rad} h${(-(w - rad)).toFixed(1)} z"/>`;
    if (hl || best) svg += `<text class="value" x="${labelW + w + 6}" y="${y + rowH / 2 + 4}">${format(r.value)}</text>`;
    svg += "</g>";
  });
  svg += `<line class="axis" x1="${labelW}" x2="${labelW}" y1="${top - 4}" y2="${height - 22}"/>`;
  svg += "</svg>";
  return svg;
}

const name = (r) => `${r.label}${r.category === "micro" ? " ·" : ""}`;

function testSection(test) {
  const rows = ok
    .filter((r) => r.tests?.[test.key])
    .map((r) => {
      const t = r.tests[test.key];
      const errors = t.errors ? `, ${int(t.errors)} errors` : "";
      return {
        name: r.name,
        label: name(r),
        value: t.rps,
        tip: `${r.label}: ${int(t.rps)} req/s · p50 ${ms(t.latency_ms.p50)} ms · p99 ${ms(t.latency_ms.p99)} ms · CPU app ${Math.round(t.avg_cpu_pct ?? 0)}% / db ${Math.round(t.pg_avg_cpu_pct ?? 0)}%${errors}`,
      };
    });
  const keel = rows.find((r) => r.name === HIGHLIGHT);
  const rank = keel ? [...rows].sort((a, b) => b.value - a.value).findIndex((r) => r.name === HIGHLIGHT) + 1 : null;
  return `<section class="card">
  <h3 id="${test.key}-t">${esc(test.label)} <span class="unit">requests / s</span></h3>
  <p class="sub">${esc(test.what)}${keel ? ` · Keel ${int(keel.value)} req/s, #${rank} of ${rows.length}` : ""}</p>
  ${barChart({ id: test.key, rows, unit: "req/s" })}
</section>`;
}

function latencySection() {
  const rows = ok
    .filter((r) => r.tests?.deals_list)
    .map((r) => {
      const t = r.tests.deals_list;
      return { name: r.name, label: name(r), value: t.latency_ms.p99, tip: `${r.label}: p99 ${ms(t.latency_ms.p99)} ms · p50 ${ms(t.latency_ms.p50)} ms · p90 ${ms(t.latency_ms.p90)} ms` };
    });
  return `<section class="card">
  <h3 id="lat-t">p99 latency, auth + paginated list <span class="unit">ms, lower is better</span></h3>
  <p class="sub">The slowest 1% of requests at 256 concurrent connections.</p>
  ${barChart({ id: "lat", rows, lowerIsBetter: true, format: (v) => ms(v) })}
</section>`;
}

function memorySection() {
  const rows = ok.map((r) => {
    const peak = Math.max(...Object.values(r.tests ?? {}).map((t) => t.peak_mem_mb ?? 0));
    return { name: r.name, label: name(r), value: peak, tip: `${r.label}: peak ${int(peak)} MB under load · idle ${int(r.idle_mem_mb)} MB · image ${int(r.image_mb)} MB · ready in ${int(r.startup_ms)} ms` };
  });
  return `<section class="card">
  <h3 id="mem-t">Peak memory under load <span class="unit">MB, lower is better</span></h3>
  <p class="sub">Highest container memory seen during any test (all worker processes together).</p>
  ${barChart({ id: "mem", rows, lowerIsBetter: true, format: (v) => int(v) })}
</section>`;
}

function table() {
  const sorted = [...ok].sort((a, b) => (b.tests?.deals_list?.rps ?? 0) - (a.tests?.deals_list?.rps ?? 0));
  let h = `<table><thead><tr><th>Framework</th><th>Lang</th><th>Kind</th>`;
  for (const t of TESTS) h += `<th class="n">${esc(t.label)}<br><span class="unit">req/s · p99 ms</span></th>`;
  h += `<th class="n">Idle MB</th><th class="n">Peak MB</th><th class="n">Ready ms</th><th class="n">Image MB</th></tr></thead><tbody>`;
  for (const r of sorted) {
    const peak = Math.max(...Object.values(r.tests ?? {}).map((t) => t.peak_mem_mb ?? 0));
    h += `<tr${r.name === HIGHLIGHT ? ' class="hl"' : ""}><td>${esc(r.label)}</td><td>${esc(r.language)}</td><td>${esc(r.category)}</td>`;
    for (const t of TESTS) {
      const v = r.tests?.[t.key];
      const err = v?.errors ? ` <span class="err" title="${esc(JSON.stringify(v.error_detail))}">${int(v.errors)} err</span>` : "";
      h += `<td class="n">${v ? `${int(v.rps)}<br><span class="unit">${ms(v.latency_ms.p99)}</span>${err}` : "–"}</td>`;
    }
    h += `<td class="n">${int(r.idle_mem_mb)}</td><td class="n">${int(peak)}</td><td class="n">${int(r.startup_ms)}</td><td class="n">${int(r.image_mb)}</td></tr>`;
  }
  h += "</tbody></table>";
  if (failed.length) h += `<p class="sub">Not benchmarked: ${failed.map((r) => `${esc(r.label)} (${esc(r.error?.split("\n")[0] ?? "failed")})`).join("; ")}.</p>`;
  return h;
}

const s = ok[0]?.settings ?? {};
const html = `<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Keel benchmark</title>
<style>
.viz-root{color-scheme:light;--page:#f9f9f7;--surface-1:#fcfcfb;--text-primary:#0b0b0b;--text-secondary:#52514e;--muted:#898781;--grid:#e1e0d9;--axis:#c3c2b7;--accent:#2a78d6;--rest:#aeaca4;--border:rgba(11,11,11,.10);--hl-row:#eef4fc}
@media (prefers-color-scheme:dark){:root:where(:not([data-theme="light"])) .viz-root{color-scheme:dark;--page:#0d0d0d;--surface-1:#1a1a19;--text-primary:#f0efec;--text-secondary:#c3c2b7;--muted:#898781;--grid:#2c2c2a;--axis:#383835;--accent:#3987e5;--rest:#5a5a55;--border:rgba(255,255,255,.10);--hl-row:#1c2a3b}}
:root[data-theme="dark"] .viz-root{color-scheme:dark;--page:#0d0d0d;--surface-1:#1a1a19;--text-primary:#f0efec;--text-secondary:#c3c2b7;--muted:#898781;--grid:#2c2c2a;--axis:#383835;--accent:#3987e5;--rest:#5a5a55;--border:rgba(255,255,255,.10);--hl-row:#1c2a3b}
*{box-sizing:border-box}
body{margin:0}
.viz-root{background:var(--page);color:var(--text-primary);font:14px/1.45 system-ui,-apple-system,"Segoe UI",sans-serif;padding:28px 24px 40px;min-height:100vh}
.wrap{max-width:1360px;margin:0 auto}
h1{font-size:22px;margin:0 0 4px}
h2{font-size:16px;margin:28px 0 10px}
h3{font-size:14px;margin:0 0 2px}
.lede{color:var(--text-secondary);margin:0 0 4px;max-width:900px}
.unit{color:var(--muted);font-weight:400;font-size:12px}
.sub{color:var(--text-secondary);font-size:12.5px;margin:0 0 8px}
.grid2{display:grid;grid-template-columns:repeat(auto-fit,minmax(560px,1fr));gap:16px}
.card{background:var(--surface-1);border:1px solid var(--border);border-radius:10px;padding:14px 16px 8px}
.legend{display:flex;gap:18px;align-items:center;color:var(--text-secondary);font-size:12.5px;margin:10px 0 0}
.sw{display:inline-block;width:12px;height:12px;border-radius:3px;vertical-align:-2px;margin-right:6px}
.chart{width:100%;height:auto;display:block}
.chart .grid{stroke:var(--grid);stroke-width:1}
.chart .axis{stroke:var(--axis);stroke-width:1}
.chart .tick{fill:var(--muted);font-size:11px;font-variant-numeric:tabular-nums}
.chart .label{fill:var(--text-secondary);font-size:12px}
.chart .value{fill:var(--text-primary);font-size:11.5px;font-variant-numeric:tabular-nums}
.chart .bar{fill:var(--rest)}
.chart .hl .bar{fill:var(--accent)}
.chart .hl .label{fill:var(--text-primary);font-weight:600}
.chart .hit{fill:transparent}
.chart .row:hover .bar{opacity:.85}
.chart .row:hover .hit{fill:var(--grid);opacity:.45}
#tip{position:fixed;pointer-events:none;background:var(--surface-1);color:var(--text-primary);border:1px solid var(--border);box-shadow:0 4px 16px rgba(0,0,0,.12);border-radius:8px;padding:6px 9px;font-size:12px;max-width:420px;display:none;z-index:10}
table{width:100%;border-collapse:collapse;background:var(--surface-1);border:1px solid var(--border);border-radius:10px;overflow:hidden;font-size:12.5px}
th,td{padding:7px 9px;border-bottom:1px solid var(--grid);text-align:left;vertical-align:top}
th{color:var(--text-secondary);font-weight:600}
td.n,th.n{text-align:right;font-variant-numeric:tabular-nums}
tr.hl td{background:var(--hl-row);font-weight:600}
.err{color:#d03b3b;font-weight:600}
.findings{margin-top:18px}.findings ul,.findings ol{padding-left:20px;margin:6px 0 10px;max-width:1100px}.findings li{margin:5px 0}.findings h3{margin-top:12px}code{font-size:12px;background:var(--grid);padding:0 4px;border-radius:4px}
ul.notes{color:var(--text-secondary);font-size:13px;max-width:1000px;padding-left:18px}
</style></head>
<body><div class="viz-root"><div class="wrap">
<h1>Keel vs. other web frameworks</h1>
<p class="lede">Every framework implements the same API (bench/SPEC.md) and is measured alone: the app gets ${esc(s.cpus?.app ? `CPUs ${s.cpus.app}` : "8 CPUs")} and 4 GB, PostgreSQL 17 its own CPUs, wrk its own. ${s.connections ?? 256} connections, ${s.warmup_s ?? 10} s warmup, then the median of ${s.runs ?? 3} × ${s.duration_s ?? 15} s runs. Docker ${esc(env.docker ?? "")} on a 24-core Intel Core Ultra 9 275HX laptop (Windows 11, WSL2).</p>
<div class="legend"><span><span class="sw" style="background:var(--accent)"></span>Keel</span><span><span class="sw" style="background:var(--rest)"></span>other frameworks</span><span>· marks a micro-framework (no ORM/auth layer of its own)</span></div>
${fragment("findings.html")}
<h2>Throughput</h2>
<div class="grid2">
${TESTS.map(testSection).join("\n")}
${latencySection()}
</div>
<h2>Resources</h2>
<div class="grid2">${memorySection()}</div>
<h2>All numbers</h2>
${table()}
${fragment("method-notes.html")}
</div><div id="tip"></div></div>
<script>
const tip = document.getElementById("tip");
document.querySelectorAll(".chart .row").forEach((row) => {
  row.addEventListener("mousemove", (e) => {
    tip.textContent = row.dataset.tip;
    tip.style.display = "block";
    tip.style.left = Math.min(e.clientX + 14, innerWidth - tip.offsetWidth - 8) + "px";
    tip.style.top = (e.clientY + 14) + "px";
  });
  row.addEventListener("mouseleave", () => (tip.style.display = "none"));
});
</script>
</body></html>`;

writeFileSync(join(DIR, "report.html"), html);
console.log(`wrote ${join(DIR, "report.html")} (${ok.length} frameworks, ${failed.length} failed)`);

// ---------------------------------------------------------------- inline
// A compact page for a narrow column on the host's own background: the
// numbers as JSON plus a small script that draws the same ranked bars, theme
// colours from the host's CSS variables, no page title.

const inlineData = ok.map((r) => ({
  n: r.label,
  k: r.name,
  m: r.category === "micro" ? 1 : 0,
  mem: Math.round(Math.max(...Object.values(r.tests ?? {}).map((t) => t.peak_mem_mb ?? 0))),
  t: Object.fromEntries(
    TESTS.map((t) => {
      const v = r.tests[t.key];
      return [t.key, [Math.round(v.rps), +v.latency_ms.p99.toFixed(1), Math.round(v.avg_cpu_pct ?? 0), Math.round(v.pg_avg_cpu_pct ?? 0)]];
    }),
  ),
}));
const inlineTests = TESTS.map((t) => ({ key: t.key, label: t.label, what: t.what }));

const inlineScript = String.raw`
const fmt = (n) => Math.round(n).toLocaleString("en-US");
const tickLabel = (n) => (n >= 1e6 ? +(n / 1e6).toFixed(2) + "M" : n >= 1e3 ? +(n / 1e3).toFixed(1) + "k" : String(n));
const esc = (s) => String(s).replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);
function ticksFor(v) {
  const rough = (v || 1) / 4, p = 10 ** Math.floor(Math.log10(rough));
  const step = [1, 2, 2.5, 5, 10].map((m) => m * p).find((x) => x >= rough);
  return Array.from({ length: Math.ceil(v / step) + 1 }, (_, i) => i * step);
}
function chart(rows, lowerIsBetter) {
  rows = [...rows].sort((a, b) => (lowerIsBetter ? a.value - b.value : b.value - a.value));
  const L = 196, W = 700, RH = 17, G = 5, TOP = 6, PW = W - L - 58;
  const ticks = ticksFor(Math.max(...rows.map((r) => r.value))), max = ticks[ticks.length - 1];
  const H = TOP + rows.length * (RH + G) + 26, x = (v) => L + (v / max) * PW;
  let s = '<svg class="chart" viewBox="0 0 ' + W + " " + H + '" role="img">';
  for (const t of ticks) s += '<line class="grid" x1="' + x(t) + '" x2="' + x(t) + '" y1="2" y2="' + (H - 22) + '"/><text class="tick" x="' + x(t) + '" y="' + (H - 6) + '" text-anchor="middle">' + tickLabel(t) + "</text>";
  rows.forEach((r, i) => {
    const y = TOP + i * (RH + G), hl = r.k === "keel", w = Math.max(1.5, x(r.value) - L), bh = RH - 6, rad = Math.min(4, w / 2, bh / 2);
    s += '<g class="row' + (hl ? " hl" : "") + '" data-tip="' + esc(r.tip) + '"><rect class="hit" x="0" y="' + (y - G / 2) + '" width="' + W + '" height="' + (RH + G) + '"/>';
    s += '<text class="label" x="' + (L - 10) + '" y="' + (y + RH / 2 + 4) + '" text-anchor="end">' + esc(r.label) + "</text>";
    s += '<path class="bar" d="M' + L + "," + (y + 3) + " h" + (w - rad) + " a" + rad + "," + rad + " 0 0 1 " + rad + "," + rad + " v" + (bh - 2 * rad) + " a" + rad + "," + rad + " 0 0 1 -" + rad + "," + rad + " h" + -(w - rad) + ' z"/>';
    if (hl || i === 0) s += '<text class="value" x="' + (L + w + 6) + '" y="' + (y + RH / 2 + 4) + '">' + r.text + "</text>";
    s += "</g>";
  });
  return s + '<line class="axis" x1="' + L + '" x2="' + L + '" y1="2" y2="' + (H - 22) + '"/></svg>';
}
const label = (d) => d.n + (d.m ? " ·" : "");
let html = "";
for (const t of TESTS) {
  const rows = DATA.map((d) => {
    const [rps, p99, cpu, db] = d.t[t.key];
    return { k: d.k, label: label(d), value: rps, text: fmt(rps), tip: d.n + ": " + fmt(rps) + " req/s · p99 " + p99 + " ms · CPU app " + cpu + "% / db " + db + "%" };
  });
  html += "<section><h3>" + esc(t.label) + ' <span class="unit">req/s · ' + esc(t.what) + "</span></h3>" + chart(rows, false) + "</section>";
}
html += '<section><h3>Peak memory under load <span class="unit">MB, lower is better</span></h3>' + chart(DATA.map((d) => ({ k: d.k, label: label(d), value: d.mem, text: fmt(d.mem) + " MB", tip: d.n + ": peak " + fmt(d.mem) + " MB" })), true) + "</section>";
const sorted = [...DATA].sort((a, b) => b.t.deals_list[0] - a.t.deals_list[0]);
html += '<h3>All results <span class="unit">req/s, sorted by the paginated-list test</span></h3><div class="tw"><table><thead><tr><th>Framework</th>' + TESTS.map((t) => '<th class="n">' + esc(t.label) + "</th>").join("") + '<th class="n">Peak MB</th></tr></thead><tbody>';
for (const d of sorted) html += "<tr" + (d.k === "keel" ? ' class="hl"' : "") + "><td>" + esc(label(d)) + "</td>" + TESTS.map((t) => '<td class="n">' + fmt(d.t[t.key][0]) + "</td>").join("") + '<td class="n">' + fmt(d.mem) + "</td></tr>";
document.getElementById("r").insertAdjacentHTML("beforeend", html + "</tbody></table></div>");
const tip = document.getElementById("tip");
document.querySelectorAll(".chart .row").forEach((row) => {
  row.addEventListener("mousemove", (e) => {
    tip.textContent = row.dataset.tip;
    tip.style.display = "block";
    tip.style.left = Math.max(4, Math.min(e.clientX + 12, innerWidth - tip.offsetWidth - 6)) + "px";
    tip.style.top = e.clientY + 14 + "px";
  });
  row.addEventListener("mouseleave", () => (tip.style.display = "none"));
});
`;

const inlineStyle = `
.r{--accent:#3987e5;--rest:color-mix(in oklab, var(--muted-foreground, #898781) 55%, transparent);--grid:color-mix(in oklab, var(--border, #2c2c2a) 80%, transparent);color:var(--foreground);font:13px/1.45 var(--font-sans, system-ui, sans-serif)}
.r h3{font-size:13px;margin:18px 0 2px;font-weight:600}
.r .unit{color:var(--muted-foreground);font-weight:400;font-size:11.5px}
.r .cap{color:var(--muted-foreground);font-size:12px;margin:0 0 4px}
.r .legend{display:flex;flex-wrap:wrap;gap:14px;color:var(--muted-foreground);font-size:12px;margin:2px 0 4px}
.r .sw{display:inline-block;width:11px;height:11px;border-radius:3px;vertical-align:-1px;margin-right:5px}
.chart{width:100%;height:auto;display:block;margin-top:4px}
.chart .grid{stroke:var(--grid);stroke-width:1}
.chart .axis{stroke:var(--border);stroke-width:1}
.chart .tick{fill:var(--muted-foreground);font-size:10.5px;font-variant-numeric:tabular-nums}
.chart .label{fill:var(--muted-foreground);font-size:11.5px}
.chart .value{fill:var(--foreground);font-size:11px;font-variant-numeric:tabular-nums}
.chart .bar{fill:var(--rest)}
.chart .hl .bar{fill:var(--accent)}
.chart .hl .label{fill:var(--foreground);font-weight:600}
.chart .hit{fill:transparent}
.chart .row:hover .hit{fill:var(--grid);opacity:.5}
#tip{position:fixed;pointer-events:none;background:var(--popover,#222);color:var(--popover-foreground,#eee);border:1px solid var(--border);border-radius:8px;padding:5px 8px;font-size:11.5px;max-width:340px;display:none;z-index:10;box-shadow:0 4px 14px rgba(0,0,0,.18)}
.r table{width:100%;border-collapse:collapse;font-size:11.5px;margin-top:6px}
.r th,.r td{padding:4px 6px;border-bottom:1px solid var(--grid);text-align:left;white-space:nowrap}
.r th{color:var(--muted-foreground);font-weight:600;white-space:normal;vertical-align:bottom}
.r td.n,.r th.n{text-align:right;font-variant-numeric:tabular-nums}
.r tr.hl td{color:var(--foreground);font-weight:700;background:color-mix(in oklab, var(--accent) 14%, transparent)}
.tw{overflow-x:auto}
`;

const inline = `<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<style>${inlineStyle}</style></head>
<body><div class="r" id="r">
<p class="cap">Each app limited to 8 CPUs and 4 GB and run alone; 256 connections; median of 3 × 15 s runs after a 10 s warmup; PostgreSQL 17 and wrk on their own CPUs. Hover a bar for p99 latency and CPU.</p>
<div class="legend"><span><span class="sw" style="background:var(--accent)"></span>Keel</span><span><span class="sw" style="background:var(--rest)"></span>other frameworks</span><span>· micro-framework (no ORM/auth layer of its own)</span></div>
</div><div id="tip"></div>
<script>
const DATA = ${JSON.stringify(inlineData)};
const TESTS = ${JSON.stringify(inlineTests)};
${inlineScript}</script>
</body></html>`;
writeFileSync(join(DIR, "report-inline.html"), inline);
console.log(`wrote ${join(DIR, "report-inline.html")}`);
