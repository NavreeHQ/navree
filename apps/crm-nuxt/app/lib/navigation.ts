import { Building2, Columns3, Gauge, ListChecks, ScrollText, Settings, UserCog, Users } from "@lucide/vue";
import type { Component } from "vue";

export type NavItem = { href: string; label: string; icon: Component; shortcut: string };

/** The work screens, in sidebar order, with their `g` shortcuts. */
export const WORK_NAV: readonly NavItem[] = [
  { href: "/", label: "Dashboard", icon: Gauge, shortcut: "G H" },
  { href: "/deals", label: "Deals", icon: Columns3, shortcut: "G D" },
  { href: "/companies", label: "Companies", icon: Building2, shortcut: "G C" },
  { href: "/contacts", label: "Contacts", icon: Users, shortcut: "G P" },
  { href: "/tasks", label: "Tasks", icon: ListChecks, shortcut: "G T" },
];

export const SETTINGS_NAV = {
  general: { href: "/settings", label: "Settings", icon: Settings, shortcut: "G S" },
  users: { href: "/settings/users", label: "Users and roles", icon: UserCog, shortcut: "" },
  audit: { href: "/settings/audit", label: "Audit log", icon: ScrollText, shortcut: "" },
} as const satisfies Record<string, NavItem>;

/** `g` + key → path. */
export const GO_KEYS: Record<string, string> = { h: "/", d: "/deals", c: "/companies", p: "/contacts", t: "/tasks", s: "/settings" };

export function isActive(path: string, href: string): boolean {
  return href === "/" ? path === "/" : path === href || path.startsWith(`${href}/`);
}

export type Crumb = { label: string; href?: string };

const SECTIONS: Record<string, string> = {
  deals: "Deals",
  companies: "Companies",
  contacts: "Contacts",
  tasks: "Tasks",
  settings: "Settings",
};
const SETTINGS_SECTIONS: Record<string, string> = { users: "Users and roles", audit: "Audit log" };
const RECORD_NOUN: Record<string, string> = { deals: "Deal", companies: "Company", contacts: "Contact" };

/**
 * The top bar's breadcrumb for a path: the screen, then the settings section
 * or the record (its name once the screen has loaded it). Only the last crumb
 * has no link.
 */
export function crumbsFor(path: string, recordTitle?: string | null): Crumb[] {
  const [section, rest] = path.split("/").filter(Boolean);
  if (!section) return [{ label: "Dashboard" }];
  const label = SECTIONS[section];
  if (!label) return [{ label: "Not found" }];
  if (!rest) return [{ label }];
  if (section === "settings") return [{ label, href: "/settings" }, { label: SETTINGS_SECTIONS[rest] ?? "Not found" }];
  return [{ label, href: `/${section}` }, { label: recordTitle || RECORD_NOUN[section] || label }];
}
