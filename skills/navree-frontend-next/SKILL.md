---
name: navree-frontend-next
description: "Build a Next.js front end on a Navree API: the backend-for-frontend pattern (route handlers that keep Navree's access token and rotating refresh token in httpOnly cookies), a typed client generated from the app's OpenAPI document, single-flight token refresh, two-factor sign-in, the Origin check that stops CSRF, CASL abilities from `GET /api/me/abilities`, real time over the Pusher protocol with pusher-js, MSW mocks, tests, and the navree.com dark theme. Use when creating or changing a web app that talks to a Navree API, wiring sign-in, refresh or sign-out, calling the API from React, subscribing to private channels in the browser, or theming an app to look like navree.com. The reference app is apps/crm-web."
---

# A Next.js front end on a Navree API

The browser never holds a token. A Next.js server sits between the browser
and Navree as a **backend-for-frontend (BFF)**: route handlers under
`src/app/api` and `src/app/broadcasting` keep Navree's JWT access token and
rotating refresh token in httpOnly cookies and forward each call with
`Authorization: Bearer`. The browser calls only its own origin. The one
exception is the real-time socket, which connects straight to Navree's
Pusher-protocol server; private channels are authorized through the BFF.

```
browser ── same-origin /api/*, /broadcasting/auth ──▶ Next.js BFF ── Bearer ──▶ Navree API
   └────────────── ws://api-host/app/{key} (pusher-js) ───────────────────────▶ Navree real time
```

`apps/crm-web` (the Navree CRM) is the reference: copy from it. Everything
below is what it does, with file paths. Architecture background:
`/docs/patterns/backend-for-frontend/`.

## Steps

1. **Create the app.** Next.js (App Router, TypeScript, Tailwind CSS v4),
   then add the runtime pieces:
   `pnpm add openapi-fetch @tanstack/react-query @casl/ability @casl/react pusher-js zod react-hook-form @hookform/resolvers sonner cmdk radix-ui lucide-react cn class-variance-authority`
   and for development
   `pnpm add -D openapi-typescript msw vitest jsdom @testing-library/react @playwright/test`.
2. **Export the API contract.** From the Navree app:
   `navree openapi:export web/openapi.json` (or `cargo run -- openapi:export
   ../web/openapi.json`; the CRM uses
   `cargo run -p crm-api -- openapi:export ../crm-web/openapi.json`). Add
   `"gen:api": "openapi-typescript ./openapi.json -o src/lib/api/schema.d.ts"`
   and run `pnpm gen:api` whenever the API changes.
3. **Add the BFF.** Copy `src/lib/bff/bff.ts` (framework-free, unit-tested)
   and `src/lib/bff/http-errors.ts`; create the instance in
   `src/lib/bff/server.ts`; add the route handlers (next section).
4. **Add the API client.** `src/lib/api/client.ts` (openapi-fetch,
   same-origin, hooks for 401 and 403), `errors.ts` (Navree's error body as
   `ApiError`), `form-errors.ts` (400 `errors` onto form fields) and
   `types.ts` (friendly aliases over the generated schema).
5. **Load the user and their abilities** after sign-in:
   `GET /api/auth/me`, then `GET /api/me/abilities` into one app-wide CASL
   ability; render nothing behind sign-in until both are applied.
6. **Add real time** with the pusher-js wrapper (`src/lib/realtime/`), and
   list the web origin in the API's `realtime.allowed_origins`.
7. **Theme it** with the navree.com tokens and fonts (section "The theme").
8. **Test it**: Vitest for the BFF and the client, MSW for an in-browser
   mock API, Playwright against the real API.

## The BFF

### Route handlers

| File | Calls | Purpose |
|---|---|---|
| `src/app/api/auth/login/route.ts` | `bff.login` | Signs in at Navree, sets the cookies, returns only `{user}` (or `{two_factor_required, expires_in}`) |
| `src/app/api/auth/2fa/verify/route.ts` | `bff.verifyTwoFactor` | Completes a two-factor sign-in with `{code}` or `{recovery_code}` |
| `src/app/api/auth/logout/route.ts` | `bff.logout` | Revokes the refresh token at Navree, clears the cookies |
| `src/app/api/auth/refresh/route.ts` | `bff.refreshSession` | Explicit refresh (the proxy also refreshes on its own) |
| `src/app/api/[...path]/route.ts` | `bff.proxy` | Every other `/api/*` call, all methods |
| `src/app/broadcasting/auth/route.ts` | `bff.proxy(request, "/broadcasting/auth")` | Signs private-channel subscriptions |

```ts
// src/lib/bff/server.ts
import { createBff } from "./bff";

export const bff = createBff({
  apiUrl: process.env.NAVREE_API_URL ?? "http://127.0.0.1:8080",
  secureCookies:
    process.env.NAVREE_COOKIE_SECURE !== undefined
      ? process.env.NAVREE_COOKIE_SECURE === "1"
      : process.env.NODE_ENV === "production",
  allowedOrigins: process.env.APP_ORIGINS?.split(",").map((o) => o.trim()).filter(Boolean),
  trustedProxyHops: Number.parseInt(process.env.NAVREE_TRUSTED_PROXY_HOPS ?? "0", 10) || 0,
});
```

```ts
// src/app/api/[...path]/route.ts
import { bff } from "@/lib/bff/server";

function handle(request: Request): Promise<Response> {
  return bff.proxy(request, new URL(request.url).pathname);
}

export const dynamic = "force-dynamic";
export { handle as DELETE, handle as GET, handle as HEAD, handle as PATCH, handle as POST, handle as PUT };
```

### What the BFF guarantees

- **Cookies.** `navree_at` (access token, `Max-Age` = `expires_in`) and
  `navree_rt` (refresh token, 30 days), both `HttpOnly; SameSite=Lax;
  Path=/`, `Secure` in production. The two-factor challenge lives in
  `navree_2fa` on `Path=/api/auth/2fa` only. Token pairs in any
  `/api/auth/*` response (a password change, for example) replace the
  cookies and are stripped from the body.
- **Refresh.** On a 401, or when only the refresh cookie is left, the proxy
  refreshes once and retries. Refresh is **single-flight per refresh
  token**: Navree rotates refresh tokens and revokes the whole family when
  one is reused, so parallel requests must share one refresh. A finished
  refresh is remembered for 30 s for requests still carrying the old cookie.
  A failed refresh clears both cookies and the browser goes to `/login`.
- **CSRF.** Every non-GET request whose `Origin` is not the app's own host
  (or listed in `APP_ORIGINS`) gets `403 {"message": "Invalid origin"}`
  before anything reaches Navree.
- **Pass-through.** Status, body, `content-type` (CSV exports included),
  `content-disposition`, `x-request-id`, `retry-after` and `x-ratelimit-*`
  come back unchanged; authenticated responses use `cache: "no-store"`.
- **Client address.** The browser's own `X-Forwarded-For` and `X-Real-IP`
  never reach Navree. Behind N trusted reverse proxies set
  `NAVREE_TRUSTED_PROXY_HOPS=N`; the BFF then sends the address the
  outermost proxy saw. List the BFF's address in the API's
  `APP_HTTP__TRUSTED_PROXIES` so Navree's login throttle and per-IP rate
  limit apply per browser.

### The browser side of sign-in

```ts
// src/lib/api/client.ts (abridged)
const client = createClient<paths>({ baseUrl: "", credentials: "same-origin", fetch: fetchApi });
export const api = Object.assign(client, {
  // Same hooks for non-JSON calls: CSV export, channel authorization.
  rawFetch: (input: string, init?: RequestInit) => fetchApi(new Request(input, { credentials: "same-origin", ...init })),
});
// fetchApi: on 401 → apiEvents.onUnauthorized (router.replace("/login?next=…")); on 403 → a toast.
```

- Read data with TanStack Query and `unwrap(api.GET("/api/v1/deals", { params: { query } }))`;
  `unwrap` throws an `ApiError` built from Navree's error body
  (`{statusCode, message, error, errors?}`).
- Show 400 validation errors next to fields with
  `applyFieldErrors(error, form.setError, ["email", "password"])`.
- Don't retry 4xx in queries (`retry: (n, e) => !(isApiError(e) && e.status < 500) && n < 2`).
- Only same-site paths are valid `?next=` destinations after sign-in.

### Abilities

```ts
// src/lib/casl/ability.ts
export function createAppAbility(rules: readonly CaslRule[] = []): AppAbility {
  return createMongoAbility<AppAbility>(rules as RawRuleOf<AppAbility>[], {
    detectSubjectType: (o) => (typeof o === "string" ? o : o.__type) as ExtractSubjectType<AppSubjects>,
  });
}
```

Every API resource carries a constant `__type` (`"Deal"`), so records work
as CASL subjects as they come from JSON: `<Can I="update" this={deal}>`,
`ability.can("move", deal)`. Hide what the rules deny; the API still enforces
every rule (403), so the UI is a convenience, never the check.

## Real time

Navree's `RealtimeModule` speaks the Pusher protocol, so pusher-js connects
to it directly. The CRM wraps pusher-js in a small typed client
(`src/lib/realtime/pusher.ts`, tested in `pusher.test.ts`):

```ts
import Pusher from "pusher-js";

const pusher = new Pusher(env.pusherKey, {
  cluster: "mt1", // required by pusher-js, unused when wsHost is set
  wsHost: env.wsHost,
  wsPort: env.wsPort,
  wssPort: env.wsPort,
  forceTLS: false,
  enabledTransports: ["ws", "wss"],
  disableStats: true,
  channelAuthorization: {
    // POST /broadcasting/auth through the BFF, which adds the bearer token.
    customHandler: ({ socketId, channelName }, callback) => {
      api
        .rawFetch("/broadcasting/auth", {
          method: "POST",
          headers: { "Content-Type": "application/json", Accept: "application/json" },
          body: JSON.stringify({ socket_id: socketId, channel_name: channelName }),
        })
        .then(async (r) => {
          if (!r.ok) throw new Error(`Channel authorization failed (${r.status})`);
          callback(null, await r.json());
        })
        .catch((e: unknown) => callback(e instanceof Error ? e : new Error(String(e)), null));
    },
  },
});

pusher.subscribe("private-crm.deals").bind("deal.updated", ({ deal }) => {
  void queryClient.invalidateQueries({ queryKey: ["deals"] });
});
```

- The wrapper exposes `listen(channel, event, callback)`, `leave(channel)`
  and `disconnect()`, adds the `private-` prefix, subscribes once per
  channel and reports `unavailable`/`failed` once. In mock mode the same
  interface is served by a window event bus (`mock-bus.ts`).
- Treat real time as optional: on an update, **invalidate queries** rather
  than patching caches by hand; queries still refetch on focus when the
  socket is down.
- The API must allow the web origin: `realtime.allowed_origins` in
  `config/app.toml` (the WebSocket checks `Origin`), and the browser needs
  `NEXT_PUBLIC_WS_HOST`, `NEXT_PUBLIC_WS_PORT` and `NEXT_PUBLIC_PUSHER_KEY`
  (the API's `realtime.key`).

## Configuration

| Variable | Where | Default | Meaning |
|---|---|---|---|
| `NAVREE_API_URL` | server, request time | `http://127.0.0.1:8080` | Where the BFF forwards |
| `NAVREE_COOKIE_SECURE` | server | on in production | `0` to serve a production build over plain http |
| `APP_ORIGINS` | server | none | Extra origins allowed to send non-GET requests (proxy rewrites Host) |
| `NAVREE_TRUSTED_PROXY_HOPS` | server | `0` | Reverse proxies that append the client to `X-Forwarded-For` |
| `NEXT_PUBLIC_API_MOCKS` | build | `0` | `1` serves the API from MSW in the browser |
| `NEXT_PUBLIC_REALTIME` | build | `1` | `0` turns the socket off |
| `NEXT_PUBLIC_PUSHER_KEY` | build | `navree-crm-key` | The API's `realtime.key` |
| `NEXT_PUBLIC_WS_HOST` / `NEXT_PUBLIC_WS_PORT` | build | `127.0.0.1` / `8080` | The API's WebSocket listener |

## The theme

The CRM looks like navree.com: dark only, ink surfaces, one green.

- **Tokens.** Copy the `@theme` block of `apps/website/src/app/globals.css`:
  `ink-0..3` (`#040605`, `#0a0e0c`, `#111714`, `#16201b`), `line`
  (`#1c2420`), `line-2` (`#2a3530`), `fg` (`#e9efeb`), `fg-1..3`,
  `accent` (`#3dff8f`), `accent-ink` (`#03130a`), `amber` (`#ffbf5e`,
  warnings only), `danger` (`#ff6b6b`), the glow shadows and
  `--ease-out-quint`. Map shadcn's semantic variables onto them
  (`background`=ink-0, `card`/`popover`=ink-1, `muted`/`secondary`=ink-2,
  `primary`/`ring`=accent, `border`=line, `input`=line-2) and put `dark` on
  `<html>` with `@custom-variant dark (&:where(.dark, .dark *))`.
- **`accent` is Navree green**, not shadcn's grey hover surface. After
  `shadcn add`, replace `focus:bg-accent`/`hover:bg-accent` with
  `bg-ink-3`/`bg-ink-2`. Use green for the one primary action per view
  (`Button` default: green pill with `hover:shadow-glow`), focus rings, the
  current navigation item and the one figure that matters (won deals).
  Record links in tables and timelines are quiet (`.record-link`).
- **Fonts**, self-hosted with `next/font/local` from `@fontsource-variable/mona-sans`
  (display: page titles and big figures, `wdth` 112, tight tracking),
  `@fontsource-variable/geist` (UI) and `@fontsource/monaspace-neon` (eyebrows,
  table headings, ids). See `src/app/fonts.ts`. No runtime font requests.
- **`cn` knows the type scale.** `src/lib/utils.ts` builds `cn` with
  `createCn` from `cn/config` and adds `title`, `figure`, `eyebrow` and
  `display-*` to the font-size group; without it `cn("text-figure", "text-fg")`
  drops the size.
- **Component language.** `.surface` panels (ink-1, 1px line, top highlight),
  rounded-full buttons and badges, rounded-lg inputs on ink-1 with a line-2
  edge, mono uppercase `.eyebrow` and table headings, popovers and dialogs on
  ink-1 with `shadow-float` over an ink scrim, toasts on ink-2 with a colored
  icon, empty states with an icon tile in a faint green glow, skeletons that
  mirror the real layout with a slow shimmer.
- **Shell.** A sticky top bar (blurred ink) with the command menu trigger,
  notifications and the account menu; an ink-1 sidebar that collapses to
  icons and becomes a sheet on phones; a **command menu** on Ctrl/Cmd K
  (`cmdk`) to jump to screens, create records and search through the API.
- **Accessibility and layout.** WCAG AA on every ink (fg-3 is the faintest
  text), a visible green focus outline, `aria-label` on icon buttons, a skip
  link, keyboard drag and drop on the board, and no horizontal page scroll
  at 390 px: tables hide secondary columns below `sm`/`md`/`lg`, figure grids
  go 2×2, toolbars wrap.

## Testing

- **BFF:** unit tests with plain `Request`s and an injected `fetch`
  (`src/lib/bff/bff.test.ts`): cookies set and cleared, refresh once and
  single-flight, Origin rejection, token stripping, two-factor.
- **Client and UI:** Vitest with jsdom (`src/lib/api/client.test.ts`,
  `src/app/login/login-screen.test.tsx`), the pusher-js wrapper with a fake
  `Pusher` class.
- **Mock API:** MSW handlers mirror the API's semantics, including the BFF's
  (`src/mocks/`); `pnpm dev:mock` runs the whole app without a backend.
- **End to end:** Playwright. `pnpm e2e` runs against the mock API;
  `E2E_REAL_API=1 pnpm e2e` starts the real API on a fresh seeded database
  and a production build; `E2E_BASE_URL=http://localhost:3000 E2E_REAL_API=1 pnpm e2e`
  uses servers you started. Run the browser in the API's time zone (UTC).

## Checks

```bash
pnpm lint && pnpm typecheck && pnpm test && pnpm build
pnpm e2e                         # mock API
E2E_REAL_API=1 pnpm e2e          # real API
pnpm screenshots http://localhost:3000   # every screen at 1440 and 390 px into ./screenshots
```

## Common mistakes

- Putting a token in `localStorage` or a non-httpOnly cookie, or calling
  Navree from the browser: everything goes through the BFF.
- Refreshing in parallel. Two requests spending the same refresh token make
  Navree revoke the session; keep the refresh single-flight.
- Forgetting the Origin check on a new non-GET route handler: route every
  call through `bff.proxy` (or `bff.originAllowed`).
- Forwarding the browser's `X-Forwarded-For` to Navree: the throttle would
  trust a header the browser wrote.
- Naming channels with the prefix twice: the wrapper adds `private-`, so
  listen on `crm.deals`, not `private-crm.deals`.
- A web origin missing from `realtime.allowed_origins`: the socket is
  refused and the page silently falls back to refetching.
- Using `bg-accent` as a hover color, raw hex values in components, or a
  second green element competing with the primary action.
