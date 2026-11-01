import tailwindcss from "@tailwindcss/vite";

/**
 * Navree CRM on Nuxt 4.
 *
 * The browser app is a single-page app (`ssr: false`): every screen sits behind
 * sign-in and loads its data with TanStack Query, like a desktop tool. Nitro is
 * the backend-for-frontend: `server/api/**` and `server/routes/broadcasting/**`
 * hold the Navree tokens in httpOnly cookies and forward calls to the API. The
 * browser never sees a token.
 */
export default defineNuxtConfig({
  compatibilityDate: "2026-10-01",
  ssr: false,
  devtools: { enabled: false },
  telemetry: false,

  modules: ["@pinia/nuxt"],

  // Components are imported explicitly: every file says where its parts come from.
  components: { dirs: [] },

  css: ["~/assets/css/main.css"],

  app: {
    head: {
      htmlAttrs: { lang: "en", class: "dark" },
      // The title template is a function, so it is set in app.vue (nuxt.config's head is serialized).
      title: "Navree CRM",
      meta: [
        { name: "description", content: "Pipeline, accounts and follow-ups for the Navree sales team." },
        { name: "theme-color", content: "#040605" },
        { name: "color-scheme", content: "dark" },
      ],
      link: [{ rel: "icon", type: "image/svg+xml", href: "/icon.svg" }],
    },
  },

  runtimeConfig: {
    // Server only. Overridden at runtime by NUXT_NAVREE_API_URL, NUXT_COOKIE_SECURE, ...
    // (server/lib/instance.ts also reads the plain NAVREE_* names, which the Next.js CRM uses too).
    navreeApiUrl: "",
    public: {
      /** NUXT_PUBLIC_API_MOCKS=1 serves the API from MSW in the browser (demo data, no crm-api). */
      apiMocks: false,
      /** NUXT_PUBLIC_REALTIME=0 turns the WebSocket off (queries still refetch on focus). */
      realtime: true,
      pusherKey: "navree-crm-key",
      wsHost: "127.0.0.1",
      wsPort: 8092,
      /** Shows the seeded demo accounts on the login page (always on with the mock API). */
      demoAccounts: false,
    },
  },

  typescript: {
    strict: true,
    typeCheck: false,
  },

  vite: {
    plugins: [tailwindcss()],
    optimizeDeps: {
      include: [
        "@tanstack/vue-query",
        "@casl/ability",
        "@casl/vue",
        "reka-ui",
        "@vueuse/core",
        "vee-validate",
        "zod",
        "pusher-js",
        "vue-sonner",
        "class-variance-authority",
        "clsx",
        "tailwind-merge",
        "@lucide/vue",
      ],
    },
  },

  nitro: {
    // The BFF streams API responses through; nothing is pre-rendered.
    compressPublicAssets: true,
  },
});
