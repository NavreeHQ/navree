import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { isListParam, mergeListParams, mergeListQuery, useListQuery } from "./list-query-state.js";

const nav = vi.hoisted(() => ({ search: "", replace: vi.fn() }));
vi.mock("next/navigation", () => ({
  useRouter: () => ({ replace: nav.replace }),
  usePathname: () => "/deals",
  useSearchParams: () => new URLSearchParams(nav.search),
}));

beforeEach(() => {
  nav.search = "";
  nav.replace.mockReset();
});

const lastUrl = () => decodeURIComponent(nav.replace.mock.calls.at(-1)![0] as string);

describe("mergeListQuery", () => {
  it("is the same function as mergeListParams, its first name", () => {
    expect(mergeListParams).toBe(mergeListQuery);
  });

  it("replaces the list parameters and keeps the page's own", () => {
    const merged = mergeListQuery(new URLSearchParams("tab=open&filter[stage]=lead&page=3"), { filter: { stage: "won" } });
    expect(decodeURIComponent(merged.toString())).toBe("tab=open&filter[stage]=won");
  });

  it("leaves out values equal to the defaults", () => {
    const merged = mergeListQuery(new URLSearchParams(), { per_page: 25, sort: ["-created_at"], search: "a" }, { per_page: 25, sort: ["-created_at"] });
    expect(merged.toString()).toBe("search=a");
  });

  it("knows which parameters a list query owns", () => {
    expect(["filter", "filter[a]", "filter[a][gte]", "sort", "search", "page", "per_page", "cursor", "before", "limit"].every(isListParam)).toBe(true);
    expect(["tab", "q", "per", "after", "filters[a]"].some(isListParam)).toBe(false);
  });

  it("drops the cursor parameters with the others, and writes the canonical form", () => {
    const merged = mergeListQuery(
      new URLSearchParams("tab=open&cursor=a&before=b&limit=5&filter=1&q=old&page=2"),
      { limit: 10, filter: { stage: { in: ["lead", "won"] } }, search: "agra corp" },
    );
    expect(merged.toString()).toBe("tab=open&q=old&filter%5Bstage%5D%5Bin%5D=lead%2Cwon&search=agra+corp&limit=10");
  });
});

describe("useListQuery", () => {
  it("reads the list query from the URL over the defaults", () => {
    nav.search = "filter[stage]=won&sort=-amount&page=2";
    const { result } = renderHook(() => useListQuery({ per_page: 25 }));
    expect(result.current.query).toEqual({ filter: { stage: "won" }, sort: ["-amount"], page: 2, per_page: 25 });
  });

  it("writes filters back with router.replace, on page 1, keeping other parameters", () => {
    nav.search = "tab=open&page=4";
    const { result } = renderHook(() => useListQuery());
    act(() => result.current.setFilter("stage", "won"));
    expect(lastUrl()).toBe("/deals?tab=open&filter[stage]=won");
    expect(nav.replace.mock.calls.at(-1)![1]).toEqual({ scroll: false });
  });

  it("sets the search, the sort and the page", () => {
    nav.search = "page=3";
    const { result } = renderHook(() => useListQuery());
    act(() => result.current.setSearch("agra"));
    expect(lastUrl()).toBe("/deals?search=agra");

    act(() => result.current.toggleSort("name"));
    expect(lastUrl()).toBe("/deals?sort=name");

    act(() => result.current.setPage(5));
    expect(lastUrl()).toBe("/deals?page=5");
  });

  it("clears the search and drops to the bare path when nothing is left", () => {
    nav.search = "search=agra";
    const { result } = renderHook(() => useListQuery());
    act(() => result.current.setSearch(""));
    expect(lastUrl()).toBe("/deals");
  });

  it("reads the grammar's names: operators, null, before and limit", () => {
    nav.search = "filter[amount][gte]=100&filter[owner_id][null]=1&before=abc&limit=20";
    const { result } = renderHook(() => useListQuery({ per_page: 25 }));
    // A page that goes by cursor takes no default page size.
    expect(result.current.query).toEqual({
      filter: { amount: { gte: "100" }, owner_id: { null: true } },
      before: "abc",
      limit: 20,
    });
    expect(result.current.problems).toEqual([]);
  });

  it("does not read names the grammar does not have", () => {
    nav.search = "filters[stage]=won&q=agra&per=10&after=abc&f.stage=won";
    const { result } = renderHook(() => useListQuery());
    expect(result.current.query).toEqual({});
    expect(result.current.problems).toEqual([]);
  });

  it("keeps what is right in an edited address and names what is not", () => {
    nav.search = "filter[stage]=won&filter[amount][regex]=1&page=0&sort=-amount";
    const { result } = renderHook(() => useListQuery());
    expect(result.current.query).toEqual({ filter: { stage: "won" }, sort: ["-amount"] });
    expect(result.current.problems.map((problem) => problem.param)).toEqual(["filter[amount][regex]", "page"]);
  });

  it("goes back to the first page of a cursor list when a filter, the search or the sort changes", () => {
    nav.search = "cursor=abc&before=def&limit=20&tab=open";
    const { result } = renderHook(() => useListQuery());
    act(() => result.current.setFilter("stage", "won"));
    expect(lastUrl()).toBe("/deals?tab=open&filter[stage]=won&limit=20");
    act(() => result.current.setSearch("agra"));
    expect(lastUrl()).toBe("/deals?tab=open&search=agra&limit=20");
    act(() => result.current.toggleSort("name"));
    expect(lastUrl()).toBe("/deals?tab=open&sort=name&limit=20");
  });

  it("writes a filter with an operator, and a list as `in`", () => {
    nav.search = "";
    const { result } = renderHook(() => useListQuery());
    act(() => result.current.setFilter("amount", { gte: 100, lte: 900 }));
    expect(lastUrl()).toBe("/deals?filter[amount][gte]=100&filter[amount][lte]=900");
    act(() => result.current.setFilter("stage", ["lead", "won"]));
    expect(lastUrl()).toBe("/deals?filter[stage][in]=lead,won");
  });

  it("writes a field named like a method of every object as an ordinary field", () => {
    nav.search = "filter[constructor]=x";
    const { result } = renderHook(() => useListQuery());
    expect(Object.keys(result.current.query.filter ?? {})).toEqual(["constructor"]);
    act(() => result.current.setFilter("toString", "y"));
    expect(lastUrl()).toBe("/deals?filter[constructor]=x&filter[toString]=y");
  });
});
