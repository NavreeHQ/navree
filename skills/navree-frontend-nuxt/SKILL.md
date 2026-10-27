---
name: navree-frontend-nuxt
description: "Build a Nuxt (3.13+ or 4) front end on a Navree API with the `@navree/nuxt` module: the backend-for-frontend as Nitro routes (httpOnly token cookies, single-flight refresh, two-factor sign-in, CSRF, `/broadcasting/auth`), a session gate that guards pages and refreshes tokens before server rendering, and auto-imported `useNavreeAuth`, `useWorkspace`, `useNavreeClient`, `useListQuery`, `useOperation`, `useChannel`, `useAbility`, `<NavreeDataTable>` and `<Can>`; with `@navree/client` (the typed client from `navree openapi:client`, `NavreeError` with codes) and `@navree/theme` (navree.com's dark theme for Tailwind 4 and shadcn-vue). Use when creating or changing a Nuxt or Vue app that talks to a Navree API, wiring sign-in or sign-out, calling the API in pages, server routes or during SSR, building list screens, subscribing to private channels, following operations, translating API errors with @nuxtjs/i18n, or theming like navree.com."
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

Cookies `navree_at`/`navree_rt` (httpOnly, `SameSite=Lax`, `Secure` in
production), `navree_2fa` on `/api/auth/2fa`; refresh single-flight per
refresh token; `403 invalid_origin` for cross-origin writes; `X-Workspace`
forwarded (else the cookie); bodies streamed (CSV, SSE); the browser's
`X-Forwarded-For` dropped (`NAVREE_TRUSTED_PROXY_HOPS` behind proxies). Its
own errors: `session_expired`, `two_factor_expired`, `invalid_origin`,
`api_unreachable`, `unexpected_response`.

## Errors and locales

- `unwrap` throws `NavreeError` (`status`, `code`, `errors`, `is(...)`,
  `requestId`).
- With @nuxtjs/i18n: `const { t, te } = useI18n(); translateError(error, t,
  { has: te })` (keys `errors.<code>`, then `errors.unknown`).
- `useHead({ htmlAttrs: { lang: locale, dir: () => direction(locale.value) } })`
  with `direction` from `@navree/nuxt/vue`; use logical classes.

## Checks

```bash
pnpm typecheck && pnpm test && pnpm build
cd packages/js && pnpm check && pnpm smoke
```

## Common mistakes

- Keeping a hand-written `server/api/[...path].ts` next to the module (two
  handlers for `/api/**`): remove the app's BFF files when adopting it.
- Calling `$fetch("/api/...")` during SSR without cookies: use
  `useNavreeClient()` or `useNavree().fetch`, which forward them in-process.
- Turning on `realtime` or `abilities` without installing `pusher-js` or
  `@casl/ability` and `@casl/vue`.
- Reading `useListQuery().query` without `.value` in script code.
- Using `bg-accent` as a hover color or green for navigation (see the
  theme rules in `/docs/frontend/theming/#the-rules`).
