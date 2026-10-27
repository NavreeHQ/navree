"use client";

/**
 * Abilities in React: one app-wide CASL ability from the rules
 * `GET /api/me/abilities` serves, `useAbility()` and `<Can>` (from
 * `@casl/react`, which re-renders its users when the rules change).
 */
import { AbilityProvider as CaslAbilityProvider, Can as CaslCan, useAbility as useCaslAbility, type CanProps } from "@casl/react";
import { createAbility, type AppAbility, type CaslRule } from "@navree/client/abilities";
import { useEffect, useState, type ReactNode } from "react";

export interface AbilityProviderProps {
  /** The user's rules; when they change (sign-in, a workspace switch) the ability updates in place. */
  rules: readonly CaslRule[] | undefined;
  children: ReactNode;
}

/** Provides the app's ability to `useAbility()` and `<Can>`. Everything is denied until the rules arrive. */
export function AbilityProvider({ rules, children }: AbilityProviderProps) {
  const [ability] = useState(() => createAbility(rules ?? []));
  useEffect(() => {
    ability.update((rules ?? []) as never);
  }, [ability, rules]);
  return <CaslAbilityProvider value={ability}>{children}</CaslAbilityProvider>;
}

/** The app's ability: `ability.can("update", deal)`. Re-renders when the rules change. */
export function useAbility(): AppAbility {
  try {
    return useCaslAbility<AppAbility>();
  } catch {
    throw new Error("useAbility() needs an <AbilityProvider> above it");
  }
}

/** `<Can I="update" this={deal}>…</Can>` against the app's ability. */
export const Can = CaslCan as (props: CanProps<AppAbility>) => ReactNode;
