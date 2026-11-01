// What people do with Lighthouse's issues, through its JSON API (the same
// calls the dashboard's buttons make): resolve some issues, resolve one in
// the next release, ignore a few for good or for a while, reopen one and
// leave a comment. Run it once the `ui` traffic has filled the issue list,
// and run `ui` again afterwards: a resolved issue that happens again
// reopens as a regression.
//
//   k6 run -e STAGE=triage scenarios/lighthouse-lifecycle.js

import http from 'k6/http';
import { check, fail } from 'k6';
import { BASE_URL, OPERATOR_EMAIL } from '../lib/config.js';
import { signIn } from '../lib/auth.js';

const LH = `${BASE_URL}/_lighthouse/api`;

export const options = { vus: 1, iterations: 1, thresholds: { checks: ['rate>0.95'] } };

const headers = (token) => ({ 'Content-Type': 'application/json', Accept: 'application/json', Authorization: `Bearer ${token}` });

function issues(token, query) {
  const res = http.get(`${LH}/issues?${query}`, { headers: headers(token), tags: { name: 'GET /_lighthouse/api/issues' } });
  if (!check(res, { 'issues: 200': (r) => r.status === 200 })) fail(`issues: ${res.status} ${String(res.body).slice(0, 200)}`);
  return res.json('data') || [];
}

function act(token, id, action, body) {
  const res = http.post(`${LH}/issues/${id}/${action}`, JSON.stringify(body || {}), {
    headers: headers(token), tags: { name: `POST /_lighthouse/api/issues/{id}/${action}` },
  });
  check(res, { [`${action}: 2xx`]: (r) => r.status >= 200 && r.status < 300 });
  return res;
}

const has = (issue, text) => `${issue.title} ${issue.culprit}`.toLowerCase().includes(text);

export default function () {
  const token = signIn(OPERATOR_EMAIL).token;
  const open = issues(token, 'status=open&per_page=100');
  console.log(`open issues: ${open.length}`);
  const stage = __ENV.STAGE || 'triage';
  if (stage === 'triage') {
    // Resolved: these come back in the next traffic round as regressions.
    for (const issue of open.filter((i) => has(i, 'probability') || has(i, 'duplicate'))) {
      act(token, issue.id, 'resolve', { comment: 'Fixed in the pricing change.' });
    }
    // Resolved in the next release.
    for (const issue of open.filter((i) => has(i, 'template')).slice(0, 1)) {
      act(token, issue.id, 'resolve', { in_next_release: true, comment: 'Template ships with the next release.' });
    }
    // Ignored: for good, for a while, and for a number of occurrences.
    const ignored = open.filter((i) => has(i, 'currency') || has(i, 'ledger') || has(i, 'exchange'));
    ignored.forEach((issue, n) => {
      const body = n === 0 ? { comment: 'Known: the upstream is being replaced.' }
        : n === 1 ? { until: new Date(Date.now() + 6 * 3600 * 1000).toISOString(), comment: 'Quiet until the afternoon.' }
        : { occurrences: 50 };
      act(token, issue.id, 'ignore', body);
    });
  }
  if (stage === 'settle') {
    // After the load: more issues are resolved or ignored, so that the lists
    // for those statuses have entries while nothing is happening any more.
    const pick = (issues, texts, n) => issues.filter((i) => texts.some((t) => has(i, t))).slice(0, n);
    for (const issue of pick(open, ['quote reminder', 'contacts.export', 'activities-by-deal', 'slow-request/export'], 4)) {
      act(token, issue.id, 'resolve', { comment: 'Fixed in the release that is going out today.' });
    }
    for (const issue of pick(open, ['mail.relay', 'webhooks.redeliver'], 1)) {
      act(token, issue.id, 'resolve', { in_next_release: true, comment: 'Waiting for the next release.' });
    }
    for (const issue of pick(open, ['payments service answered', '127.0.0.1:9'], 2)) {
      act(token, issue.id, 'ignore', { comment: 'The provider is aware of it.' });
    }
  }
  if (stage === 'reopen') {
    const resolved = issues(token, 'status=resolved&per_page=5');
    if (resolved.length) act(token, resolved[resolved.length - 1].id, 'reopen');
  }
}
