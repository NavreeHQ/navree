// The baseline: a mix of what a multi-tenant app does all day, in one run.
//
//   45%  CRUD on a workspace-scoped ORM model (tasks.js)
//   20%  list projects and tasks
//   15%  full-text search (search.js)
//   10%  realtime publish (realtime.js)
//    7%  queue a report and wait for the worker (reports.js)
//    3%  sign in (auth.js; Argon2id)
//
// plus, from the first two virtual users every 10th iteration, a WebSocket
// round trip (subscribe, publish, receive).
//
//   k6 run -e PROFILE=load -e VUS=50 -e DURATION=3m scenarios/baseline.js

import { check } from 'k6';
import { signInPool, sessionFor } from '../lib/auth.js';
import { WORKSPACE } from '../lib/config.js';
import { get } from '../lib/http.js';
import { options as profileOptions } from '../lib/profiles.js';
import { summaryFor } from '../lib/summary.js';
import { signInStep } from './auth.js';
import { crudStep, projectIds } from './tasks.js';
import { reportStep } from './reports.js';
import { searchStep } from './search.js';
import { publishStep, roundTripStep } from './realtime.js';

const routes = [
  'POST /api/auth/login',
  'GET /api/v1/projects',
  'GET /api/v1/tasks',
  'POST /api/v1/tasks',
  'GET /api/v1/tasks/{id}',
  'PATCH /api/v1/tasks/{id}',
  'DELETE /api/v1/tasks/{id}',
  'GET /api/v1/projects/search',
  'POST /api/v1/reports',
  'GET /api/v1/reports/{id}',
  'POST /api/v1/live/publish',
];

// A per-route threshold makes k6 report each route's latency in the
// summary; the run's own thresholds come from the profile.
const perRoute = {};
for (const name of routes) {
  perRoute[`http_req_duration{name:${name}}`] = [`p(99)<${Number(__ENV.ROUTE_P99_MS || 5000)}`];
}

export const options = profileOptions({
  extra: {
    ...perRoute,
    report_completed: ['rate>0.99'],
    realtime_delivered: ['rate>0.95'],
  },
});

export function setup() {
  const pool = signInPool(Number(__ENV.POOL || 50)).map((s) => ({ token: s.token, workspace: WORKSPACE }));
  const projects = projectIds(pool[0]);
  const channel = publishStep(pool[0]);
  return { pool, projects, channel };
}

export default function (data) {
  const session = sessionFor(data.pool);
  const roll = Math.random();
  if (roll < 0.45) {
    crudStep(session, data.projects);
  } else if (roll < 0.65) {
    check(get(session, '/v1/projects?per_page=20', 'GET /api/v1/projects'), { 'projects: 200': (r) => r.status === 200 });
    check(get(session, '/v1/tasks?per_page=20', 'GET /api/v1/tasks'), { 'tasks: 200': (r) => r.status === 200 });
  } else if (roll < 0.8) {
    searchStep(session);
  } else if (roll < 0.9) {
    publishStep(session);
  } else if (roll < 0.97) {
    reportStep(session);
  } else {
    signInStep();
  }
  if (__VU <= 2 && __ITER % 10 === 0) {
    roundTripStep(session, data.channel);
  }
}

export const handleSummary = summaryFor('baseline');
