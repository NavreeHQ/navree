import path from "node:path";
import type { NextConfig } from "next";

/**
 * The Navree packages (`@navree/next`, `@navree/client`, `@navree/bff`) are linked
 * from this checkout's `packages/js`, which is outside this app's directory. Next
 * reads files outside the app only below its root, so the root is the checkout,
 * and the packages are compiled with the app. See README.md, "The Navree packages".
 */
const checkout = path.resolve(process.cwd(), "../..");

/**
 * The browser never talks to the Navree API directly. The route handlers
 * `src/app/api/[...path]/route.ts` and `src/app/broadcasting/auth/route.ts` are the
 * backend-for-frontend (`@navree/next`, created in `src/lib/navree.ts`): they keep the
 * JWTs in httpOnly cookies and forward requests to `NAVREE_API_URL` with
 * `Authorization: Bearer`.
 */
const nextConfig: NextConfig = {
  // The CRM is a cookie-authenticated client app: every screen loads its data in
  // the browser through TanStack Query. Cache Components (and Activity-preserved
  // hidden routes) buy nothing here and keep stale dialogs/duplicate DOM around,
  // so we keep the classic App Router rendering model.
  cacheComponents: false,
  // Allow the dev server to be opened on 127.0.0.1 as well as localhost.
  allowedDevOrigins: ["127.0.0.1", "localhost"],
  turbopack: { root: checkout },
  outputFileTracingRoot: checkout,
  transpilePackages: ["@navree/next", "@navree/client", "@navree/bff"],
};

export default nextConfig;
