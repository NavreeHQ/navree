# @navree/nuxt

Nuxt on a Navree API, with one line in `nuxt.config.ts`. Nitro serves the backend-for-frontend, a session gate keeps pages behind sign-in and refreshes expired tokens before a page renders, and the composables and the data table are auto-imported. It works with server-side rendering on and off.

It needs Node.js 20 or later, Nuxt 4 and Vue 3.5.

## Install

```sh
npm install @navree/nuxt @navree/client @navree/theme
```

For abilities add `@casl/ability` and `@casl/vue`; for real time add `pusher-js`.

## Quick start

Generate the client's schema from your Navree app, then add the module:

```sh
navree openapi:client --out web/app/lib/api
```

```ts
// nuxt.config.ts
export default defineNuxtConfig({
  modules: ["@navree/nuxt"],
  navree: {
    apiUrl: "http://127.0.0.1:8080",
    sessionGate: { loginPath: "/login", publicPaths: ["/", "/pricing"] },
    abilities: true,
  },
});
```

## What is in it

- The module: the backend-for-frontend as Nitro routes (httpOnly token cookies, refresh rotation, CSRF, channel authorization) and the session gate.
- Composables for auth, workspaces, abilities, real-time channels, operations and list queries, and `<NavreeDataTable>`.
- `@navree/nuxt/vue`, `@navree/nuxt/abilities` (needs `@casl/ability` and `@casl/vue`), `@navree/nuxt/realtime` (needs `pusher-js`) and `@navree/nuxt/server` are separate entry points.

Documentation: https://navree.com/docs/frontend/nuxt/

## License

MIT OR Apache-2.0
