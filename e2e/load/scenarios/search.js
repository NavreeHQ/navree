// navree-search: full-text queries on Meilisearch through
// `GET /api/v1/projects/search`, scoped to the workspace and filtered by
// the access rules.

import { check } from 'k6';
import { signInPool, sessionFor } from '../lib/auth.js';
import { WORKSPACE } from '../lib/config.js';
import { get, json, pick } from '../lib/http.js';
import { options as profileOptions } from '../lib/profiles.js';
import { summaryFor } from '../lib/summary.js';

/** Words the seeded projects contain, a few misspelled. */
const TERMS = ['launch', 'mobile', 'billing', 'warehouse', 'security', 'planning', 'portal', 'retention', 'hiring', 'route', 'onbording', 'secruity'];

/** One search; the seeded data has a hit for every term. */
export function searchStep(session) {
  const q = pick(TERMS);
  const res = get(session, `/v1/projects/search?q=${encodeURIComponent(q)}`, 'GET /api/v1/projects/search');
  check(res, {
    'search: 200': (r) => r.status === 200,
    'search: has hits': (r) => (json(r, 'data') || []).length > 0,
  });
}

export const options = profileOptions();

export function setup() {
  return { pool: signInPool(Number(__ENV.POOL || 20)).map((s) => ({ token: s.token, workspace: WORKSPACE })) };
}

export default function (data) {
  searchStep(sessionFor(data.pool));
}

export const handleSummary = summaryFor('search');
