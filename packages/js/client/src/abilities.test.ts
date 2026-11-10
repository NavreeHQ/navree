import { describe, expect, it, vi } from "vitest";
import { ABILITIES_PATH, createAbility, fetchAbilities, subjectType, type CaslRule } from "./abilities.js";
import { NavreeError } from "./errors.js";

const rules: CaslRule[] = [
  { action: "read", subject: "all" },
  { action: ["update", "move"], subject: "Deal", conditions: { owner_id: "u1" } },
  { action: "delete", subject: "Deal", inverted: true, reason: "Only managers delete deals." },
  { action: "update", subject: "User", fields: ["name"], conditions: { id: "u1" } },
];

describe("createAbility", () => {
  const ability = createAbility(rules);

  it("checks type names and API records by their __type", () => {
    expect(ability.can("read", "Company")).toBe(true);
    expect(ability.can("update", { __type: "Deal", owner_id: "u1" })).toBe(true);
    expect(ability.can("move", { __type: "Deal", owner_id: "u2" })).toBe(false);
    expect(ability.cannot("delete", "Deal")).toBe(true);
    expect(ability.relevantRuleFor("delete", "Deal")?.reason).toBe("Only managers delete deals.");
  });

  it("checks fields", () => {
    expect(ability.can("update", { __type: "User", id: "u1" }, "name")).toBe(true);
    expect(ability.can("update", { __type: "User", id: "u1" }, "email")).toBe(false);
  });

  it("starts empty and takes new rules after sign-in or a workspace switch", () => {
    const empty = createAbility();
    expect(empty.can("read", "Deal")).toBe(false);
    empty.update(rules as never);
    expect(empty.can("read", "Deal")).toBe(true);
  });

  it("names a subject's type", () => {
    expect(subjectType("Deal")).toBe("Deal");
    expect(subjectType({ __type: "Deal" })).toBe("Deal");
  });
});

describe("fetchAbilities", () => {
  it("loads the signed-in user's rules from GET /api/me/abilities", async () => {
    const fetch = vi.fn(async (_path: string, _init?: RequestInit) => Response.json(rules));
    expect(await fetchAbilities({ fetch })).toEqual(rules);
    expect(fetch.mock.calls[0]![0]).toBe(ABILITIES_PATH);
    expect(new Headers(fetch.mock.calls[0]![1]?.headers).get("accept")).toBe("application/json");
  });

  it("throws the API's error", async () => {
    const fetch = vi.fn(async () =>
      Response.json({ statusCode: 401, message: "Unauthorized", error: "Unauthorized", code: "unauthenticated" }, { status: 401 }),
    );
    await expect(fetchAbilities({ fetch })).rejects.toBeInstanceOf(NavreeError);
  });
});
