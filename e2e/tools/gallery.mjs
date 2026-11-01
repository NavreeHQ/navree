// Builds the evidence gallery (index.html) from a UI evidence run's files:
//
//   <dir>/shots/<phase>/results*.json  the crawler's records, per phase and service
//   <dir>/shots/website.json           what the website says (website.mjs)
//   <dir>/meta.json                    the run: commit, images, load figures
//   <dir>/notes.json                   (optional) the reviewer's notes
//
// The page is one self-contained file: inline style, no scripts but a
// filter, relative links to the screenshots.
//
//   node gallery.mjs <dir>

import { readFile, readdir, writeFile } from 'node:fs/promises';
import { existsSync } from 'node:fs';
import path from 'node:path';

const dir = path.resolve(process.argv[2] || '.');
const read = async (name, fallback) => {
  const file = path.join(dir, name);
  return existsSync(file) ? JSON.parse(await readFile(file, 'utf8')) : fallback;
};

const meta = await read('meta.json', {});
const notes = await read('notes.json', { tools: {}, pages: {} });
const website = await read('shots/website.json', { tools: {} });
const toolsConfig = await read('tools.json', { tools: [] });
const phases = ['empty', 'load', 'after'];
const phaseLabel = {
  empty: 'Before any traffic (a fresh tool)',
  load: 'Under load (k6 running)',
  after: 'After the load',
};
const results = {};
for (const phase of phases) {
  results[phase] = [];
  const phaseDir = path.join(dir, 'shots', phase);
  if (!existsSync(phaseDir)) continue;
  for (const name of (await readdir(phaseDir)).filter((f) => /^results.*\.json$/.test(f)).sort()) {
    results[phase].push(...JSON.parse(await readFile(path.join(phaseDir, name), 'utf8')));
  }
}

const esc = (s) => String(s ?? '').replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]);
const img = (file, cls = '') => (file ? `<a href="shots/${esc(file)}" target="_blank"><img class="${cls}" loading="lazy" src="shots/${esc(file)}" alt=""></a>` : '<div class="none">no image</div>');

/** The problems of one record, as short labels. */
function flags(rec, width) {
  const out = [];
  if (rec.error) out.push(['bad', `error: ${rec.error.slice(0, 80)}`]);
  if (rec.status >= 400 && !rec.expected_status && rec.kind !== 'state') out.push(['bad', `HTTP ${rec.status}`]);
  if (rec.status >= 400 && rec.kind === 'state' && !rec.expected_status) out.push(['warn', `HTTP ${rec.status}`]);
  if (rec.console_errors?.length) out.push(['bad', `${rec.console_errors.length} console error(s)`]);
  if (rec.failed_requests?.length) out.push(['bad', `${rec.failed_requests.length} failed request(s)`]);
  if (rec.horizontalOverflow) out.push(['bad', `horizontal overflow at ${width}px`]);
  if (rec.still_signing_in) out.push(['bad', 'still asks to sign in']);
  // A table that scrolls inside its card on a phone is fine; on a desktop, or cut off with no scroll, it is a finding.
  const wide = (rec.wideTables || []).filter((t) => width >= 1000 || t.overflowX !== 'auto');
  if (wide.length) out.push(['warn', `table wider than its box at ${width}px (${wide.map((t) => `${t.table}>${t.box}, overflow-x ${t.overflowX}`).join('; ')})`]);
  if (rec.brokenImages?.length) out.push(['bad', 'broken image']);
  if (rec.truncated) out.push(['warn', 'screenshot cut at 14000px']);
  const flat = (rec.charts || []).filter((c) => c.shapes !== null && (c.shapes === 0 || (c.w >= 400 && c.shapes <= 2)));
  if (flat.length) out.push(['warn', `${flat.length} chart(s) with no data drawn`]);
  if (rec.radiiOver4?.length) out.push(['rule', `corners over 4px: ${rec.radiiOver4.map((r) => `${r.radius}x${r.count}`).join(', ')}`]);
  if (rec.pills?.length) out.push(['rule', `${rec.pills.length} pill(s)`]);
  if (rec.glows?.length) out.push(['rule', `${rec.glows.length} glow/shadow(s)`]);
  if (rec.letterTiles?.length) out.push(['rule', `${rec.letterTiles.length} letter tile(s)`]);
  if (rec.wall_ms > 1500) out.push(['warn', `slow: ${rec.wall_ms} ms`]);
  return out;
}

/** Joins desktop and mobile records of a phase by tool, kind and URL. */
function pairs(records, tool) {
  const map = new Map();
  for (const r of records.filter((x) => x.tool === tool)) {
    const key = `${r.kind || 'page'}|${r.state || ''}|${r.url}|${r.label || ''}`;
    const entry = map.get(key) || { key, url: r.url, kind: r.kind || 'page', state: r.state, label: r.label };
    entry[r.width >= 1000 ? 'desktop' : 'mobile'] = r;
    map.set(key, entry);
  }
  return [...map.values()];
}

const toolNames = [...new Set(['lighthouse', 'fleet', 'helm', 'logbook', 'api-docs', 'graphiql', 'ui-gallery', 'oauth', 'mailpit',
  ...toolsConfig.tools.map((t) => t.name), ...phases.flatMap((p) => results[p].map((r) => r.tool))])].filter((t) => phases.some((p) => results[p].some((r) => r.tool === t)));
const titleOf = (name) => toolsConfig.tools.find((t) => t.name === name)?.title || name;

function stats(tool) {
  const s = { pages: 0, states: 0, mobile: 0, problems: 0, consoleErrors: 0, failed: 0, overflow: 0, rule: 0, slowest: 0 };
  for (const phase of phases) {
    for (const p of pairs(results[phase], tool)) {
      const d = p.desktop;
      const m = p.mobile;
      if (p.kind === 'state') s.states += 1;
      else s.pages += 1;
      if (m) s.mobile += 1;
      for (const [rec, width] of [[d, 1440], [m, 390]]) {
        if (!rec) continue;
        const f = flags(rec, width);
        if (f.some(([k]) => k === 'bad')) s.problems += 1;
        s.consoleErrors += rec.console_errors?.length || 0;
        s.failed += rec.failed_requests?.length || 0;
        if (rec.horizontalOverflow) s.overflow += 1;
        if (f.some(([k]) => k === 'rule')) s.rule += 1;
        s.slowest = Math.max(s.slowest, rec.wall_ms || 0);
      }
    }
  }
  return s;
}

function card(p, tool, phase) {
  const d = p.desktop;
  const m = p.mobile;
  const all = [...(d ? flags(d, 1440) : []), ...(m ? flags(m, 390).filter(([, t]) => !(d && flags(d, 1440).some(([, x]) => x === t))) : [])];
  const note = notes.pages?.[`${phase}|${tool}|${p.url}`] || notes.pages?.[`${tool}|${p.url}`];
  const status = d?.status ?? m?.status ?? '';
  const timing = (r) => (r?.wall_ms != null ? `${r.wall_ms} ms` : '');
  const stateTag = p.kind === 'state' ? `<span class="tag">state: ${esc(p.state)}</span>` : '';
  const hasBad = all.some(([k]) => k === 'bad');
  return `<article class="card ${hasBad ? 'has-bad' : ''}" data-bad="${hasBad ? 1 : 0}">
  <header>
    <h4>${stateTag}<code>${esc(p.url)}</code></h4>
    <div class="meta">${p.label ? `<span>${esc(p.label)}</span>` : ''}<span>${d?.title ? esc(d.title) : ''}</span>
      <span>HTTP ${esc(status)}</span><span>desktop ${esc(timing(d))}</span><span>phone ${esc(timing(m))}</span></div>
    <ul class="flags">${all.length ? all.map(([k, t]) => `<li class="${k}">${esc(t)}</li>`).join('') : '<li class="good">no automatic findings</li>'}</ul>
    ${d?.console_errors?.length ? `<details><summary>console errors</summary><pre>${esc(d.console_errors.join('\n'))}</pre></details>` : ''}
    ${d?.failed_requests?.length ? `<details><summary>failed requests</summary><pre>${esc(d.failed_requests.map((f) => `${f.status || f.failure} ${f.url}`).join('\n'))}</pre></details>` : ''}
    ${m?.horizontalOverflow ? `<details><summary>overflow at 390 px</summary><pre>${esc((m.offenders || []).map((o) => `${o.el} right=${o.right} ${o.text}`).join('\n'))}</pre></details>` : ''}
    ${note ? `<p class="note">${esc(note)}</p>` : ''}
  </header>
  <div class="shots"><div class="desk">${img(d?.file, 'desk')}</div><div class="phone">${img(m?.file, 'phone')}</div></div>
</article>`;
}

function websiteBlock(tool) {
  const site = website.tools?.[tool];
  if (!site) return '';
  const crm = (site.shots || []).filter((x) => x.kind === 'crm-image');
  if (crm.length) {
    return `<div class="compare"><h3>The website's screens of the CRM (static images on /showcase/crm)</h3><div class="crmgrid">${crm.map((x) => `<figure><figcaption>${esc(x.name)}</figcaption>${img(x.file)}</figure>`).join('')}</div></div>`;
  }
  const shot = (kind, n, width) => site.shots?.find((s) => s.kind === kind && s.width === width && (n ? s.n === n : true));
  const real = pairs(results.after.length ? results.after : results.load, tool).find((p) => p.kind === 'page');
  return `<div class="compare">
  <h3>The website beside the real tool</h3>
  <div class="two">
    <figure><figcaption>On navree.com/ecosystem (${esc(website.site || '')}), first frame</figcaption>${shot('mock', 1, 1440) ? img(shot('mock', 1, 1440).file) : '<div class="none">not captured</div>'}</figure>
    <figure><figcaption>The real tool, first page, 1440 px</figcaption>${img(real?.desktop?.file)}</figure>
  </div>
  <div class="two">
    <figure><figcaption>The website, second frame (the mock moves)</figcaption>${shot('mock', 2, 1440) ? img(shot('mock', 2, 1440).file) : ''}</figure>
    <figure><figcaption>On the website at 390 px / the real tool at 390 px</figcaption><div class="pair">${shot('mock', 1, 390) ? img(shot('mock', 1, 390).file, 'phone') : ''}${img(real?.mobile?.file, 'phone')}</div></figure>
  </div>
  ${site.text ? `<details><summary>What the website says about ${esc(tool)}</summary><pre>${esc(site.text)}</pre></details>` : ''}
  </div>`;
}

const toolNote = (tool) => notes.tools?.[tool];

// Harbor and Navigator are command line tools: real terminal output, as
// text and as an image of that text, beside the website's mock-up.
const cliDir = path.join(dir, 'shots', 'cli');
const cliFiles = existsSync(cliDir) ? (await readdir(cliDir)).filter((f) => f.endsWith('.png')).sort() : [];
async function cliSection(tool, title, prefix, about) {
  const files = cliFiles.filter((f) => f.startsWith(prefix));
  if (!files.length) return '';
  const site = website.tools?.[tool];
  const mock = site?.shots?.find((x) => x.kind === 'mock' && x.n === 1 && x.width === 1440);
  const n = toolNote(tool);
  const items = [];
  for (const f of files) {
    const txt = f.replace(/\.png$/, '.txt');
    const text = existsSync(path.join(cliDir, txt)) ? await readFile(path.join(cliDir, txt), 'utf8') : '';
    const command = text.split(/\r?\n/)[0].replace(/^\$ /, '');
    items.push(`<article class="card"><header><h4><code>${esc(command)}</code></h4></header>
      <div class="shots one"><div class="desk">${img(`cli/${f}`, 'desk')}</div></div>
      <details><summary>the text</summary><pre>${esc(text)}</pre></details></article>`);
  }
  return `<section id="${esc(tool)}">
  <h2>${esc(title)} <small>command line</small></h2>
  <p class="figures">${esc(about)}</p>
  ${n ? `<div class="verdict"><strong>${esc(n.verdict || '')}</strong>${n.items?.length ? `<ul>${n.items.map((i) => `<li>${esc(i)}</li>`).join('')}</ul>` : ''}</div>` : ''}
  ${mock ? `<div class="compare"><h3>The website's mock-up</h3><figure>${img(mock.file)}</figure></div>` : ''}
  <h3>What the commands printed</h3><div class="cards">${items.join('')}</div>
</section>`;
}
const cliSections = [
  await cliSection('harbor', 'Harbor', 'harbor-', 'Harbor provisions and deploys over SSH. No server was available here, so these are the commands that run on their own (init, secrets, the CI workflow) and the error a deploy command gives without a provisioned server. The website shows a full `navree deploy`; that was not run.'),
  await cliSection('navigator', 'Navigator', 'navigator-', 'Navigator is an MCP server for coding agents, started as `navree mcp`. This is a real session over stdio against the source of the evidence app: the handshake, the tool list and four calls. The tools that ask the running app (routes, modules) answer with the error they give when the app is not built.'),
].join('\n');

// Screenshots that do not come from the crawl: shots/extra/<file>.png, described in shots/extra/notes.txt (file|text).
const extraDir = path.join(dir, 'shots', 'extra');
let extraSection = '';
if (existsSync(path.join(extraDir, 'notes.txt'))) {
  const rows = (await readFile(path.join(extraDir, 'notes.txt'), 'utf8')).split(/\r?\n/).filter(Boolean).map((l) => l.split('|'));
  extraSection = `<section id="extra"><h2>Extra</h2><div class="cards">${rows.map(([f, text]) => `<article class="card"><header><h4><code>${esc(f)}</code></h4><p class="note">${esc(text)}</p></header><div class="shots one"><div class="desk">${img(`extra/${f}`, 'desk')}</div></div></article>`).join('')}</div></section>`;
}

const sections = toolNames.map((tool) => {
  const s = stats(tool);
  const n = toolNote(tool);
  const phaseHtml = phases.filter((p) => pairs(results[p], tool).length).map((phase) => {
    const list = pairs(results[phase], tool);
    const pages = list.filter((p) => p.kind === 'page');
    const states = list.filter((p) => p.kind === 'state');
    return `<details class="phase" ${phase === 'after' || (!results.after.length && phase === 'load') ? 'open' : ''}>
  <summary>${esc(phaseLabel[phase])}: ${pages.length} pages, ${states.length} states</summary>
  ${pages.length ? `<h3>Pages</h3><div class="cards">${pages.map((p) => card(p, tool, phase)).join('')}</div>` : ''}
  ${states.length ? `<h3>States</h3><div class="cards">${states.map((p) => card(p, tool, phase)).join('')}</div>` : ''}
</details>`;
  }).join('\n');
  return `<section id="${esc(tool)}">
  <h2>${esc(titleOf(tool))} <small>${esc(toolsConfig.tools.find((t) => t.name === tool)?.package || '')}</small></h2>
  <p class="figures">${s.pages} pages and ${s.states} states photographed; ${s.problems} with a finding; ${s.consoleErrors} console errors, ${s.failed} failed requests, ${s.overflow} overflowing at 390 px, ${s.rule} touching the UI rules; slowest page ${s.slowest} ms.</p>
  ${n ? `<div class="verdict"><strong>${esc(n.verdict || '')}</strong>${n.items?.length ? `<ul>${n.items.map((i) => `<li>${esc(i)}</li>`).join('')}</ul>` : ''}</div>` : ''}
  ${websiteBlock(tool)}
  ${phaseHtml}
</section>`;
}).join('\n');

const overview = toolNames.map((tool) => {
  const s = stats(tool);
  return `<tr><td><a href="#${esc(tool)}">${esc(titleOf(tool))}</a></td><td>${s.pages}</td><td>${s.states}</td><td>${s.mobile}</td><td>${s.problems}</td><td>${s.consoleErrors}</td><td>${s.failed}</td><td>${s.overflow}</td><td>${s.slowest} ms</td></tr>`;
}).join('');

const metaRows = Object.entries(meta.environment || {}).map(([k, v]) => `<tr><th>${esc(k)}</th><td>${esc(v)}</td></tr>`).join('');

const html = `<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>Navree UI evidence ${esc(meta.commit || '')}</title>
<style>
  :root { color-scheme: light; --line:#d6d6d0; --ink:#161616; --mute:#5c5c57; --bad:#b3261e; --warn:#8a5a00; --good:#1d6b34; --rule:#4b3f8f; }
  * { box-sizing: border-box; }
  body { margin: 0; font: 15px/1.5 system-ui, "Segoe UI", sans-serif; color: var(--ink); background: #fafaf7; }
  main { max-width: 1500px; margin: 0 auto; padding: 24px 20px 80px; }
  h1 { font-size: 26px; margin: 0 0 4px; } h2 { font-size: 22px; margin: 48px 0 6px; border-top: 2px solid var(--ink); padding-top: 14px; } h2 small { font-size: 13px; font-weight: 400; color: var(--mute); margin-left: 8px; }
  h3 { font-size: 16px; margin: 18px 0 8px; } h4 { margin: 0 0 4px; font-size: 14px; word-break: break-all; }
  table { border-collapse: collapse; margin: 12px 0; } th, td { border: 1px solid var(--line); padding: 4px 10px; text-align: left; vertical-align: top; } td:not(:first-child) { text-align: right; } th { background:#f0f0ea; }
  .meta-table td { text-align: left; }
  code, pre { font: 12.5px/1.45 ui-monospace, Consolas, monospace; } pre { background:#f0f0ea; padding: 8px; overflow:auto; max-height: 260px; white-space: pre-wrap; }
  .lead { color: var(--mute); max-width: 900px; }
  .filter { margin: 14px 0; } nav a { margin-right: 14px; }
  .figures { color: var(--mute); }
  .verdict { background: #fff; border-left: 4px solid var(--ink); padding: 8px 14px; margin: 10px 0; } .verdict ul { margin: 6px 0 2px; padding-left: 18px; }
  .cards { display: grid; grid-template-columns: repeat(auto-fill, minmax(560px, 1fr)); gap: 14px; }
  .card { background: #fff; border: 1px solid var(--line); border-radius: 4px; padding: 10px; display: flex; flex-direction: column; gap: 8px; } .card.has-bad { border-color: var(--bad); }
  .meta { display: flex; flex-wrap: wrap; gap: 4px 12px; color: var(--mute); font-size: 12.5px; }
  .tag { background: #ececf5; color: var(--rule); padding: 0 6px; margin-right: 6px; border-radius: 2px; font-size: 12px; font-weight: 600; }
  .flags { list-style: none; margin: 6px 0 0; padding: 0; display: flex; flex-wrap: wrap; gap: 4px; } .flags li { padding: 1px 6px; border: 1px solid; border-radius: 2px; font-size: 12px; }
  .flags .bad { color: var(--bad); } .flags .warn { color: var(--warn); } .flags .good { color: var(--good); } .flags .rule { color: var(--rule); }
  .shots.one { grid-template-columns: 1fr; }
  .shots { display: grid; grid-template-columns: 1fr 120px; gap: 10px; align-items: start; }
  .desk, .phone { max-height: 460px; overflow: hidden; border: 1px solid var(--line); position: relative; } .desk img { width: 100%; display: block; } .phone img { width: 100%; display: block; }
  .none { padding: 30px 8px; color: var(--mute); text-align: center; font-size: 12px; }
  .compare { background: #fff; border: 1px solid var(--line); padding: 12px; margin: 12px 0; } .two { display: grid; grid-template-columns: 1fr 1fr; gap: 14px; margin-bottom: 12px; } figure { margin: 0; } figcaption { font-size: 12.5px; color: var(--mute); margin-bottom: 4px; }
  .crmgrid { display: grid; grid-template-columns: repeat(4, 1fr); gap: 10px; } .crmgrid img { width: 100%; border: 1px solid var(--line); display: block; }
  .compare img { width: 100%; border: 1px solid var(--line); display: block; } .pair { display: grid; grid-template-columns: 1fr 1fr; gap: 8px; } .pair img { max-height: 420px; object-fit: cover; object-position: top; }
  .note { background: #fff7e0; border-left: 3px solid var(--warn); padding: 6px 10px; margin: 4px 0; font-size: 13px; }
  summary { cursor: pointer; font-weight: 600; margin: 8px 0; }
  @media (max-width: 900px) { .cards { grid-template-columns: 1fr; } .two { grid-template-columns: 1fr; } }
</style></head><body><main>
<h1>Navree UI evidence</h1>
<p class="lead">Every page of every UI tool the evidence app serves, photographed at 1440 and 390 pixels wide, with its HTTP status, console errors, failed requests and load time, and the website's mock-up beside the real tool. ${esc(meta.summary || '')} Click an image to open it at full size. The written findings are in <a href="report.md">report.md</a>.</p>
<table class="meta-table">${metaRows}</table>
<h2 style="border:0;margin-top:20px">Overview</h2>
<table><tr><th>Tool</th><th>Pages</th><th>States</th><th>Also at 390 px</th><th>With a finding</th><th>Console errors</th><th>Failed requests</th><th>Overflow at 390</th><th>Slowest</th></tr>${overview}</table>
<p class="filter"><label><input type="checkbox" id="only-bad"> Show only the pages with a finding (red)</label></p>
<nav>${toolNames.map((t) => `<a href="#${esc(t)}">${esc(titleOf(t))}</a>`).join('')}${cliFiles.some((f) => f.startsWith('harbor-')) ? '<a href="#harbor">Harbor</a>' : ''}${cliFiles.some((f) => f.startsWith('navigator-')) ? '<a href="#navigator">Navigator</a>' : ''}${extraSection ? '<a href="#extra">Extra</a>' : ''}</nav>
${sections}
${cliSections}
${extraSection}
<script>document.getElementById('only-bad').addEventListener('change', e => { document.querySelectorAll('.card').forEach(c => { c.style.display = e.target.checked && c.dataset.bad !== '1' ? 'none' : ''; }); });</script>
</main></body></html>`;

await writeFile(path.join(dir, 'index.html'), html);
console.log(`wrote ${path.join(dir, 'index.html')}: ${toolNames.length} tools`);
