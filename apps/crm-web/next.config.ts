import type { NextConfig } from "next";

/**
 * The browser never talks to the Keel API directly. Route handlers under
 * `src/app/api` and `src/app/broadcasting` are a backend-for-frontend: they
 * keep the JWTs in httpOnly cookies and forward requests to `KEEL_API_URL`
 * with `Authorization: Bearer` (see `src/lib/bff`).
 */
const nextConfig: NextConfig = {
  // The CRM is a cookie-authenticated client app: every screen loads its data in
  // the browser through TanStack Query. Cache Components (and Activity-preserved
  // hidden routes) buy nothing here and keep stale dialogs/duplicate DOM around,
  // so we keep the classic App Router rendering model.
  cacheComponents: false,
  // Allow the dev server to be opened on 127.0.0.1 as well as localhost.
  allowedDevOrigins: ["127.0.0.1", "localhost"],
};

export default nextConfig;
