import { describe, expect, it } from "vitest";
import { ACTION_OPTIONS, entryLabel } from "./actions";

/**
 * The audit actions crm-api records for each operation the CRM offers. Data rows come from
 * navree-audit's change observer (`<model>.<created|updated|deleted>` for each model write in a
 * request's unit of work); companies and contacts are soft-deleted by an update that sets
 * `deleted_at` (crm-companies companies/service.rs, contacts/service.rs), deals and activities
 * are deleted; users are never deleted. Role changes come from navree-access (`assign_role` on create,
 * `sync_user_roles` on PATCH, crm-users service.rs); sign-ins from navree-auth.
 */
const RECORDED_BY_OPERATION: Record<string, string[]> = {
  "POST /api/v1/companies": ["company.created"],
  "PATCH /api/v1/companies/{id}": ["company.updated"],
  "DELETE /api/v1/companies/{id}": ["company.updated"],
  "POST /api/v1/contacts": ["contact.created"],
  "PATCH /api/v1/contacts/{id}": ["contact.updated"],
  "DELETE /api/v1/contacts/{id}": ["contact.updated"],
  "GET /api/v1/contacts/export": ["contacts.exported"],
  "POST /api/v1/deals": ["deal.created"],
  "PATCH /api/v1/deals/{id}": ["deal.updated"],
  "POST /api/v1/deals/{id}/move": ["deal.updated"],
  "DELETE /api/v1/deals/{id}": ["activity.deleted", "deal.deleted"],
  "POST /api/v1/activities": ["activity.created"],
  "PATCH /api/v1/activities/{id}": ["activity.updated"],
  "POST /api/v1/activities/{id}/complete": ["activity.updated"],
  "DELETE /api/v1/activities/{id}": ["activity.deleted"],
  "POST /api/v1/users": ["auth.register", "user.created", "access.role_assigned"],
  "PATCH /api/v1/users/{id} {name}": ["user.updated"],
  "PATCH /api/v1/users/{id} {role}": ["access.user_roles_synced"],
  "POST /api/auth/login": ["auth.login"],
  "POST /api/auth/login (wrong password)": ["auth.login_failed"],
  "POST /api/auth/logout": ["auth.logout"],
  "any request the access rules refuse (403)": ["access.denied"],
};
const RECORDED = new Set(Object.values(RECORDED_BY_OPERATION).flat());
const values: string[] = ACTION_OPTIONS.map((o) => o.value);

describe("audit log action filter", () => {
  it("offers only actions crm-api records", () => {
    expect(values.filter((action) => !RECORDED.has(action))).toEqual([]);
  });

  it("finds the rows of every CRM operation", () => {
    const missing = Object.entries(RECORDED_BY_OPERATION).filter(([, actions]) =>
      actions.some((action) => !values.includes(action) && action !== "auth.register"),
    );
    expect(missing.map(([operation]) => operation)).toEqual([]);
  });

  it("says that company and contact deletions are updates, and names role changes plainly", () => {
    const label = (value: string) => ACTION_OPTIONS.find((o) => o.value === value)?.label;
    expect(label("company.updated")).toBe("Company updated or deleted");
    expect(label("contact.updated")).toBe("Contact updated or deleted");
    expect(label("access.role_assigned")).toBe("Role assigned");
    expect(label("access.user_roles_synced")).toBe("Role changed");
  });
});

describe("audit entry label", () => {
  const softDelete = { old_values: { deleted_at: null }, new_values: { deleted_at: "2026-10-25T18:18:35Z" } };

  it("shows a soft delete (an update that sets deleted_at) as a deletion", () => {
    expect(entryLabel({ action: "company.updated", ...softDelete })).toBe("Company deleted");
    expect(entryLabel({ action: "contact.updated", ...softDelete })).toBe("Contact deleted");
  });

  it("shows other changes by their action", () => {
    expect(entryLabel({ action: "company.updated", old_values: { name: "A" }, new_values: { name: "B" } })).toBe(
      "Company updated",
    );
    expect(entryLabel({ action: "deal.deleted", old_values: { title: "X" }, new_values: null })).toBe("Deal deleted");
    expect(entryLabel({ action: "access.user_roles_synced", old_values: null, new_values: null })).toBe("Role changed");
    expect(entryLabel({ action: "auth.login_failed", old_values: null, new_values: null })).toBe("Auth login failed");
  });
});
