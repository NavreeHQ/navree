import { describe, expect, it } from "vitest";
import {
  fromSorting,
  listQuery,
  listQuerySerializer,
  parseListQuery,
  serializeListQuery,
  setFilter,
  sortDirection,
  toggleSort,
  toSorting,
  withPage,
} from "./list-query";

const decoded = (params: URLSearchParams) => decodeURIComponent(params.toString());

describe("serializeListQuery", () => {
  it("writes filters, sort, search and paging in Navree's list-query format", () => {
    const params = serializeListQuery({
      filter: { status: "active", created_at: { gte: "2026-10-01", lt: "2026-11-01" } },
      sort: ["-created_at", "name"],
      search: "acme",
      page: 2,
      per_page: 25,
    });

    expect(decoded(params)).toBe(
      "filter[status]=active&filter[created_at][gte]=2026-10-01&filter[created_at][lt]=2026-11-01" +
        "&sort=-created_at,name&search=acme&page=2&per_page=25",
    );
  });

  it("writes a list as `in`, booleans and numbers as text, dates as ISO 8601, null as `[null]=true`", () => {
    const params = serializeListQuery({
      filter: {
        stage: ["lead", "won"],
        archived: false,
        amount: { gte: 1000 },
        due_at: { lte: new Date("2026-10-27T00:00:00Z") },
        owner_id: { null: true },
        tag: { not_in: ["spam", "test"] },
      },
    });

    expect(decoded(params)).toBe(
      "filter[stage][in]=lead,won&filter[archived]=false&filter[amount][gte]=1000" +
        "&filter[due_at][lte]=2026-10-27T00:00:00.000Z&filter[owner_id][null]=true&filter[tag][not_in]=spam,test",
    );
  });

  it("leaves out empty parts and writes cursor pagination", () => {
    const params = serializeListQuery({
      filter: { status: undefined as unknown as string, owner_id: "" },
      sort: [],
      search: "  ",
      cursor: "eyJpZCI6NDJ9",
      limit: 50,
    });

    expect(params.toString()).toBe("cursor=eyJpZCI6NDJ9&limit=50");
  });
});

describe("parseListQuery", () => {
  it("reads what serializeListQuery writes", () => {
    const query = {
      filter: { status: "active", created_at: { gte: "2026-10-01" }, stage: { in: ["lead", "won"] } },
      sort: ["-created_at", "name"],
      search: "acme",
      page: 3,
      per_page: 50,
    };

    expect(parseListQuery(serializeListQuery(query))).toEqual(query);
    expect(parseListQuery(serializeListQuery(query).toString())).toEqual(query);
    expect(parseListQuery(`?${serializeListQuery(query).toString()}`)).toEqual(query);
  });

  it("reads a Next.js searchParams object and ignores parameters it doesn't own", () => {
    expect(
      parseListQuery({ "filter[owner_id]": "u1", sort: "name", page: "x", per_page: ["10", "20"], tab: "open" }),
    ).toEqual({ filter: { owner_id: "u1" }, sort: ["name"], per_page: 10 });
  });

  it("reads cursor pagination", () => {
    expect(parseListQuery("cursor=abc&limit=20")).toEqual({ cursor: "abc", limit: 20 });
  });

  it("returns an empty query for an empty string", () => {
    expect(parseListQuery("")).toEqual({});
  });
});

describe("listQuery builder", () => {
  it("builds a query step by step without changing earlier steps", () => {
    const base = listQuery().where("status", "active").perPage(25);
    const sorted = base.sort("-created_at").search("acme");
    const ranged = sorted.where("created_at", "gte", "2026-10-01").whereIn("stage", ["lead", "won"]).page(2);

    expect(base.toQuery()).toEqual({ filter: { status: "active" }, per_page: 25 });
    expect(ranged.toQuery()).toEqual({
      filter: { status: "active", created_at: { gte: "2026-10-01" }, stage: { in: ["lead", "won"] } },
      sort: ["-created_at"],
      search: "acme",
      page: 2,
      per_page: 25,
    });
    expect(ranged.toString()).toBe(serializeListQuery(ranged.toQuery()).toString());
    expect(ranged.toSearchParams().get("filter[stage][in]")).toBe("lead,won");
  });

  it("combines operators on one field and replaces an equal filter", () => {
    const query = listQuery()
      .where("amount", "gte", 100)
      .where("amount", "lt", 500)
      .where("status", "lead")
      .where("status", "won")
      .toQuery();

    expect(query.filter).toEqual({ amount: { gte: 100, lt: 500 }, status: "won" });
  });

  it("starts from an existing query and switches to cursor paging", () => {
    const query = listQuery({ search: "a", page: 4 }).cursor("abc").limit(10).toQuery();
    expect(query).toEqual({ search: "a", cursor: "abc", limit: 10 });
  });

  it("removes a filter with without()", () => {
    expect(listQuery().where("a", 1).where("b", 2).without("a").toQuery()).toEqual({ filter: { b: 2 } });
  });
});

describe("listQuerySerializer (openapi-fetch's querySerializer)", () => {
  it("writes nested filters in brackets, lists in `in` and the rest as plain parameters", () => {
    expect(
      decodeURIComponent(
        listQuerySerializer({
          filter: { status: "active", created_at: { gte: "2026-10-01" }, stage: ["lead", "won"] },
          sort: ["-created_at"],
          page: 2,
          ids: ["a", "b"],
          include: undefined,
        }),
      ),
    ).toBe("filter[status]=active&filter[created_at][gte]=2026-10-01&filter[stage][in]=lead,won&sort=-created_at&page=2&ids=a&ids=b");
  });
});

describe("sorting", () => {
  it("cycles a column ascending, descending, then off", () => {
    expect(toggleSort(undefined, "name")).toEqual(["name"]);
    expect(toggleSort(["name"], "name")).toEqual(["-name"]);
    expect(toggleSort(["-name"], "name")).toEqual([]);
  });

  it("replaces other columns unless multi-sort is asked for", () => {
    expect(toggleSort(["-created_at"], "name")).toEqual(["name"]);
    expect(toggleSort(["-created_at"], "name", { multi: true })).toEqual(["-created_at", "name"]);
    expect(toggleSort(["-created_at", "name"], "name", { multi: true })).toEqual(["-created_at", "-name"]);
  });

  it("reports a column's direction", () => {
    expect(sortDirection(["-created_at", "name"], "created_at")).toBe("desc");
    expect(sortDirection(["-created_at", "name"], "name")).toBe("asc");
    expect(sortDirection(["name"], "email")).toBeUndefined();
  });

  it("converts to and from TanStack Table's SortingState", () => {
    expect(toSorting(["-created_at", "name"])).toEqual([
      { id: "created_at", desc: true },
      { id: "name", desc: false },
    ]);
    expect(fromSorting([{ id: "created_at", desc: true }])).toEqual(["-created_at"]);
  });
});

describe("query updates", () => {
  it("setFilter changes one filter and goes back to the first page", () => {
    const query = { filter: { status: "lead" }, page: 3, per_page: 25 };
    expect(setFilter(query, "owner_id", "u1")).toEqual({
      filter: { status: "lead", owner_id: "u1" },
      per_page: 25,
    });
    expect(setFilter(query, "status", undefined)).toEqual({ per_page: 25 });
    expect(query.page).toBe(3);
  });

  it("withPage sets the page and drops page 1", () => {
    expect(withPage({ search: "a" }, 2)).toEqual({ search: "a", page: 2 });
    expect(withPage({ search: "a", page: 2 }, 1)).toEqual({ search: "a" });
  });
});
