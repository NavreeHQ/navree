import { randomBytes } from "node:crypto";
import { defineConfig, devices, type PlaywrightTestConfig } from "@playwright/test";

/**
 * End-to-end tests against the real Navree CRM API, no mocks.
 *
 * `pnpm e2e` starts both servers:
 * 1. crm-api on http://127.0.0.1:8092 (`scripts/start-api.mjs --fresh --db e2e`): migrate:fresh,
 *    db:seed crm-demo and serve, on its own SQLite file target/builds/crm-nuxt/e2e.db and a fresh
 *    APP_KEY per run;
 * 2. this app's production build (`nuxt build`, then `node .output/server/index.mjs`) on :3102,
 *    talking to it through the `@navree/nuxt` module's BFF (the Navree packages must be built first: `pnpm packages`).
 *
 * `E2E_BASE_URL=http://localhost:3102 pnpm e2e` runs the specs against servers you started yourself
 * (crm-api seeded with crm-demo, e.g. `pnpm api:fresh` and `pnpm dev`).
 */
const port = Number(process.env.E2E_PORT ?? 3102);
const baseURL = process.env.E2E_BASE_URL ?? `http://localhost:${port}`;
const apiUrl = "http://127.0.0.1:8092";

// One key per run, shared by every Playwright process through the environment. Test-only.
process.env.E2E_APP_KEY ??= `base64:${randomBytes(32).toString("base64")}`;

const servers: PlaywrightTestConfig["webServer"] = [
  {
    name: "crm-api",
    command: "node scripts/start-api.mjs --fresh --db e2e",
    url: `${apiUrl}/health/ready`,
    env: { APP_KEY: process.env.E2E_APP_KEY, CRM_API_PORT: "8092", CRM_WEB_ORIGIN: `http://localhost:${port}` },
    timeout: 3 * 60_000,
    reuseExistingServer: false,
    stdout: "pipe",
    stderr: "pipe",
  },
  {
    name: "crm-nuxt",
    command: "pnpm run build && node .output/server/index.mjs",
    url: `${baseURL}/login`,
    env: {
      PORT: String(port),
      NAVREE_API_URL: apiUrl,
      // A production server over plain http: allow the token cookies without Secure.
      NAVREE_COOKIE_SECURE: "0",
      NUXT_PUBLIC_API_MOCKS: "0",
      NUXT_PUBLIC_NAVREE_REALTIME_WS_HOST: "127.0.0.1",
      NUXT_PUBLIC_NAVREE_REALTIME_WS_PORT: "8092",
    },
    timeout: 6 * 60_000,
    reuseExistingServer: false,
    stdout: "pipe",
  },
];

export default defineConfig({
  testDir: "./e2e",
  // One shared database: the specs run one at a time so they never race.
  fullyParallel: false,
  workers: 1,
  forbidOnly: Boolean(process.env.CI),
  retries: process.env.CI ? 2 : 0,
  timeout: 60_000,
  expect: { timeout: 10_000 },
  reporter: process.env.CI ? [["list"], ["html", { open: "never" }]] : "list",
  use: {
    baseURL,
    trace: "retain-on-failure",
    // crm-api's day boundaries ("due today", "overdue") are UTC; run the browser in UTC too.
    timezoneId: "UTC",
  },
  projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"] } }],
  webServer: process.env.E2E_BASE_URL ? undefined : servers,
});
