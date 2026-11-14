/**
 * `@navree/nuxt`: a Nuxt module for apps on a Navree API.
 *
 * ```ts
 * // nuxt.config.ts
 * export default defineNuxtConfig({
 *   modules: ["@navree/nuxt"],
 *   navree: { realtime: { key: "app-key", wsHost: "127.0.0.1", wsPort: 8080 }, abilities: true },
 * });
 * ```
 *
 * Nitro serves the backend-for-frontend (`/api/**`, `/broadcasting/auth`)
 * with the tokens in httpOnly cookies; a session gate keeps pages behind
 * sign-in; composables (`useNavreeAuth`, `useWorkspace`, `useNavreeClient`,
 * `useOperation`, `useListQuery`, `useChannel`, `useAbility`) and
 * `<NavreeDataTable>` are auto-imported.
 */
import {
  addComponent,
  addImports,
  addPlugin,
  addServerHandler,
  addServerImports,
  createResolver,
  defineNuxtModule,
} from "@nuxt/kit";
import type { NuxtModule } from "@nuxt/schema";
import { defaults, setupNavree, type NavreeModuleOptions } from "./setup.js";

export type { NavreeModuleOptions, RealtimeSettings } from "./setup.js";
export type { SessionGateOptions } from "./server/handlers.js";

const navreeModule: NuxtModule<NavreeModuleOptions> = defineNuxtModule<NavreeModuleOptions>({
  meta: {
    name: "@navree/nuxt",
    configKey: "navree",
    compatibility: { nuxt: ">=4.6.4" },
  },
  defaults,
  setup(options, nuxt) {
    const { resolve } = createResolver(import.meta.url);
    setupNavree(options, nuxt as never, {
      resolve,
      addServerHandler: (handler) => addServerHandler(handler as never),
      addServerImports: (imports) => addServerImports(imports),
      addImports: (imports) => addImports(imports),
      addPlugin: (plugin) => addPlugin(plugin as never),
      addComponent: (component) => void addComponent(component),
    });
  },
});

export default navreeModule;
