/**
 * What the Nuxt module does, written against a small slice of `@nuxt/kit`
 * so it can be tested without a Nuxt build.
 */
import { defu } from "defu";
import type { BffPages, BffSocialPaths } from "@navree/bff";
import type { SessionGateOptions } from "./server/handlers.js";

export interface RealtimeSettings {
  /** The API's `realtime.key`. */
  key: string;
  wsHost: string;
  wsPort: number;
  forceTLS?: boolean;
}

export interface NavreeModuleOptions {
  /** The Navree API (default `http://127.0.0.1:8080`). `NAVREE_API_URL` or `NUXT_NAVREE_API_URL` override it at runtime. */
  apiUrl?: string;
  /** The API prefix the BFF forwards (default `/api`), or `false` to mount the routes yourself. */
  apiPrefix?: string | false;
  /** `POST /broadcasting/auth` for private channels (default true). */
  broadcasting?: boolean;
  /** More prefixes to forward with the session's token, such as a server-rendered tool at `/admin`. */
  mounts?: string[];
  /**
   * Social sign-in: `GET /auth/social/{provider}` and `GET /auth/callback/{provider}` (default off).
   * The routes end on `pages.signIn`, `pages.twoFactor` and the page the sign-in started from.
   */
  social?: boolean;
  /** Where the social sign-in routes are mounted (defaults `/auth/social` and `/auth/callback`). */
  socialPaths?: BffSocialPaths;
  /**
   * The pages the BFF's redirects end on: `signIn` (default: the session gate's `loginPath`),
   * `twoFactor` (`/two-factor`) and `verifyEmail` (`/verify-email`, where the link in the
   * verification mail lands).
   */
  pages?: BffPages;
  /** Send a page request that the API refuses with 401 (a tool mounted at `/admin`) to the sign-in page. Default off. */
  redirectNavigations?: boolean;
  /** Pages behind sign-in (default: everything but `/login`), or `false` for none. */
  sessionGate?: SessionGateOptions | false;
  /** The API's `workspace.header` (default `X-Workspace`). */
  workspaceHeader?: string;
  /** The API's real-time server, for pusher-js (needs `pusher-js`). Off by default. */
  realtime?: RealtimeSettings | false;
  /** One app-wide CASL ability loaded after sign-in (needs `@casl/ability` and `@casl/vue`). Off by default. */
  abilities?: boolean;
}

/** The `@nuxt/kit` functions the module uses. */
export interface NuxtKit {
  resolve(path: string): string;
  addServerHandler(handler: { route?: string; method?: string; middleware?: boolean; handler: string }): void;
  addServerImports(imports: { name: string; from: string }[]): void;
  addImports(imports: { name: string; as?: string; from: string }[]): void;
  addPlugin(plugin: string | { src: string; mode?: "client" | "server" | "all" }): void;
  addComponent(component: { name: string; export?: string; filePath: string }): void;
}

export interface NuxtLike {
  options: {
    runtimeConfig: Record<string, unknown> & { public: Record<string, unknown> };
    build: { transpile: unknown[] };
  };
}

export const defaults = {
  apiPrefix: "/api",
  broadcasting: true,
  mounts: [] as string[],
  social: false,
  sessionGate: { loginPath: "/login" } as SessionGateOptions | false,
  workspaceHeader: "X-Workspace",
  realtime: false as RealtimeSettings | false,
  abilities: false,
};

export function setupNavree(options: NavreeModuleOptions, nuxt: NuxtLike, kit: NuxtKit): void {
  const settings = { ...defaults, ...options };
  const runtime = (file: string) => kit.resolve(`./runtime/${file}`);
  const apiPrefix = settings.apiPrefix === false ? false : settings.apiPrefix.replace(/\/+$/, "");
  const sessionGate = settings.sessionGate === false ? false : { loginPath: "/login", ...settings.sessionGate };

  nuxt.options.runtimeConfig.navree = defu(nuxt.options.runtimeConfig.navree as object | undefined, {
    apiUrl: settings.apiUrl ?? "",
    workspaceHeader: settings.workspaceHeader,
    sessionGate,
    pages: { ...(sessionGate === false ? {} : { signIn: sessionGate.loginPath }), ...settings.pages },
    socialPaths: settings.socialPaths,
    redirectNavigations: settings.redirectNavigations,
  });
  nuxt.options.runtimeConfig.public.navree = defu(nuxt.options.runtimeConfig.public.navree as object | undefined, {
    authPrefix: `${apiPrefix === false ? "/api" : apiPrefix}/auth`,
    operationsPath: `${apiPrefix === false ? "/api" : apiPrefix}/operations`,
    loginPath: sessionGate === false ? "/login" : sessionGate.loginPath,
    workspaceHeader: settings.workspaceHeader,
    realtime: settings.realtime,
    abilities: settings.abilities,
  });

  // The backend-for-frontend: one catch-all handler routes sign-in, refresh and sign-out, and proxies the rest.
  const bff = runtime("server/bff-handler.js");
  if (apiPrefix !== false) kit.addServerHandler({ route: `${apiPrefix}/**`, handler: bff });
  if (settings.broadcasting) kit.addServerHandler({ route: "/broadcasting/auth", method: "post", handler: bff });
  for (const mount of settings.mounts) kit.addServerHandler({ route: `${mount.replace(/\/+$/, "")}/**`, handler: bff });
  if (settings.social) {
    const start = (settings.socialPaths?.start ?? "/auth/social").replace(/\/+$/, "");
    const callback = (settings.socialPaths?.callback ?? "/auth/callback").replace(/\/+$/, "");
    kit.addServerHandler({ route: `${start}/**`, method: "get", handler: bff });
    kit.addServerHandler({ route: `${callback}/**`, method: "get", handler: bff });
  }
  if (sessionGate !== false) kit.addServerHandler({ middleware: true, handler: runtime("server/session-gate.js") });

  kit.addServerImports([
    { name: "getNavreeSession", from: runtime("server/utils.js") },
    { name: "useNavreeBff", from: runtime("server/utils.js") },
  ]);

  const composables = runtime("composables.js");
  const vue = kit.resolve("./vue/index.js");
  kit.addImports([
    { name: "useNavree", from: composables },
    { name: "useNavreeAuth", from: composables },
    { name: "useWorkspace", from: composables },
    { name: "useNavreeClient", from: composables },
    { name: "useOperation", from: vue },
    { name: "useListQuery", from: vue },
    { name: "applyFieldErrors", from: vue },
  ]);
  kit.addComponent({ name: "NavreeDataTable", export: "NavreeDataTable", filePath: vue });
  kit.addComponent({ name: "NavreePagination", export: "NavreePagination", filePath: vue });

  kit.addPlugin(runtime("plugin.js"));
  if (settings.realtime) {
    const realtime = kit.resolve("./vue/realtime.js");
    kit.addPlugin({ src: runtime("plugin-realtime.js"), mode: "client" });
    kit.addImports([
      { name: "useChannel", from: realtime },
      { name: "useRealtimeState", from: realtime },
    ]);
  }
  if (settings.abilities) {
    const abilities = kit.resolve("./vue/abilities.js");
    kit.addPlugin(runtime("plugin-abilities.js"));
    kit.addImports([
      { name: "useAbility", from: abilities },
      { name: "useAbilityReady", from: abilities },
    ]);
    kit.addComponent({ name: "Can", export: "Can", filePath: abilities });
  }

  nuxt.options.build.transpile.push(kit.resolve("./runtime"));
}
