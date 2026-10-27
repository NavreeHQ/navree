/** Real time (client only): one pusher-js connection, opened once the app is mounted. */
import { defineNuxtPlugin, useNuxtApp, useRuntimeConfig } from "#imports";
import { createRealtimeController, REALTIME_KEY } from "../vue/realtime.js";

export default defineNuxtPlugin({
  name: "navree-realtime",
  dependsOn: ["navree"],
  setup() {
    const nuxtApp = useNuxtApp();
    const server = useRuntimeConfig().public.navree.realtime;
    if (!server) return;
    const controller = createRealtimeController({
      server,
      authorize: (path, init) => nuxtApp.$navree.client.raw(path, init),
    });
    nuxtApp.vueApp.provide(REALTIME_KEY, controller);
    nuxtApp.hook("app:mounted", () => controller.connect());
    return { provide: { navreeRealtime: controller } };
  },
});
