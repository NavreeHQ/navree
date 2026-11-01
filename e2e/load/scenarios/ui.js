// The traffic the UI tools need to have something to show: what a real
// app's day looks like, failures included.
//
//   setup   one burst of queue work and mail, so Fleet has a backlog to
//           drain and Lighthouse has jobs that fail and retry
//   steps   every iteration picks one behaviour: reads and writes, errors
//           with distinct causes, slow requests and queries, outgoing calls
//           that fail, jobs, mail, client mistakes (401, 403, 404, 422)
//
// The routes under /api/demo are the evidence app's (modules/demo). The
// failing ones answer 5xx on purpose: the run's own checks look for the
// status each route is meant to give, and k6 is told that any status is
// an answer.
//
//   k6 run -e PROFILE=load -e VUS=8 -e DURATION=4m scenarios/ui.js

import http from 'k6/http';
import { check, sleep } from 'k6';
import { Counter } from 'k6/metrics';
import { API, BASE_URL, OPERATOR_EMAIL, PASSWORD, WORKSPACE } from '../lib/config.js';
import { signIn, signInPool, sessionFor, loadUser } from '../lib/auth.js';
import { get, json, pick, post } from '../lib/http.js';
import { summaryFor } from '../lib/summary.js';
import { crudStep, projectIds } from './tasks.js';
import { searchStep } from './search.js';
import { publishStep } from './realtime.js';
import { reportStep } from './reports.js';

http.setResponseCallback(http.expectedStatuses({ min: 200, max: 599 }));

/** Responses that did not have the status their route is meant to give. */
export const unexpected = new Counter('ui_unexpected_status');

const num = (name, fallback) => Number(__ENV[name] || fallback);

/** A weighted pick from `[[value, weight], ...]`. */
function weighted(items) {
  const total = items.reduce((sum, [, w]) => sum + w, 0);
  let roll = Math.random() * total;
  for (const [value, w] of items) {
    roll -= w;
    if (roll <= 0) return value;
  }
  return items[0][0];
}

function expectStatus(res, wanted, label) {
  const ok = Array.isArray(wanted) ? wanted.includes(res.status) : res.status === wanted;
  if (!ok) unexpected.add(1);
  check(res, { [`${label}: ${wanted}`]: () => ok });
}

const ERRORS = [
  ['card-declined', 12], ['stage-probability', 6], ['duplicate-domain', 5],
  ['mail-refused', 4], ['template-missing', 3], ['currency-parse', 3], ['panic', 2],
];
const SLOW_REQUESTS = [['board', 4], ['import', 1], ['export', 3]];
const SLOW_QUERIES = [['contacts-by-id', 4], ['activities-by-deal', 3], ['report-rollup', 2]];
const OUTGOING = [
  ['exchange-timeout', 2, [200]], ['payments-503', 4, [502]], ['refused', 3, [502]],
  ['dns', 3, [200, 502]], ['ok-mailpit', 5, [200]], ['ok-search', 5, [200]],
];
const JOBS = [
  ['ok', 6], ['slow', 2], ['flaky', 3], ['mail-refused', 2], ['export', 1], ['webhook-gone', 1], ['notify', 2],
];

/** Sessions for the setup: a pool of members, the operator, a viewer and a member without tool access. */
export function uiSetup() {
  const pool = signInPool(num('POOL', 30)).map((s) => ({ token: s.token, workspace: WORKSPACE }));
  const operator = { token: signIn(OPERATOR_EMAIL).token, workspace: WORKSPACE };
  const viewer = { token: signIn('ops-viewer@evidence.test').token, workspace: WORKSPACE };
  const support = { token: signIn('support@evidence.test').token, workspace: WORKSPACE };
  const projects = projectIds(operator);
  if (num('BURST', 1) > 0) burst(operator);
  return { pool, operator, viewer, support, projects };
}

/** One burst of queue work and mail, queued through the demo routes. */
export function burst(operator) {
  const queue = (kind, count) => {
    const res = post(operator, '/demo/jobs', 'POST /api/demo/jobs', { kind, count });
    check(res, { [`burst ${kind}: 202`]: (r) => r.status === 202 });
  };
  const scale = num('BURST_SCALE', 1);
  queue('newsletter', Math.round(600 * scale));
  queue('ok', Math.round(300 * scale));
  queue('slow', Math.round(40 * scale));
  queue('flaky', Math.round(60 * scale));
  queue('mail-refused', Math.round(25 * scale));
  queue('export', Math.round(18 * scale));
  queue('webhook-gone', Math.round(15 * scale));
  queue('notify', Math.round(80 * scale));
  const mail = post(operator, '/demo/mail', 'POST /api/demo/mail', { count: Math.round(120 * scale) });
  check(mail, { 'burst mail: 202': (r) => r.status === 202 });
}

/** One iteration: a behaviour picked by weight. */
export function uiStep(data) {
  const session = sessionFor(data.pool);
  const behaviour = weighted([
    ['reads', 24], ['crud', 6], ['search', 6], ['graphql', 3], ['publish', 3], ['report', 2],
    ['error', 9], ['slow-request', 5], ['slow-query', 5], ['n-plus-one', 3], ['outgoing', 8],
    ['jobs', 4], ['mail', 2], ['client-mistake', 6], ['viewer', 2], ['signin', 1],
  ]);
  switch (behaviour) {
    case 'reads': {
      expectStatus(get(session, '/v1/projects?per_page=20', 'GET /api/v1/projects'), 200, 'projects');
      expectStatus(get(session, '/v1/tasks?per_page=20', 'GET /api/v1/tasks'), 200, 'tasks');
      expectStatus(get(session, '/flags', 'GET /api/flags'), 200, 'flags');
      break;
    }
    case 'crud':
      crudStep(session, data.projects);
      break;
    case 'search':
      searchStep(session);
      break;
    case 'graphql': {
      const res = http.post(
        `${BASE_URL}/graphql`,
        JSON.stringify({ query: '{ taskCounts { todo doing done } tasks(limit: 5) { id title status priority } }' }),
        {
          headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${session.token}`, 'X-Workspace': session.workspace },
          tags: { name: 'POST /graphql' },
        },
      );
      expectStatus(res, 200, 'graphql');
      break;
    }
    case 'publish':
      publishStep(session);
      break;
    case 'report':
      reportStep(session, 10);
      break;
    case 'error': {
      const kind = weighted(ERRORS);
      expectStatus(get(session, `/demo/errors/${kind}`, `GET /api/demo/errors/${kind}`), 500, `error ${kind}`);
      break;
    }
    case 'slow-request': {
      const kind = weighted(SLOW_REQUESTS);
      expectStatus(get(session, `/demo/slow-request/${kind}`, `GET /api/demo/slow-request/${kind}`), 200, `slow request ${kind}`);
      break;
    }
    case 'slow-query': {
      const kind = weighted(SLOW_QUERIES);
      expectStatus(get(session, `/demo/slow-query/${kind}`, `GET /api/demo/slow-query/${kind}`), 200, `slow query ${kind}`);
      break;
    }
    case 'n-plus-one':
      expectStatus(get(session, '/demo/n-plus-one', 'GET /api/demo/n-plus-one'), 200, 'n+1');
      break;
    case 'outgoing': {
      const [kind, , wanted] = OUTGOING[Math.floor(Math.random() * OUTGOING.length)];
      expectStatus(get(session, `/demo/outgoing/${kind}`, `GET /api/demo/outgoing/${kind}`), wanted, `outgoing ${kind}`);
      break;
    }
    case 'jobs': {
      const kind = weighted(JOBS);
      expectStatus(post(session, '/demo/jobs', 'POST /api/demo/jobs', { kind, count: 1 + Math.floor(Math.random() * 4) }), 202, `job ${kind}`);
      break;
    }
    case 'mail':
      expectStatus(post(session, '/demo/mail', 'POST /api/demo/mail', { count: 1 + Math.floor(Math.random() * 3) }), 202, 'mail');
      break;
    case 'client-mistake':
      clientMistake(session, data);
      break;
    case 'viewer': {
      // A viewer reads, and may not write.
      expectStatus(get(data.viewer, '/v1/tasks?per_page=10', 'GET /api/v1/tasks'), 200, 'viewer reads');
      const denied = post(data.viewer, '/v1/tasks', 'POST /api/v1/tasks', { project_id: pick(data.projects), title: 'Not allowed' });
      expectStatus(denied, 403, 'viewer write');
      break;
    }
    case 'signin':
      http.post(`${API}/auth/login`, JSON.stringify({ email: loadUser(1 + Math.floor(Math.random() * 200)), password: PASSWORD }), {
        headers: { 'Content-Type': 'application/json', Accept: 'application/json' },
        tags: { name: 'POST /api/auth/login' },
      });
      break;
  }
}

/** The mistakes clients make: no token, a wrong password, unknown ids and routes, bad bodies. */
function clientMistake(session, data) {
  const json_ = { 'Content-Type': 'application/json', Accept: 'application/json' };
  const which = Math.floor(Math.random() * 6);
  if (which === 0) {
    expectStatus(http.get(`${API}/v1/tasks`, { headers: json_, tags: { name: 'GET /api/v1/tasks (no token)' } }), 401, 'no token');
  } else if (which === 1) {
    const res = http.post(`${API}/auth/login`, JSON.stringify({ email: loadUser(2), password: 'not-the-password' }), {
      headers: json_, tags: { name: 'POST /api/auth/login (wrong password)' },
    });
    expectStatus(res, [401, 422], 'wrong password');
  } else if (which === 2) {
    const id = '0196a000-0000-7000-8000-' + String(Math.floor(Math.random() * 1e12)).padStart(12, '0');
    expectStatus(get(session, `/v1/tasks/${id}`, 'GET /api/v1/tasks/{id}'), 404, 'unknown task');
  } else if (which === 3) {
    expectStatus(http.get(`${API}/v1/no-such-route`, { headers: json_, tags: { name: 'GET /api/v1/no-such-route' } }), 404, 'unknown route');
  } else if (which === 4) {
    expectStatus(post(session, '/v1/tasks', 'POST /api/v1/tasks', { project_id: pick(data.projects), title: '', priority: 9 }), [400, 422], 'invalid task');
  } else {
    expectStatus(get(data.support, '/v1/projects?per_page=5', 'GET /api/v1/projects'), [200, 403], 'support member');
  }
}

export const options = {
  scenarios: {
    ui: {
      executor: 'ramping-vus',
      startVUs: 0,
      stages: [
        { duration: __ENV.RAMP || '20s', target: num('VUS', 8) },
        { duration: __ENV.DURATION || '4m', target: num('VUS', 8) },
        { duration: '10s', target: 0 },
      ],
      gracefulRampDown: '15s',
      exec: 'default',
    },
  },
  thresholds: { checks: ['rate>0.97'] },
  setupTimeout: '10m',
  summaryTrendStats: ['avg', 'min', 'med', 'p(90)', 'p(95)', 'p(99)', 'max', 'count'],
};

export function setup() {
  return uiSetup();
}

export default function (data) {
  uiStep(data);
  sleep(0.2 + Math.random() * 0.8);
}

export const handleSummary = summaryFor('ui');
