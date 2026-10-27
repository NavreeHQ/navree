import type { NextConfig } from "next";

/**
 * The browser never talks to the Navree API directly. Route handlers under
 * `app/api`, `app/admin`, `app/auth` and `app/broadcasting` are a
 * backend-for-frontend: they keep the JWTs in httpOnly cookies and forward
 * requests to `NAVREE_API_URL` with `Authorization: Bearer` (see `lib/bff`).
 */
const nextConfig: NextConfig = {
  // Every screen loads its data in the browser through TanStack Query.
  cacheComponents: false,
  // Allow the dev server to be opened on 127.0.0.1 as well as localhost.
  allowedDevOrigins: ["127.0.0.1", "localhost"],
};

export default nextConfig;
