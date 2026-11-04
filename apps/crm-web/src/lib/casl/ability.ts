import type { AppAbility } from "@navree/client/abilities";

/**
 * The app's one ability is built by `@navree/next/abilities` from the rules `GET /api/me/abilities`
 * serves (`<AbilityProvider>`, `useAbility()` and `<Can>`); `createAbility` builds one outside React
 * (the mock API). Every API resource carries a constant `__type`, which is the subject.
 */
export type { AppAbility };

/**
 * Actions the API grants: CRUD + `manage` (any), plus `move` on Deal, `export`
 * on Contact and `complete` on Activity.
 */
export type AppAction = "read" | "create" | "update" | "delete" | "manage" | "move" | "export" | "complete";
