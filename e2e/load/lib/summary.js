// handleSummary for every scenario: the full k6 summary as JSON, a compact
// digest (what summary.json of a run reads) and the text report on stdout.

import { textSummary } from './text.js';
import { PROFILE, SUMMARY_DIR } from './config.js';

const metric = (data, name) => (data.metrics[name] ? data.metrics[name].values : undefined);

/** The numbers a run reports, from k6's end-of-test data. */
export function digest(scenario, data) {
  const duration = metric(data, 'http_req_duration') || {};
  const failed = metric(data, 'http_req_failed') || {};
  const reqs = metric(data, 'http_reqs') || {};
  const checks = metric(data, 'checks') || {};
  const thresholds = {};
  let passed = true;
  for (const [name, m] of Object.entries(data.metrics)) {
    if (!m.thresholds) continue;
    for (const [expr, result] of Object.entries(m.thresholds)) {
      thresholds[`${name}: ${expr}`] = result.ok;
      if (!result.ok) passed = false;
    }
  }
  const perRequest = {};
  for (const [name, m] of Object.entries(data.metrics)) {
    const match = /^http_req_duration\{name:(.+)\}$/.exec(name);
    if (match) perRequest[match[1]] = m.values;
  }
  return {
    scenario,
    profile: PROFILE,
    passed,
    duration_ms: data.state ? data.state.testRunDurationMs : undefined,
    requests: reqs.count,
    rps: reqs.rate,
    error_rate: failed.rate,
    checks_rate: checks.rate,
    latency_ms: {
      avg: duration.avg,
      med: duration.med,
      p90: duration['p(90)'],
      p95: duration['p(95)'],
      p99: duration['p(99)'],
      max: duration.max,
    },
    vus_max: (metric(data, 'vus_max') || {}).value,
    iterations: (metric(data, 'iterations') || {}).count,
    thresholds,
    per_request: perRequest,
  };
}

/** `export const handleSummary = summaryFor('baseline');` in a scenario. */
export function summaryFor(scenario) {
  // SUMMARY_NAME names the files of a run that is not the scenario's only one.
  const name = __ENV.SUMMARY_NAME || scenario;
  return (data) => {
    const out = { stdout: textSummary(data) };
    if (SUMMARY_DIR) {
      const base = `${SUMMARY_DIR}/${name}-${PROFILE}`;
      out[`${base}.json`] = JSON.stringify(data, null, 2);
      out[`${base}.digest.json`] = JSON.stringify(digest(name, data), null, 2);
      out[`${base}.txt`] = textSummary(data);
    }
    return out;
  };
}
