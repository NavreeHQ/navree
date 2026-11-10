# @navree/bff

The backend-for-frontend for Navree APIs, on web-standard `Request` and `Response`. The browser only talks to your app's own server. This package keeps the access token and the rotating refresh token in httpOnly cookies and forwards requests to the API with `Authorization: Bearer`.

`@navree/next` and `@navree/nuxt` mount it for you. Any other server that speaks web requests can mount it too. It needs Node.js 20 or later.

## Install

```sh
npm install @navree/bff
```

## Quick start

```ts
import { bffOptionsFromEnv, createBff } from "@navree/bff";

// NAVREE_API_URL, NAVREE_COOKIE_SECURE, APP_ORIGINS and the other NAVREE_* settings.
const bff = createBff(bffOptionsFromEnv());

// In a catch-all route for /api/*: sign-in, two-factor, sign-out and refresh are
// answered here, every other call goes to the API with the bearer token.
export const handler = (request: Request) => bff.handle(request);

// In middleware or server rendering: a valid access token, refreshed when it has expired.
const session = await bff.ensureSession(request);
```

## What it does

- Sign-in with two-factor, sign-out and a single-flight refresh, so concurrent requests share one rotation.
- Social sign-in, the link in the verification mail, and a redirect to the sign-in page for page requests that lose their session.
- A streaming proxy for `/api/*`, `/broadcasting/auth` and any server-rendered tool you mount.
- An `Origin` check on every non-GET request, which is the CSRF defence for the cookie hop.
- The browser's `X-Forwarded-For` never reaches the API. The BFF sends its own only when it knows the client address.
- `@navree/bff/testing`: a scripted API and browser requests for tests.

Documentation: https://navree.com/docs/frontend/auth-and-bff/

## License

MIT OR Apache-2.0
