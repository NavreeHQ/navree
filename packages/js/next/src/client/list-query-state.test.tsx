import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { isListParam, mergeListParams, useListQuery } from "./list-query-state.js";

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

describe("mergeListParams", () => {
  it("replaces the list parameters and keeps the page's own", () => {
    const merged = mergeListParams(new URLSearchParams("tab=open&filter[stage]=lead&page=3"), { filter: { stage: "won" } });
    expect(decodeURIComponent(merged.toString())).toBe("tab=open&filter[stage]=won");
  });

  it("leaves out values equal to the defaults", () => {
    const merged = mergeListParams(new URLSearchParams(), { per_page: 25, sort: ["-created_at"], search: "a" }, { per_page: 25, sort: ["-created_at"] });
    expect(merged.toString()).toBe("search=a");
  });

  it("knows which parameters a list query owns", () => {
    expect(["filter[a]", "filter[a][gte]", "sort", "search", "page", "per_page", "cursor", "limit"].every(isListParam)).toBe(true);
    expect(isListParam("tab")).toBe(false);
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
    act(() => result.current.setSearch("acme"));
    expect(lastUrl()).toBe("/deals?search=acme");

    act(() => result.current.toggleSort("name"));
    expect(lastUrl()).toBe("/deals?sort=name");

    act(() => result.current.setPage(5));
    expect(lastUrl()).toBe("/deals?page=5");
  });

  it("clears the search and drops to the bare path when nothing is left", () => {
    nav.search = "search=acme";
    const { result } = renderHook(() => useListQuery());
    act(() => result.current.setSearch(""));
    expect(lastUrl()).toBe("/deals");
  });
});
