// The states that matter besides the walk: signed out, a wrong password,
// signed in without the ability (403), signed in with read-only abilities,
// a page that does not exist, and the interactions listed per tool in
// `custom` (filters, forms with errors, actions, a resolved issue ...).
//
// Each state opens its own browser context, so no cookie leaks between
// them, and yields one record like the walk's pages (`kind: "state"`).

import path from 'node:path';
import {
  BASE_URL, PASSWORD, collect, contextOptions, fileFor, shoot, signInIfAsked,
} from './lib.mjs';

const SUPPORT = process.env.SUPPORT_EMAIL || 'support@evidence.test';
const VIEWER = process.env.VIEWER_EMAIL || 'ops-viewer@evidence.test';
const OPERATOR = process.env.EMAIL || 'operator@evidence.test';

/** Runs `fn(page, bag)` in a fresh context, signed in as `email` (or signed out when null). */
async function withSession(browser, width, email, fn) {
  const context = await browser.newContext(contextOptions(width));
  const page = await context.newPage();
  const bag = collect(page);
  try {
    if (email) {
      await page.goto(BASE_URL + '/', { waitUntil: 'load' }).catch(() => {});
    }
    return await fn(page, bag, context);
  } finally {
    await context.close();
  }
}

function describe(rec, tool, width, phase, state, expectedStatus) {
  rec.kind = 'state';
  rec.state = state;
  rec.tool = tool.name;
  rec.width = width;
  rec.phase = phase;
  // A state records what the page does; a 401, 403 or 404 is what some of
  // them are meant to show.
  if (expectedStatus && rec.status === expectedStatus) {
    rec.ok = true;
    rec.expected_status = expectedStatus;
  }
  return rec;
}

async function shotState(page, bag, tool, width, dir, rel, phase, state, url, opts = {}) {
  const file = fileFor(`${state}-${url}`, width, `${tool.name}--state-`);
  const rec = await shoot(page, bag, {
    url: BASE_URL + url, file, outDir: dir, relDir: rel, label: opts.label || state, navigate: opts.navigate !== false,
    settle: opts.settle, noLogin: !!opts.noLogin,
  });
  describe(rec, tool, width, phase, state, opts.expectedStatus);
  console.log(`${rec.ok ? 'ok  ' : 'FAIL'} ${width} ${tool.name} [${state}] ${rec.url} ${rec.status ?? ''} ${rec.error ?? ''}`);
  return rec;
}

/** Fills the sign-in screen of the page with the wrong password. */
async function wrongPassword(page) {
  const field = page.locator('input[type="password"]').first();
  if (!(await field.isVisible().catch(() => false))) return false;
  await page.locator('input[type="email"], input[name="email"], input[type="text"]').first().fill(OPERATOR);
  await field.fill('not-the-password');
  await Promise.all([page.waitForLoadState('load').catch(() => {}), field.press('Enter')]);
  await page.waitForLoadState('networkidle', { timeout: 3000 }).catch(() => {});
  await page.waitForTimeout(600);
  return true;
}

export async function runStates(browser, tool, { width, outDir, relDir, phase }) {
  const out = [];
  const list = tool.states || [];
  const root = tool.roots[0];
  const inner = tool.state_pages || [];

  if (list.includes('signed-out')) {
    out.push(await withSession(browser, width, null, (page, bag) =>
      shotState(page, bag, tool, width, outDir, relDir, phase, 'signed-out', root, { noLogin: true, label: 'Signed out: the tool asks for a sign-in' })));
  }
  if (list.includes('wrong-password')) {
    out.push(await withSession(browser, width, null, async (page, bag) => {
      await page.goto(BASE_URL + root, { waitUntil: 'load' }).catch(() => {});
      await wrongPassword(page);
      return shotState(page, bag, tool, width, outDir, relDir, phase, 'wrong-password', root, { navigate: false, noLogin: true, label: 'Wrong password' });
    }));
  }
  for (const [state, email, label] of [
    ['forbidden', SUPPORT, 'Signed in as a member without the tool ability'],
    ['view-only', VIEWER, 'Signed in with read-only abilities'],
  ]) {
    if (!list.includes(state)) continue;
    const pages = state === 'view-only' ? [root, ...inner] : [root];
    out.push(...await withSession(browser, width, email, async (page, bag) => {
      await page.goto(BASE_URL + root, { waitUntil: 'load' }).catch(() => {});
      await signInIfAsked(page, email, PASSWORD);
      const recs = [];
      for (const p of pages) recs.push(await shotState(page, bag, tool, width, outDir, relDir, phase, `${state}`, p, { label: `${label} (${p})`, expectedStatus: state === 'forbidden' ? 403 : undefined }));
      return recs;
    }));
  }
  if (list.includes('not-found')) {
    out.push(await withSession(browser, width, OPERATOR, async (page, bag) => {
      await page.goto(BASE_URL + root, { waitUntil: 'load' }).catch(() => {});
      await signInIfAsked(page);
      return shotState(page, bag, tool, width, outDir, relDir, phase, 'not-found', tool.not_found || `${root}/does-not-exist`, { label: 'A page that does not exist', expectedStatus: 404 });
    }));
  }
  for (const name of tool.custom || []) {
    const fn = custom[name];
    if (!fn) {
      console.log(`no custom state ${name}`);
      continue;
    }
    try {
      const recs = await fn({ browser, tool, width, outDir, relDir, phase });
      out.push(...[].concat(recs).filter(Boolean));
    } catch (err) {
      console.log(`FAIL ${width} ${tool.name} [${name}] ${String(err).slice(0, 300)}`);
      out.push({ tool: tool.name, phase, width, kind: 'state', state: name, url: root, ok: false, error: String(err).slice(0, 400) });
    }
  }
  return out;
}

/** A session signed in as the operator (or `email`), on the tool's root. */
async function operatorSession(browser, tool, width, email = OPERATOR) {
  const context = await browser.newContext(contextOptions(width));
  const page = await context.newPage();
  const bag = collect(page);
  await page.goto(BASE_URL + tool.roots[0], { waitUntil: 'load' }).catch(() => {});
  await signInIfAsked(page, email, PASSWORD);
  return { context, page, bag };
}

/** Fills the visible fields of a form: text and numbers with a value, selects with their first option. */
async function fillForm(page, values = {}) {
  const fields = await page.locator('form.stack input:not([type=hidden]):not([type=checkbox]):not([type=submit]), form.stack textarea, form.stack select').all();
  for (const field of fields) {
    const name = (await field.getAttribute('name')) || '';
    const tag = await field.evaluate((el) => el.tagName.toLowerCase());
    const type = (await field.getAttribute('type')) || 'text';
    if (!(await field.isVisible().catch(() => false)) || (await field.isDisabled().catch(() => true))) continue;
    if (name in values) {
      await field.fill(String(values[name])).catch(() => {});
    } else if (tag === 'select') {
      const options = await field.locator('option').evaluateAll((os) => os.map((o) => o.value).filter(Boolean));
      if (options.length) await field.selectOption(options[0]).catch(() => {});
    } else if (type === 'number') {
      await field.fill('2').catch(() => {});
    } else if (type === 'date') {
      await field.fill('2026-12-15').catch(() => {});
    } else if (type === 'text' || tag === 'textarea') {
      const current = await field.inputValue().catch(() => '');
      if (!current) await field.fill('Check the evidence run').catch(() => {});
    }
  }
}

const shot = (ctx, state, url, opts) => shotState(ctx.page, ctx.bag, ctx.tool, ctx.width, ctx.outDir, ctx.relDir, ctx.phase, state, url, opts);

/** Runs `body(ctx)` in a session signed in as the operator and returns its records. */
async function session({ browser, tool, width, outDir, relDir, phase }, body, email = OPERATOR) {
  const { context, page, bag } = await operatorSession(browser, tool, width, email);
  const ctx = { page, bag, tool, width, outDir, relDir, phase };
  const out = [];
  try {
    await body(ctx, out);
  } finally {
    await context.close();
  }
  return out;
}

/** Per-tool interactions, by the names listed in tools.json. */
export const custom = {
  /** Lighthouse: resolve an issue with the page's button, then reopen it. */
  'lighthouse-issue-actions': (args) => session(args, async (ctx, out) => {
    const { page } = ctx;
    await page.goto(`${BASE_URL}/_lighthouse/issues?status=open&kind=slow_request`, { waitUntil: 'load' });
    await page.locator('a[href*="/_lighthouse/issues/"]').first().click();
    await page.waitForLoadState('load');
    const url = new URL(page.url()).pathname;
    await page.fill('#comment', 'Fixed by the caching change').catch(() => {});
    await page.click('button[data-action="resolve"]');
    await page.waitForLoadState('load').catch(() => {});
    await page.waitForTimeout(1200);
    out.push(await shot(ctx, 'issue-resolved', url, { navigate: false, label: 'An issue after Resolve' }));
    const reopen = page.locator('button[data-action="reopen"]');
    if (await reopen.isVisible().catch(() => false)) {
      await reopen.click();
      await page.waitForTimeout(1200);
      out.push(await shot(ctx, 'issue-reopened', url, { navigate: false, label: 'The same issue after Reopen' }));
    }
  }),

  /** Logbook follows the system's colour scheme: its list and a request's page with a light system. */
  'logbook-light': async ({ browser, tool, width, outDir, relDir, phase }) => {
    const context = await browser.newContext({ ...contextOptions(width), colorScheme: 'light' });
    const page = await context.newPage();
    const bag = collect(page);
    const ctx = { page, bag, tool, width, outDir, relDir, phase };
    const out = [];
    try {
      out.push(await shot(ctx, 'light-scheme', '/_logbook', { label: 'The list with a light system colour scheme' }));
      const href = await page.locator('a[href*="/_logbook/entries/"]').first().getAttribute('href');
      if (href) out.push(await shot(ctx, 'light-scheme-entry', href, { label: 'An entry with a light system colour scheme' }));
    } finally {
      await context.close();
    }
    return out;
  },

  /** Fleet: a newsletter of 2500 mails lands on the mail queue; Fleet is photographed while it drains. */
  'fleet-burst': (args) => session(args, async (ctx, out) => {
    const { page } = ctx;
    const login = await page.request.post(`${BASE_URL}/api/auth/login`, { data: { email: OPERATOR, password: PASSWORD } });
    const token = (await login.json()).access_token;
    await page.goto(BASE_URL + '/_fleet#overview', { waitUntil: 'load' });
    await page.waitForTimeout(1500);
    const queued = await page.request.post(`${BASE_URL}/api/demo/jobs`, {
      data: { kind: 'newsletter', count: 2500 },
      headers: { Authorization: `Bearer ${token}`, 'X-Workspace': 'acme' },
    });
    if (queued.status() !== 202) throw new Error(`the burst was refused: ${queued.status()}`);
    await page.waitForTimeout(5000);
    out.push(await shot(ctx, 'burst-start', '/_fleet#overview', { navigate: false, label: 'Fleet, 5 s after 2500 mails were queued' }));
    await page.waitForTimeout(10000);
    out.push(await shot(ctx, 'burst-balancing', '/_fleet#overview', { navigate: false, label: 'Fleet, 15 s after: workers moved to the mail queue' }));
    await page.goto(BASE_URL + '/_fleet#supervisors', { waitUntil: 'load' });
    await page.waitForTimeout(1200);
    out.push(await shot(ctx, 'burst-supervisors', '/_fleet#supervisors', { navigate: false, label: 'The supervisors during the burst' }));
    await page.goto(BASE_URL + '/_fleet#jobs', { waitUntil: 'load' });
    await page.waitForTimeout(1200);
    out.push(await shot(ctx, 'burst-jobs', '/_fleet#jobs', { navigate: false, label: 'Pending and completed jobs during the burst' }));
    // Mails are quick: the backlog clears in seconds. Slow jobs (2 to 4 s of
    // work each) hold a backlog long enough to see Fleet add processes; not
    // while k6 is loading the app (its reports run on the same queue).
    if (ctx.phase === 'after') {
      const slow = await page.request.post(`${BASE_URL}/api/demo/jobs`, {
        data: { kind: 'slow', count: 240 },
        headers: { Authorization: `Bearer ${token}`, 'X-Workspace': 'acme' },
      });
      if (slow.status() !== 202) throw new Error(`the slow jobs were refused: ${slow.status()}`);
      await page.goto(BASE_URL + '/_fleet#overview', { waitUntil: 'load' });
      await page.waitForTimeout(20000);
      out.push(await shot(ctx, 'slow-backlog', '/_fleet#overview', { navigate: false, label: 'Fleet 20 s after 240 slow jobs were queued' }));
      await page.waitForTimeout(25000);
      out.push(await shot(ctx, 'slow-backlog-later', '/_fleet#overview', { navigate: false, label: 'Fleet 45 s after: has it added processes?' }));
    }
  }),

  /** Mailpit: the first message that Lighthouse's alerts sent. */
  'mailpit-alert': (args) => session(args, async (ctx, out) => {
    const { page } = ctx;
    await page.goto(BASE_URL + '/', { waitUntil: 'load' });
    await page.waitForTimeout(1500);
    await page.getByText('[Lighthouse]').first().click({ timeout: 5000 });
    await page.waitForTimeout(1500);
    out.push(await shot(ctx, 'alert-email', '/', { navigate: false, label: 'An alert email from Lighthouse, as Mailpit shows it' }));
  }),

  /** Fleet: pause every supervisor, photograph, continue. */
  'fleet-pause': (args) => session(args, async (ctx, out) => {
    const { page } = ctx;
    await page.goto(BASE_URL + '/_fleet#overview', { waitUntil: 'load' });
    await page.waitForTimeout(1500);
    await page.getByRole('button', { name: /pause all/i }).click({ timeout: 4000 });
    await page.waitForTimeout(2500);
    out.push(await shot(ctx, 'paused', '/_fleet#overview', { navigate: false, label: 'Fleet after Pause all' }));
    await page.getByRole('button', { name: /continue all|resume all|continue/i }).first().click({ timeout: 4000 }).catch(() => {});
    await page.waitForTimeout(2000);
    out.push(await shot(ctx, 'continued', '/_fleet#overview', { navigate: false, label: 'Fleet after Continue' }));
  }),

  /** Fleet: a failed job's page, then Retry. */
  'fleet-retry': (args) => session(args, async (ctx, out) => {
    const { page } = ctx;
    await page.goto(BASE_URL + '/_fleet#failed', { waitUntil: 'load' });
    await page.waitForTimeout(1500);
    await page.locator('table tbody tr').first().click({ timeout: 4000 });
    await page.waitForTimeout(1200);
    out.push(await shot(ctx, 'failed-job', page.url().replace(BASE_URL, ''), { navigate: false, label: 'A failed job: attempts and error' }));
    await page.getByRole('button', { name: /retry/i }).first().click({ timeout: 4000 });
    await page.waitForTimeout(1500);
    out.push(await shot(ctx, 'failed-job-retried', page.url().replace(BASE_URL, ''), { navigate: false, label: 'After Retry' }));
  }),

  /** Helm: list states (filtered, empty search) and the command palette. */
  'helm-lists': (args) => session(args, async (ctx, out) => {
    const { page } = ctx;
    out.push(await shot(ctx, 'filtered', '/admin/resources/tasks?filters%5Bstatus%5D=doing&filters%5Boverdue%5D=1&sort=-due_on', { label: 'A filtered and sorted list' }));
    out.push(await shot(ctx, 'no-results', '/admin/resources/tasks?search=zzzzqqqq', { label: 'A search with no result' }));
    out.push(await shot(ctx, 'projects-filtered', '/admin/resources/projects?filters%5Bstatus%5D=archived', { label: 'Archived projects' }));
    await page.goto(BASE_URL + '/admin', { waitUntil: 'load' });
    await page.keyboard.press('Control+K');
    await page.waitForTimeout(600);
    out.push(await shot(ctx, 'command-palette', '/admin', { navigate: false, label: 'The command palette (Ctrl K)' }));
  }),

  /** Helm: a form submitted empty (validation), then filled (created). */
  'helm-forms': (args) => session(args, async (ctx, out) => {
    const { page } = ctx;
    await page.goto(BASE_URL + '/admin/resources/tasks/new', { waitUntil: 'load' });
    await page.waitForTimeout(500);
    out.push(await shot(ctx, 'create-form', '/admin/resources/tasks/new', { navigate: false, label: 'The create form' }));
    await page.getByRole('button', { name: /^(create|save)/i }).first().click();
    await page.waitForLoadState('load').catch(() => {});
    await page.waitForTimeout(600);
    out.push(await shot(ctx, 'validation-errors', new URL(page.url()).pathname, { navigate: false, label: 'The create form submitted empty' }));
    await page.goto(BASE_URL + '/admin/resources/tasks/new', { waitUntil: 'load' });
    await fillForm(page, { title: `Evidence task ${ctx.width}` });
    await page.getByRole('button', { name: /^(create|save)/i }).first().click();
    await page.waitForLoadState('load').catch(() => {});
    await page.waitForTimeout(800);
    out.push(await shot(ctx, 'created', new URL(page.url()).pathname, { navigate: false, label: 'After the create form is filled in and saved' }));
  }),

  /** Helm: rows ticked, a bulk action confirmed. */
  'helm-bulk': (args) => session(args, async (ctx, out) => {
    const { page } = ctx;
    await page.goto(BASE_URL + '/admin/resources/tasks?filters%5Bstatus%5D=todo', { waitUntil: 'load' });
    const boxes = page.locator('tbody input[type="checkbox"]');
    const n = Math.min(await boxes.count(), 3);
    for (let i = 0; i < n; i++) await boxes.nth(i).check({ force: true }).catch(() => {});
    await page.waitForTimeout(400);
    out.push(await shot(ctx, 'rows-ticked', '/admin/resources/tasks?filters[status]=todo', { navigate: false, label: 'Three rows ticked: the bulk actions are live' }));
    page.once('dialog', (d) => d.accept());
    await page.getByRole('button', { name: /mark tasks done/i }).first().click({ timeout: 4000 });
    await page.waitForLoadState('load').catch(() => {});
    await page.waitForTimeout(800);
    out.push(await shot(ctx, 'bulk-action-done', new URL(page.url()).pathname, { navigate: false, label: 'After Mark tasks done was confirmed' }));
  }),

  /** Helm: a read-only account ticks a row and runs a bulk action it has no ability for. */
  'helm-viewer-action': (args) => session(args, async (ctx, out) => {
    const { page } = ctx;
    await page.goto(BASE_URL + '/admin/resources/tasks?filters%5Bstatus%5D=todo', { waitUntil: 'load' });
    await page.locator('tbody input[type="checkbox"]').first().check({ force: true });
    page.once('dialog', (d) => d.accept());
    await page.getByRole('button', { name: /mark tasks done/i }).first().click({ timeout: 4000 });
    await page.waitForLoadState('load').catch(() => {});
    await page.waitForTimeout(800);
    out.push(await shot(ctx, 'viewer-runs-action', new URL(page.url()).pathname, { navigate: false, label: 'A read-only account runs a bulk action' }));
  }, VIEWER),

  /** Helm: a record's edit form with an invalid value. */
  'helm-edit-invalid': (args) => session(args, async (ctx, out) => {
    const { page } = ctx;
    await page.goto(BASE_URL + '/admin/resources/tasks', { waitUntil: 'load' });
    const href = await page.locator('tbody a[href*="/admin/resources/tasks/"]').filter({ hasNotText: /^Load task/ }).first().getAttribute('href');
    await page.goto(`${BASE_URL}${href.replace(/\/$/, '')}/edit`, { waitUntil: 'load' });
    await page.locator('input[name="title"]').fill('');
    await page.getByRole('button', { name: /^(save|update)/i }).first().click();
    await page.waitForLoadState('load').catch(() => {});
    await page.waitForTimeout(600);
    out.push(await shot(ctx, 'edit-invalid', new URL(page.url()).pathname, { navigate: false, label: 'The edit form with the title cleared' }));
  }),

  /** GraphiQL: run a query and show its result. */
  'graphiql-run': (args) => session(args, async (ctx, out) => {
    const { page } = ctx;
    await page.goto(BASE_URL + '/graphql', { waitUntil: 'load' });
    await page.waitForTimeout(2500);
    await page.locator('.graphiql-query-editor .CodeMirror, .graphiql-query-editor .cm-content').first().click();
    await page.keyboard.press('Control+A');
    await page.keyboard.type('{ __schema { queryType { name } types { name } } }');
    await page.locator('.graphiql-execute-button').click();
    await page.waitForTimeout(2500);
    out.push(await shot(ctx, 'query-result', '/graphql', { navigate: false, label: 'A query run in GraphiQL' }));
  }),

  /** The API reference: open an operation. */
  'api-docs-operation': (args) => session(args, async (ctx, out) => {
    const { page } = ctx;
    await page.goto(BASE_URL + '/api/docs', { waitUntil: 'load' });
    await page.locator('nav a[data-target^="op-"]').nth(2).click({ timeout: 4000 }).catch(() => {});
    out.push(await shot(ctx, 'operation', '/api/docs', { navigate: false, label: 'An operation open in the reference' }));
  }),
};

export { path };
