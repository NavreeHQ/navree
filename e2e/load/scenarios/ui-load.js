// The load the screenshots are taken under: the baseline mix at the
// profile's virtual users (50 by default) and the `ui` traffic (failures,
// slow calls, jobs, mail) at a few more, in one run, so the tools show a
// busy app while the browser walks them.
//
//   k6 run -e VUS=50 -e DURATION=6m -e UI_VUS=6 scenarios/ui-load.js

import http from 'k6/http';
import { sleep } from 'k6';
import { signInPool, sessionFor } from '../lib/auth.js';
import { WORKSPACE } from '../lib/config.js';
import { summaryFor } from '../lib/summary.js';
import { stages } from '../lib/profiles.js';
import { projectIds } from './tasks.js';
import { publishStep } from './realtime.js';
import { uiSetup, uiStep } from './ui.js';
import baseline from './baseline.js';

http.setResponseCallback(http.expectedStatuses({ min: 200, max: 599 }));

const num = (name, fallback) => Number(__ENV[name] || fallback);

export const options = {
  scenarios: {
    baseline: {
      executor: 'ramping-vus',
      startVUs: 0,
      stages: stages('load'),
      gracefulRampDown: '10s',
      exec: 'baselineFn',
    },
    ui: {
      executor: 'constant-vus',
      vus: num('UI_VUS', 6),
      duration: __ENV.UI_DURATION || `${__ENV.DURATION || '3m'}`,
      exec: 'uiFn',
    },
  },
  // The `ui` routes fail on purpose: only the baseline's requests are
  // held to the profile's latency and error thresholds.
  thresholds: {
    'http_req_duration{scenario:baseline}': [`p(95)<${num('P95_MS', 500)}`, `p(99)<${num('P99_MS', 1500)}`],
    'checks{scenario:baseline}': ['rate>0.99'],
    'checks{scenario:ui}': ['rate>0.97'],
  },
  setupTimeout: '10m',
  summaryTrendStats: ['avg', 'min', 'med', 'p(90)', 'p(95)', 'p(99)', 'max', 'count'],
};

export function setup() {
  const ui = uiSetup();
  const pool = ui.pool.slice(0, 20);
  const projects = projectIds(ui.operator);
  const channel = publishStep(pool[0]);
  return { ...ui, baseline: { pool, projects, channel } };
}

export function baselineFn(data) {
  baseline(data.baseline);
}

export function uiFn(data) {
  uiStep(data);
  sleep(0.2 + Math.random() * 0.8);
}

export const handleSummary = summaryFor('ui-load');
