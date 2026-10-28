// The load profiles every scenario runs under, and their thresholds.
//
//   smoke   1 to 2 virtual users for 30 s: does every request work?
//   load    a ramp to VUS (default 50), held for DURATION (default 3m):
//           sustained throughput and latency
//   spike   a jump to SPIKE_VUS (default 200) for 30 s and back: does it
//           recover, and does nothing fail?
//
// Overrides: -e VUS=80 -e DURATION=5m -e SPIKE_VUS=300. Thresholds can be
// loosened per run with -e P95_MS=.. -e P99_MS=.. -e ERROR_RATE=..

import { PROFILE } from './config.js';

const num = (name, fallback) => Number(__ENV[name] || fallback);

/** The executor stages of a profile. */
export function stages(profile = PROFILE) {
  const vus = num('VUS', 50);
  switch (profile) {
    case 'smoke':
      return [{ duration: __ENV.DURATION || '30s', target: num('VUS', 2) }];
    case 'load':
      return [
        { duration: __ENV.RAMP || '30s', target: vus },
        { duration: __ENV.DURATION || '3m', target: vus },
        { duration: '10s', target: 0 },
      ];
    case 'spike': {
      const spike = num('SPIKE_VUS', 200);
      return [
        { duration: '20s', target: Math.max(1, Math.round(vus / 5)) },
        { duration: '10s', target: spike },
        { duration: __ENV.DURATION || '30s', target: spike },
        { duration: '10s', target: Math.max(1, Math.round(vus / 5)) },
        { duration: '20s', target: Math.max(1, Math.round(vus / 5)) },
        { duration: '5s', target: 0 },
      ];
    }
    default:
      throw new Error(`unknown PROFILE "${profile}": smoke, load or spike`);
  }
}

/** The default thresholds of a profile: latency percentiles and errors. */
export function thresholds(profile = PROFILE) {
  const p95 = num('P95_MS', profile === 'spike' ? 1500 : 500);
  const p99 = num('P99_MS', profile === 'spike' ? 3000 : 1500);
  const errors = num('ERROR_RATE', 0.01);
  return {
    http_req_failed: [`rate<${errors}`],
    http_req_duration: [`p(95)<${p95}`, `p(99)<${p99}`],
    checks: [`rate>${1 - errors}`],
  };
}

/**
 * k6 `options` for a scenario: one ramping-VUs scenario running `exec`
 * under the profile, the profile's thresholds plus `extra` (per-request
 * thresholds keyed like `'http_req_duration{name:GET /api/v1/tasks}'`).
 */
export function options({ exec = 'default', extra = {}, profile = PROFILE } = {}) {
  return {
    scenarios: {
      [profile]: {
        executor: 'ramping-vus',
        startVUs: 0,
        stages: stages(profile),
        gracefulRampDown: '10s',
        exec,
      },
    },
    thresholds: { ...thresholds(profile), ...extra },
    setupTimeout: '5m',
    summaryTrendStats: ['avg', 'min', 'med', 'p(90)', 'p(95)', 'p(99)', 'max', 'count'],
    discardResponseBodies: false,
    tags: { profile },
  };
}
