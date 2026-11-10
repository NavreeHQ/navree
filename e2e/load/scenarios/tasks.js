// navree-orm + navree-workspace: CRUD on a workspace-scoped model
// (`/api/v1/tasks`), with route-model binding and access checks.

import { check } from 'k6';
import { signInPool, sessionFor } from '../lib/auth.js';
import { WORKSPACE } from '../lib/config.js';
import { del, get, json, patch, pick, post } from '../lib/http.js';
import { options as profileOptions } from '../lib/profiles.js';
import { summaryFor } from '../lib/summary.js';

/** The workspace's project ids (one page), to attach tasks to. */
export function projectIds(session) {
  const res = get(session, '/v1/projects?per_page=50', 'GET /api/v1/projects');
  check(res, { 'projects: 200': (r) => r.status === 200 });
  return (json(res, 'data') || []).map((p) => p.id);
}

/** Create, read, update, list and delete one task. */
export function crudStep(session, projects) {
  const created = post(session, '/v1/tasks', 'POST /api/v1/tasks', {
    project_id: pick(projects),
    title: `Load task ${__VU}-${__ITER}`,
    priority: 1 + (__ITER % 4),
  });
  if (!check(created, { 'task create: 201': (r) => r.status === 201 })) return;
  const id = json(created, 'id');

  const shown = get(session, `/v1/tasks/${id}`, 'GET /api/v1/tasks/{id}');
  check(shown, { 'task show: 200': (r) => r.status === 200 && json(r, 'id') === id });

  const updated = patch(session, `/v1/tasks/${id}`, 'PATCH /api/v1/tasks/{id}', { status: 'doing' });
  check(updated, { 'task update: 200': (r) => r.status === 200 && json(r, 'status') === 'doing' });

  const listed = get(session, '/v1/tasks?status=doing&per_page=20', 'GET /api/v1/tasks');
  check(listed, { 'task list: 200': (r) => r.status === 200 });

  const deleted = del(session, `/v1/tasks/${id}`, 'DELETE /api/v1/tasks/{id}');
  check(deleted, { 'task delete: 204': (r) => r.status === 204 });
}

export const options = profileOptions();

export function setup() {
  const pool = signInPool(Number(__ENV.POOL || 20)).map((s) => ({ token: s.token, workspace: WORKSPACE }));
  return { pool, projects: projectIds(pool[0]) };
}

export default function (data) {
  crudStep(sessionFor(data.pool), data.projects);
}

export const handleSummary = summaryFor('tasks');
