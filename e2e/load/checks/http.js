// The HTTP smoke checks: every row of e2e/checks/http.json, once, as the
// operator (a super admin) in the first seeded workspace. Each row becomes
// a k6 check named after it, so the run's summary lists pass or fail per
// row (`checks-smoke.json`, read into summary.json).
//
// A row: { name, package, method, path, expect: 200 | [200, 302],
//          auth?: true, workspace?: true, headers?: {..}, body?: {..},
//          json_path?: "data.0.id" }

import http from 'k6/http';
import { check } from 'k6';
import { signIn } from '../lib/auth.js';
import { BASE_URL, OPERATOR_EMAIL, WORKSPACE } from '../lib/config.js';
import { json } from '../lib/http.js';
import { summaryFor } from '../lib/summary.js';

const rows = JSON.parse(open(__ENV.CHECKS_FILE || '/checks/http.json'));

export const options = {
  vus: 1,
  iterations: 1,
  thresholds: { checks: ['rate==1'] },
};

export function setup() {
  return { token: signIn(OPERATOR_EMAIL).token };
}

export default function (data) {
  for (const row of rows) {
    const headers = { Accept: 'application/json' };
    if (row.auth) headers.Authorization = `Bearer ${data.token}`;
    if (row.workspace) headers['X-Workspace'] = WORKSPACE;
    if (row.body) headers['Content-Type'] = 'application/json';
    Object.assign(headers, row.headers || {});
    const res = http.request(row.method || 'GET', BASE_URL + row.path, row.body ? JSON.stringify(row.body) : null, {
      headers,
      redirects: 0,
      tags: { name: `check: ${row.name}` },
      responseCallback: http.expectedStatuses({ min: 100, max: 599 }),
    });
    const expected = Array.isArray(row.expect) ? row.expect : [row.expect || 200];
    const ok = check(res, {
      [`${row.package}: ${row.name}`]: (r) =>
        expected.includes(r.status) && (!row.json_path || json(r, row.json_path) !== undefined),
    });
    if (!ok) {
      console.error(`FAIL ${row.name}: ${row.method || 'GET'} ${row.path} -> ${res.status} ${String(res.body).slice(0, 300)}`);
    }
  }
}

export const handleSummary = summaryFor('checks');
