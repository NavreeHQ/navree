import { describe, expect, it } from "vitest";
import { abilityFrom } from "./abilities";

describe("abilities", () => {
  it("follows the API's rules, conditions included", () => {
    const ability = abilityFrom([
      { action: "read", subject: "Project" },
      { action: "create", subject: "Project" },
      { action: "update", subject: "Project", conditions: { created_by: "u1" } },
    ]);

    expect(ability.can("read", "Project")).toBe(true);
    expect(ability.can("delete", "Project")).toBe(false);
    expect(ability.can("update", { __type: "Project", created_by: "u1" } as never)).toBe(true);
    expect(ability.can("update", { __type: "Project", created_by: "u2" } as never)).toBe(false);
  });

  it("lets a super admin do anything", () => {
    const ability = abilityFrom([{ action: "manage", subject: "all" }]);
    expect(ability.can("delete", "Workspace")).toBe(true);
  });
});
