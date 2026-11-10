import { describe, expect, it } from "vitest";
import {
  checkListQuery,
  firstPage,
  fromSorting,
  isListParam,
  listQuery,
  listQuerySerializer,
  parseListQuery,
  serializeListQuery,
  setFilter,
  sortDirection,
  toggleSort,
  toSorting,
  withDefaults,
  withPage,
} from "./list-query.js";

const decoded = (params: URLSearchParams) => decodeURIComponent(params.toString());

describe("serializeListQuery", () => {
  it("writes filters, sort, search and paging in Navree's list-query format", () => {
    const params = serializeListQuery({
      filter: { status: "active", created_at: { gte: "2026-10-01", lt: "2026-11-01" } },
      sort: ["-created_at", "name"],
      search: "agra",
      page: 2,
      per_page: 25,
    });

    expect(decoded(params)).toBe(
      "filter[status]=active&filter[created_at][gte]=2026-10-01&filter[created_at][lt]=2026-11-01" +
        "&sort=-created_at,name&search=agra&page=2&per_page=25",
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

  it("writes the page before a position, after cursor and before limit", () => {
    expect(serializeListQuery({ before: "eyJpZCI6NDJ9", limit: 20 }).toString()).toBe("before=eyJpZCI6NDJ9&limit=20");
    expect(serializeListQuery({ limit: 20, before: "b", cursor: "a" }).toString()).toBe("cursor=a&before=b&limit=20");
  });

  it("writes an equal filter plain, even when it is given as `eq`", () => {
    const params = serializeListQuery({ filter: { stage: { eq: "won" }, amount: { gte: 1, eq: 5 } } });
    expect(params.toString()).toBe("filter%5Bstage%5D=won&filter%5Bamount%5D%5Bgte%5D=1&filter%5Bamount%5D=5");
  });

  it("trims the search and the sort terms, as the reader does", () => {
    expect(serializeListQuery({ search: "  agra corp ", sort: [" -name ", " "] }).toString()).toBe("sort=-name&search=agra+corp");
  });
});

describe("parseListQuery", () => {
  it("reads what serializeListQuery writes", () => {
    const query = {
      filter: { status: "active", created_at: { gte: "2026-10-01" }, stage: { in: ["lead", "won"] } },
      sort: ["-created_at", "name"],
      search: "agra",
      page: 3,
      per_page: 50,
    };

    expect(parseListQuery(serializeListQuery(query))).toEqual(query);
    expect(parseListQuery(serializeListQuery(query).toString())).toEqual(query);
    expect(parseListQuery(`?${serializeListQuery(query).toString()}`)).toEqual(query);
  });

  it("reads a Next.js searchParams object and ignores parameters it doesn't own", () => {
    // A repeated page size keeps the last one, as in the Rust reader; a page that is not a number is left out.
    expect(
      parseListQuery({ "filter[owner_id]": "u1", sort: "name", page: "x", per_page: ["10", "20"], tab: "open" }),
    ).toEqual({ filter: { owner_id: "u1" }, sort: ["name"], per_page: 20 });
  });

  it("reads before, and `filter[x][eq]` as a plain filter", () => {
    expect(parseListQuery("before=abc&limit=5&filter[stage][eq]=won")).toEqual({ before: "abc", limit: 5, filter: { stage: "won" } });
  });

  it("parseListQuery is the query of checkListQuery", () => {
    const text = "filter[a]=1&page=0&sort=x";
    expect(parseListQuery(text)).toEqual(checkListQuery(text).query);
    expect(parseListQuery(text)).toEqual({ filter: { a: "1" }, sort: ["x"] });
  });

  it("reads cursor pagination", () => {
    expect(parseListQuery("cursor=abc&limit=20")).toEqual({ cursor: "abc", limit: 20 });
  });

  it("returns an empty query for an empty string", () => {
    expect(parseListQuery("")).toEqual({});
  });
});

describe("checkListQuery", () => {
  it("keeps what is right and names what is wrong, as written", () => {
    const { query, problems } = checkListQuery("?filter[stage]=won&filter[hot][regex]=1&page=0&owner_id=3");
    expect(query).toEqual({ filter: { stage: "won" } });
    expect(problems.map((problem) => problem.param)).toEqual(["filter[hot][regex]", "page"]);
    expect(problems.every((problem) => problem.message !== "")).toBe(true);
  });

  it("finds no problem in an empty query or in other parameters", () => {
    expect(checkListQuery("")).toEqual({ query: {}, problems: [] });
    expect(checkListQuery({ tab: "open", range: "30d" })).toEqual({ query: {}, problems: [] });
  });

  it("reads what serializeListQuery writes, with no problem", () => {
    const written = serializeListQuery({
      filter: { stage: { in: ["lead", "won"] }, owner_id: { null: true }, title: { contains: "a b" } },
      sort: ["-created_at"],
      search: "agra",
      before: "xyz",
      limit: 10,
    });
    expect(checkListQuery(written).problems).toEqual([]);
  });
});

describe("isListParam", () => {
  it("knows which parameters a list query owns", () => {
    for (const key of ["filter", "filter[a]", "filter[a][gte]", "sort", "search", "page", "per_page", "cursor", "before", "limit"]) {
      expect(isListParam(key)).toBe(true);
    }
    for (const key of ["tab", "q", "per", "after", "filters[a]", "owner_id", "range"]) expect(isListParam(key)).toBe(false);
  });
});

describe("listQuery builder", () => {
  it("builds a query step by step without changing earlier steps", () => {
    const base = listQuery().where("status", "active").perPage(25);
    const sorted = base.sort("-created_at").search("agra");
    const ranged = sorted.where("created_at", "gte", "2026-10-01").whereIn("stage", ["lead", "won"]).page(2);

    expect(base.toQuery()).toEqual({ filter: { status: "active" }, per_page: 25 });
    expect(ranged.toQuery()).toEqual({
      filter: { status: "active", created_at: { gte: "2026-10-01" }, stage: { in: ["lead", "won"] } },
      sort: ["-created_at"],
      search: "agra",
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

  it("goes back to numbered paging, and backwards by cursor", () => {
    expect(listQuery({ cursor: "abc", limit: 10 }).page(2).toQuery()).toEqual({ page: 2 });
    expect(listQuery({ page: 2 }).before("abc").toQuery()).toEqual({ before: "abc" });
  });

  it("removes a filter with without()", () => {
    expect(listQuery().where("a", 1).where("b", 2).without("a").toQuery()).toEqual({ filter: { b: 2 } });
  });
});

describe("listQuerySerializer (the client's querySerializer)", () => {
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

  it("writes the list query's own parameters in the canonical order, before the others", () => {
    expect(
      listQuerySerializer({
        tab: "open",
        limit: 20,
        before: "b",
        search: " agra ",
        sort: "-name",
        filter: { stage: "won" },
      }),
    ).toBe("filter%5Bstage%5D=won&sort=-name&search=agra&before=b&limit=20&tab=open");
  });

  it("leaves a value of another type to the generic writer", () => {
    expect(listQuerySerializer({ page: "2", search: 5, filter: "x" })).toBe("page=2&search=5&filter=x");
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

  it("sorts by at most four fields: a fifth is not added", () => {
    expect(toggleSort(["a", "b", "c", "d"], "e", { multi: true })).toEqual(["a", "b", "c", "d"]);
    expect(toggleSort(["a", "b", "c", "d"], "d", { multi: true })).toEqual(["a", "b", "c", "-d"]);
    expect(toggleSort(["a", "b", "c"], "e", { multi: true })).toEqual(["a", "b", "c", "e"]);
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

  it("setFilter also leaves the cursor and the page before it", () => {
    expect(setFilter({ cursor: "a", before: "b", limit: 10 }, "stage", "won")).toEqual({ limit: 10, filter: { stage: "won" } });
    expect(setFilter({ filter: { a: "1" } }, "a", { gte: "" })).toEqual({});
    expect(setFilter({}, "a", [])).toEqual({});
    expect(setFilter({}, "flag", false)).toEqual({ filter: { flag: false } });
  });

  it("withPage sets the page and drops page 1", () => {
    expect(withPage({ search: "a" }, 2)).toEqual({ search: "a", page: 2 });
    expect(withPage({ search: "a", page: 2 }, 1)).toEqual({ search: "a" });
  });

  it("withPage switches to numbered paging: no cursor, before or limit", () => {
    expect(withPage({ cursor: "a", before: "b", limit: 10, per_page: 5 }, 3)).toEqual({ per_page: 5, page: 3 });
  });

  it("firstPage drops the page, the cursor and the page before, and keeps the page size", () => {
    expect(firstPage({ page: 3, per_page: 25, cursor: "a", before: "b", limit: 10, search: "x" })).toEqual({
      per_page: 25,
      limit: 10,
      search: "x",
    });
  });

  it("withDefaults puts the query over its defaults without mixing the two ways of paging", () => {
    expect(withDefaults({ search: "a" }, { per_page: 25, sort: ["-created_at"] })).toEqual({
      per_page: 25,
      sort: ["-created_at"],
      search: "a",
    });
    expect(withDefaults({ sort: ["name"] }, { sort: ["-created_at"] })).toEqual({ sort: ["name"] });
    // A cursor in the address takes no default page size; a page takes no default cursor size.
    expect(withDefaults({ cursor: "abc" }, { per_page: 25 })).toEqual({ cursor: "abc" });
    expect(withDefaults({ cursor: "abc" }, { limit: 25 })).toEqual({ cursor: "abc", limit: 25 });
    expect(withDefaults({ page: 2 }, { limit: 25, cursor: "a" })).toEqual({ page: 2 });
    const defaults = { filter: { stage: "lead" } };
    withDefaults({}, defaults).filter!.stage = "won";
    expect(defaults.filter.stage).toBe("lead");
  });
});
