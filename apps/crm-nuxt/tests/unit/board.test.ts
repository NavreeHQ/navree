// @vitest-environment node
import { describe, expect, it } from "vitest";
import { columnsFrom, dropIndex, neighbourStage, placeCard, stageOf, STAGE_ORDER } from "@/features/deals/board";
import type { Deal, DealBoard, DealStage } from "@/lib/api/types";

/** A board with the given card ids per stage, positions shuffled to prove the sort. */
function board(columns: Partial<Record<DealStage, string[]>>): DealBoard {
  return {
    stages: STAGE_ORDER.map((stage) => {
      const ids = columns[stage] ?? [];
      const deals = ids.map((id, i) => ({ id, stage, position: (i + 1) * 1000 }) as unknown as Deal).reverse();
      return { stage, label: stage, count: deals.length, amount_cents: 0, deals };
    }),
  };
}

describe("deal board maths", () => {
  it("builds one column per stage, in pipeline order, cards sorted by position", () => {
    const columns = columnsFrom(board({ lead: ["a", "b", "c"], won: ["w"] }));
    expect(Object.keys(columns)).toEqual(["lead", "qualified", "proposal", "negotiation", "won", "lost"]);
    expect(columns.lead).toEqual(["a", "b", "c"]);
    expect(columns.won).toEqual(["w"]);
    expect(columns.lost).toEqual([]);
  });

  it("has empty columns while the board is loading", () => {
    const columns = columnsFrom(undefined);
    expect(STAGE_ORDER.every((stage) => columns[stage].length === 0)).toBe(true);
  });

  it("finds the stage a card is in", () => {
    const columns = columnsFrom(board({ proposal: ["p1"], lost: ["l1"] }));
    expect(stageOf(columns, "p1")).toBe("proposal");
    expect(stageOf(columns, "l1")).toBe("lost");
    expect(stageOf(columns, "nope")).toBeUndefined();
  });

  it("moves a card to another stage at an index, without touching the input", () => {
    const before = columnsFrom(board({ lead: ["a", "b"], qualified: ["q1", "q2"] }));
    const after = placeCard(before, "a", "qualified", 1);
    expect(after.lead).toEqual(["b"]);
    expect(after.qualified).toEqual(["q1", "a", "q2"]);
    expect(before.lead).toEqual(["a", "b"]);
    expect(before.qualified).toEqual(["q1", "q2"]);
  });

  it("reorders within a stage and clamps the index to the column", () => {
    const columns = columnsFrom(board({ lead: ["a", "b", "c"] }));
    expect(placeCard(columns, "a", "lead", 2).lead).toEqual(["b", "c", "a"]);
    expect(placeCard(columns, "c", "lead", -5).lead).toEqual(["c", "a", "b"]);
    expect(placeCard(columns, "a", "won", 99).won).toEqual(["a"]);
  });

  it("drops above the first card whose midpoint is below the pointer", () => {
    const midpoints = [100, 200, 300];
    expect(dropIndex(midpoints, 50)).toBe(0);
    expect(dropIndex(midpoints, 150)).toBe(1);
    expect(dropIndex(midpoints, 299)).toBe(2);
    expect(dropIndex(midpoints, 301)).toBe(3);
    expect(dropIndex([], 10)).toBe(0);
  });

  it("steps to the neighbouring stage with the arrow keys, stopping at the edges", () => {
    expect(neighbourStage("lead", 1)).toBe("qualified");
    expect(neighbourStage("qualified", -1)).toBe("lead");
    expect(neighbourStage("lead", -1)).toBe("lead");
    expect(neighbourStage("won", 1)).toBe("lost");
    expect(neighbourStage("lost", 1)).toBe("lost");
  });
});
