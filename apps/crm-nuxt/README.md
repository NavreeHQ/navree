# Navree CRM on Nuxt

The Navree CRM front end on Nuxt 4: Vue 3, TypeScript (strict), Tailwind CSS 4, shadcn-vue (Reka UI) restyled to
the navree.com tokens, Pinia, TanStack Query for Vue, VeeValidate with zod, and pusher-js. It is the same CRM as
`apps/crm-web`, screen for screen, against the same API (`apps/crm-api`).

Nitro is the backend-for-frontend. The browser calls only this app's own `/api/*` and `/broadcasting/auth`; Nitro
keeps Navree's access token and rotating refresh token in httpOnly cookies and forwards each call with
`Authorization: Bearer`. The browser never sees a token.

```
browser ── same-origin /api/*, /broadcasting/auth ──▶ Nitro BFF (server/) ── Bearer ──▶ Navree API (crm-api)
   └──────────── ws://127.0.0.1:8092/app/{key} (pusher-js) ───────────────────────────▶ Navree real time
```

## Run

```bash
pnpm install

# 1. The API: crm-api on http://127.0.0.1:8092, on its own SQLite file
#    target/builds/crm-nuxt/crm.db (seeded with crm-demo the first time).
pnpm api                # serve (seeds a new database first)
pnpm api:fresh          # migrate:fresh + db:seed crm-demo, then serve
node scripts/start-api.mjs --worker   # also run queue:work (deal notifications)

# 2. The app on http://localhost:3102 (another terminal)
pnpm dev
NUXT_PUBLIC_DEMO_ACCOUNTS=1 pnpm dev  # one-click demo accounts on the login page

# Or with no API at all: MSW serves the API in the browser with demo data.
pnpm dev:mock
```

Sign in as `rep1@navree.test`, `rep2@`, `manager@` or `admin@navree.test`; every account uses `password123`.

`scripts/start-api.mjs` runs the crm-api binary from `$CRM_API_BIN`, else `target/builds/crm-nuxt/crm-api(.exe)`,
else a copy of `target/builds/patterns/debug/crm-api(.exe)`. Build one with
`CARGO_TARGET_DIR=target/builds/crm-nuxt cargo build -p crm-api` and point `CRM_API_BIN` at it. It never touches
`apps/crm-api`'s own database or `.env`: it sets `APP_DATABASE__URL`, `APP_HTTP__ADDR`, `APP_APP__URL`, an `APP_KEY`
kept in `target/builds/crm-nuxt/app-key.txt`, `APP_REALTIME__ALLOWED_ORIGINS` (this app's origin, for the socket) and
a lifted per-IP rate limit (every browser reaches the API from 127.0.0.1 through the BFF). Change the port with
`CRM_API_PORT` and the web origin with `CRM_WEB_ORIGIN`.

### Production build

```bash
pnpm build
NAVREE_API_URL=http://127.0.0.1:8092 PORT=3102 node .output/server/index.mjs
```

The server reads its settings from the environment at start (it does not load `.env`). Every setting, with its
default, is in [`.env.example`](.env.example):

| Variable | Default | Meaning |
|---|---|---|
| `NAVREE_API_URL` (or `NUXT_NAVREE_API_URL`) | `http://127.0.0.1:8092` | Where the BFF forwards |
| `NAVREE_COOKIE_SECURE` | on in production | `0` to serve a production build over plain http |
| `APP_ORIGINS` | none | Extra origins allowed to send non-GET requests (a proxy that rewrites `Host`) |
| `NAVREE_TRUSTED_PROXY_HOPS` | `0` | Reverse proxies in front of Nuxt that append the client to `X-Forwarded-For` |
| `NUXT_PUBLIC_API_MOCKS` | `0` | `1` serves the API from MSW in the browser |
| `NUXT_PUBLIC_REALTIME` | `1` | `0` turns the socket off |
| `NUXT_PUBLIC_PUSHER_KEY` | `navree-crm-key` | The API's `realtime.key` |
| `NUXT_PUBLIC_WS_HOST` / `NUXT_PUBLIC_WS_PORT` | `127.0.0.1` / `8092` | The API's WebSocket listener |
| `NUXT_PUBLIC_DEMO_ACCOUNTS` | `0` | Lists the demo accounts on the login page |

## Check

```bash
pnpm typecheck     # vue-tsc, strict
pnpm test          # Vitest: units, the BFF and its Nitro handlers, the login page in a real Nuxt app
pnpm e2e           # Playwright against the real API (below)
pnpm screenshots   # every screen at 1440 and 390 px into ./screenshots (git-ignored)
pnpm gen:api       # regenerate app/lib/api/schema.d.ts from openapi.json (runs `navree openapi:client`, so `navree` is on PATH)
```

`pnpm e2e` needs nothing listening on :8092 or :3102. Playwright starts:

1. crm-api (`node scripts/start-api.mjs --fresh --db e2e`): `migrate:fresh`, `db:seed crm-demo` and `serve` on
   http://127.0.0.1:8092, on its own database `target/builds/crm-nuxt/e2e.db` with a fresh `APP_KEY` per run;
2. this app's production build (`pnpm build`, then `node .output/server/index.mjs`) on :3102 with
   `NAVREE_COOKIE_SECURE=0`.

The specs run one at a time against that shared database in a UTC browser (the API's "today" is UTC).
`E2E_BASE_URL=http://localhost:3102 pnpm e2e` runs them against servers you started yourself (`pnpm api:fresh`,
`pnpm dev`).

| Spec | What it proves |
|---|---|
| `login` | A wrong password is refused with no session; signing in sets only httpOnly cookies |
| `smoke` | The server redirects signed-out visitors to `/login?next=`; every screen renders; Ctrl K searches the API; `g d` |
| `bff` | Cross-origin mutations get 403 (CSRF); an expired access token is refreshed and the refresh cookie rotates; sign-out revokes the refresh token at the API |
| `shell` | The breadcrumb; Ctrl B folds the sidebar to icons and the choice survives a reload; the menus; the phone sheet |
| `deals` | Create a deal, drag it between stages, dropping on Lost asks for a reason; a rep can't edit another rep's deal (no control, and a 403) |
| `keyboard` | A deal moves between stages with the keyboard alone |
| `realtime` | A deal moved in one browser moves on another person's board without a reload (private channel authorized through the BFF) |
| `companies`, `tasks`, `admin` | Create a company and a contact; add and complete a task due today; add a user with a role and find it in the audit log |

## How it talks to the API

`server/lib/bff.ts` is the BFF, framework-free (web `Request` in, `Response` out) and unit-tested;
`server/lib/instance.ts` creates it from the environment. The Nitro routes are one line each:

| Route | Does |
|---|---|
| `server/api/auth/login.post.ts` | Signs in at Navree, sets `navree_at` and `navree_rt`, returns only `{user}` (or `{two_factor_required, expires_in}`) |
| `server/api/auth/2fa/verify.post.ts` | Completes a two-factor sign-in with `{code}` or `{recovery_code}`; the challenge waits in `navree_2fa` (`Path=/api/auth/2fa`) |
| `server/api/auth/refresh.post.ts` | Rotates the cookies (204) or ends the session (401) |
| `server/api/auth/logout.post.ts` | Revokes the refresh token at Navree (`{all: true}`: every device) and clears the cookies |
| `server/api/[...path].ts` | Every other `/api/*` call, every method |
| `server/routes/broadcasting/auth.post.ts` | Signs private-channel subscriptions for pusher-js |
| `server/middleware/session-gate.ts` | A page request with neither cookie gets a 302 to `/login?next=...` before the app loads |

- Cookies are `HttpOnly; SameSite=Lax; Path=/`, `Secure` in production. A token pair in any `/api/auth/*` response
  (a password change) replaces the cookies and is stripped from the body.
- On a 401, or when only the refresh cookie is left, the BFF refreshes once and retries. Refresh is single-flight per
  refresh token (Navree rotates refresh tokens and revokes the family on reuse); a finished refresh is remembered for
  30 s for requests still carrying the old cookie. A failed refresh clears both cookies.
- Every non-GET request whose `Origin` is not this app's host (or in `APP_ORIGINS`) gets 403 before reaching Navree.
- The browser's `X-Forwarded-For` never reaches Navree; behind N trusted proxies set `NAVREE_TRUSTED_PROXY_HOPS=N`.

In the browser, `app/lib/api/client.ts` is an openapi-fetch client typed from `openapi.json` (generated into
`schema.d.ts` by `navree openapi:client`), same-origin, with hooks for 401 (back to `/login?next=`) and 403 (a toast).
`unwrap()` turns Navree's error body (`{statusCode, message, error, errors?}`) into an `ApiError`; forms put 400
`errors` next to their fields.

## Design

Dark only, on the navree.com tokens (`apps/website/DESIGN.md`), following its app UI rules (§9):

- **Shell**: the shadcn `sidebar-07` layout. A 16rem sidebar on ink-1 with the app switcher on top, sentence-case
  groups, 32px items with lucide icons (ink-2 hover, ink-3 for the current page), Settings' sections as sub-items on
  a guide line, and the account menu at the foot. Ctrl/Cmd B (or `[`) folds it to a 3rem icon rail, kept per device;
  on phones it is an off-canvas sheet. A 48px top bar holds the sidebar trigger, the breadcrumb (the record's name on
  detail screens; a back arrow on phones), search (Ctrl/Cmd K), the live status, Create and notifications.
- **Corners**: 4px on controls, 6px on surfaces; the radius scale in `app/assets/css/main.css` collapses onto those
  two. No pills, no glows; `rounded-full` only for status dots.
- **Color**: green for the primary action, focus and positive data (won amounts). Navigation, selection, filters and
  hover use the ink steps.
- **Density**: 14px body, 13px secondary, 40px table rows with 12px cell padding, 24px page padding, 24px semibold
  page titles. Mona Sans only for big figures; Geist for the UI; Monaspace Neon for ids and shortcuts.
- **Command palette** (Ctrl/Cmd K or `/`): go to a screen, create a record, or search companies, contacts and deals
  through the API. `?` lists every shortcut; `g` then a letter goes somewhere, `c` then a letter creates something.
- Every screen has loading skeletons, empty and error states, and works at 390 px without horizontal page scroll.

## Layout

- `server/`: the BFF (above).
- `app/lib/api/`: `schema.d.ts` (generated), `types.ts` (aliases), `client.ts`, `errors.ts`, `form-errors.ts`,
  `keys.ts` (query keys).
- `app/lib/casl/`: one CASL ability for the app, rules from `GET /api/me/abilities`, applied before any screen renders
  (`app/middleware/session.global.ts`).
- `app/lib/realtime/`: pusher-js wrapper (`pusher.ts`), the mock bus for `dev:mock`, event payloads.
  `app/composables/useRealtime.ts` listens on `private-crm.deals` and `private-users.{id}`.
- `app/lib/forms/zod-schema.ts`: zod 4 schemas as VeeValidate typed schemas.
- `app/stores/ui.ts` (Pinia): sidebar, palette, dialogs, live status, the record in the breadcrumb.
- `app/features/*`: TanStack Query hooks and feature components per resource.
- `app/components/ui/*`: shadcn-vue primitives restyled to the tokens; `app/components/shell/*`: the app shell;
  `app/components/crm/*`: shared CRM pieces.
- `app/pages/*`: the screens; `app/mocks/*`: the MSW mock API.
- `tests/unit/*`, `tests/nuxt/*`: Vitest; `e2e/*`: Playwright.
