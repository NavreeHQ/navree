// Writes <dir>/meta.json for the gallery from the facts a UI evidence run
// collected: the commit, the images that ran, the host, the load and what
// the data holds.
//
//   node ui-meta.mjs <dir>

import { readFile, readdir, writeFile } from 'node:fs/promises';
import { existsSync } from 'node:fs';
import path from 'node:path';

const dir = path.resolve(process.argv[2] || '.');
const facts = path.join(dir, 'data', 'facts');
const text = async (name) => (existsSync(path.join(facts, name)) ? (await readFile(path.join(facts, name), 'utf8')).trim() : '');
const json = async (file, fallback = null) => {
  try {
    return JSON.parse(await readFile(file, 'utf8'));
  } catch {
    return fallback;
  }
};
const lines = (s) => s.split('\n').map((l) => l.trim()).filter(Boolean);

const commit = await text('commit.txt');
const [author, date, subject] = (await text('commit-info.txt')).split('|');
const docker = await json(path.join(facts, 'docker-version.json'), {});
const info = await json(path.join(facts, 'docker-info.json'), {});
const run = await json(path.join(facts, 'run.json'), {});
const images = lines(await text('images.jsonl')).map((l) => {
  try {
    return JSON.parse(l);
  } catch {
    return null;
  }
}).filter(Boolean);
const mb = (n) => `${(n / 1e6).toFixed(0)} MB`;

const k6dir = path.join(dir, 'data', 'k6');
const digests = {};
if (existsSync(k6dir)) {
  for (const f of (await readdir(k6dir)).filter((x) => x.endsWith('.digest.json'))) {
    const d = await json(path.join(k6dir, f));
    if (d) digests[d.scenario] = d;
  }
}
const k6line = (d) => (d ? `${d.requests?.toLocaleString('en')} requests, ${Math.round(d.rps)} req/s, p95 ${d.latency_ms?.p95?.toFixed(0)} ms, p99 ${d.latency_ms?.p99?.toFixed(0)} ms, ${(d.checks_rate * 100).toFixed(2)}% of checks passed (each route's own expected status)` : 'not run');

const filled = await json(path.join(facts, 'filled.json'), null);
const after = await json(path.join(facts, 'after.json'), null);
const count = (page) => (page && page.meta ? page.meta.total ?? page.meta.total_items : null) ?? (page?.data ? page.data.length : null);
const dataLine = after
  ? `Lighthouse after the load: ${count(after.lighthouse_open_issues)} open, ${count(after.lighthouse_resolved_issues)} resolved, ${count(after.lighthouse_ignored_issues)} ignored issues; Mailpit holds ${after.mailpit?.Messages ?? '?'} messages`
  : '';

const environment = {
  Commit: `${commit.slice(0, 12)} (${(await text('branch.txt')) || 'branch unknown'}): ${subject || ''}`,
  Run: `${run.started || ''} to ${run.finished || ''}`,
  Docker: `${docker.Server?.Version || '?'} (${info.OperatingSystem || ''}), ${info.NCPU || '?'} CPUs, ${info.MemTotal ? (info.MemTotal / 2 ** 30).toFixed(1) : '?'} GiB, kernel ${info.KernelVersion || '?'}`,
  Images: images.map((i) => `${i.image} (${mb(i.size)})`).join('; '),
  PostgreSQL: lines(await text('postgres-version.txt')).slice(0, 2).join(' / '),
  'Load, the walk': `${run.load_vus} users on the baseline mix plus ${run.ui_vus} on the failing, slow and queued traffic, for ${run.load_duration}`,
  'k6, under load': k6line(digests['ui-load']),
  'k6, the fill': k6line(digests['ui-fill']),
  Data: dataLine,
};
await writeFile(path.join(dir, 'meta.json'), JSON.stringify({ commit, author, date, summary: dataLine, environment, digests }, null, 2));
console.log(`wrote ${path.join(dir, 'meta.json')}`);
