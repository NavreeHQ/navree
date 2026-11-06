import { createAbility } from "@navree/client/abilities";
import { AbilityProvider, Can } from "@navree/next/abilities";
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import type { CaslRule, Deal } from "@/lib/api/types";
import { rulesFor } from "@/mocks/abilities";

const REP1 = "0190a1b2-0000-7000-8000-000000000001";
const REP2 = "0190a1b2-0000-7000-8000-000000000002";

/** Rep rules exactly as the API serves them in CASL JSON. */
const repRules: CaslRule[] = [
  { action: "read", subject: ["Company", "Contact", "Deal", "Activity", "User"] },
  { action: "create", subject: ["Company", "Contact", "Deal", "Activity"] },
  { action: ["update", "delete"], subject: ["Company", "Contact", "Activity"], conditions: { owner_id: REP1 } },
  { action: "complete", subject: "Activity", conditions: { assignee_id: REP1 } },
  { action: "complete", subject: "Activity", conditions: { owner_id: REP1 } },
  { action: ["update", "move"], subject: "Deal", conditions: { owner_id: REP1 } },
  { action: "delete", subject: "Deal", conditions: { owner_id: REP1 } },
  { action: "delete", subject: "Deal", conditions: { stage: { $in: ["won", "lost"] } }, inverted: true },
  { action: "move", subject: "Deal", conditions: { stage: { $in: ["won", "lost"] } }, inverted: true },
];

function deal(overrides: Partial<Deal> = {}): Deal {
  return {
    __type: "Deal",
    id: "d1",
    title: "Fleet telematics rollout",
    amount_cents: 4_800_000,
    currency: "INR",
    stage: "proposal",
    probability: 50,
    position: 0,
    expected_close_date: null,
    closed_at: null,
    lost_reason: null,
    company_id: "c1",
    company: { id: "c1", name: "Doaba" },
    contact_id: null,
    contact: null,
    owner_id: REP1,
    owner: { id: REP1, name: "Priya Verma" },
    created_at: "2026-10-01T10:00:00Z",
    updated_at: "2026-10-01T10:00:00Z",
    ...overrides,
  };
}

afterEach(cleanup);

describe("CASL ability", () => {
  it("detects the subject type from __type on API records", () => {
    const ability = createAbility([{ action: "read", subject: "Deal" }]);

    expect(ability.can("read", deal())).toBe(true);
    expect(ability.can("read", { ...deal(), __type: "Contact" } as never)).toBe(false);
    expect(ability.can("read", "Deal")).toBe(true);
  });

  it("lets a rep update and move their own deal", () => {
    const ability = createAbility(repRules);

    expect(ability.can("update", deal())).toBe(true);
    expect(ability.can("move", deal())).toBe(true);
  });

  it("does not let a rep update or move another rep's deal", () => {
    const ability = createAbility(repRules);
    const theirs = deal({ owner_id: REP2, owner: { id: REP2, name: "Tarun Bhatia" } });

    expect(ability.can("update", theirs)).toBe(false);
    expect(ability.can("move", theirs)).toBe(false);
    expect(ability.can("read", theirs)).toBe(true);
  });

  it("does not let a rep reopen (move) a won or lost deal, even their own", () => {
    const ability = createAbility(repRules);

    expect(ability.can("move", deal({ stage: "negotiation" }))).toBe(true);
    expect(ability.can("move", deal({ stage: "won" }))).toBe(false);
    expect(ability.can("move", deal({ stage: "lost" }))).toBe(false);
    expect(ability.can("update", deal({ stage: "won" }))).toBe(true);
    const managerRules = createAbility(rulesFor({ id: "m1", role: "manager" }));
    expect(managerRules.can("move", deal({ stage: "won", owner_id: REP2 }))).toBe(true);
  });

  it("does not let a rep delete a won or lost deal, even their own (last rule wins)", () => {
    const ability = createAbility(repRules);

    expect(ability.can("delete", deal({ stage: "negotiation" }))).toBe(true);
    expect(ability.can("delete", deal({ stage: "won" }))).toBe(false);
    expect(ability.can("delete", deal({ stage: "lost" }))).toBe(false);
  });

  it("lets a rep complete tasks assigned to them or owned by them, not other people's", () => {
    const ability = createAbility(repRules);
    const task = (owner_id: string, assignee_id: string | null) =>
      ({ __type: "Activity", id: "t1", kind: "task", owner_id, assignee_id }) as never;

    expect(ability.can("complete", task("manager", REP1))).toBe(true);
    expect(ability.can("complete", task(REP1, null))).toBe(true);
    expect(ability.can("complete", task(REP2, REP2))).toBe(false);
    expect(ability.can("update", task("manager", REP1))).toBe(false);
  });

  it("type-level checks are true when a conditional rule exists (CASL semantics)", () => {
    const ability = createAbility(repRules);

    expect(ability.can("update", "Deal")).toBe(true);
    expect(ability.can("export", "Contact")).toBe(false);
    expect(ability.can("create", "User")).toBe(false);
  });

  it("matches the mock API's role rules: manager can export and reassign, admin manages all", () => {
    const manager = createAbility(rulesFor({ id: "m1", role: "manager" }));
    const admin = createAbility(rulesFor({ id: "a1", role: "admin" }));
    const rep = createAbility(rulesFor({ id: REP1, role: "rep" }));
    const theirs = deal({ owner_id: REP2 });

    expect(manager.can("update", theirs)).toBe(true);
    expect(manager.can("export", "Contact")).toBe(true);
    expect(manager.can("create", "User")).toBe(false);
    expect(admin.can("create", "User")).toBe(true);
    expect(admin.can("delete", deal({ stage: "won" }))).toBe(true);
    expect(rep.can("update", theirs)).toBe(false);
    expect(rep.can("update", deal())).toBe(true);
  });

  it("<Can I='update' this={deal}> hides actions the API would refuse", () => {
    const mine = deal({ id: "mine", title: "Mine" });
    const theirs = deal({ id: "theirs", title: "Theirs", owner_id: REP2, owner: { id: REP2, name: "Tarun Bhatia" } });

    render(
      <AbilityProvider rules={repRules}>
        {[mine, theirs].map((d) => (
          <Can key={d.id} I="update" this={d}>
            <button>Edit {d.title}</button>
          </Can>
        ))}
      </AbilityProvider>,
    );

    expect(screen.queryByRole("button", { name: "Edit Mine" })).not.toBeNull();
    expect(screen.queryByRole("button", { name: "Edit Theirs" })).toBeNull();
  });
});
