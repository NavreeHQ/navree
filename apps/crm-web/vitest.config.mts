import { fileURLToPath } from "node:url";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: { "@": fileURLToPath(new URL("./src", import.meta.url)) },
    // The Navree packages are linked from the checkout (README.md, "The Navree packages") and
    // have a React of their own in `packages/js`. Their components must use this app's React,
    // or hooks fail with "Invalid hook call".
    dedupe: ["react", "react-dom"],
  },
  test: {
    environment: "jsdom",
    include: ["src/**/*.test.{ts,tsx}"],
    restoreMocks: true,
    // `@casl/react` is a dependency of `@navree/next`, outside this app. Vitest would load it with
    // Node's own resolution (and its own React); processing it here resolves React as the app does.
    server: { deps: { inline: ["@casl/react"] } },
  },
});
