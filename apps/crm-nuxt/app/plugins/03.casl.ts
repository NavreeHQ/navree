import { abilitiesPlugin } from "@casl/vue";
import { createAppAbility } from "@/lib/casl/ability";

/**
 * One CASL ability for the whole app. It starts empty; the shell loads the
 * signed-in user's rules from `GET /api/me/abilities` and applies them with
 * `ability.update()`, which re-renders everything that asked `can(...)`.
 */
export default defineNuxtPlugin({
  name: "navree-casl",
  setup(nuxtApp) {
    const ability = createAppAbility();
    nuxtApp.vueApp.use(abilitiesPlugin, ability, { useGlobalProperties: true });
    return { provide: { ability } };
  },
});
