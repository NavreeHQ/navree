/**
 * Abilities in Vue: one app-wide CASL ability (`@casl/vue`) from the rules
 * `GET /api/me/abilities` serves. Every Navree record carries `__type`, so
 * records are subjects as they come from JSON.
 *
 * `@navree/nuxt/abilities` needs `@casl/ability` and `@casl/vue`.
 */
import { abilitiesPlugin, useAbility as useCaslAbility } from "@casl/vue";
import { createAbility, type AppAbility, type CaslRule } from "@navree/client/abilities";
import { inject, readonly, ref, type App, type InjectionKey, type Ref } from "vue";

export { Can } from "@casl/vue";
export type { AppAbility, CaslRule } from "@navree/client/abilities";

/** Creates the app's ability (empty until `ability.update(rules)`) and installs `@casl/vue` with it. */
export function installAbilities(app: App, rules: readonly CaslRule[] = []): AppAbility {
  const ability = createAbility(rules);
  app.use(abilitiesPlugin, ability, { useGlobalProperties: true });
  return ability;
}

const READY: InjectionKey<Readonly<Ref<boolean>>> = Symbol("navree-ability-ready");

/**
 * Provides the "rules loaded" flag to the app and returns the ref that sets it. `true` once the
 * rules for the current user and workspace are in, `false` while they load (after sign-in and after
 * a workspace switch). Signed out there is nothing to load, so it is `true`.
 */
export function provideAbilityReady(app: App): Ref<boolean> {
  const ready = ref(false);
  app.provide(READY, readonly(ready));
  return ready;
}

/**
 * Whether the app's rules are loaded. Until they are, `can(...)` answers from an empty rule set, so a
 * shell that shows or hides screens by ability waits for this (a skeleton until it is `true`).
 * Apps without `abilities: true` get a flag that is always `true`.
 */
export function useAbilityReady(): Readonly<Ref<boolean>> {
  return inject(READY, readonly(ref(true)));
}

/** The app's ability: `can("update", deal)` is reactive. */
export function useAbility(): AppAbility {
  return useCaslAbility<AppAbility>();
}
