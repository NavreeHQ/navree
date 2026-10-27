/**
 * Friendly aliases for the API schema. Everything in the app imports types from
 * here, never from `schema.d.ts` (generated from crm-api's OpenAPI document by
 * `pnpm gen:api`), so a renamed schema only needs this file to be re-pointed.
 */
import type { components, paths as GeneratedPaths } from "./schema";

type Schemas = components["schemas"];

/** Navree error body: `{statusCode, message, error, code?, errors?}`. */
export type ApiErrorBody = Schemas["HttpError"];
/** Every list page inlines the same `meta` object. */
export type PageMeta = Schemas["CompanyPage"]["meta"];
export type Page<T> = { data: T[]; meta: PageMeta };

/**
 * A CASL raw rule as `GET /api/me/abilities` serves it. navree-access has no
 * OpenAPI schema for it, so it is written out here.
 */
export type CaslRule = {
  action: string | string[];
  subject: string | string[];
  conditions?: { [field: string]: unknown };
  fields?: string | string[];
  inverted?: boolean;
  reason?: string;
};

export type OwnerRef = Schemas["OwnerRef"];
export type CompanyRef = Schemas["CompanyRef"];
export type ContactRef = Schemas["ContactRef"];

export type Role = Schemas["UserRole"];
/**
 * `GET /api/v1/users` items. `GET /api/auth/me` and the login `user` return the
 * same shape (crm-users adds `roles` through a user resource extension), although
 * the framework documents them as `UserResource`, which has no `roles`.
 */
export type User = Schemas["User"];
export type Me = Schemas["Me"];
export type LoginInput = Schemas["LoginInput"];
/** Navree's login response (tokens + user). Only the BFF ever sees it. */
export type NavreeLoginResponse = Schemas["LoginResponse"];
/** Navree's login answer for an account with two-factor authentication. */
export type TwoFactorChallenge = Schemas["TwoFactorChallenge"];
export type TwoFactorVerifyInput = Schemas["TwoFactorVerifyInput"];
export type TokenPair = Schemas["TokenPair"];
/**
 * What the browser gets from the BFF's `POST /api/auth/login`: `{user}` (tokens
 * stay in httpOnly cookies), or, for a 2FA account, `{two_factor_required, challenge}`
 * where `challenge` is an opaque handle the BFF keeps the challenge token behind.
 */
export type BffLoginResponse = { user: User };
export type BffTwoFactorRequired = { two_factor_required: true; expires_in: number };
export type BffLoginResult = BffLoginResponse | BffTwoFactorRequired;
/** `POST /api/auth/2fa/verify` through the BFF: the challenge token is in a cookie, so only the proof is sent. */
export type BffTwoFactorInput = { code?: string; recovery_code?: string };
export type UserCreateInput = Schemas["CreateUser"];
export type UserUpdateInput = Schemas["UpdateUser"];

export type CompanySize = Schemas["CompanySize"];
export type CompanySort = Schemas["CompanySort"];
export type Company = Schemas["Company"];
export type CompanyDetail = Schemas["CompanyDetail"];
export type CompanyInput = Schemas["CompanyInput"];
export type CompanyUpdateInput = Schemas["CompanyUpdateInput"];
export type CompanyFilters = Schemas["CompanyFilters"];

export type ContactStatus = Schemas["ContactStatus"];
export type ContactFilters = Schemas["ContactFilters"];
export type Contact = Schemas["Contact"];
export type ContactInput = Schemas["ContactInput"];
export type ContactUpdateInput = Schemas["ContactUpdateInput"];

export type DealStage = Schemas["Stage"];
export type Deal = Schemas["Deal"];
export type DealInput = Schemas["DealInput"];
export type DealUpdateInput = Schemas["DealUpdateInput"];
export type DealMoveInput = Schemas["DealMoveInput"];
export type BoardStage = Schemas["BoardStage"];
export type DealBoard = Schemas["DealBoard"];

export type ActivityKind = Schemas["ActivityKind"];
export type ActivitySubjectType = Schemas["SubjectType"];
export type Activity = Schemas["Activity"];
export type ActivityInput = Schemas["ActivityInput"];
export type ActivityUpdateInput = Schemas["ActivityUpdateInput"];
export type ActivityDueFilter = "overdue" | "today" | "upcoming";

export type Dashboard = Schemas["Dashboard"];
export type AppNotification = Schemas["Notification"];
export type AuditEntry = Schemas["AuditEntry"];

/** Every resource the UI authorizes against carries `__type` for CASL. */
export type SubjectName = "Company" | "Contact" | "Deal" | "Activity" | "User";
export type AppSubject = Company | Contact | Deal | Activity | User;

// ---- Query parameters ----------------------------------------------------------
//
// navree-api cannot document query parameters yet (framework open issue W2/W6), so
// the generated `paths` say `query?: never` for every list endpoint. These are the
// filters of spec §7, merged into `paths` below so openapi-fetch type-checks them.

type Paging = { page?: number; per_page?: number };

export type UserListQuery = { search?: string } & Paging;
export type CompanyListQuery = {
  search?: string;
  owner_id?: string;
  industry?: string;
  sort?: CompanySort;
} & Paging;
export type ContactExportQuery = {
  search?: string;
  company_id?: string;
  status?: ContactStatus;
  owner_id?: string;
};
export type ContactListQuery = ContactExportQuery & Paging;
export type DealListQuery = {
  stage?: DealStage;
  owner_id?: string;
  company_id?: string;
  search?: string;
} & Paging;
export type DealBoardQuery = { owner_id?: string };
export type ActivityListQuery = {
  subject_type?: ActivitySubjectType;
  subject_id?: string;
  kind?: ActivityKind;
  assignee_id?: string;
  due?: ActivityDueFilter;
  completed?: boolean;
} & Paging;
export type NotificationListQuery = { unread?: boolean } & Paging;
export type AuditListQuery = {
  actor_id?: string;
  subject_type?: string;
  subject_id?: string;
  action?: string;
} & Paging;

interface QueryParams {
  "/api/v1/users": UserListQuery;
  "/api/v1/companies": CompanyListQuery;
  "/api/v1/contacts": ContactListQuery;
  "/api/v1/contacts/export": ContactExportQuery;
  "/api/v1/deals": DealListQuery;
  "/api/v1/deals/board": DealBoardQuery;
  "/api/v1/activities": ActivityListQuery;
  "/api/notifications": NotificationListQuery;
  "/api/audit": AuditListQuery;
}

type WithQuery<Operation, Query> = Operation extends { parameters: infer P }
  ? Omit<Operation, "parameters"> & { parameters: Omit<P, "query"> & { query?: Query } }
  : Operation;

/** The generated paths, with the documented query parameters of the list endpoints. */
export type paths = {
  [P in keyof GeneratedPaths]: P extends keyof QueryParams
    ? Omit<GeneratedPaths[P], "get"> & { get: WithQuery<GeneratedPaths[P]["get"], QueryParams[P]> }
    : GeneratedPaths[P];
};

// ---- Constants -------------------------------------------------------------------

/** Stage order and probabilities from the product spec (section 6). */
export const DEAL_STAGES: readonly { stage: DealStage; label: string; probability: number }[] = [
  { stage: "lead", label: "Lead", probability: 10 },
  { stage: "qualified", label: "Qualified", probability: 25 },
  { stage: "proposal", label: "Proposal", probability: 50 },
  { stage: "negotiation", label: "Negotiation", probability: 75 },
  { stage: "won", label: "Won", probability: 100 },
  { stage: "lost", label: "Lost", probability: 0 },
] as const;

export const OPEN_STAGES: readonly DealStage[] = ["lead", "qualified", "proposal", "negotiation"];

export const COMPANY_SIZES: readonly CompanySize[] = ["1-10", "11-50", "51-200", "201-1000", "1000+"];
export const CONTACT_STATUSES: readonly ContactStatus[] = ["lead", "prospect", "customer", "churned"];
export const ACTIVITY_KINDS: readonly ActivityKind[] = ["note", "call", "email", "meeting", "task"];
export const ROLES: readonly Role[] = ["admin", "manager", "rep"];

/** The largest deal value crm-api accepts, in minor units (OpenAPI `maximum` on `amount_cents`). */
export const MAX_AMOUNT_CENTS = 1_000_000_000_000_000;

export function stageLabel(stage: DealStage): string {
  return DEAL_STAGES.find((s) => s.stage === stage)?.label ?? stage;
}

export function stageProbability(stage: DealStage): number {
  return DEAL_STAGES.find((s) => s.stage === stage)?.probability ?? 0;
}
