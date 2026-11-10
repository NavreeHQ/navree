# @navree/next

Next.js (App Router) on a Navree API. The server side mounts the backend-for-frontend as route handlers, gives server components the signed-in user and a typed client, and guards pages in the proxy. The browser side is hooks and components: list queries in the URL, a data table, operations, workspaces and form errors.

It needs Node.js 20 or later, Next.js 15 or later and React 19.

## Install

```sh
npm install @navree/next @navree/client @navree/theme
```

For abilities add `@casl/ability` and `@casl/react`; for real time add `pusher-js`.

## Quick start

Generate the client's schema from your Navree app, then create the app's Navree:

```sh
navree openapi:client
```

```ts
// src/lib/navree.ts
import { createNavree } from "@navree/next/server";

export const navree = createNavree();
```

Settings come from the `NAVREE_*` variables when the first request arrives. Mount the route handlers and guard the pages:

```ts
// src/app/api/[...path]/route.ts
import { navree } from "@/lib/navree";

export const dynamic = "force-dynamic";
export const { GET, POST, PUT, PATCH, DELETE, HEAD } = navree.handlers;
```

```ts
// src/proxy.ts (middleware.ts before Next.js 16)
import { navree } from "@/lib/navree";

export const proxy = navree.proxy({ loginPath: "/login", publicPaths: ["/", "/pricing"] });

export const config = { matcher: ["/((?!_next/|favicon.ico).*)"] };
```

## What is in it

- `@navree/next/server`: `createNavree()` with the route handlers, `auth()` for server components, `client()` and the proxy that refreshes expired tokens before a page renders.
- `@navree/next`: hooks and components for list queries, a data table, operations, workspaces and form errors.
- `@navree/next/abilities` (needs `@casl/ability` and `@casl/react`) and `@navree/next/realtime` (needs `pusher-js`) are separate entry points.
- Tokens stay in httpOnly cookies, through `@navree/bff`.

Documentation: https://navree.com/docs/frontend/nextjs/

## License

MIT OR Apache-2.0
