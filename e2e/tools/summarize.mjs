// Assembles <out>/summary.json and <out>/summary.md from a run's files:
// checks.jsonl (one line per harness step), k6/*.digest.json (per scenario),
// k6/checks-smoke.json (the HTTP rows), screenshots/results.json and
// resources/*.jsonl (docker stats samples). Run by e2e/run.sh in a node
// container; also runnable by hand: node e2e/tools/summarize.mjs <out>.

import { readFile, readdir, writeFile } from 'node:fs/promises';
import path from 'node:path';

const out = process.argv[2];
if (!out) {
  console.error('usage: summarize.mjs <results dir>');
  process.exit(2);
}

const read = async (file, fallback) => {
  try {
    return await readFile(path.join(out, file), 'utf8');
  } catch {
    return fallback;
  }
};
const readJson = async (file, fallback) => {
  const text = await read(file, undefined);
  if (text === undefined) return fallback;
  try {
    return JSON.parse(text);
  } catch {
    return fallback;
  }
};
const list = async (dir) => {
  try {
    return await readdir(path.join(out, dir));
  } catch {
    return [];
  }
};
const jsonl = (text) =>
  text
    .split('\n')
    .filter((line) => line.trim())
    .map((line) => {
      try {
        return JSON.parse(line);
      } catch {
        return null;
      }
    })
    .filter(Boolean);

// Harness steps.
const checks = jsonl(await read('checks.jsonl', ''));

// HTTP rows: one check per row in the k6 checks summary.
const httpSummary = await readJson('k6/checks-smoke.json', null);
const httpRows = [];
const walk = (group) => {
  for (const c of group.checks || []) {
    httpRows.push({ name: c.name, status: c.fails === 0 && c.passes > 0 ? 'pass' : 'fail' });
  }
  for (const g of group.groups || []) walk(g);
};
if (httpSummary && httpSummary.root_group) walk(httpSummary.root_group);
for (const row of httpRows) {
  if (row.name.startsWith('sign-in:')) continue;
  checks.push({ name: `http: ${row.name}`, status: row.status, seconds: 0, detail: '' });
}

// k6 scenarios.
const scenarios = [];
for (const file of (await list('k6')).filter((f) => f.endsWith('.digest.json')).sort()) {
  const digest = await readJson(`k6/${file}`, null);
  if (!digest || digest.scenario === 'checks') continue;
  scenarios.push(digest);
}

// Screenshots.
const shots = await readJson('screenshots/results.json', []);
for (const shot of shots) {
  checks.push({
    name: `screenshot: ${shot.tool} ${shot.url || shot.path} @${shot.width}${shot.kind === 'state' ? ` [${shot.state}]` : ''}`,
    status: shot.ok ? 'pass' : 'fail',
    seconds: 0,
    detail: shot.ok ? shot.file : shot.error || `status ${shot.status}, expect_found ${shot.expect_found}`,
  });
}

// Resources: peak and mean CPU and memory per container and phase.
const parsePercent = (s) => Number(String(s || '0').replace('%', '')) || 0;
const parseBytes = (s) => {
  const m = /^([\d.]+)\s*([KMGT]?i?B)/.exec(String(s || '').trim());
  if (!m) return 0;
  const units = { B: 1, KB: 1e3, MB: 1e6, GB: 1e9, TB: 1e12, KiB: 1024, MiB: 1024 ** 2, GiB: 1024 ** 3, TiB: 1024 ** 4 };
  return Number(m[1]) * (units[m[2]] || 1);
};
const resources = {};
for (const file of (await list('resources')).filter((f) => f.endsWith('.jsonl')).sort()) {
  const phase = file.replace(/\.jsonl$/, '');
  const byContainer = {};
  for (const s of jsonl(await read(`resources/${file}`, ''))) {
    const name = String(s.Name || s.Container || '').replace(/^navree-e2e-/, '').replace(/-1$/, '');
    const entry = (byContainer[name] ||= { samples: 0, cpu_sum: 0, cpu_max: 0, mem_max_mb: 0, mem_sum_mb: 0 });
    const cpu = parsePercent(s.CPUPerc);
    const mem = parseBytes(String(s.MemUsage || '').split('/')[0]) / 1e6;
    entry.samples += 1;
    entry.cpu_sum += cpu;
    entry.cpu_max = Math.max(entry.cpu_max, cpu);
    entry.mem_sum_mb += mem;
    entry.mem_max_mb = Math.max(entry.mem_max_mb, mem);
  }
  resources[phase] = Object.fromEntries(
    Object.entries(byContainer).map(([name, e]) => [
      name,
      {
        samples: e.samples,
        cpu_percent_avg: +(e.cpu_sum / e.samples).toFixed(1),
        cpu_percent_max: +e.cpu_max.toFixed(1),
        mem_mb_avg: +(e.mem_sum_mb / e.samples).toFixed(1),
        mem_mb_max: +e.mem_max_mb.toFixed(1),
      },
    ]),
  );
}

const imageSize = Number((await read('image-size.txt', '0')).trim()) || null;
const passed = checks.filter((c) => c.status === 'pass').length;
const failed = checks.filter((c) => c.status === 'fail').length;
const summary = {
  run_id: process.env.RUN_ID || path.basename(out),
  status: failed === 0 ? 'pass' : 'fail',
  started_at: process.env.STARTED || null,
  finished_at: process.env.FINISHED || null,
  load_profile: process.env.LOAD_PROFILE || null,
  commit: (await read('commit.txt', '')).trim() || null,
  docker: (await read('docker-version.txt', '')).trim() || null,
  host: (await read('host.txt', '')).trim() || null,
  image_size_mb: imageSize ? +(imageSize / 1e6).toFixed(1) : null,
  totals: { checks: checks.length, passed, failed },
  checks,
  scenarios,
  resources,
};
await writeFile(path.join(out, 'summary.json'), JSON.stringify(summary, null, 2));

// A human-readable digest next to it.
const f = (v, d = 1) => (typeof v === 'number' ? v.toFixed(d) : '-');
const md = [];
md.push(`# Run ${summary.run_id}`, '');
md.push(`Status: **${summary.status}** (${passed} passed, ${failed} failed). Commit ${summary.commit || '?'}. Host: ${summary.host || '?'}. Image: ${summary.image_size_mb ?? '?'} MB.`, '');
if (scenarios.length) {
  md.push('## Load', '', '| Scenario | Profile | Requests | RPS | Errors | p50 ms | p95 ms | p99 ms | Max VUs | Thresholds |', '|---|---|---|---|---|---|---|---|---|---|');
  for (const s of scenarios) {
    md.push(`| ${s.scenario} | ${s.profile} | ${s.requests ?? '-'} | ${f(s.rps)} | ${f((s.error_rate || 0) * 100, 2)}% | ${f(s.latency_ms.med)} | ${f(s.latency_ms.p95)} | ${f(s.latency_ms.p99)} | ${s.vus_max ?? '-'} | ${s.passed ? 'pass' : 'FAIL'} |`);
  }
  md.push('');
}
for (const [phase, containers] of Object.entries(resources)) {
  md.push(`## Resources: ${phase}`, '', '| Container | CPU avg % | CPU max % | Mem avg MB | Mem max MB |', '|---|---|---|---|---|');
  for (const [name, r] of Object.entries(containers)) {
    md.push(`| ${name} | ${r.cpu_percent_avg} | ${r.cpu_percent_max} | ${r.mem_mb_avg} | ${r.mem_mb_max} |`);
  }
  md.push('');
}
md.push('## Checks', '');
for (const c of checks) md.push(`- ${c.status === 'pass' ? 'pass' : c.status === 'skip' ? 'skip' : '**FAIL**'} ${c.name}${c.detail && c.status !== 'pass' ? `: ${c.detail}` : ''}`);
await writeFile(path.join(out, 'summary.md'), `${md.join('\n')}\n`);
console.log(`summary: ${summary.status}, ${passed}/${checks.length} checks passed`);
