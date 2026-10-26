import { randomBytes } from "node:crypto";
import path from "node:path";
import { defineConfig, devices, type PlaywrightTestConfig } from "@playwright/test";

/**
 * End-to-end tests.
 *
 * - `pnpm e2e`: against the in-browser MSW mock API (`next dev` with
 *   `NEXT_PUBLIC_API_MOCKS=1`); only the smoke test runs meaningfully there.
 * - `E2E_REAL_API=1 pnpm e2e`: the whole system, no mocks. Playwright starts
 *   crm-api on a fresh, seeded database (`e2e/start-api.mjs`: migrate:fresh,
 *   db:seed crm-demo, serve on 127.0.0.1:8080) and the production build of this
 *   app (`pnpm build && pnpm start` on :3000) talking to it through the BFF.
 *   The first run compiles the API (several minutes).
 * - `E2E_BASE_URL=http://localhost:3000`: use servers you started yourself.
 */
const port = Number(process.env.E2E_PORT ?? 3000);
const baseURL = process.env.E2E_BASE_URL ?? `http://localhost:${port}`;
const realApi = process.env.E2E_REAL_API === "1";

const repoRoot = path.resolve(__dirname, "../..");
const apiUrl = "http://127.0.0.1:8080";

// One key per run, shared by every Playwright process through the environment: the API signs
// its JWTs with a key derived from APP_KEY. Test-only; nothing persists beyond the run.
process.env.E2E_APP_KEY ??= `base64:${randomBytes(32).toString("base64")}`;

const realServers: PlaywrightTestConfig["webServer"] = [
  {
    name: "crm-api",
    command: "node e2e/start-api.mjs",
    url: `${apiUrl}/health/ready`,
    env: {
      CARGO_TARGET_DIR: process.env.CARGO_TARGET_DIR ?? path.join(repoRoot, "target/agents/crm-e2e"),
      APP_KEY: process.env.E2E_APP_KEY,
      // A database of its own: the suite never touches the dev database (storage/crm.db).
      APP_DATABASE__URL: "sqlite://storage/e2e.db?mode=rwc",
      // Every browser reaches the API through the BFF, i.e. from 127.0.0.1: lift the per-IP limit.
      APP_HTTP__PER_IP_RATE_LIMIT: "100000",
      APP_LOG__LEVEL: process.env.E2E_API_LOG ?? "warn",
    },
    // The first `cargo run` compiles the whole API.
    timeout: 20 * 60_000,
    reuseExistingServer: false,
    stdout: "pipe",
    stderr: "pipe",
  },
  {
    name: "crm-web",
    command: `pnpm run build && pnpm start --port ${port}`,
    url: `${baseURL}/login`,
    env: {
      NAVIS_API_URL: apiUrl,
      // `next start` is a production server: allow the token cookies over plain http.
      NAVIS_COOKIE_SECURE: "0",
      NEXT_PUBLIC_API_MOCKS: "0",
      NEXT_PUBLIC_REALTIME: "1",
      NEXT_PUBLIC_PUSHER_KEY: "navis-crm-key",
      NEXT_PUBLIC_WS_HOST: "127.0.0.1",
      NEXT_PUBLIC_WS_PORT: "8080",
    },
    timeout: 5 * 60_000,
    reuseExistingServer: false,
    stdout: "pipe",
  },
];

const mockServer: PlaywrightTestConfig["webServer"] = {
  command: `pnpm exec next dev --port ${port}`,
  url: `${baseURL}/login`,
  env: { NEXT_PUBLIC_API_MOCKS: "1" },
  reuseExistingServer: !process.env.CI,
  timeout: 120_000,
};

export default defineConfig({
  testDir: "./e2e",
  // The real API is one shared database: run the specs one at a time so they never race.
  fullyParallel: !realApi,
  workers: realApi ? 1 : undefined,
  forbidOnly: Boolean(process.env.CI),
  retries: process.env.CI ? 2 : 0,
  timeout: realApi ? 60_000 : 30_000,
  expect: { timeout: 10_000 },
  reporter: process.env.CI ? [["list"], ["html", { open: "never" }]] : "list",
  use: {
    baseURL,
    trace: "retain-on-failure",
    // crm-api's day boundaries ("due today", "overdue") are UTC (spec §9.5); run the browser in UTC too.
    timezoneId: "UTC",
  },
  projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"] } }],
  webServer: process.env.E2E_BASE_URL ? undefined : realApi ? realServers : mockServer,
});
