// navree-lighthouse: are Lighthouse's counts exact under load?
//
// Drives a mix of cheap requests as fast as the virtual users can send
// them (reads, writes, 401s, 404s on a route and on no route at all), then
// compares the requests Lighthouse recorded during the run with the
// requests k6 sent. Lighthouse counts every request in memory on the
// thread that handled it and writes the counts every second, so the two
// must match; only details (traces) may be sampled out, and those are
// reported too.
//
//   k6 run -e PROFILE=load -e VUS=200 -e DURATION=2m scenarios/lighthouse.js
//
// How the count is taken: setup() signs the operator in, then reads
// `GET /_lighthouse/api/overview?period=24h` until the figure stops
// moving (`lighthouse_requests_before`); teardown() does the same after
// the run (`lighthouse_requests_after`). Lighthouse does not record its
// own routes, so the reads are left out of k6's side, and so is the
// operator's sign-in (it lands before the first read). handleSummary
// writes `k6/lighthouse-<profile>.count.json`, which e2e/run.sh turns into
// the check "lighthouse: request count matches k6".

import http from 'k6/http';
import { check, sleep } from 'k6';
import { Gauge } from 'k6/metrics';
import { signInPool, sessionFor } from '../lib/auth.js';
import { BASE_URL, API, OPERATOR_EMAIL, PASSWORD, PROFILE, SUMMARY_DIR, WORKSPACE } from '../lib/config.js';
import { get } from '../lib/http.js';
import { options as profileOptions } from '../lib/profiles.js';
import { digest } from '../lib/summary.js';
import { textSummary } from '../lib/text.js';
import { crudStep, projectIds } from './tasks.js';

const READ = 'lighthouse: read overview';
const OPERATOR = 'lighthouse: operator sign-in';
/** The largest difference allowed between the two counts, in percent. */
const TOLERANCE = Number(__ENV.COUNT_TOLERANCE_PERCENT || 1);

const before = new Gauge('lighthouse_requests_before');
const after = new Gauge('lighthouse_requests_after');
const sampledBefore = new Gauge('lighthouse_sampled_out_before');
const sampledAfter = new Gauge('lighthouse_sampled_out_after');

export const options = profileOptions({
  extra: {
    // Counted separately, so the summary carries them.
    [`http_reqs{name:${READ}}`]: ['count>=0'],
    [`http_reqs{name:${OPERATOR}}`]: ['count>=0'],
    // Throughput first: the latency budget of this run is loose.
    http_req_duration: [`p(95)<${Number(__ENV.P95_MS || 2000)}`],
  },
});

/** The overview's figures, as the operator. */
function overview(token) {
  const res = http.get(`${BASE_URL}/_lighthouse/api/overview?period=24h`, {
    headers: { Authorization: `Bearer ${token}`, Accept: 'application/json' },
    tags: { name: READ },
  });
  check(res, { 'lighthouse overview: 200': (r) => r.status === 200 });
  if (res.status !== 200) {
    throw new Error(`GET /_lighthouse/api/overview: ${res.status} ${String(res.body).slice(0, 200)}`);
  }
  return { requests: res.json('requests'), sampledOut: res.json('sampled_out') };
}

/** Reads the overview until two reads two seconds apart agree. */
function settled(token) {
  let last = overview(token);
  for (let i = 0; i < 30; i++) {
    sleep(2);
    const next = overview(token);
    if (next.requests === last.requests && next.sampledOut === last.sampledOut) return next;
    last = next;
  }
  return last;
}

export function setup() {
  const res = http.post(`${API}/auth/login`, JSON.stringify({ email: OPERATOR_EMAIL, password: PASSWORD }), {
    headers: { 'Content-Type': 'application/json', Accept: 'application/json' },
    tags: { name: OPERATOR },
  });
  if (res.status !== 200) throw new Error(`operator sign-in: ${res.status}`);
  const operator = res.json('access_token');
  const start = settled(operator);
  before.add(start.requests);
  sampledBefore.add(start.sampledOut);
  const pool = signInPool(Number(__ENV.POOL || 20)).map((s) => ({ token: s.token, workspace: WORKSPACE }));
  return { operator, pool, projects: projectIds(pool[0]) };
}

const MISSING_TASK = '00000000-0000-7000-8000-000000000000';

/** One request of the mix (and, now and then, a task's whole life). */
export function lighthouseStep(session, projects) {
  const roll = Math.random();
  if (roll < 0.35) {
    check(get(session, '/auth/me', 'GET /api/auth/me'), { 'me: 200': (r) => r.status === 200 });
  } else if (roll < 0.6) {
    check(get(session, '/v1/tasks?per_page=20', 'GET /api/v1/tasks'), { 'tasks: 200': (r) => r.status === 200 });
  } else if (roll < 0.75) {
    check(get(session, '/v1/projects?per_page=50', 'GET /api/v1/projects'), {
      'projects: 200': (r) => r.status === 200,
    });
  } else if (roll < 0.85) {
    const res = http.get(`${API}/v1/tasks/${MISSING_TASK}`, {
      headers: { Authorization: `Bearer ${session.token}`, 'X-Workspace': session.workspace, Accept: 'application/json' },
      tags: { name: 'GET /api/v1/tasks/{id} (missing)' },
      responseCallback: http.expectedStatuses(404),
    });
    check(res, { 'missing task: 404': (r) => r.status === 404 });
  } else if (roll < 0.92) {
    const res = http.get(`${API}/auth/me`, {
      headers: { Accept: 'application/json' },
      tags: { name: 'GET /api/auth/me (guest)' },
      responseCallback: http.expectedStatuses(401),
    });
    check(res, { 'guest: 401': (r) => r.status === 401 });
  } else if (roll < 0.97) {
    const res = http.get(`${API}/v1/no-such-route/${__ITER}`, {
      headers: { Accept: 'application/json' },
      tags: { name: 'GET unmatched' },
      responseCallback: http.expectedStatuses(404),
    });
    check(res, { 'unmatched: 404': (r) => r.status === 404 });
  } else {
    crudStep(session, projects);
  }
}

export default function (data) {
  lighthouseStep(sessionFor(data.pool), data.projects);
}

export function teardown(data) {
  const end = settled(data.operator);
  after.add(end.requests);
  sampledAfter.add(end.sampledOut);
}

const value = (data, name, field) => (data.metrics[name] ? data.metrics[name].values[field] : 0);

/** k6's requests against Lighthouse's, as a check result. */
export function countCheck(data) {
  const total = value(data, 'http_reqs', 'count');
  const reads = value(data, `http_reqs{name:${READ}}`, 'count');
  const operator = value(data, `http_reqs{name:${OPERATOR}}`, 'count');
  const k6 = total - reads - operator;
  const lighthouse = value(data, 'lighthouse_requests_after', 'value') - value(data, 'lighthouse_requests_before', 'value');
  const sampledOut = value(data, 'lighthouse_sampled_out_after', 'value') - value(data, 'lighthouse_sampled_out_before', 'value');
  const difference = k6 === 0 ? 100 : (Math.abs(lighthouse - k6) / k6) * 100;
  return {
    passed: k6 > 0 && difference <= TOLERANCE,
    k6_requests: k6,
    lighthouse_requests: lighthouse,
    missing: k6 - lighthouse,
    difference_percent: Math.round(difference * 10000) / 10000,
    tolerance_percent: TOLERANCE,
    sampled_out: sampledOut,
    k6_total_requests: total,
    lighthouse_reads: reads,
    operator_sign_ins: operator,
    rps: value(data, 'http_reqs', 'rate'),
  };
}

export function handleSummary(data) {
  const count = countCheck(data);
  const text = `${textSummary(data)}\n\nlighthouse count check: ${JSON.stringify(count)}\n`;
  const out = { stdout: text };
  if (SUMMARY_DIR) {
    const base = `${SUMMARY_DIR}/lighthouse-${PROFILE}`;
    out[`${base}.json`] = JSON.stringify(data, null, 2);
    out[`${base}.digest.json`] = JSON.stringify({ ...digest('lighthouse', data), lighthouse_count: count }, null, 2);
    out[`${base}.txt`] = text;
    out[`${base}.count.json`] = JSON.stringify(count);
  }
  return out;
}

