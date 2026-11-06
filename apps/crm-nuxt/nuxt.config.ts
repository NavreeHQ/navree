import { createRequire } from "node:module";
import { dirname } from "node:path";
import { fileURLToPath } from "node:url";
import tailwindcss from "@tailwindcss/vite";
import { navreeOptions } from "./navree.options";

/** This app, and the Navree front-end packages (`packages/js`) that are installed into it with `link:` (see README). */
const appRoot = fileURLToPath(new URL(".", import.meta.url));
const navreePackages = fileURLToPath(new URL("../../packages/js", import.meta.url));

/**
 * The `h3` that Nitro (Nuxt's server) is built on. `@navree/nuxt` is linked from the checkout, so its
 * server handlers would otherwise bundle the `h3` of `packages/js/nuxt/node_modules` beside Nitro's own.
 * `undefined` when it can't be found, which leaves the bundler to resolve it.
 */
function nitroH3(): string | undefined {
  try {
    const fromApp = createRequire(import.meta.url);
    const fromNuxt = createRequire(fromApp.resolve("nuxt/package.json"));
    const fromNitro = createRequire(fromNuxt.resolve("nitropack/package.json"));
    return dirname(fromNitro.resolve("h3/package.json"));
  } catch {
    return undefined;
  }
}
const h3 = nitroH3();

/**
 * Navree CRM on Nuxt 4.
 *
 * The browser app is a single-page app (`ssr: false`): every screen sits behind
 * sign-in and loads its data with TanStack Query, like a desktop tool. The
 * `@navree/nuxt` module is the backend-for-frontend: Nitro serves `/api/**` and
 * `/broadcasting/auth` with the Navree tokens in httpOnly cookies, a session gate
 * keeps pages behind sign-in, and the module's composables carry sign-in, real
 * time and abilities. The browser never sees a token. The module's settings are in
 * `navree.options.ts`.
 */
export default defineNuxtConfig({
  compatibilityDate: "2026-10-01",
  ssr: false,
  devtools: { enabled: false },
  telemetry: false,

  modules: ["@navree/nuxt", "@pinia/nuxt"],

  navree: navreeOptions(),

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
    // The API (NAVREE_API_URL), the cookies (NAVREE_COOKIE_SECURE) and the socket
    // (NUXT_PUBLIC_NAVREE_REALTIME_*) are the module's settings: see navree.options.ts and .env.example.
    public: {
      /** NUXT_PUBLIC_API_MOCKS=1 serves the API from MSW in the browser (demo data, no crm-api). */
      apiMocks: false,
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
    resolve: {
      // The packages are linked from the checkout, so they resolve their own copies of these from
      // packages/js. One copy of each keeps Vue's injection, vue-router's state, CASL's ability and
      // the socket shared between the packages and the app.
      dedupe: ["vue", "vue-router", "@casl/ability", "@casl/vue", "pusher-js"],
    },
    server: {
      // The linked packages sit outside this app's folder, and naming a folder here replaces Vite's own default.
      fs: { allow: [appRoot, navreePackages] },
    },
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
    // Bundle the linked packages into the server instead of loading them from the checkout at run time.
    externals: { inline: ["@navree/nuxt", "@navree/bff", "@navree/client"] },
    // ... with Nitro's own h3, so the handlers and the server share one.
    ...(h3 ? { alias: { h3 } } : {}),
  },
});
