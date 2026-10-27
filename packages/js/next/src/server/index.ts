/**
 * `@navree/next/server`: route handlers, `auth()`, `client()` and the proxy
 * for a Next.js app on a Navree API. Server only.
 */
export { createNavree, type Navree, type NavreeNextOptions, type NavreeProxyOptions, type NavreeSession } from "./navree.js";
export {
  ACCESS_COOKIE,
  bffOptionsFromEnv,
  CHALLENGE_COOKIE,
  createBff,
  REFRESH_COOKIE,
  WORKSPACE_COOKIE,
  type Bff,
  type BffOptions,
  type Session,
} from "@navree/bff";
