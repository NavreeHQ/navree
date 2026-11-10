// navree-auth: sign in (Argon2id verification included) and read the
// signed-in user. Runs alone or as part of baseline.js.

import { check } from 'k6';
import { signIn, loadUser } from '../lib/auth.js';
import { get } from '../lib/http.js';
import { options as profileOptions } from '../lib/profiles.js';
import { summaryFor } from '../lib/summary.js';
import { WORKSPACE } from '../lib/config.js';

/** One sign-in as a random load account, then `GET /api/auth/me`. */
export function signInStep() {
  const n = 1 + Math.floor(Math.random() * 200);
  const email = loadUser(n);
  const { token } = signIn(email);
  const me = get({ token, workspace: WORKSPACE }, '/auth/me', 'GET /api/auth/me');
  check(me, {
    'me: 200': (r) => r.status === 200,
    'me: same user': (r) => r.json('email') === email || r.json('user.email') === email,
  });
}

export const options = profileOptions({
  extra: { 'http_req_duration{name:POST /api/auth/login}': ['p(95)<800'] },
});

export default function () {
  signInStep();
}

export const handleSummary = summaryFor('auth');
