/**
 * CASL rules per role, as crm-api's `abilities::define` serves them from
 * `GET /api/me/abilities`:
 *
 * - admin: super-admin, `manage all`;
 * - manager: permissions companies.manage, contacts.manage, contacts.export,
 *   deals.manage, activities.manage, users.read (compiled to rules), plus the
 *   base rules every signed-in user gets;
 * - rep: read everything; create records; update/delete Company, Contact and
 *   Activity they own; complete an Activity assigned to them or owned by them;
 *   update + move + delete Deal they own; and, last (last match
 *   wins), cannot delete a Deal that is won or lost (everyone but admin).
 */
import type { CaslRule } from "@/lib/api/types";
import type { UserRow } from "./seed";

const RECORDS = ["Company", "Contact", "Deal", "Activity"];

export function rulesFor(user: Pick<UserRow, "id" | "role">): CaslRule[] {
  if (user.role === "admin") {
    return [{ action: "manage", subject: "all" }];
  }

  const rules: CaslRule[] = [];

  if (user.role === "manager") {
    // from_permissions: companies.manage, contacts.manage, deals.manage, activities.manage,
    // contacts.export, users.read
    rules.push({ action: "manage", subject: "Company" });
    rules.push({ action: "manage", subject: "Contact" });
    rules.push({ action: "manage", subject: "Deal" });
    rules.push({ action: "manage", subject: "Activity" });
    rules.push({ action: "export", subject: "Contact" });
    rules.push({ action: "read", subject: "User" });
  }

  const own = { owner_id: user.id };
  rules.push({ action: "read", subject: [...RECORDS, "User"] });
  rules.push({ action: "create", subject: RECORDS });
  rules.push({ action: ["update", "delete"], subject: ["Company", "Contact", "Activity"], conditions: own });
  rules.push({ action: "complete", subject: "Activity", conditions: { assignee_id: user.id } });
  rules.push({ action: "complete", subject: "Activity", conditions: own });
  rules.push({ action: ["update", "move"], subject: "Deal", conditions: own });
  rules.push({ action: "delete", subject: "Deal", conditions: own });
  rules.push({
    action: "delete",
    subject: "Deal",
    conditions: { stage: { $in: ["won", "lost"] } },
    inverted: true,
    reason: "Won and lost deals are kept for reporting.",
  });
  if (user.role !== "manager") {
    // crm-api: reps can't reopen a closed deal (so they can't reopen it and then delete it).
    rules.push({
      action: "move",
      subject: "Deal",
      conditions: { stage: { $in: ["won", "lost"] } },
      inverted: true,
      reason: "Only a manager can reopen a won or lost deal.",
    });
  }
  return rules;
}
