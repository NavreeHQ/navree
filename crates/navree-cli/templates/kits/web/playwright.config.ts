import { defineConfig, devices } from "@playwright/test";

/**
 * Journeys against the real API: Playwright starts the API (`cargo run -- serve` in the app
 * root, on the database of `APP_DATABASE__URL`, seeded with `navree migrate:fresh --seed`) and
 * the built web app (`pnpm build` first), unless they already run (`reuseExistingServer`).
 */
export default defineConfig({
  testDir: "./e2e",
  fullyParallel: false,
  workers: 1,
  retries: process.env.CI ? 1 : 0,
  reporter: process.env.CI ? "github" : "list",
  use: {
    baseURL: process.env.E2E_BASE_URL ?? "http://localhost:3000",
    trace: "retain-on-failure",
  },
  projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"] } }],
  webServer: [
    {
      command: "cargo run --quiet -- serve",
      cwd: "..",
      url: "http://127.0.0.1:8080/health/live",
      reuseExistingServer: true,
      timeout: 600_000,
    },
    {
      command: "pnpm start",
      url: "http://localhost:3000/sign-in",
      reuseExistingServer: true,
      timeout: 120_000,
    },
  ],
});
