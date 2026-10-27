import { describe, expect, it, vi } from "vitest";
import { collectAll, facetCounts, hasNextPage, isCursorPage, isPage, pageRange, pages, type Page } from "./pagination.js";

const page = <T>(data: T[], current_page: number, last_page: number, per_page = 2, total = 5): Page<T> => ({
  data,
  meta: { current_page, per_page, total, last_page },
});

describe("page shapes", () => {
  it("tells offset pages from cursor pages", () => {
    expect(isPage(page([1], 1, 1))).toBe(true);
    expect(isCursorPage({ data: [1], next_cursor: null })).toBe(true);
    expect(isPage({ data: [1], next_cursor: "a" })).toBe(false);
    expect(isCursorPage(page([1], 1, 1))).toBe(false);
    expect(isPage(null)).toBe(false);
  });

  it("knows whether another page follows", () => {
    expect(hasNextPage(page([1, 2], 1, 3))).toBe(true);
    expect(hasNextPage(page([5], 3, 3))).toBe(false);
    expect(hasNextPage({ data: [1], next_cursor: "abc" })).toBe(true);
    expect(hasNextPage({ data: [1], next_cursor: null })).toBe(false);
  });

  it("gives the rows a page shows, counting from 1", () => {
    expect(pageRange({ current_page: 2, per_page: 25, total: 60, last_page: 3 })).toEqual({ from: 26, to: 50 });
    expect(pageRange({ current_page: 3, per_page: 25, total: 60, last_page: 3 })).toEqual({ from: 51, to: 60 });
    expect(pageRange({ current_page: 1, per_page: 25, total: 0, last_page: 1 })).toEqual({ from: 0, to: 0 });
    expect(pageRange({ current_page: 9, per_page: 25, total: 60, last_page: 3 })).toEqual({ from: 0, to: 0 });
  });
});

describe("facetCounts", () => {
  it("reads facets sent as a map of counts", () => {
    const meta = { current_page: 1, per_page: 25, total: 7, last_page: 1, facets: { status: { active: 5, lead: 2 } } };
    expect(facetCounts(meta, "status")).toEqual([
      { value: "active", count: 5 },
      { value: "lead", count: 2 },
    ]);
  });

  it("reads facets sent as a list of values, keeping labels", () => {
    const meta = {
      current_page: 1,
      per_page: 25,
      total: 7,
      last_page: 1,
      facets: { stage: [{ value: "won", count: 4, label: "Won" }, { value: null, count: 3 }] },
    };
    expect(facetCounts(meta, "stage")).toEqual([
      { value: "won", count: 4, label: "Won" },
      { value: "", count: 3 },
    ]);
  });

  it("is empty for a field without facets", () => {
    expect(facetCounts({ current_page: 1, per_page: 1, total: 0, last_page: 1 }, "status")).toEqual([]);
  });
});

describe("walking every page", () => {
  it("follows offset pages to the last one", async () => {
    const fetchPage = vi.fn(async (query: { page?: number }) => {
      const n = query.page ?? 1;
      return page([`${n}a`, `${n}b`], n, 3);
    });

    const seen: string[][] = [];
    for await (const p of pages(fetchPage, { search: "x" })) seen.push(p.data);

    expect(seen).toEqual([
      ["1a", "1b"],
      ["2a", "2b"],
      ["3a", "3b"],
    ]);
    expect(fetchPage.mock.calls.map(([q]) => q)).toEqual([
      { search: "x" },
      { search: "x", page: 2 },
      { search: "x", page: 3 },
    ]);
  });

  it("follows cursors until next_cursor is null", async () => {
    const cursors: Record<string, { data: number[]; next_cursor: string | null }> = {
      start: { data: [1, 2], next_cursor: "c2" },
      c2: { data: [3], next_cursor: null },
    };
    const fetchPage = vi.fn(async (query: { cursor?: string }) => cursors[query.cursor ?? "start"]!);

    expect(await collectAll(fetchPage, { limit: 2 })).toEqual([1, 2, 3]);
    expect(fetchPage.mock.calls.map(([q]) => q)).toEqual([{ limit: 2 }, { limit: 2, cursor: "c2" }]);
  });

  it("stops at maxPages", async () => {
    const fetchPage = vi.fn(async (query: { page?: number }) => page([query.page ?? 1], query.page ?? 1, 100));
    expect(await collectAll(fetchPage, {}, { maxPages: 2 })).toEqual([1, 2]);
    expect(fetchPage).toHaveBeenCalledTimes(2);
  });
});
