// Small request helpers: every request is tagged with a stable `name` (the
// route template), so per-route latencies group in the summary instead of
// one series per id.

import http from 'k6/http';
import { API } from './config.js';
import { headers } from './auth.js';

const body = (data) => (data === undefined ? null : JSON.stringify(data));

/** A request to `${API}${path}` as `session`, tagged `name`. */
export function api(session, method, path, name, data) {
  return http.request(method, `${API}${path}`, body(data), {
    headers: headers(session.token, session.workspace),
    tags: { name },
  });
}

export const get = (session, path, name) => api(session, 'GET', path, name);
export const post = (session, path, name, data) => api(session, 'POST', path, name, data);
export const patch = (session, path, name, data) => api(session, 'PATCH', path, name, data);
export const del = (session, path, name) => api(session, 'DELETE', path, name);

/** `res.json(selector)`, or undefined when the body is not JSON. */
export function json(res, selector) {
  try {
    return res.json(selector);
  } catch (_) {
    return undefined;
  }
}

/** A random element of `items`. */
export const pick = (items) => items[Math.floor(Math.random() * items.length)];
