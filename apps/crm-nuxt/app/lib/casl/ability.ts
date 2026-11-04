import type { MongoAbility } from "@casl/ability";
import type { AppSubject, SubjectName } from "@/lib/api/types";

/**
 * Actions the API grants: CRUD + `manage` (any), plus `move` on Deal, `export`
 * on Contact and `complete` on Activity.
 */
export type AppAction = "read" | "create" | "update" | "delete" | "manage" | "move" | "export" | "complete";

/** Subjects are type names (`"Deal"`) or API records carrying `__type`. */
export type AppSubjects = SubjectName | AppSubject | "all";

/**
 * The app's ability, as `useAbility<AppAbility>()` returns it. The `@navree/nuxt` module builds
 * the one app-wide ability from the rules `GET /api/me/abilities` serves (`navree.abilities`),
 * loads them after sign-in and drops them on sign-out; this type narrows its actions and subjects.
 * Every API resource carries a constant `__type`, so records are subjects as they come from JSON.
 */
export type AppAbility = MongoAbility<[AppAction, AppSubjects]>;
