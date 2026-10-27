import { describe, expect, expectTypeOf, it } from "vitest";
import { queryKeys } from "./query-keys.js";

describe("queryKeys", () => {
  const deals = queryKeys("deals");

  it("nests every key under the resource, so invalidating `all` reaches lists and records", () => {
    expect(deals.all).toEqual(["deals"]);
    expect(deals.lists()).toEqual(["deals", "list"]);
    expect(deals.list({ filter: { stage: "won" }, page: 2 })).toEqual(["deals", "list", { filter: { stage: "won" }, page: 2 }]);
    expect(deals.list()).toEqual(["deals", "list", {}]);
    expect(deals.details()).toEqual(["deals", "detail"]);
    expect(deals.detail(42)).toEqual(["deals", "detail", "42"]);
  });

  it("keeps the literal types for TanStack Query", () => {
    expectTypeOf(deals.all).toEqualTypeOf<readonly ["deals"]>();
    expectTypeOf(deals.detail("a")).toEqualTypeOf<readonly ["deals", "detail", string]>();
  });

  it("adds custom keys under the resource", () => {
    const keys = queryKeys("deals", { board: (owner?: string) => ["board", owner ?? "all"] as const });
    expect(keys.board("u1")).toEqual(["deals", "board", "u1"]);
    expect(keys.all).toEqual(["deals"]);
  });
});
