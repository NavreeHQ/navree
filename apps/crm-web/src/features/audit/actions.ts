/**
 * Audit actions as crm-api records them, for the admin audit log.
 *
 * Data changes come from navree-audit's change observer: `<entity>.created|updated|deleted`
 * for every write through `Repo`. Companies and contacts are soft-deleted (an update that
 * sets `deleted_at`), so their deletions are recorded as `company.updated` /
 * `contact.updated`; deals and activities are really deleted (`deal.deleted`,
 * `activity.deleted`); users are never deleted. Role changes come from navree-access:
 * `access.role_assigned` when an admin creates a user, `access.user_roles_synced` when
 * `PATCH /api/v1/users/{id}` changes the role. Sign-ins and refused requests are `auth.*`
 * and `access.denied`.
 */
export const AUDIT_ACTIONS = [
  "company.created",
  "company.updated",
  "contact.created",
  "contact.updated",
  "contacts.exported",
  "deal.created",
  "deal.updated",
  "deal.deleted",
  "activity.created",
  "activity.updated",
  "activity.deleted",
  "user.created",
  "user.updated",
  "auth.login",
  "auth.login_failed",
  "auth.logout",
  "access.role_assigned",
  "access.user_roles_synced",
  "access.denied",
] as const;

/** Labels that read better than the action name spelled out. */
const LABELS: Record<string, string> = {
  "access.role_assigned": "Role assigned",
  "access.user_roles_synced": "Role changed",
};

/** Filter labels where one action covers more than its name says. */
const FILTER_LABELS: Record<string, string> = {
  "company.updated": "Company updated or deleted",
  "contact.updated": "Contact updated or deleted",
};

/** `deal.updated` → "Deal updated"; a few actions have plainer names. */
export function actionLabel(action: string): string {
  if (LABELS[action]) return LABELS[action];
  const text = action.replace(/[._]/g, " ");
  return text.charAt(0).toUpperCase() + text.slice(1);
}

/** The options of the audit log's action filter. */
export const ACTION_OPTIONS = AUDIT_ACTIONS.map((value) => ({
  value,
  label: FILTER_LABELS[value] ?? actionLabel(value),
}));

type Values = Record<string, unknown> | null | undefined;

/** The label of one audit entry: a soft delete (`<entity>.updated` setting `deleted_at`) reads as a deletion. */
export function entryLabel(entry: { action: string; old_values?: Values; new_values?: Values }): string {
  const softDeleted =
    entry.action.endsWith(".updated") &&
    entry.new_values?.deleted_at != null &&
    (entry.old_values?.deleted_at ?? null) === null;
  if (softDeleted) return actionLabel(entry.action.replace(/\.updated$/, ".deleted"));
  return actionLabel(entry.action);
}
