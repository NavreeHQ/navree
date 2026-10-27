/**
 * With NUXT_PUBLIC_API_MOCKS=1 the API is served by an MSW service worker in
 * the browser (demo data, no crm-api). The plugin awaits the worker, so the
 * first requests are already intercepted when the app mounts.
 */
export default defineNuxtPlugin({
  name: "navree-mocks",
  parallel: false,
  async setup() {
    if (!useRuntimeConfig().public.apiMocks) return;
    try {
      const { startMockApi } = await import("@/mocks/browser");
      await startMockApi();
    } catch (error) {
      console.error("[navree] Could not start the mock API", error);
    }
  },
});
