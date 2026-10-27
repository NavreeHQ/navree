// A minimal app on the module, built by test/smoke.mjs to check that the module works in a real Nuxt build.
export default defineNuxtConfig({
  compatibilityDate: "2026-10-01",
  devtools: { enabled: false },
  telemetry: false,
  modules: ["@navree/nuxt"],
  navree: {
    apiUrl: "http://127.0.0.1:9",
    mounts: ["/admin"],
    sessionGate: { loginPath: "/login", publicPaths: ["/about"] },
    realtime: { key: "app-key", wsHost: "127.0.0.1", wsPort: 9 },
    abilities: true,
  },
});
