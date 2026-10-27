import { createMongoAbility, type MongoAbility, type RawRuleOf } from "@casl/ability";
import type { CaslRule } from "@/lib/api/types";

/** The actions the API grants: CRUD and `manage` (any). */
export type AppAction = "read" | "create" | "update" | "delete" | "manage";

/** A subject name (`"Project"`) or an API record, which carries its subject as `__type`. */
export type AppSubject = string | { __type: string };

export type AppAbility = MongoAbility<[AppAction, AppSubject]>;

/**
 * The ability of the rules the API serves (`GET /api/me/abilities` outside a workspace,
 * `GET /api/v1/settings/abilities` inside one), so buttons follow the API's rules. The API
 * checks again on every request: hiding a button is never the protection.
 */
export function abilityFrom(rules: readonly CaslRule[] = []): AppAbility {
  return createMongoAbility<AppAbility>(rules as RawRuleOf<AppAbility>[], {
    detectSubjectType: (subject) => (typeof subject === "string" ? subject : subject.__type),
  });
}
