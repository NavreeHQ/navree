import type { CaslRule } from "@navree/client/abilities";
import { act, render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { AbilityProvider, Can, useAbility } from "./abilities.js";

const rep: CaslRule[] = [
  { action: "read", subject: "all" },
  { action: "update", subject: "Deal", conditions: { owner_id: "u1" } },
];
const manager: CaslRule[] = [{ action: "manage", subject: "all" }];

function DeleteButton() {
  const ability = useAbility();
  return ability.can("delete", "Deal") ? <button>Delete</button> : <span>read only</span>;
}

describe("AbilityProvider", () => {
  it("hides what the rules deny with <Can>, checking records by their __type", () => {
    render(
      <AbilityProvider rules={rep}>
        <Can I="update" this={{ __type: "Deal", owner_id: "u1" }}>
          <span>edit mine</span>
        </Can>
        <Can I="update" this={{ __type: "Deal", owner_id: "u2" }}>
          <span>edit theirs</span>
        </Can>
        <Can not I="delete" a="Deal">
          <span>no delete</span>
        </Can>
      </AbilityProvider>,
    );

    expect(screen.getByText("edit mine")).toBeTruthy();
    expect(screen.queryByText("edit theirs")).toBeNull();
    expect(screen.getByText("no delete")).toBeTruthy();
  });

  it("re-renders useAbility() users when the rules change (a workspace switch)", () => {
    const { rerender } = render(
      <AbilityProvider rules={rep}>
        <DeleteButton />
      </AbilityProvider>,
    );
    expect(screen.getByText("read only")).toBeTruthy();

    act(() => {
      rerender(
        <AbilityProvider rules={manager}>
          <DeleteButton />
        </AbilityProvider>,
      );
    });
    expect(screen.getByRole("button", { name: "Delete" })).toBeTruthy();
  });

  it("denies everything until the rules arrive", () => {
    render(
      <AbilityProvider rules={undefined}>
        <DeleteButton />
      </AbilityProvider>,
    );
    expect(screen.getByText("read only")).toBeTruthy();
  });

  it("useAbility outside the provider says what's missing", () => {
    expect(() => render(<DeleteButton />)).toThrow(/AbilityProvider/);
  });
});
