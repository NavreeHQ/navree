/**
 * Abilities: one app-wide CASL ability. Its rules (`GET /api/me/abilities`)
 * load after sign-in and again after a workspace switch; signed out, every
 * rule is dropped.
 */
import { fetchAbilities } from "@navree/client/abilities";
import { defineNuxtPlugin, useNuxtApp } from "#imports";
import { watch } from "vue";
import { installAbilities, provideAbilityReady } from "../vue/abilities.js";

export default defineNuxtPlugin({
  name: "navree-abilities",
  dependsOn: ["navree"],
  setup() {
    const nuxtApp = useNuxtApp();
    const { auth, workspace, client } = nuxtApp.$navree;
    const ability = installAbilities(nuxtApp.vueApp);
    const ready = provideAbilityReady(nuxtApp.vueApp);

    const load = async () => {
      // Not known yet whether anyone is signed in: nothing is loaded, and nothing is decided.
      if (auth.status.value === "unknown") {
        ready.value = false;
        return;
      }
      if (!auth.signedIn.value) {
        ability.update([]);
        ready.value = true;
        return;
      }
      ready.value = false;
      try {
        ability.update((await fetchAbilities({ fetch: (path, init) => client.raw(path, init) })) as never);
      } catch {
        ability.update([]);
      } finally {
        ready.value = true;
      }
    };
    watch([auth.status, workspace.workspace], () => void load(), { immediate: true });
    return { provide: { ability } };
  },
});
