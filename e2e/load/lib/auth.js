// Signing in and the headers every authenticated request sends.

import http from 'k6/http';
import { check, fail } from 'k6';
import { API, LOAD_USERS, PASSWORD, WORKSPACE } from './config.js';

/** The seeded load account number `n` (1-based, wraps around). */
export function loadUser(n) {
  const index = ((n - 1) % LOAD_USERS) + 1;
  return `load-${String(index).padStart(3, '0')}@system.test`;
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

/**
 * A token for `email` that is still good: the one given, until it is about to
 * expire, then a fresh sign-in. Access tokens live 15 minutes
 * (`auth.jwt.access_ttl_secs`) and a run can be longer; each virtual user
 * keeps its own copy, made on its first use and replaced after
 * `TOKEN_REFRESH_MS` (10 minutes).
 */
const issued = {};
export function freshToken(email, token) {
  const refreshAfter = Number(__ENV.TOKEN_REFRESH_MS || 10 * 60 * 1000);
  const now = Date.now();
  let entry = issued[email];
  if (!entry) entry = issued[email] = { token, at: now };
  if (now - entry.at > refreshAfter) {
    // The sign-in can be refused (the login throttle): keep the token and try
    // again in 30 seconds, instead of failing every iteration meanwhile.
    const res = http.post(`${API}/auth/login`, JSON.stringify({ email, password: PASSWORD }), {
      headers: { 'Content-Type': 'application/json', Accept: 'application/json' },
      tags: { name: 'POST /api/auth/login (token refresh)' },
    });
    const fresh = res.status === 200 ? res.json('access_token') : null;
    entry = issued[email] = fresh ? { token: fresh, at: now } : { token: entry.token, at: now - refreshAfter + 30 * 1000 };
  }
  return entry.token;
}

/** `session` with a token that is still good for `email`. */
export function liveSession(session, email) {
  return { ...session, token: freshToken(email, session.token) };
}

/** This virtual user's session from a pool made by `signInPool`. */
export function sessionFor(pool) {
  const index = (__VU - 1) % pool.length;
  return liveSession(pool[index], loadUser(index + 1));
}
