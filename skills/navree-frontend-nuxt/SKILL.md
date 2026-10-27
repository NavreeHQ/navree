---
name: navree-frontend-nuxt
description: "Build a Nuxt 4 (Vue 3) front end on a Navree API: Nitro server routes as the backend-for-frontend (Navree's access token and rotating refresh token in httpOnly cookies, single-flight refresh, the Origin check that stops CSRF, a server-side session gate, private-channel authorization), a typed openapi-fetch client generated from the app's OpenAPI document, TanStack Query for Vue, Pinia, VeeValidate with zod, CASL abilities from `GET /api/me/abilities`, real time with pusher-js, MSW mocks, Vitest and Playwright tests, and the navree.com app UI (shadcn-vue on Reka UI, the sidebar-07 shell, 4px corners, a command palette). Use when creating or changing a Vue or Nuxt app that talks to a Navree API, wiring sign-in, refresh or sign-out in Nitro, calling the API from Vue, subscribing to private channels in the browser, or theming a Vue app like navree.com. The reference app is apps/crm-nuxt."
---

# A Nuxt front end on a Navree API

The browser never holds a token. Nitro, Nuxt's server, is the
**backend-for-frontend (BFF)**: server routes under `server/api` and
`server/routes/broadcasting` keep Navree's JWT access token and rotating
refresh token in httpOnly cookies and forward each call with
`Authorization: Bearer`. The browser calls only its own origin. The one
exception is the real-time socket, which connects straight to Navree's
Pusher-protocol server; private channels are authorized through the BFF.

```
browser ── same-origin /api/*, /broadcasting/auth ──▶ Nitro BFF ── Bearer ──▶ Navree API
   └────────────── ws://api-host/app/{key} (pusher-js) ─────────────────────▶ Navree real time
```

`apps/crm-nuxt` (the Navree CRM on Nuxt) is the reference: copy from it.
Every path below is a file there. Architecture background:
`/docs/patterns/backend-for-frontend/`. The React version of the same app is
`apps/crm-web` (skill `navree-frontend-next`); the BFF module is the same
code.

## Steps

1. **Create the app** (Nuxt 4, `app/` source directory) and add the pieces:
   `pnpm add @pinia/nuxt pinia @tanstack/vue-query openapi-fetch @casl/ability @casl/vue pusher-js vee-validate zod reka-ui @vueuse/core vue-sonner @lucide/vue class-variance-authority clsx tailwind-merge`
   and for development
   `pnpm add -D tailwindcss @tailwindcss/vite tw-animate-css openapi-typescript msw vitest @nuxt/test-utils @vue/test-utils happy-dom @playwright/test vue-tsc typescript h3`.
2. **Configure Nuxt** (`nuxt.config.ts`): `ssr: false` (every screen sits
   behind sign-in; Nitro still serves the BFF), `modules: ["@pinia/nuxt"]`,
   `vite.plugins: [tailwindcss()]`, `typescript.strict: true`, and the
   runtime config below. The CRM also sets `components: { dirs: [] }` so
   every file imports its components explicitly.
3. **Export the API contract.** From the Navree app:
   `navree openapi:export web/openapi.json` (the CRM:
   `cargo run -p crm-api -- openapi:export ../crm-nuxt/openapi.json`). Add
   `"gen:api": "openapi-typescript ./openapi.json -o app/lib/api/schema.d.ts"`
   and run `pnpm gen:api` whenever the API changes.
4. **Add the BFF.** Copy `server/lib/bff.ts` (framework-free, unit-tested)
   and `server/lib/http-errors.ts`, create the instance in
   `server/lib/instance.ts`, add the routes and the session gate (next
   section).
5. **Add the API client and plugins**: `app/lib/api/client.ts`,
   `errors.ts`, `form-errors.ts`, `keys.ts`, `types.ts`; plugins for TanStack
   Query, CASL and the 401/403 hooks (`app/plugins/02.vue-query.ts`,
   `03.casl.ts`, `04.api-events.ts`).
6. **Load the user and their abilities before any screen renders**
   (`app/middleware/session.global.ts`).
7. **Add real time** (`app/lib/realtime/pusher.ts`,
   `app/composables/useRealtime.ts`) and list the web origin in the API's
   `realtime.allowed_origins`.
8. **Theme it** with the navree.com tokens and the app UI rules ("The look").
9. **Test it**: Vitest for the BFF, its Nitro handlers and components;
   Playwright against the real API.

## The BFF in Nitro

### Routes

| File | Calls | Purpose |
|---|---|---|
| `server/api/auth/login.post.ts` | `bff.login` | Signs in at Navree, sets the cookies, returns only `{user}` (or `{two_factor_required, expires_in}`) |
| `server/api/auth/2fa/verify.post.ts` | `bff.verifyTwoFactor` | Completes a two-factor sign-in with `{code}` or `{recovery_code}` |
| `server/api/auth/logout.post.ts` | `bff.logout` | Revokes the refresh token at Navree (`{all: true}`: every device), clears the cookies |
| `server/api/auth/refresh.post.ts` | `bff.refreshSession` | Explicit refresh: 204 with new cookies, or 401 |
| `server/api/[...path].ts` | `bff.proxy` | Every other `/api/*` call, every method |
| `server/routes/broadcasting/auth.post.ts` | `bff.proxy(request, "/broadcasting/auth")` | Signs private-channel subscriptions |
| `server/middleware/session-gate.ts` | | A page request (`Accept: text/html`) with neither cookie gets `302 /login?next=...` |

Nitro routes match the most specific file first, so the four `auth` routes win
over the catch-all. Each route is one line: convert the h3 event to a web
`Request` and hand it to the BFF.

```ts
// server/lib/instance.ts
import { createBff, type Bff } from "./bff";

let instance: Bff | undefined;

/** Created on first use: settings are read at runtime, and the single-flight refresh map is shared. */
export function useBff(): Bff {
  if (instance) return instance;
  const config = useRuntimeConfig();
  instance = createBff({
    apiUrl: process.env.NAVREE_API_URL || config.navreeApiUrl || "http://127.0.0.1:8092",
    secureCookies: flag(process.env.NAVREE_COOKIE_SECURE), // undefined → on in production
    allowedOrigins: (process.env.APP_ORIGINS ?? "").split(",").map((o) => o.trim()).filter(Boolean),
    trustedProxyHops: Number.parseInt(process.env.NAVREE_TRUSTED_PROXY_HOPS ?? "0", 10) || 0,
  });
  return instance;
}
```

```ts
// server/api/[...path].ts
import { useBff } from "../lib/instance";

export default defineEventHandler((event) => {
  const request = toWebRequest(event);
  return useBff().proxy(request, new URL(request.url).pathname);
});

// server/api/auth/login.post.ts
export default defineEventHandler((event) => useBff().login(toWebRequest(event)));

// server/routes/broadcasting/auth.post.ts
export default defineEventHandler((event) => useBff().proxy(toWebRequest(event), "/broadcasting/auth"));
```

```ts
// server/middleware/session-gate.ts (abridged)
const PASS_THROUGH = /^\/(api|broadcasting|_nuxt|__nuxt|_ipx|login)(\/|$)|\.[a-z0-9]+$/i;

export default defineEventHandler((event) => {
  if (event.method !== "GET" && event.method !== "HEAD") return;
  const url = getRequestURL(event);
  if (PASS_THROUGH.test(url.pathname)) return;
  if (!(getRequestHeader(event, "accept") ?? "").includes("text/html")) return;
  if (useRuntimeConfig(event).public.apiMocks) return; // the mock API keeps its session in the tab
  if (getCookie(event, ACCESS_COOKIE) || getCookie(event, REFRESH_COOKIE)) return;
  const target = url.pathname + url.search;
  return sendRedirect(event, target === "/" ? "/login" : `/login?next=${encodeURIComponent(target)}`, 302);
});
```

Whether a cookie is still valid is the API's call; the gate only spares a
signed-out visitor the app shell. The app redirects on a 401 too.

### What the BFF guarantees

- **Cookies.** `navree_at` (access token, `Max-Age` = `expires_in`) and
  `navree_rt` (refresh token, 30 days), both `HttpOnly; SameSite=Lax;
  Path=/`, `Secure` in production. The two-factor challenge lives in
  `navree_2fa` on `Path=/api/auth/2fa` only. A token pair in any
  `/api/auth/*` response (`PATCH /api/auth/me/password`) replaces the cookies
  and is stripped from the body.
- **Refresh.** On a 401, or when only the refresh cookie is left, the proxy
  refreshes once and retries. Refresh is **single-flight per refresh token**:
  Navree rotates refresh tokens and revokes the family when one is reused,
  so parallel requests share one refresh. A finished refresh is remembered
  for 30 s for requests still carrying the old cookie. A failed refresh
  clears both cookies.
- **CSRF.** Every non-GET request whose `Origin` is not the app's own host
  (or listed in `APP_ORIGINS`) gets `403 {"message": "Invalid origin"}`
  before anything reaches Navree.
- **Pass-through.** Status, body (streamed), `content-type` (CSV included),
  `content-disposition`, `x-request-id`, `retry-after` and `x-ratelimit-*`
  come back; upstream calls use `cache: "no-store"`.
- **Client address.** The browser's `X-Forwarded-For` never reaches Navree.
  Behind N trusted reverse proxies set `NAVREE_TRUSTED_PROXY_HOPS=N`; the BFF
  sends the address the outermost proxy saw.

## The browser side

### Runtime config

```ts
// nuxt.config.ts (abridged)
runtimeConfig: {
  navreeApiUrl: "", // server only; NUXT_NAVREE_API_URL (server/lib/instance.ts also reads NAVREE_API_URL)
  public: {
    apiMocks: false, // NUXT_PUBLIC_API_MOCKS=1: MSW in the browser
    realtime: true, // NUXT_PUBLIC_REALTIME=0: no socket
    pusherKey: "navree-crm-key", // the API's realtime.key
    wsHost: "127.0.0.1",
    wsPort: 8092,
    demoAccounts: false,
  },
},
```

`NUXT_PUBLIC_*` variables override these when the server starts, so one build
runs anywhere. `app/composables/useCrmConfig.ts` reads them typed.

### The client

```ts
// app/lib/api/client.ts (abridged)
const client = createClient<paths>({ baseUrl: "", credentials: "same-origin", fetch: fetchApi });
export const api = Object.assign(client, {
  // Same hooks for non-JSON calls: CSV export, channel authorization.
  rawFetch: (input: string, init?: RequestInit) => fetchApi(new Request(input, { credentials: "same-origin", ...init })),
});
// fetchApi: 401 → apiEvents.onUnauthorized, 403 → apiEvents.onForbidden (set by app/plugins/04.api-events.ts)
```

- `app/plugins/04.api-events.ts` sets the hooks: a 401 clears the cached
  user and `router.replace("/login?next=…")`; a 403 is a toast.
- `unwrap(api.GET(...))` returns the data or throws an `ApiError` built from
  Navree's error body (`{statusCode, message, error, errors?}`).
- Only same-site paths are valid `?next=` destinations after sign-in.

### Queries

One `QueryClient` (`app/plugins/02.vue-query.ts`): `staleTime` 20 s, no retry
on 4xx, and mutation errors become toasts unless they are 401, 403 or a 400
the form shows. Hooks live per feature and take refs or getters:

```ts
// app/features/deals/queries.ts
export function useDeals(params: MaybeRefOrGetter<DealListParams>) {
  return useQuery({
    queryKey: computed(() => keys.deals.list(toValue(params))),
    queryFn: () => unwrap(api.GET("/api/v1/deals", { params: { query: toValue(params) } })),
    placeholderData: keepPreviousData,
  });
}
```

List filters live in the URL (`app/composables/useUrlState.ts`:
`url.set({ search: "acme" })` replaces the entry and resets the page), so
views survive reloads and can be shared.

### Session and abilities

```ts
// app/middleware/session.global.ts (abridged)
export default defineNuxtRouteMiddleware(async (to) => {
  if (to.path === "/login") return;
  const { $queryClient, $ability } = useNuxtApp();
  try {
    await $queryClient.fetchQuery({ queryKey: keys.auth.user, queryFn: fetchCurrentUser, retry: false });
    const rules = await $queryClient.fetchQuery({ queryKey: keys.auth.abilities, queryFn: fetchAbilityRules });
    $ability.update(rules);
  } catch (error) {
    if (isApiError(error) && error.status === 401) return navigateTo(`/login?next=${encodeURIComponent(to.fullPath)}`);
  }
});
```

One CASL ability for the app (`app/plugins/03.casl.ts` installs
`abilitiesPlugin` from `@casl/vue`; `app/lib/casl/ability.ts` creates it).
Every API resource carries a constant `__type` (`"Deal"`), so records work as
subjects straight from JSON: `useAbility<AppAbility>().can("move", deal)`.
Hide what the rules deny; the API still enforces every rule (403).

### Forms

VeeValidate with a zod 4 schema through a small adapter
(`app/lib/forms/zod-schema.ts`, `toTypedSchema`), and the API's 400 errors
mapped back onto the fields:

```ts
// API field → form field (abridged from app/features/deals/DealFormDialog.vue).
const FIELD_MAP = { title: "title", company_id: "company", contact_id: "contact", amount_cents: "amount" } as const;

const form = useForm({ validationSchema: toTypedSchema(schema), initialValues: initialValues() });
const onSubmit = form.handleSubmit((values) => {
  create.mutate(toBody(values), {
    onSuccess: () => (open.value = false),
    onError: (error) => {
      // Field messages land on their fields; anything unmatched comes back as the form's root message.
      const root = applyFieldErrors(error, form.setFieldError, FIELD_MAP);
      if (root) rootError.value = root;
    },
  });
});
```

`app/components/crm/TextField.vue` binds a labelled input with `useField`, so
dialogs read as a list of fields (`<TextField name="name" label="Name" required />`).

## Real time

Navree's `RealtimeModule` speaks the Pusher protocol, so pusher-js connects
to it directly. `app/lib/realtime/pusher.ts` wraps it:

```ts
const pusher = new Pusher(settings.key, {
  cluster: "",
  wsHost: settings.wsHost,
  wsPort: settings.wsPort,
  wssPort: settings.wsPort,
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
```

`app/composables/useRealtime.ts` (mounted once by the shell) listens on
`private-crm.deals` (`deal.updated`: put the deal in the cache, **invalidate**
the board, lists and dashboard, flash the card) and `private-users.{id}`
(`notification.created`: refresh the bell, show a toast), and reports the
socket state to the Pinia store for the top bar. Real time is optional: if the
socket can't connect, queries still refetch on focus. The API must list the
web origin in `realtime.allowed_origins` (the WebSocket checks `Origin`).

## The look

Dark only, on the navree.com tokens, following `apps/website/DESIGN.md` §9
(app UI). `app/assets/css/main.css` holds it all.

- **Tokens.** The site's `@theme` colors (`ink-0..3`, `line`, `line-2`,
  `fg`, `fg-1..3`, `accent` `#3dff8f`, `accent-ink`, `amber`, `danger`),
  shadcn's semantic names mapped onto them (`background`=ink-0,
  `card`/`popover`=ink-1, `muted`/`secondary`=ink-2, `primary`/`ring`=accent,
  `border`=line, `input`=line-2), `class="dark"` on `<html>`. `accent` is
  Navree green, never a hover color: hover is `bg-ink-2`, selection `bg-ink-3`.
- **Corners.** The radius scale collapses onto two values: `rounded-sm`/`md`
  = 4px (buttons, inputs, menu items, badges, tabs, avatars), `rounded-lg`
  and up = 6px (cards, popovers, dialogs). `rounded-full` only for status
  dots. No pills, no glows.
- **Fonts**, self-hosted from Fontsource packages with `@font-face` in the
  CSS (no runtime requests): Geist for the UI and page titles (24px
  semibold), Mona Sans for big figures, Monaspace Neon for ids and shortcuts.
- **Primitives.** shadcn-vue (`components.json`, Reka UI) in
  `app/components/ui`, restyled: after `pnpm dlx shadcn-vue@latest add <name>`,
  swap `bg-accent` hovers for ink, `rounded-full`/`rounded-lg` controls for
  `rounded-md`, and drop shadows that glow.
- **Shell** (`app/layouts/default.vue`, `app/components/shell/*`), the
  `sidebar-07` pattern: a 16rem ink-1 sidebar with an app switcher (32px logo
  tile, name, context, chevrons), sentence-case groups of 32px items (16px
  lucide icon, 14px text), sub-items on a 1px guide line, the account menu in
  the footer; Ctrl/Cmd B folds it to a 3rem icon rail, kept in localStorage
  (`useLocalStorage` in the Pinia store); a sheet below `md`. A 48px top bar
  holds the trigger, a separator, the breadcrumb (`crumbsFor` in
  `app/lib/navigation.ts`; detail pages call `useRecordCrumb(() => deal.title)`),
  search, live status, Create and notifications.
- **Command palette** on Ctrl/Cmd K or `/` (`CommandPalette.vue`: a
  shadcn-vue `Dialog` around Reka's `ListboxRoot` and `ListboxFilter`): screens, create actions, and
  records searched through the API. `?` lists the shortcuts; `g` and `c`
  sequences go to a screen or create a record (`app/composables/useShortcuts.ts`).
- **States.** Skeletons that mirror the layout, empty states with a reason and
  a way out, `ApiUnavailable` when the API is down, `RecordMissing` for a
  404, toasts with `vue-sonner`. Tables hide secondary columns on small
  screens; nothing scrolls sideways at 390 px.

## Testing

- **BFF:** `tests/unit/bff.test.ts` drives `createBff` with plain `Request`s
  and a scripted `fetch`: cookies set and cleared, refresh once and
  single-flight, the Origin check, token stripping, two-factor, the client
  address.
- **Nitro handlers:** `tests/unit/nitro-handlers.test.ts` mounts the real
  files from `server/` on an h3 app (Nitro's auto-imports stubbed with the h3
  functions of the same name) and calls them with web Requests, the session
  gate included.
- **Components in Nuxt:** a second Vitest project with
  `defineVitestProject` from `@nuxt/test-utils/config` (environment `nuxt`);
  `tests/nuxt/login.test.ts` mounts the login page with `mountSuspended` and
  the app's real plugins, with `mockNuxtImport("navigateTo", ...)`.
- **Units:** the client and error mapping, CASL, the board's drag maths,
  formatting, navigation, the pusher-js wrapper with a fake `Pusher`, the MSW
  mock API.
- **End to end:** Playwright against the real API: `playwright.config.ts`
  starts crm-api on a fresh seeded SQLite file and the production build
  (`node .output/server/index.mjs`, `NAVREE_COOKIE_SECURE=0`), one worker, a
  UTC browser. Specs cover sign-in, the CSRF 403, refresh rotation, sign-out
  revocation, drag and keyboard moves, a 403 for another rep's deal, real
  time across two browser contexts, and the shell.

## Checks

```bash
pnpm typecheck && pnpm test && pnpm build
pnpm e2e                       # real API on :8092 + production build on :3102
pnpm screenshots               # every screen at 1440 and 390 px into ./screenshots
pnpm dev:mock                  # the whole app on MSW, no API
```

## Common mistakes

- Putting a token in `localStorage`, a non-httpOnly cookie or the Nuxt
  payload, or calling Navree from the browser: everything goes through Nitro.
- `useCookie` or `$fetch` to Navree from a page: with `ssr: false` that runs
  in the browser. Server routes only.
- Refreshing in parallel. Two requests spending the same refresh token make
  Navree revoke the session; keep the BFF's refresh single-flight and share
  one BFF instance per server (`useBff()`).
- A new non-GET server route that skips `bff.proxy` (or `bff.originAllowed`):
  it has no CSRF check.
- Nesting a `TooltipTrigger` around a `DropdownMenuTrigger` or
  `PopoverTrigger`: Reka anchors both to the nearest popper (the tooltip's)
  and the menu is never positioned. Pass the trigger element to the content:
  `<DropdownMenuContent :reference="triggerEl">` (see `UserMenu.vue`).
- Naming channels with the prefix twice: the wrapper adds `private-`, so
  listen on `crm.deals`.
- A web origin missing from `realtime.allowed_origins`: the socket is refused
  and the page silently falls back to refetching.
- Green for navigation, selection or hover, `rounded-full` buttons, glowing
  shadows, or raw hex values in components.
