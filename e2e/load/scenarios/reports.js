// navree-queue: `POST /api/v1/reports` queues `reports.generate` in the
// request's unit of work; a worker process runs it. The step polls the
// report until it is `done` and records how long the queue took
// (`report_done_ms`), so a run proves the worker keeps up under load.

import { check, sleep } from 'k6';
import { Trend, Rate } from 'k6/metrics';
import { signInPool, sessionFor } from '../lib/auth.js';
import { WORKSPACE } from '../lib/config.js';
import { get, json, post } from '../lib/http.js';
import { options as profileOptions } from '../lib/profiles.js';
import { summaryFor } from '../lib/summary.js';

/** From the request that queued the report to the read that saw it done. */
export const reportDone = new Trend('report_done_ms', true);
/** Reports done within the polling window. */
export const reportCompleted = new Rate('report_completed');

/** Queue one report; poll it every 250 ms for up to `waitSecs`. */
export function reportStep(session, waitSecs = 15) {
  const started = Date.now();
  const res = post(session, '/v1/reports', 'POST /api/v1/reports');
  if (!check(res, { 'report queued: 201': (r) => r.status === 201 })) return;
  const id = json(res, 'id');
  const deadline = started + waitSecs * 1000;
  while (Date.now() < deadline) {
    sleep(0.25);
    const shown = get(session, `/v1/reports/${id}`, 'GET /api/v1/reports/{id}');
    if (json(shown, 'status') === 'done') {
      reportDone.add(Date.now() - started);
      reportCompleted.add(true);
      check(shown, { 'report done: task counts': (r) => json(r, 'result.tasks') !== undefined });
      return;
    }
  }
  reportCompleted.add(false);
}

export const options = profileOptions({
  extra: { report_completed: ['rate>0.99'], report_done_ms: ['p(95)<5000'] },
});

export function setup() {
  return { pool: signInPool(Number(__ENV.POOL || 20)).map((s) => ({ token: s.token, workspace: WORKSPACE })) };
}

export default function (data) {
  reportStep(sessionFor(data.pool));
}

export const handleSummary = summaryFor('reports');
