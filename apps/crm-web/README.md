# Navree CRM web

The Next.js 16 front end for Navree CRM (`apps/crm-api`). It talks to the API contract in
`docs/superpowers/specs/2026-10-25-keel-framework-design.md` §7 and runs standalone against an in-browser mock of it.

## Run

```bash
pnpm install

# With the mock API (no crm-api needed). Sign in as rep1@navree.test / password123
# (also rep2@, manager@, admin@navree.test). The login page has one-click demo accounts.
pnpm dev:mock

# Against the real API (default): crm-api on http://127.0.0.1:8080
pnpm dev
NAVREE_API_URL=http://127.0.0.1:9000 pnpm dev   # API elsewhere
```

The whole system locally (each long-running command in its own terminal):

```bash
cd apps/crm-api
echo "APP_KEY=base64:$(openssl rand -base64 32)" > .env   # once: tokens survive API restarts
cargo run -p crm-api -- migrate:fresh --seed   # schema + demo data (storage/crm.db)
cargo run -p crm-api -- serve                  # http://127.0.0.1:8080 (REST + Pusher WebSocket)
cargo run -p crm-api -- queue:work             # optional: deal notifications and mail
cargo run -p crm-api -- schedule:work          # optional: 08:00 UTC task reminders, audit pruning

cd apps/crm-web
pnpm install
pnpm dev                                       # http://localhost:3000, sign in as rep1@navree.test / password123
```

### How it talks to the API

The Next.js server is a backend-for-frontend (spec §7, §10). The browser only calls same-origin `/api/*` and
`/broadcasting/auth`; route handlers in `src/app/api` and `src/app/broadcasting` forward them to `NAVREE_API_URL` with
`Authorization: Bearer <access token>`. Tokens never reach the browser:

- `POST /api/auth/login` stores Navree's access and refresh tokens in httpOnly cookies (`navree_at`, `navree_rt`;
  `SameSite=Lax`, `Path=/`, `Secure` in production) and returns only `{user}`.
- Accounts with two-factor authentication: Navree answers the login with a challenge instead. The BFF keeps the
  challenge token in its own httpOnly cookie (`navree_2fa`, `Path=/api/auth/2fa`, as long as the challenge lives) and
  returns `{two_factor_required: true, expires_in}`; the login page then asks for the authenticator code (or a
  recovery code) and posts `{code}` / `{recovery_code}` to `POST /api/auth/2fa/verify`, where the BFF adds the
  challenge token, stores the tokens like login and returns `{user}`.
- A token pair in any other `/api/auth/*` response (e.g. `PATCH /api/auth/me/password`) replaces the cookies and is
  stripped from the body.
- On a 401 from Navree the BFF refreshes once (single-flight per refresh token, because refresh tokens rotate and reuse
  revokes the family), updates both cookies and retries; if the refresh fails it clears the cookies and the browser goes
  to `/login`.
- `POST /api/auth/logout` revokes the refresh token at Navree and clears the cookies.
- Non-GET requests whose `Origin` isn't this app get 403 (CSRF defence for the cookie hop).
- Responses (status, body, content type, including `text/csv`) are streamed through.
- Client address: the browser's own `X-Forwarded-For` and `X-Real-IP` never reach Navree (Next.js keeps a client's
  `X-Forwarded-For` as is, so it is the browser's word). By default Navree therefore sees every web user at the BFF's
  address, and its login throttle and per-IP rate limit are shared by all of them. In production, put Next.js behind
  a reverse proxy that appends the client to `X-Forwarded-For`, make Next.js reachable only through it, and set
  `NAVREE_TRUSTED_PROXY_HOPS` to the number of such proxies: the BFF then sends Navree a single
  `X-Forwarded-For: <client>` (the entry the outermost proxy added). crm-api believes it only from peers listed in
  `APP_HTTP__TRUSTED_PROXIES` (default `127.0.0.1,::1`, enough for a BFF on the same host); in production, list the
  BFF's address there.

Errors follow the NestJS format `{statusCode, message, error}`; 400 validation errors carry `errors: {field: [..]}`,
which forms show next to the fields. See `.env.example` for every setting. Mock data lives in the tab's
sessionStorage; run `__navreeMock.reset()` in the console (or open a new tab) to start over. In mock mode MSW answers
`/api/*` in the browser with the same BFF semantics.

## Check

```bash
pnpm lint        # ESLint (next/core-web-vitals + TypeScript)
pnpm typecheck   # tsc --noEmit
pnpm test        # Vitest: BFF route handlers, API client + error mapping, CASL, formatting, audit actions, mock API
pnpm build
pnpm e2e         # Playwright smoke test; starts `next dev` with the mock API on :3000
E2E_REAL_API=1 pnpm e2e   # the whole system: real crm-api + production build, every spec
```

`E2E_REAL_API=1 pnpm e2e` needs Rust (cargo) and nothing listening on :8080 or :3000. Playwright then starts:

1. crm-api (`e2e/start-api.mjs`, from `apps/crm-api`): `cargo run -p crm-api -- migrate:fresh`, `db:seed crm-demo`,
   then `serve` on http://127.0.0.1:8080, with `CARGO_TARGET_DIR=target/agents/crm-e2e` (repo root), its own database
   `apps/crm-api/storage/e2e.db` (the dev database `crm.db` is untouched), a fresh `APP_KEY` per run and the per-IP
   rate limit lifted (every browser reaches the API from 127.0.0.1 through the BFF). The first run compiles the API.
2. this app: `pnpm build && pnpm start` on :3000 with `NAVREE_API_URL=http://127.0.0.1:8080`, `NAVREE_COOKIE_SECURE=0`.

The specs (`e2e/*.spec.ts`) run one at a time against that shared database, in a UTC browser (the API's "today" is
UTC). `E2E_BASE_URL=http://localhost:3000 E2E_REAL_API=1 pnpm e2e` runs them against servers you started yourself
(seeded with `crm-demo`).

## Layout

- `src/lib/bff/`: the BFF (`bff.ts`, framework-free and unit-tested) and its instance (`server.ts`).
- `src/lib/api/`: `schema.d.ts` (generated by `pnpm gen:api` from `openapi.json`, which crm-api writes with
  `cargo run -p crm-api -- openapi:export ../crm-web/openapi.json`), `types.ts` (the aliases the app imports, plus the
  list endpoints' query parameters, which navree-api cannot document yet), `client.ts` (openapi-fetch, same-origin,
  401/403 hooks), `errors.ts` (NestJS error bodies), `form-errors.ts` (400 `errors` to form fields).
- `src/lib/casl/`: one `createMongoAbility` for the app, rules from `GET /api/me/abilities`, `__type` subject detection.
- `src/lib/realtime/`: Laravel Echo + pusher-js (`private-crm.deals`, `private-users.{id}`), optional; channel auth
  goes through the BFF's `/broadcasting/auth`.
- `src/features/*`: TanStack Query hooks and feature components per resource.
- `src/app/(app)/*`: screens behind sign-in; `src/app/login`.
- `src/mocks/`: MSW handlers, seed data and CASL rules mirroring crm-api.
