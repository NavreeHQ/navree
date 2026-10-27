import type { Role } from "@/lib/api/types";

export function roleLabel(roles: readonly string[]): string {
  if (roles.includes("admin")) return "Admin";
  if (roles.includes("manager")) return "Manager";
  if (roles.includes("rep")) return "Sales rep";
  return roles[0] ?? "Member";
}

export const ROLE_HELP: Record<Role, string> = {
  admin: "Everything, including users, roles and the audit log",
  manager: "All customer records, reassigning deals, exporting contacts",
  rep: "Reads everything; edits and deletes only their own records",
};
