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
  /** Same-origin requests through the BFF (on the server: in-process, with the browser's cookies). */
  fetch: (path: string, init?: RequestInit) => Promise<Response>;
}

type EventFetch = (path: string, init?: RequestInit) => Promise<Response>;

export default defineNuxtPlugin({
  name: "navree",
  enforce: "pre",
  setup(): { provide: { navree: NavreeApp } } {
    const config = useRuntimeConfig().public.navree;
    const event = import.meta.server ? useRequestEvent() : undefined;
    const origin = import.meta.server ? useRequestURL().origin : "";
    // Nitro's event.fetch serves a path in-process and adds the browser's request headers (its cookies) under ours.
    const eventFetch = event ? ((event as unknown as { fetch: EventFetch }).fetch as EventFetch) : undefined;

    const send: EventFetch = (path, init) =>
      eventFetch ? eventFetch(path, init) : globalThis.fetch(path, { credentials: "same-origin", ...init });

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
      // On the server, send the client's request to the BFF in-process: as a path, with its own headers and body.
      fetch: eventFetch
        ? async (request) => {
            const url = new URL(request.url);
            const method = request.method.toUpperCase();
            return eventFetch(url.pathname + url.search, {
              method,
              headers: Object.fromEntries(request.headers),
              body: method === "GET" || method === "HEAD" ? undefined : await request.arrayBuffer(),
            });
          }
        : undefined,
      onUnauthorized: () => {
        if (import.meta.client) void navigateTo(`${config.loginPath}?next=${encodeURIComponent(location.pathname + location.search)}`);
      },
    });

    return { provide: { navree: { auth, workspace, client, fetch: send } } };
  },
});
