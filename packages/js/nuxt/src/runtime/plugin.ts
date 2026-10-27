/**
 * The module's app plugin: one sign-in state, one workspace and one typed
 * client per app, provided as `$navree` (`useNavree()`).
 */
import { createNavreeClient, type NavreeClient } from "@navree/client";
import { defineNuxtPlugin, navigateTo, refreshNuxtData, useCookie, useRequestEvent, useRequestURL, useRuntimeConfig } from "#imports";
import { createNavreeAuth, type NavreeAuth } from "../vue/auth.js";
import { createWorkspace, WORKSPACE_COOKIE, type Workspace } from "../vue/workspace.js";

export interface NavreeApp {
  auth: NavreeAuth<unknown>;
  workspace: Workspace;
  client: NavreeClient<object>;
  /** Same-origin requests through the BFF (on the server: Nitro's local fetch with the browser's cookies). */
  fetch: (path: string, init?: RequestInit) => Promise<Response>;
}

export default defineNuxtPlugin({
  name: "navree",
  enforce: "pre",
  setup(): { provide: { navree: NavreeApp } } {
    const config = useRuntimeConfig().public.navree;
    const event = import.meta.server ? useRequestEvent() : undefined;
    const origin = import.meta.server ? useRequestURL().origin : "";

    // On the server, Nitro's event.fetch calls the BFF routes in-process and forwards the browser's cookies.
    const send = (path: string, init?: RequestInit): Promise<Response> =>
      event ? (event as unknown as { fetch: (input: string, init?: RequestInit) => Promise<Response> }).fetch(path, init) : globalThis.fetch(path, { credentials: "same-origin", ...init });

    const auth = createNavreeAuth<unknown>({
      fetch: send,
      authPrefix: config.authPrefix,
      onSignedOut: () => {
        if (import.meta.client) void navigateTo(config.loginPath);
      },
    });

    const cookie = useCookie<string | null | undefined>(WORKSPACE_COOKIE, { sameSite: "lax", maxAge: 60 * 60 * 24 * 365, path: "/" });
    const workspace = createWorkspace(cookie, () => {
      void refreshNuxtData();
    });

    const client = createNavreeClient<object>({
      baseUrl: origin,
      workspace: () => workspace.workspace.value,
      workspaceHeader: config.workspaceHeader,
      fetch: event ? (request) => (event as unknown as { fetch: (input: Request) => Promise<Response> }).fetch(request) : undefined,
      onUnauthorized: () => {
        if (import.meta.client) void navigateTo(`${config.loginPath}?next=${encodeURIComponent(location.pathname + location.search)}`);
      },
    });

    return { provide: { navree: { auth, workspace, client, fetch: send } } };
  },
});
