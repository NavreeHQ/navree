// Run-wide settings, from the environment k6 runs with (`-e KEY=value` or
// the container's environment; e2e/run.sh sets them).

/** The app's base URL, without a trailing slash. */
export const BASE_URL = (__ENV.BASE_URL || 'http://localhost:47080').replace(/\/$/, '');

/** The API prefix (`http.api_prefix`). */
export const API = `${BASE_URL}/api`;

/** The profile: `smoke`, `load` or `spike` (see profiles.js). */
export const PROFILE = __ENV.PROFILE || 'smoke';

/** The seeded workspace the load runs in (a slug or an id). */
export const WORKSPACE = __ENV.WORKSPACE || 'acme';

/** The password of every seeded account (apps/evidence-app/src/seed.rs). */
export const PASSWORD = __ENV.PASSWORD || 'evidence-password';

/** How many seeded load accounts exist (`load-001@evidence.test` and up). */
export const LOAD_USERS = Number(__ENV.LOAD_USERS || 200);

/** The operator account (a super admin). */
export const OPERATOR_EMAIL = __ENV.OPERATOR_EMAIL || 'operator@evidence.test';

/** The realtime application key (`realtime.key`). */
export const REALTIME_KEY = __ENV.REALTIME_KEY || 'evidence';

/** Where handleSummary writes its files (a directory; mounted at /out/k6). */
export const SUMMARY_DIR = __ENV.K6_SUMMARY_DIR || '';
