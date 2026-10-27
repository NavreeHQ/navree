/**
 * Abilities in Vue: one app-wide CASL ability (`@casl/vue`) from the rules
 * `GET /api/me/abilities` serves. Every Navree record carries `__type`, so
 * records are subjects as they come from JSON.
 *
 * `@navree/nuxt/abilities` needs `@casl/ability` and `@casl/vue`.
 */
import { abilitiesPlugin, useAbility as useCaslAbility } from "@casl/vue";
import { createAbility, type AppAbility, type CaslRule } from "@navree/client/abilities";
import type { App } from "vue";

export { Can } from "@casl/vue";
export type { AppAbility, CaslRule } from "@navree/client/abilities";

/** Creates the app's ability (empty until `ability.update(rules)`) and installs `@casl/vue` with it. */
export function installAbilities(app: App, rules: readonly CaslRule[] = []): AppAbility {
  const ability = createAbility(rules);
  app.use(abilitiesPlugin, ability, { useGlobalProperties: true });
  return ability;
}

/** The app's ability: `can("update", deal)` is reactive. */
export function useAbility(): AppAbility {
  return useCaslAbility<AppAbility>();
}
