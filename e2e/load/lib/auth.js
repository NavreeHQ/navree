// Signing in and the headers every authenticated request sends.

import http from 'k6/http';
import { check, fail } from 'k6';
import { API, LOAD_USERS, PASSWORD, WORKSPACE } from './config.js';

/** The seeded load account number `n` (1-based, wraps around). */
export function loadUser(n) {
  const index = ((n - 1) % LOAD_USERS) + 1;
  return `load-${String(index).padStart(3, '0')}@evidence.test`;
}

/**
 * Signs in with `POST /api/auth/login` and returns `{ token, user }`.
 * Fails the iteration (not the run) when the sign-in is refused.
 */
export function signIn(email, password = PASSWORD) {
  const res = http.post(`${API}/auth/login`, JSON.stringify({ email, password }), {
    headers: { 'Content-Type': 'application/json', Accept: 'application/json' },
    tags: { name: 'POST /api/auth/login' },
  });
  const ok = check(res, {
    'sign-in: 200': (r) => r.status === 200,
    'sign-in: access token': (r) => !!(r.json('access_token')),
  });
  if (!ok) {
    fail(`sign-in failed for ${email}: ${res.status} ${String(res.body).slice(0, 200)}`);
  }
  return { token: res.json('access_token'), user: res.json('user') };
}

/** JSON headers with the bearer token and the workspace. */
export function headers(token, workspace = WORKSPACE) {
  return {
    'Content-Type': 'application/json',
    Accept: 'application/json',
    Authorization: `Bearer ${token}`,
    'X-Workspace': workspace,
  };
}

/**
 * Signs in `count` load accounts once, for `setup()`: virtual user `n`
 * then uses `sessions[(n - 1) % count]`, so a run measures the API, not
 * Argon2id on every iteration (the sign-in scenario measures that).
 */
export function signInPool(count) {
  const sessions = [];
  for (let n = 1; n <= count; n++) {
    sessions.push(signIn(loadUser(n)));
  }
  return sessions;
}

/** This virtual user's session from a pool made by `signInPool`. */
export function sessionFor(pool) {
  return pool[(__VU - 1) % pool.length];
}
