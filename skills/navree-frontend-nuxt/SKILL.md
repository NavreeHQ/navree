---
name: navree-frontend-nuxt
description: "Build a Nuxt (4.6.3 or later) front end on a Navree API with the `@navree/nuxt` module: the backend-for-frontend as Nitro routes (httpOnly token cookies, single-flight refresh, two-factor sign-in, CSRF, `/broadcasting/auth`), a session gate that guards pages and refreshes tokens before server rendering, and auto-imported `useNavreeAuth`, `useWorkspace`, `useNavreeClient`, `useListQuery`, `useOperation`, `useChannel`, `useAbility`, `<NavreeDataTable>` and `<Can>`; with `@navree/client` (the typed client from `navree openapi:client`, `NavreeError` with codes) and `@navree/theme` (navree.com's dark theme for Tailwind 4 and shadcn-vue). Use when creating or changing a Nuxt or Vue app that talks to a Navree API, wiring sign-in or sign-out, calling the API in pages, server routes or during SSR, building list screens, subscribing to private channels, following operations, translating API errors with @nuxtjs/i18n, or theming like navree.com. The reference app, with the full app UI (sidebar shell, command palette) and Playwright tests, is apps/crm-nuxt."
---

# A Nuxt front end on a Navree API

`@navree/nuxt` is a Nuxt module. Nitro serves the **backend-for-frontend**
at `/api/**` and `/broadcasting/auth`: Navree's access token and rotating
refresh token live in httpOnly cookies and every call is forwarded with
`Authorization: Bearer`. A session gate keeps pages behind sign-in and
refreshes expired tokens before a page renders. The browser never holds a
token.

Docs: `/docs/frontend/nuxt/` (options, composables, what the module adds),
`/docs/frontend/typed-client/`, `/docs/frontend/auth-and-bff/`,
`/docs/frontend/data-tables/`, `/docs/frontend/theming/`,
`/docs/frontend/localization/`. Sources and tests: `packages/js/nuxt`
(`test/fixture` is a minimal app; `pnpm smoke` builds and checks it).
`apps/crm-nuxt` is the Navree CRM on Nuxt: the reference for the look,
the shell and the tests ("The reference app" below).

## Steps

1. **Install** `@navree/nuxt`, `@navree/client`, `@navree/theme` (from the
   repository's `packages/js`: `pnpm install && pnpm build`, then `link:`
   dependencies), plus `@casl/ability @casl/vue` for abilities and
   `pusher-js` for real time.
2. **Add the module:**

   ```ts
   // nuxt.config.ts
   export default defineNuxtConfig({
     modules: ["@navree/nuxt"],
     css: ["~/assets/css/main.css"],
     navree: {
       apiUrl: "http://127.0.0.1:8080",               // NAVREE_API_URL / NUXT_NAVREE_API_URL win at runtime
       sessionGate: { loginPath: "/login", publicPaths: ["/", "/pricing"] },
       realtime: { key: "app-key", wsHost: "127.0.0.1", wsPort: 8080 }, // optional, needs pusher-js
       abilities: true,                                // optional, needs @casl/ability and @casl/vue
       mounts: ["/admin"],                            // optional: more prefixes to forward
     },
   });
   ```

3. **Generate the schema:** `navree openapi:client --out web/app/lib/api`.
4. **Theme** (`app/assets/css/main.css`): `@import "tailwindcss";
   @import "@navree/theme/theme.css"; @import "@navree/theme/fonts.css";
   @source "../../../node_modules/@navree/nuxt/dist";` and
   `app: { head: { htmlAttrs: { class: "dark" } } }`.
5. **Pages:** use the auto-imported composables (below).
6. **Test:** the vue pieces from `@navree/nuxt/vue` with `@vue/test-utils`
   and a memory router; server handlers from `@navree/nuxt/server` on an h3
   app with `toWebHandler`; `@navree/bff/testing` for a scripted API
   (`fakeNavree`, `serveFakeNavree` for a built app).

## Composables (auto-imported)

| Composable | Use |
|---|---|
| `useNavreeAuth<User>()` | `user`, `status`, `signedIn`, `load()`, `signIn({email, password})` → `{twoFactor:false, user}` or `{twoFactor:true, expiresIn}`, `verifyTwoFactor({code})`, `signOut({all?})` |
| `useNavreeClient<paths>()` | The typed client (cookie mode, `X-Workspace` from the cookie; in-process on the server) |
| `useWorkspace()` | `workspace` (computed) and `setWorkspace(slug)` (the `navree_workspace` cookie, then `refreshNuxtData()`) |
| `useListQuery(defaults)` | `query`, `setQuery`, `setFilter`, `setSearch`, `toggleSort`, `setPage` in the route's query string |
| `useOperation(idRef)` | `operation`, `error`, `finished`, `cancel()` over `/api/operations/{id}/events` |
| `useChannel(channel, event, handler)`, `useRealtimeState()` | With `realtime`: private channels by bare name; a ref/getter channel moves the listener |
| `useAbility()`, `<Can>` | With `abilities`: rules load from `GET /api/me/abilities` after sign-in and each workspace switch |
| `applyFieldErrors(error, setFieldError, { fields, onOther })` | A 400's field errors onto vee-validate fields |
| `<NavreeDataTable>`, `<NavreePagination>` | The list table (`@update:query`, `@row-click`, `#empty` slot) |

Server side (Nitro auto-imports): `getNavreeSession(event)` →
`{user, accessToken, workspace}` or `null`; `useNavreeBff(event)` → the BFF
(`.api(path, {accessToken, workspace})`, `.me(token)`).

```html
<script setup lang="ts">
import type { paths } from "~/lib/api/schema";
const api = useNavreeClient<paths>();
const list = useListQuery({ per_page: 25 });
const { data, pending } = await useAsyncData("deals", () => unwrap(api.GET("/api/v1/deals", { params: { query: list.query.value } })), { watch: [list.query] });
</script>

<template>
  <NavreeDataTable :columns="columns" :data="data?.data" :meta="data?.meta" :query="list.query.value"
    :loading="pending" clickable @update:query="list.setQuery" @row-click="(d) => navigateTo(`/deals/${d.id}`)" />
</template>
```

## What the BFF guarantees (don't re-implement it)

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
  sends the address the outermost proxy saw. Or set `NAVREE_CLIENT_IP_HEADER`
  to a header the proxy overwrites with the bare client address.
- **Workspaces.** `X-Workspace` is forwarded (else the `navree_workspace`
  cookie); server-sent events stream through too.
- **Its own errors:** `session_expired`, `two_factor_expired`,
  `invalid_origin`, `api_unreachable`, `unexpected_response`.

## Errors and locales

- `unwrap` throws `NavreeError` (`status`, `code`, `errors`, `is(...)`,
  `requestId`).
- With @nuxtjs/i18n: `const { t, te } = useI18n(); translateError(error, t,
  { has: te })` (keys `errors.<code>`, then `errors.unknown`).
- `useHead({ htmlAttrs: { lang: locale, dir: () => direction(locale.value) } })`
  with `direction` from `@navree/nuxt/vue`; use logical classes.

## The reference app: apps/crm-nuxt

`apps/crm-nuxt` (the Navree CRM on Nuxt) is built on the `@navree/nuxt`
module (`nuxt.config.ts` and `navree.options.ts`): copy its look, shell and
tests. Every path below is a file there. Architecture background:
`/docs/patterns/backend-for-frontend/`. The React version of the same app is
`apps/crm-web` (skill `navree-frontend-next`). The module provides the
backend-for-frontend routes and the session gate, so the app has no
`server/` folder.

Its steps:

1. **Create the app** (Nuxt 4, `app/` source directory) and add the pieces:
   `pnpm add @pinia/nuxt pinia @tanstack/vue-query @casl/ability @casl/vue pusher-js vee-validate zod reka-ui @vueuse/core vue-sonner @lucide/vue class-variance-authority clsx tailwind-merge`
   and for development
   `pnpm add -D tailwindcss @tailwindcss/vite tw-animate-css msw vitest @nuxt/test-utils @vue/test-utils happy-dom @playwright/test vue-tsc typescript h3`.
2. **Configure Nuxt** (`nuxt.config.ts`): `ssr: false` (every screen sits
   behind sign-in; Nitro still serves the BFF),
   `modules: ["@navree/nuxt", "@pinia/nuxt"]`, `navree: navreeOptions()`,
   `vite.plugins: [tailwindcss()]`, `typescript.strict: true`, and the
   runtime config below. The CRM also sets `components: { dirs: [] }` so
   every file imports its components explicitly.
3. **Export the API contract.** From the Navree app:
   `navree openapi:export web/openapi.json` (the CRM:
   `cargo run -p crm-api -- openapi:export ../crm-nuxt/openapi.json`). Add
   `"gen:api": "navree openapi:client --input ./openapi.json --out app/lib/api"`
   and run `pnpm gen:api` whenever the API changes.
4. **Set up the module.** `navree.options.ts` holds its settings (the API
   address, the session gate, the socket and the abilities); the module
   adds the BFF routes and the session gate (next section).
5. **Add the API client and plugins**: `app/lib/api/client.ts` on
   `fetch.ts` (the typed fetch core of `@navree/client`, copied),
   `errors.ts`, `form-errors.ts`, `keys.ts`, `types.ts`; plugins for TanStack
   Query, CASL and the 401/403 hooks (`app/plugins/02.vue-query.ts`,
   `03.casl.ts`, `04.api-events.ts`).
6. **Load the user and their abilities before any screen renders**
   (`app/middleware/session.global.ts`).
7. **Add real time**: turn on `realtime` in `navree.options.ts`, listen with
   the module's `useChannel` (`app/composables/useRealtime.ts`), and list the
   web origin in the API's `realtime.allowed_origins`.
8. **Theme it** with the navree.com tokens and the app UI rules ("The look").
9. **Test it**: Vitest for the app's module settings and components;
   Playwright against the real API.

### The BFF from the module

#### Routes

The module registers these Nitro routes; the app writes none of them:

| Route | Purpose |
|---|---|
| `POST /api/auth/login` | Signs in at Navree, sets the cookies, returns only `{user}` (or `{two_factor_required, expires_in}`) |
| `POST /api/auth/2fa/verify` | Completes a two-factor sign-in with `{code}` or `{recovery_code}` |
| `POST /api/auth/logout` | Revokes the refresh token at Navree (`{all: true}`: every device), clears the cookies |
| `POST /api/auth/refresh` | Explicit refresh: 204 with new cookies, or 401 |
| `/api/**` | Every other `/api/*` call, every method, with the bearer token |
| `POST /broadcasting/auth` | Signs private-channel subscriptions |
| The session gate (a server middleware) | A page request (`Accept: text/html`) with neither cookie gets `302 /login?next=...` |

All of them are `@navree/bff`, mounted by the module, with one instance per
server so the single-flight refresh map is shared. Server code of your own
reaches it with `useNavreeBff()`.

```ts
// navree.options.ts (abridged)
export function navreeOptions(env: Record<string, string | undefined> = process.env): NavreeModuleOptions {
  const mocks = env.NUXT_PUBLIC_API_MOCKS === "1" || env.NUXT_PUBLIC_API_MOCKS === "true";
  return {
    apiUrl: "http://127.0.0.1:8092", // NAVREE_API_URL changes it at runtime
    sessionGate: mocks ? false : { loginPath: "/login" },
    realtime: mocks ? false : { key: "navree-crm-key", wsHost: "127.0.0.1", wsPort: 8092 },
    abilities: true,
  };
}
```

Whether a cookie is still valid is the API's call; the gate only spares a
signed-out visitor the app shell. The app redirects on a 401 too.

### The browser side

#### Runtime config

```ts
// nuxt.config.ts (abridged)
navree: navreeOptions(), // the module's settings: apiUrl, sessionGate, realtime, abilities
runtimeConfig: {
  public: {
    apiMocks: false, // NUXT_PUBLIC_API_MOCKS=1: MSW in the browser
    demoAccounts: false,
  },
},
```

The module's settings land in runtime config, so environment variables
override them when the server starts and one build runs anywhere:
`NAVREE_API_URL` (or `NUXT_NAVREE_API_URL`) is the API's address,
`NUXT_PUBLIC_NAVREE_REALTIME=false` turns the socket off, and
`NUXT_PUBLIC_NAVREE_REALTIME_KEY`, `NUXT_PUBLIC_NAVREE_REALTIME_WS_HOST` and
`NUXT_PUBLIC_NAVREE_REALTIME_WS_PORT` say where the API's socket is.
`app/composables/useCrmConfig.ts` reads them typed.

#### The client

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

#### Queries

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
`url.set({ search: "agra" })` replaces the entry and resets the page), so
views survive reloads and can be shared.

#### Session and abilities

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

#### Forms

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

### Real time

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

### The look

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

### Testing

- **The module's settings:** `tests/unit/navree-module.test.ts` checks
  `navree.options.ts` and that the app carries no BFF of its own. The BFF,
  its cookies, refresh, Origin check and two-factor steps are tested in
  `packages/js/bff` and `packages/js/nuxt`, with `@navree/bff/testing`.
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
cd packages/js && pnpm check && pnpm smoke
```

In `apps/crm-nuxt`:

```bash
pnpm packages                  # once: installs and builds packages/js, which the app links
pnpm install
pnpm typecheck && pnpm test && pnpm build
pnpm e2e                       # real API on :8092 + production build on :3102
pnpm screenshots               # every screen at 1440 and 390 px into ./screenshots
pnpm dev:mock                  # the whole app on MSW, no API
```

## Common mistakes

- Keeping a hand-written `server/api/[...path].ts` next to the module (two
  handlers for `/api/**`): the app has no BFF files of its own.
- Calling `$fetch("/api/...")` during SSR without cookies: use
  `useNavreeClient()` or `useNavree().fetch`, which forward them in-process.
- Turning on `realtime` or `abilities` without installing `pusher-js` or
  `@casl/ability` and `@casl/vue`.
- Reading `useListQuery().query` without `.value` in script code.
- Putting a token in `localStorage`, a non-httpOnly cookie or the Nuxt
  payload, or calling Navree from the browser: everything goes through Nitro.
- `useCookie` or `$fetch` to Navree from a page: with `ssr: false` that runs
  in the browser. Server routes only.
- Refreshing in parallel. Two requests spending the same refresh token make
  Navree revoke the session; keep the BFF's refresh single-flight and share
  the module's one BFF instance per server (`useNavreeBff()`).
- A new non-GET server route that skips `useNavreeBff().proxy` (or
  `originAllowed`): it has no CSRF check.
- Nesting a `TooltipTrigger` around a `DropdownMenuTrigger` or
  `PopoverTrigger`: Reka anchors both to the nearest popper (the tooltip's)
  and the menu is never positioned. Pass the trigger element to the content:
  `<DropdownMenuContent :reference="triggerEl">` (see `UserMenu.vue`).
- Naming channels with the prefix twice: the wrapper adds `private-`, so
  listen on `crm.deals`.
- A web origin missing from `realtime.allowed_origins`: the socket is refused
  and the page silently falls back to refetching.
- Green for navigation, selection or hover (`bg-accent` is not a hover
  color), `rounded-full` buttons, glowing shadows, or raw hex values in
  components (see the theme rules in `/docs/frontend/theming/#the-rules`).
