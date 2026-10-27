/**
 * Abilities on the client: the CASL rules `GET /api/me/abilities` serves,
 * as a `@casl/ability` ability. Every Navree resource carries a constant
 * `__type` (`"Deal"`), so API records are subjects as they come from JSON.
 * The UI hides what the rules deny; the API still checks every request.
 *
 * `@navree/client/abilities` needs `@casl/ability` (an optional peer dependency).
 */
import { createMongoAbility, type MongoAbility, type MongoQuery, type RawRuleOf } from "@casl/ability";
import { NavreeError } from "./errors.js";

/** A CASL raw rule as `GET /api/me/abilities` serves it. */
export interface CaslRule {
  action: string | string[];
  subject: string | string[];
  conditions?: Record<string, unknown>;
  fields?: string | string[];
  inverted?: boolean;
  reason?: string;
}

/** A subject: a type name (`"Deal"`), `"all"`, or a record with `__type`. */
export type Subject = string | { __type: string; [field: string]: unknown };

/** The app's ability. Narrow the actions and subjects with your own `MongoAbility<[Action, Subject]>`. */
export type AppAbility = MongoAbility<[string, Subject], MongoQuery>;

/** The type name of a subject: the string itself, or a record's `__type`. */
export function subjectType(subject: Subject): string {
  return typeof subject === "string" ? subject : subject.__type;
}

/** An ability from CASL rules (empty: everything denied until `update(rules)`). */
export function createAbility<A extends AppAbility = AppAbility>(rules: readonly CaslRule[] = []): A {
  return createMongoAbility<A>(rules as unknown as RawRuleOf<A>[], {
    detectSubjectType: (subject) => subjectType(subject as Subject) as never,
  });
}

/** The route that serves the signed-in user's rules. */
export const ABILITIES_PATH = "/api/me/abilities";

/** Loads the signed-in user's rules (`client.raw` or `fetch` sends the request). */
export async function fetchAbilities(
  options: { fetch?: (path: string, init?: RequestInit) => Promise<Response>; path?: string } = {},
): Promise<CaslRule[]> {
  const send = options.fetch ?? ((path: string, init?: RequestInit) => globalThis.fetch(path, init));
  const response = await send(options.path ?? ABILITIES_PATH, { headers: { accept: "application/json" } });
  const body: unknown = await response.json().catch(() => null);
  if (!response.ok) throw NavreeError.fromResponse(response, body);
  return body as CaslRule[];
}
