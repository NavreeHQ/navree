/**
 * The slice of Nuxt's and Nitro's `#imports` the runtime files use. Nuxt
 * generates the real module when it builds the app; this declaration only
 * lets the package compile on its own.
 */
declare module "#imports" {
  import type { H3Event } from "h3";
  import type { Ref } from "vue";

  export function defineNuxtPlugin<T>(plugin: T): T;
  export function useRuntimeConfig(event?: H3Event): {
    navree: {
      apiUrl: string;
      workspaceHeader: string;
      sessionGate: false | { loginPath?: string; publicPaths?: string[] };
      pages?: { signIn?: string; twoFactor?: string; verifyEmail?: string };
      socialPaths?: { start?: string; callback?: string };
      redirectNavigations?: boolean;
    };
    public: {
      navree: {
        authPrefix: string;
        operationsPath: string;
        loginPath: string;
        workspaceHeader: string;
        realtime: false | { key: string; wsHost: string; wsPort: number; forceTLS?: boolean };
        abilities: boolean;
      };
    };
  };
  export function useCookie<T = string | null | undefined>(
    name: string,
    options?: { sameSite?: "lax" | "strict" | "none"; maxAge?: number; path?: string; secure?: boolean },
  ): Ref<T>;
  export function useRequestEvent(): H3Event | undefined;
  export function useRequestURL(): URL;
  export function navigateTo(to: string, options?: { replace?: boolean; external?: boolean }): unknown;
  export function useNuxtApp(): {
    $navree: import("./plugin.js").NavreeApp;
    vueApp: import("vue").App;
    hook(name: string, fn: (...args: unknown[]) => unknown): void;
  };
  export function refreshNuxtData(keys?: string | string[]): Promise<void>;
}
