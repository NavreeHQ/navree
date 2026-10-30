import { createMongoAbility, type ExtractSubjectType, type MongoAbility, type RawRuleOf } from "@casl/ability";
import type { AppSubject, CaslRule, SubjectName } from "@/lib/api/types";

/**
 * Actions the API grants: CRUD + `manage` (any), plus `move` on Deal, `export`
 * on Contact and `complete` on Activity.
 */
export type AppAction = "read" | "create" | "update" | "delete" | "manage" | "move" | "export" | "complete";

/** Subjects are type names (`"Deal"`) or API records carrying `__type`. */
export type AppSubjects = SubjectName | AppSubject | "all";

export type AppAbility = MongoAbility<[AppAction, AppSubjects]>;

/**
 * Creates the app ability from the CASL rule array served by `GET /api/me/abilities`.
 * Every API resource carries a constant `__type`, so CASL needs no `subject()`
 * helper (which would not survive JSON serialisation).
 */
export function createAppAbility(rules: readonly CaslRule[] = []): AppAbility {
  return createMongoAbility<AppAbility>(rules as RawRuleOf<AppAbility>[], {
    // `__type` is typed `string` for some resources in the OpenAPI document; the API always sends the subject name.
    detectSubjectType: (o) => (typeof o === "string" ? o : o.__type) as ExtractSubjectType<AppSubjects>,
  });
}
