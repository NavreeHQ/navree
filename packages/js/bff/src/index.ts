/**
 * `@navree/bff`: the backend-for-frontend for Navree APIs, on web-standard
 * `Request` and `Response`. `@navree/next` and `@navree/nuxt` mount it; any
 * other server that speaks web requests can too.
 */
export {
  ACCESS_COOKIE,
  CHALLENGE_COOKIE,
  createBff,
  readCookie,
  REFRESH_COOKIE,
  SOCIAL_COOKIE,
  tokenExpiresAt,
  WORKSPACE_COOKIE,
  type Bff,
  type BffOptions,
  type BffPages,
  type BffSocialPaths,
  type CookieNames,
  type Session,
  type TokenPair,
} from "./bff.js";
export { bffOptionsFromEnv } from "./config.js";
export { errorBody, errorResponse, reasonPhrase, type ErrorBody } from "./http-errors.js";
