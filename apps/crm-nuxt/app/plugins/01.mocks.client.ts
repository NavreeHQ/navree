import { createRealtimeController, installRealtime } from "@navree/nuxt/realtime";

/**
 * With NUXT_PUBLIC_API_MOCKS=1 the API is served by an MSW service worker in
 * the browser (demo data, no crm-api). The plugin awaits the worker, so the
 * first requests are already intercepted when the app mounts. Live updates
 * come from the mock bus, which the module's `useChannel` listens on in place
 * of the socket (the module's own socket is off in this mode).
 */
export default defineNuxtPlugin({
  name: "navree-mocks",
  parallel: false,
  async setup(nuxtApp) {
    if (!useRuntimeConfig().public.apiMocks) return;
    try {
      const [{ startMockApi }, { connectMockBus }] = await Promise.all([
        import("@/mocks/browser"),
        import("@/lib/realtime/mock-bus"),
      ]);
      await startMockApi();
      const realtime = createRealtimeController();
      await realtime.connect(() => connectMockBus());
      installRealtime(nuxtApp.vueApp, realtime);
    } catch (error) {
      console.error("[navree] Could not start the mock API", error);
    }
  },
});
