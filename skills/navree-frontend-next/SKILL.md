---
name: navree-frontend-next
description: "Build a Next.js front end on a Navree API with the @navree packages: `@navree/next` (route handlers that keep Navree's access and rotating refresh tokens in httpOnly cookies, `auth()` and `client()` in server components, the proxy that guards pages and refreshes tokens, `DataTable` and `useListQuery`, `useOperation`, workspaces, `AbilityProvider`/`<Can>`, `RealtimeProvider`/`useChannel`), `@navree/client` (the typed client generated with `navree openapi:client`, `NavreeError` with error codes, list queries, query keys) and `@navree/theme` (navree.com's dark theme for Tailwind 4 and shadcn/ui). Use when creating or changing a web app that talks to a Navree API in React or Next.js, wiring sign-in, refresh, two-factor or sign-out, calling the API from server or client components, building list screens with filters, sorting and paging, subscribing to private channels, following an operation's progress, translating API errors, or theming an app like navree.com."
---

# A Next.js front end on a Navree API

The browser never holds a token. The Next.js server is a
**backend-for-frontend (BFF)**: `@navree/next` mounts it as route handlers
that keep Navree's JWT access token and rotating refresh token in httpOnly
cookies and forward each call with `Authorization: Bearer`. The browser
calls only its own origin; the real-time socket goes straight to Navree,
with private channels authorized through the BFF.

```
browser ── same-origin /api/*, /broadcasting/auth ──▶ Next.js (@navree/next) ── Bearer ──▶ Navree API
   └────────────── ws://api-host/app/{key} (pusher-js) ─────────────────────────────────────▶ Navree real time
```

Docs: `/docs/frontend/nextjs/`, `/docs/frontend/typed-client/`,
`/docs/frontend/auth-and-bff/`, `/docs/frontend/data-tables/`,
`/docs/frontend/theming/`, `/docs/frontend/localization/`. The packages'
sources and tests are in `packages/js/` (`client`, `bff`, `next`, `theme`);
`packages/js/next/test/fixture` is a minimal working app.

## Steps

1. **Install** `@navree/next`, `@navree/client`, `@navree/theme` (from the
   repository's `packages/js`: `pnpm install && pnpm build` there, then
   `link:` dependencies), plus `@casl/ability @casl/react` for abilities,
   `pusher-js` for real time, `@tanstack/react-query` for data fetching.
2. **Generate the schema:** `navree openapi:client` (writes
   `web/src/lib/api/openapi.json` and `schema.d.ts`; `--out DIR`,
   `--input FILE`). Re-run after changing routes or types.
3. **Create the app's Navree** and mount it:

   ```ts
   // src/lib/navree.ts
   import { createNavree } from "@navree/next/server";
   export const navree = createNavree(); // NAVREE_API_URL etc. read at the first request

   // src/app/api/[...path]/route.ts
   import { navree } from "@/lib/navree";
   export const dynamic = "force-dynamic";
   export const { GET, POST, PUT, PATCH, DELETE, HEAD } = navree.handlers;

   // src/app/broadcasting/auth/route.ts
   export const dynamic = "force-dynamic";
   export const { POST } = navree.handlers;

   // src/proxy.ts (middleware.ts with `export const middleware` before Next.js 16)
   export const proxy = navree.proxy({ loginPath: "/login", publicPaths: ["/", "/docs/*"] });
   export const config = { matcher: ["/((?!_next/|favicon.ico).*)"] };
   ```

4. **The browser's client** (cookie mode):

   ```ts
   // src/lib/api/client.ts
   import { createNavreeClient, type ListQuery, type WithQuery } from "@navree/client";
   import { readWorkspaceCookie } from "@navree/next";
   import type { paths as Generated } from "./schema";

   export type paths = WithQuery<Generated, { "/api/v1/deals": ListQuery }>;
   export const api = createNavreeClient<paths>({
     workspace: () => readWorkspaceCookie(),
     onUnauthorized: () => window.location.assign(`/login?next=${encodeURIComponent(location.pathname + location.search)}`),
   });
   ```

5. **Server components:** `const session = await navree.auth<User>()`
   (`{user, accessToken, workspace}` or `null`; cached per request, never
   refreshes) and `const api = await navree.client<paths>()` (bearer mode
   to the API with the session's token and workspace).
6. **Providers:** `AbilityProvider rules={rules}` (`@navree/next/abilities`;
   load rules with `fetchAbilities({ fetch: api.raw })` from
   `@navree/client/abilities`), `RealtimeProvider server={…}
   options={{ authorize: api.raw }}` (`@navree/next/realtime`),
   `WorkspaceProvider initial={session.workspace} onChange={…}`.
7. **Theme:** `@import "tailwindcss"; @import "@navree/theme/theme.css";`
   plus fonts (`next/font/local` from the @fontsource files, see
   `/docs/frontend/theming/#fonts`) and `@source
   "../../node_modules/@navree/next/dist";`; `<html className="dark">`.
8. **Test:** Vitest with `@navree/bff/testing` (`fakeNavree`,
   `browserRequest`, `readSetCookies`, `tokenPair`, `unsignedJwt`),
   `createMemoryRealtime()` for channels, `@testing-library/react` for
   components; `serveFakeNavree(routes)` for end-to-end runs of a built app.

## What the BFF guarantees (don't re-implement it)

- Cookies `navree_at` (Max-Age = `expires_in`), `navree_rt` (30 days),
  `HttpOnly; SameSite=Lax; Path=/`, `Secure` in production; the two-factor
  challenge in `navree_2fa` on `Path=/api/auth/2fa`. Token pairs in auth
  responses are moved into cookies and stripped from bodies.
- Refresh once on a 401 (or up front when only the refresh cookie is left),
  **single-flight per refresh token** (Navree revokes the family on reuse);
  a failed refresh clears both cookies.
- Non-GET requests from another origin: `403 invalid_origin` (CSRF).
- `X-Workspace` forwarded, else the `navree_workspace` cookie; `Last-Event-ID`
  forwarded; bodies stream (CSV, SSE); the browser's `X-Forwarded-For` never
  reaches the API (`NAVREE_TRUSTED_PROXY_HOPS` behind proxies, or
  `NAVREE_CLIENT_IP_HEADER` for a header the proxy overwrites; Harbor sets
  the hop count to 1; the API trusts the BFF peer through `http.trusted_proxies`).
- Its own errors: `401 session_expired`, `401 two_factor_expired`,
  `403 invalid_origin`, `502 api_unreachable`, `502 unexpected_response`.

Sign-in: POST `{email, password}` to `/api/auth/login` with `api.raw`
(the BFF answers `{user}` or `{two_factor_required, expires_in}`), then
`/api/auth/2fa/verify` with `{code}` or `{recovery_code}`;
`/api/auth/logout` (`{all: true}` for every session).

## List screens

```tsx
"use client";
const { query, setQuery, setFilter, setSearch } = useListQuery({ per_page: 25 });
const { data, isPending } = useQuery({
  queryKey: deals.list(query), // deals = queryKeys("deals")
  queryFn: () => unwrap(api.GET("/api/v1/deals", { params: { query } })),
  placeholderData: (previous) => previous,
});
<DataTable<Deal> columns={columns} data={data?.data} meta={data?.meta} query={query}
  onQueryChange={setQuery} loading={isPending} getRowId={(d) => d.id} onRowClick={open} />;
```

- Columns are TanStack `ColumnDef`s; `meta: { sortField, align, className }
  satisfies NavreeColumnMeta`. Only columns with `sortField` sort.
- `useListQuery` needs a `<Suspense>` boundary (it reads `useSearchParams`).
- Filters: `setFilter("stage", "won")`, `setFilter("created_at", { gte, lt })`,
  `undefined` removes; every change goes back to page 1.
- `labels` translate the table (`range(from, to, total)` formats numbers).

## Other pieces

- **Errors:** `unwrap` throws `NavreeError` (`status`, `code`, `errors`,
  `fieldError(f)`, `is(...codes)`, `requestId`). Branch on the code
  (`error.is("invalid_two_factor_code")`), never on the English `message`.
  Toasts: `errorMessage(e)`
  (English) or `translateError(e, t)` with next-intl (`errors.<code>` keys,
  then `errors.unknown`). Forms: `applyFieldErrors(error, form.setError,
  ["email"])`. Don't retry 4xx in queries.
- **Operations:** `const { operation, finished, error, cancel } =
  useOperation(id)` follows `/api/operations/{id}/events` through the BFF.
- **Real time:** `useChannel("crm.deals", "deal.updated", handler)` (bare
  names are private; `null` channel listens to nothing). Invalidate queries
  on events instead of patching caches. List the web origin in the API's
  `realtime.allowed_origins`.
- **Locales:** `<html {...localeAttributes(locale)}>` sets `lang` and `dir`;
  use logical classes (`ps-*`, `text-start`, `rtl:`).

## Configuration

| Variable | Default | Meaning |
|---|---|---|
| `NAVREE_API_URL` | `http://127.0.0.1:8080` | Where the BFF forwards |
| `NAVREE_COOKIE_SECURE` | on in production | `0` for a production build over plain http |
| `APP_ORIGINS` | none | Extra origins allowed to send non-GET requests |
| `NAVREE_TRUSTED_PROXY_HOPS` | `0` | Proxies that append the client to `X-Forwarded-For` |
| `NAVREE_CLIENT_IP_HEADER` | none | Header the proxy sets to the bare client address |
| `NAVREE_WORKSPACE_HEADER` | `X-Workspace` | The API's `workspace.header` |

## Theme rules

Dark only (`class="dark"`). Green (`accent`, `#3dff8f`) for the primary
action, focus rings and positive data only; navigation, selection and hover
use the ink steps (`hover:bg-ink-2`, current `bg-ink-3`). 4px corners on
controls (`rounded-md`), 6px on surfaces (`rounded-xl`); no pills, no glows
except `shadow-float`. Add components with `pnpm dlx shadcn@latest add
<component>` (run it, don't install it: keep the `shadcn` package out of
`package.json`; its `tailwind.css` is vendored as `shadcn-tailwind.css` next
to `globals.css`), then replace `hover:bg-accent` and `focus:bg-accent` with
`hover:bg-ink-2`/`bg-ink-3`. Shells and density: `/docs/ui/app-shell/`.

## Checks

```bash
pnpm lint && pnpm typecheck && pnpm test && pnpm build
cd packages/js && pnpm check && pnpm smoke   # the packages themselves
```

## Common mistakes

- Copying the BFF into the app instead of `createNavree()`, or creating a
  new `createNavree()`/`createBff()` per request (the refresh map must be
  shared).
- Calling `auth()` and expecting a refresh: the proxy refreshes; a page
  without the proxy in front of it sees an expired token as `null`.
- Forgetting `export const dynamic = "force-dynamic"` on route handlers.
- Putting a token in `localStorage` or a readable cookie, or calling Navree
  from the browser directly.
- Importing `@navree/next/abilities` or `/realtime` without installing
  `@casl/react` or `pusher-js`.
- `useListQuery` without a `<Suspense>` boundary; columns without
  `meta.sortField` expected to sort.
- Listening on a public channel by its bare name: bare names are private;
  use `publicChannel("name")`.
- Using `bg-accent` as a hover color, or green for navigation.
