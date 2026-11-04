import { fileURLToPath } from "node:url";
import { defineVitestProject } from "@nuxt/test-utils/config";
import { defineConfig } from "vitest/config";

/**
 * Two projects:
 * - `unit` (Node): the app's module settings and layout, the API client, forms, formatting, CASL rules, the
 *   board's drag maths, the mock bus and the mock API. Fast; no Nuxt build. Needs the Navree packages built
 *   (`pnpm packages`).
 * - `nuxt`: components mounted in a real Nuxt app (`mountSuspended`), with the `@navree/nuxt` module and the
 *   app's plugins.
 */
export default defineConfig({
  test: {
    projects: [
      {
        resolve: { alias: { "@": fileURLToPath(new URL("./app", import.meta.url)) } },
        test: { name: "unit", include: ["tests/unit/**/*.test.ts"], environment: "node", restoreMocks: true },
      },
      await defineVitestProject({
        test: { name: "nuxt", include: ["tests/nuxt/**/*.test.ts"], environment: "nuxt", restoreMocks: true },
      }),
    ],
  },
});
