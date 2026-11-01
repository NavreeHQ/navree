import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    environment: "node",
    include: ["src/**/*.test.ts", "test/**/*.test.ts"],
    restoreMocks: true,
    // The type tests (`*.test-d.ts`) run through the TypeScript compiler.
    typecheck: { enabled: true, include: ["src/**/*.test-d.ts"], tsconfig: "./tsconfig.json" },
  },
});
