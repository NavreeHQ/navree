// @vitest-environment node
import { createAbility } from "@navree/client/abilities";
import { describe, expect, it } from "vitest";
import { lockReason, moveBlockedReason } from "@/features/deals/permissions";
import type { Activity, CaslRule, Company, Deal, DealStage } from "@/lib/api/types";
import type { AppAbility } from "@/lib/casl/ability";
import { rulesFor } from "@/mocks/abilities";

const REP = { id: "rep-1", role: "rep" } as const;
const OTHER_REP = "rep-2";

/** API records carry `__type`, which is how CASL tells subjects apart. */
const deal = (owner: string, stage: DealStage = "proposal") =>
  ({ __type: "Deal", id: `d-${owner}-${stage}`, stage, owner_id: owner, owner: { id: owner, name: "Tarun Bhatia" } }) as unknown as Deal;
const company = (owner: string) => ({ __type: "Company", id: `c-${owner}`, owner_id: owner }) as unknown as Company;
const task = (owner: string, assignee: string) =>
  ({ __type: "Activity", id: `a-${owner}`, owner_id: owner, assignee_id: assignee }) as unknown as Activity;

describe("the app ability, built the way the module builds it from the rules GET /api/me/abilities serves", () => {
  it("starts empty: nothing is allowed until the rules arrive", () => {
    const ability = createAbility<AppAbility>();
    expect(ability.can("read", "Deal")).toBe(false);
    ability.update(rulesFor({ id: "admin-1", role: "admin" }) as Parameters<typeof ability.update>[0]);
    expect(ability.can("delete", deal("anyone", "won"))).toBe(true);
  });

  it("lets a rep change their own records and only read everyone else's", () => {
    const ability = createAbility<AppAbility>(rulesFor(REP));
    expect(ability.can("read", company(OTHER_REP))).toBe(true);
    expect(ability.can("update", company(REP.id))).toBe(true);
    expect(ability.can("update", company(OTHER_REP))).toBe(false);
    expect(ability.can("move", deal(REP.id))).toBe(true);
    expect(ability.can("move", deal(OTHER_REP))).toBe(false);
    expect(ability.can("create", "Deal")).toBe(true);
    expect(ability.can("export", "Contact")).toBe(false);
    expect(ability.can("create", "User")).toBe(false);
  });

  it("lets anyone complete a task assigned to them, even one someone else owns", () => {
    const ability = createAbility<AppAbility>(rulesFor(REP));
    expect(ability.can("complete", task(OTHER_REP, REP.id))).toBe(true);
    expect(ability.can("complete", task(OTHER_REP, OTHER_REP))).toBe(false);
    expect(ability.can("update", task(OTHER_REP, REP.id))).toBe(false);
  });

  it("applies inverted rules last: a rep can't delete or reopen their own won deal", () => {
    const ability = createAbility<AppAbility>(rulesFor(REP));
    expect(ability.can("delete", deal(REP.id, "proposal"))).toBe(true);
    expect(ability.can("delete", deal(REP.id, "won"))).toBe(false);
    expect(ability.can("move", deal(REP.id, "lost"))).toBe(false);
  });

  it("explains a locked card: the rule's reason, or whose deal it is", () => {
    const ability = createAbility<AppAbility>(rulesFor(REP));
    expect(moveBlockedReason(ability, deal(REP.id, "won"))).toBe("Only a manager can reopen a won or lost deal.");
    expect(moveBlockedReason(ability, deal(OTHER_REP))).toBeNull();
    expect(lockReason(ability, deal(OTHER_REP))).toBe("Owned by Tarun Bhatia; you can't move it");
  });

  it("lets a manager move any deal, including closed ones, and export contacts", () => {
    const ability = createAbility<AppAbility>(rulesFor({ id: "mgr-1", role: "manager" }));
    expect(ability.can("move", deal(OTHER_REP, "won"))).toBe(true);
    expect(ability.can("export", "Contact")).toBe(true);
    expect(ability.can("read", "User")).toBe(true);
    expect(ability.can("create", "User")).toBe(false);
  });

  it("accepts the array forms of action and subject the API sends", () => {
    const rules: CaslRule[] = [{ action: ["read", "update"], subject: ["Company", "Contact"] }];
    const ability = createAbility<AppAbility>(rules);
    expect(ability.can("update", "Contact")).toBe(true);
    expect(ability.can("delete", "Contact")).toBe(false);
  });
});
