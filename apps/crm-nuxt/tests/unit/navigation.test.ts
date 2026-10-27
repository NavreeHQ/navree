import { describe, expect, it } from "vitest";
import { crumbsFor, GO_KEYS, isActive } from "@/lib/navigation";

describe("crumbsFor", () => {
  it("names a top-level screen with a single crumb", () => {
    expect(crumbsFor("/")).toEqual([{ label: "Dashboard" }]);
    expect(crumbsFor("/deals")).toEqual([{ label: "Deals" }]);
    expect(crumbsFor("/tasks")).toEqual([{ label: "Tasks" }]);
    expect(crumbsFor("/settings")).toEqual([{ label: "Settings" }]);
  });

  it("links a record back to its list and names it once loaded", () => {
    expect(crumbsFor("/deals/0198", "Pilot extension")).toEqual([
      { label: "Deals", href: "/deals" },
      { label: "Pilot extension" },
    ]);
    expect(crumbsFor("/companies/0198")).toEqual([{ label: "Companies", href: "/companies" }, { label: "Company" }]);
    expect(crumbsFor("/contacts/0198", null)).toEqual([{ label: "Contacts", href: "/contacts" }, { label: "Contact" }]);
  });

  it("names the settings sections under Settings", () => {
    expect(crumbsFor("/settings/users")).toEqual([{ label: "Settings", href: "/settings" }, { label: "Users and roles" }]);
    expect(crumbsFor("/settings/audit")).toEqual([{ label: "Settings", href: "/settings" }, { label: "Audit log" }]);
  });

  it("says Not found for paths outside the app", () => {
    expect(crumbsFor("/nowhere")).toEqual([{ label: "Not found" }]);
    expect(crumbsFor("/settings/billing")).toEqual([{ label: "Settings", href: "/settings" }, { label: "Not found" }]);
  });
});

describe("isActive", () => {
  it("matches the dashboard exactly and other screens by prefix", () => {
    expect(isActive("/", "/")).toBe(true);
    expect(isActive("/deals", "/")).toBe(false);
    expect(isActive("/deals/42", "/deals")).toBe(true);
    expect(isActive("/dealsx", "/deals")).toBe(false);
  });
});

describe("GO_KEYS", () => {
  it("maps every `g` shortcut to a screen", () => {
    expect(Object.values(GO_KEYS).every((path) => crumbsFor(path)[0]?.label !== "Not found")).toBe(true);
  });
});
