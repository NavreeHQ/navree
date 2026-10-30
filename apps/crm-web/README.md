# Navree CRM web

The Next.js 16 front end for Navree CRM (`apps/crm-api`). It talks to the crm-api REST API (its OpenAPI
document is served at `/api/openapi.json`) and runs standalone against an in-browser mock of it.

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

The Next.js server is a backend-for-frontend. The browser only calls same-origin `/api/*` and
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

Errors use Navree's JSON shape `{statusCode, message, error, code}`; 400 validation errors carry
`errors: {field: [..]}`, which forms show next to the fields. The UI branches on `code` (`invalid_two_factor_code`),
never on the English `message`, and `CODE_MESSAGES` in `src/lib/api/errors.ts` maps codes to the text people see. See `.env.example` for every setting. Mock data lives in the tab's
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
   then `serve` on http://127.0.0.1:8080, with `CARGO_TARGET_DIR=target/builds/crm-e2e` (repo root), its own database
   `apps/crm-api/storage/e2e.db` (the dev database `crm.db` is untouched), a fresh `APP_KEY` per run and the per-IP
   rate limit lifted (every browser reaches the API from 127.0.0.1 through the BFF). The first run compiles the API.
2. this app: `pnpm build && pnpm start` on :3000 with `NAVREE_API_URL=http://127.0.0.1:8080`, `NAVREE_COOKIE_SECURE=0`.

The specs (`e2e/*.spec.ts`) run one at a time against that shared database, in a UTC browser (the API's "today" is
UTC). `E2E_BASE_URL=http://localhost:3000 E2E_REAL_API=1 pnpm e2e` runs them against servers you started yourself
(seeded with `crm-demo`).

## Design

The CRM follows `apps/website/DESIGN.md` section 9, **App UI**: the navree.com tokens (dark only, ink surfaces), laid
out as a calm, dense application. The tokens in `src/app/globals.css` are the site's, mapped onto shadcn/ui's semantic
names (`accent` stays green, so the primitives in `src/components/ui` use `ink-2`/`ink-3` for hover and selection).
Fonts are self-hosted from Fontsource packages through `next/font/local` (`src/app/fonts.ts`): Mona Sans for page
titles and big figures, Geist for the UI, Monaspace Neon for ids and small counts. `cn`
(`src/lib/utils.ts`) knows the custom type scale.

The primitives in `src/components/ui` come from shadcn/ui (`components.json` is set up). Add another one with
`pnpm dlx shadcn@latest add <component>`, then restyle it to the rules below. The app does not depend on the `shadcn`
package (the CLI): the one file it used from it, `shadcn/tailwind.css` (keyframes, custom variants and utilities the
components use), is vendored as `src/app/shadcn-tailwind.css`, with its version and license at the top.

- **The shell** is the shadcn/ui `Sidebar` in the `sidebar-07` layout (`src/components/app-shell`): a workspace
  switcher with the navree mark, the Sales and Workspace groups (32px items, 16px lucide icons, ink-2 hover, ink-3 for
  the current page, no green), the user menu in the footer. It collapses to icons with Ctrl/Cmd B or the rail, remembers
  that in the `sidebar_state` cookie (read by `src/app/(app)/layout.tsx`), and is a sheet on phones.
- **The top bar** is 48px: the sidebar trigger, the breadcrumb (group, screen, record), the search field (Ctrl/Cmd K
  opens the command menu: jump to a screen, create a deal, company, contact or task, sign out, or search companies,
  contacts and deals), notifications, and the screen's actions. Screens pass their "New …", Edit and Export buttons as
  `actions` to `PageHeader`/`RecordHeader`; `PageActions` (`shell-slots.tsx`) renders them in the top bar from `md` up
  and in the page header on phones. Record screens name the record in the breadcrumb.
- **Corners**: 4px on controls (buttons, inputs, menu items, badges, tabs, avatars), 6px on cards, popovers, dialogs
  and toasts. The radius scale in `globals.css` is pinned to those two values (`rounded-md` = 4px, `rounded-xl` = 6px).
  No pills and no glows; `rounded-full` is for status dots only.
- **Green** is for the primary action, focus rings and positive data (won amounts, completed tasks, success toasts).
- **Density**: 14px body, 13px tables and secondary text, 40px table rows with 12px cell padding, 24px page padding.
- Every list hides secondary columns on small screens; the app has no horizontal page scroll at 390 px.
- `pnpm screenshots [baseURL] [outDir]` signs in as the `crm-demo` admin and captures every screen, dialog, menu,
  empty, error and loading state at 1440 and 390 px (and the collapsed sidebar at 1440) into `screenshots/`
  (git-ignored) for design review.

## Layout

- `src/lib/bff/`: the BFF (`bff.ts`, framework-free and unit-tested) and its instance (`server.ts`).
- `src/lib/api/`: `schema.d.ts` (generated by `pnpm gen:api` from `openapi.json`, which crm-api writes with
  `cargo run -p crm-api -- openapi:export ../crm-web/openapi.json`), `types.ts` (the aliases the app imports, plus the
  list endpoints' query parameters, which navree-api cannot document yet), `client.ts` (openapi-fetch, same-origin,
  401/403 hooks), `errors.ts` (Navree error bodies), `form-errors.ts` (400 `errors` to form fields).
- `src/lib/casl/`: one `createMongoAbility` for the app, rules from `GET /api/me/abilities`, `__type` subject detection.
- `src/lib/realtime/`: real time over the Pusher protocol, optional. `pusher.ts` is a small typed wrapper over pusher-js
  (`listen`, `leave`, `disconnect`; it adds the `private-` prefix) for `private-crm.deals` and `private-users.{id}`;
  channel authorization goes through the BFF's `/broadcasting/auth`. `mock-bus.ts` stands in for it with the mock API.
- `src/features/*`: TanStack Query hooks and feature components per resource.
- `src/app/(app)/*`: screens behind sign-in; `src/app/login`.
- `src/mocks/`: MSW handlers, seed data and CASL rules mirroring crm-api.
