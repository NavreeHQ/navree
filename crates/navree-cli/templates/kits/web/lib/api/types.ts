/**
 * The API's shapes, from the types generated from its OpenAPI document
 * (`lib/api/schema.d.ts`; `pnpm gen:api` regenerates them), so a changed DTO is a
 * TypeScript error here before it is a bug.
 */
import type { components } from "./schema";

type Schemas = components["schemas"];

/** The user resource, with the kit's `two_factor_enabled` (`TwoFactorStatus` in the API). */
export type User = Schemas["UserResource"] & { two_factor_enabled?: boolean };
export type Workspace = Schemas["WorkspaceResource"];
export type WorkspaceSettings = Schemas["WorkspaceSettings"];
export type Member = Schemas["MemberResource"];
export type Invitation = Schemas["InvitationResource"];
export type Project = Schemas["Project"];
export type ProjectStatus = Schemas["ProjectStatus"];
export type ProjectPage = Schemas["ProjectPage"];
/** The query of `GET /api/v1/projects`, sent as `?filter[status]=active&search=web&sort=-created_at&page=2`. */
export type ProjectListQuery = {
  filter?: { status?: ProjectStatus };
  search?: string;
  sort?: string;
  page?: number;
  per_page?: number;
};
export type CreateProject = Schemas["CreateProject"];
export type UpdateProject = Schemas["UpdateProject"];
export type BillingOverview = Schemas["BillingOverview"];
export type Plan = Schemas["PlanResource"];
export type Subscription = Schemas["SubscriptionResource"];
export type Interval = Schemas["Interval"];
export type Session = Schemas["AccountSession"];
export type Provider = Schemas["ProviderResource"];
export type NotificationSettings = Schemas["NotificationSettings"];
export type TwoFactorSetup = Schemas["TwoFactorSetup"];
export type RecoveryCodes = Schemas["RecoveryCodesResponse"];
export type CheckoutUrl = Schemas["CheckoutUrl"];

/** The roles of a workspace (`owner`, `admin`, `member`, `viewer`, or one the app defines). */
export const ROLES = ["owner", "admin", "member", "viewer"] as const;

/** An invoice (`GET /api/billing/invoices`). */
export interface Invoice {
  id: string;
  number: string;
  status: "draft" | "open" | "paid" | "void" | "uncollectible";
  total_cents: number;
  currency: string;
  pdf_url: string | null;
  hosted_url: string | null;
  created_at: string;
}

/** `{data: [...]}`, the shape of the API's lists. */
export interface DataList<T> {
  data: T[];
}

/** One rule of `GET /api/me/abilities` and `GET /api/v1/settings/abilities` (CASL's JSON). */
export interface CaslRule {
  action: string | string[];
  subject: string | string[];
  conditions?: Record<string, unknown>;
  fields?: string | string[];
  inverted?: boolean;
  reason?: string;
}

/** What the sign-in routes answer: the user, or a second step. */
export type SignInResult = { user: User } | { two_factor_required: true; expires_in: number };

/** The real-time event of a project change on `private-workspace.{id}`. */
export interface ProjectChanged {
  action: "created" | "updated" | "deleted";
  project: Project;
}
