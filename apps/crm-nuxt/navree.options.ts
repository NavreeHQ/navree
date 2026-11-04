import type { NavreeModuleOptions } from "@navree/nuxt";

/**
 * The CRM's settings for the `@navree/nuxt` module (see `nuxt.config.ts`).
 *
 * With the mock API (`NUXT_PUBLIC_API_MOCKS=1`) the browser serves the API itself and the
 * session lives in the tab, not in cookies: the server has no session to gate on and there is no
 * socket to open, so the session gate and real time are off.
 */
export function navreeOptions(env: Record<string, string | undefined> = process.env): NavreeModuleOptions {
  const mocks = env.NUXT_PUBLIC_API_MOCKS === "1" || env.NUXT_PUBLIC_API_MOCKS === "true";
  return {
    // Where crm-api listens when `pnpm api` runs it. NAVREE_API_URL changes it at runtime.
    apiUrl: "http://127.0.0.1:8092",
    // Every page but /login needs a session. The gate also refreshes an expired access token before the page loads.
    sessionGate: mocks ? false : { loginPath: "/login" },
    // The API's realtime.key and its Pusher-protocol listener. NUXT_PUBLIC_NAVREE_REALTIME_WS_HOST and
    // NUXT_PUBLIC_NAVREE_REALTIME_WS_PORT change them at runtime; NUXT_PUBLIC_NAVREE_REALTIME=false turns the socket off.
    realtime: mocks ? false : { key: "navree-crm-key", wsHost: "127.0.0.1", wsPort: 8092 },
    // One app-wide CASL ability, loaded after sign-in.
    abilities: true,
  };
}
