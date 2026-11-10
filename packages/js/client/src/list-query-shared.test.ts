import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { checkListQuery, listQuery, serializeListQuery } from "./list-query.js";

/**
 * The cases the Rust parser (`navree_http::list`) reads too. A case is never
 * changed to make this parser pass: if the two disagree, the parser is fixed.
 */
interface Case {
  name: string;
  input: string;
  canonical: string;
  problems: string[];
}

const file = JSON.parse(readFileSync(new URL("../test/list-query-cases.json", import.meta.url), "utf8")) as { cases: Case[] };

/** The parameters named, once each, in order. */
const named = (problems: readonly { param: string }[]) => [...new Set(problems.map((problem) => problem.param))].sort();

describe("the shared list-query cases", () => {
  it("are all there", () => {
    expect(file.cases.length).toBeGreaterThanOrEqual(63);
  });

  for (const c of file.cases) {
    describe(c.name, () => {
      it("writes the canonical query string and names the problem parameters", () => {
        const { query, problems } = checkListQuery(c.input);
        expect(serializeListQuery(query).toString()).toBe(c.canonical);
        expect(named(problems)).toEqual(named(c.problems.map((param) => ({ param }))));
      });

      it("reads the same with a leading ? and as URLSearchParams", () => {
        const expected = named(c.problems.map((param) => ({ param })));
        for (const input of [`?${c.input}`, new URLSearchParams(c.input)]) {
          const { query, problems } = checkListQuery(input);
          expect(serializeListQuery(query).toString()).toBe(c.canonical);
          expect(named(problems)).toEqual(expected);
        }
      });

      it("reads its own canonical form back to the same string", () => {
        const { query } = checkListQuery(c.canonical);
        expect(serializeListQuery(query).toString()).toBe(c.canonical);
      });
    });
  }
});

// ---- Limits the shared cases do not reach ----------------------------------------------

const problemsOf = (input: string) => named(checkListQuery(input).problems);

describe("limits", () => {
  it("keeps 64 conditions and names `filter` for the rest", () => {
    const many = Array.from({ length: 70 }, (_, n) => `filter[f${n}]=1`).join("&");
    const { query, problems } = checkListQuery(many);
    expect(Object.keys(query.filter ?? {})).toHaveLength(64);
    expect(named(problems)).toEqual(["filter"]);
  });

  it("still adds values to an existing `in` when 64 conditions are in", () => {
    const many = Array.from({ length: 64 }, (_, n) => `filter[f${n}][in]=a`).join("&");
    const { query, problems } = checkListQuery(`${many}&filter[f0][in]=b`);
    expect(query.filter?.f0).toEqual({ in: ["a", "b"] });
    expect(problems).toEqual([]);
  });

  it("keeps 200 values of an `in` and reports the rest once", () => {
    const values = Array.from({ length: 250 }, (_, n) => `v${n}`).join(",");
    const { query, problems } = checkListQuery(`filter[id][in]=${values}`);
    expect((query.filter?.id as { in: string[] }).in).toHaveLength(200);
    expect(problems).toHaveLength(1);
    expect(problems[0]?.param).toBe("filter[id][in]");
  });

  it("stops adding to an `in` at 200 values across repeated keys", () => {
    const first = Array.from({ length: 150 }, (_, n) => `a${n}`).join(",");
    const second = Array.from({ length: 150 }, (_, n) => `b${n}`).join(",");
    const { query, problems } = checkListQuery(`filter[id][in]=${first}&filter[id][in]=${second}`);
    expect((query.filter?.id as { in: string[] }).in).toHaveLength(200);
    expect(named(problems)).toEqual(["filter[id][in]"]);
  });

  it("measures values in bytes: 256 pass, 257 do not", () => {
    expect(problemsOf(`filter[a]=${"x".repeat(256)}`)).toEqual([]);
    expect(problemsOf(`filter[a]=${"x".repeat(257)}`)).toEqual(["filter[a]"]);
    // 86 Gurmukhi letters are 258 bytes, 85 are 255.
    expect(problemsOf(`filter[a]=${"ਪ".repeat(85)}`)).toEqual([]);
    expect(problemsOf(`filter[a]=${"ਪ".repeat(86)}`)).toEqual(["filter[a]"]);
  });

  it("drops a whole `in` list when one of its values is too long", () => {
    const { query, problems } = checkListQuery(`filter[a][in]=ok,${"x".repeat(300)}`);
    expect(query.filter).toBeUndefined();
    expect(named(problems)).toEqual(["filter[a][in]"]);
  });

  it("names the explicit-eq filter as the plain one", () => {
    expect(problemsOf(`filter[a][eq]=${"x".repeat(300)}`)).toEqual(["filter[a]"]);
    expect(problemsOf("filter[a]=1&filter[a][eq]=2")).toEqual(["filter[a]"]);
  });

  it("clips a search to 256 bytes on a character boundary and reports it", () => {
    const { query, problems } = checkListQuery(`search=${"ਪ".repeat(200)}`);
    expect(new TextEncoder().encode(query.search ?? "").length).toBeLessThanOrEqual(256);
    expect(query.search).toBe("ਪ".repeat(85));
    expect(named(problems)).toEqual(["search"]);
  });

  it("does not split a pair of UTF-16 units when it clips", () => {
    // Each emoji is 4 bytes; 64 fit in 256 bytes.
    const { query } = checkListQuery(`search=${"\u{1F600}".repeat(100)}`);
    expect(query.search).toBe("\u{1F600}".repeat(64));
  });

  it("limits a cursor to 1024 bytes", () => {
    expect(problemsOf(`cursor=${"c".repeat(1024)}`)).toEqual([]);
    expect(problemsOf(`cursor=${"c".repeat(1025)}`)).toEqual(["cursor"]);
    expect(problemsOf(`before=${"c".repeat(1025)}`)).toEqual(["before"]);
  });

  it("limits page to 10,000,000 and page size to 10,000", () => {
    expect(problemsOf("page=10000000")).toEqual([]);
    expect(problemsOf("page=10000001")).toEqual(["page"]);
    expect(problemsOf("per_page=10000&limit=10000")).toEqual(["limit"]);
    expect(problemsOf("per_page=10001")).toEqual(["per_page"]);
    expect(problemsOf("limit=10001")).toEqual(["limit"]);
    expect(checkListQuery("page=007").query.page).toBe(7);
  });

  it("reads at most 1000 parameters", () => {
    const many = Array.from({ length: 1005 }, (_, n) => `x${n}=1`).join("&");
    expect(problemsOf(many)).toEqual(["query"]);
    // The thousandth parameter is still read; the 1001st is not.
    const edge = `${Array.from({ length: 999 }, (_, n) => `x${n}=1`).join("&")}&search=a&search=b`;
    expect(checkListQuery(edge).query.search).toBe("a");
    expect(problemsOf(edge)).toEqual(["query"]);
    expect(problemsOf(`${Array.from({ length: 999 }, (_, n) => `x${n}=1`).join("&")}&search=a`)).toEqual([]);
  });

  it("reads at most 16 KiB of query string", () => {
    const long = `search=a&filter[a]=${"x".repeat(16 * 1024)}`;
    const { query, problems } = checkListQuery(long);
    expect(named(problems)).toContain("query");
    expect(query.search).toBe("a");
    expect(problemsOf(`search=${"a".repeat(16 * 1024 - 7)}`)).toEqual(["search"]);
  });

  it("measures the query string for URLSearchParams and Next.js objects too", () => {
    const huge = "x".repeat(20 * 1024);
    expect(named(checkListQuery(new URLSearchParams({ junk: huge })).problems)).toEqual(["query"]);
    expect(named(checkListQuery({ junk: huge }).problems)).toEqual(["query"]);
  });
});

// ---- Syntax details ---------------------------------------------------------------------

describe("syntax details", () => {
  it("reports each repeat but keeps the first value", () => {
    const { query, problems } = checkListQuery("filter[a][lt]=1&filter[a][lt]=2&filter[a][lt]=3");
    expect(query.filter).toEqual({ a: { lt: "1" } });
    expect(problems).toHaveLength(2);
  });

  it("keeps the last page, per_page, limit, search, cursor and before", () => {
    const { query } = checkListQuery("page=2&page=3&per_page=5&per_page=6&search=a&search=b");
    expect(query).toEqual({ page: 3, per_page: 6, search: "b" });
    expect(checkListQuery("cursor=a&cursor=b&before=c&before=d&limit=1&limit=2").query).toEqual({
      cursor: "b",
      before: "d",
      limit: 2,
    });
  });

  it("adds up a repeated sort and reports a fifth field once", () => {
    const { query, problems } = checkListQuery("sort=a&sort=-b&sort=c,d,e,f");
    expect(query.sort).toEqual(["a", "-b", "c", "d"]);
    expect(problems).toHaveLength(1);
  });

  it("trims sort terms and reads a blank one as nothing", () => {
    expect(checkListQuery("sort=%20a%20,%20,-%20b").query.sort).toEqual(["a"]);
  });

  it("does not trim an `in` part and keeps a blank one that is not empty", () => {
    expect(checkListQuery("filter[a][in]=a,%20,b").query.filter).toEqual({ a: { in: ["a", " ", "b"] } });
  });

  it("reads a whitespace-only value as empty, not as a value", () => {
    const { query, problems } = checkListQuery("filter[a]=%20%09&filter[b][null]=%20&page=%20");
    expect(query).toEqual({});
    expect(named(problems)).toEqual(["page"]);
  });

  it("keeps eq and other operators of one field together, eq in its place", () => {
    const { query } = checkListQuery("filter[a][gte]=1&filter[a]=2&filter[b]=3");
    expect(query.filter).toEqual({ a: { gte: "1", eq: "2" }, b: "3" });
    expect(serializeListQuery(query).toString()).toBe("filter%5Ba%5D%5Bgte%5D=1&filter%5Ba%5D=2&filter%5Bb%5D=3");
  });

  it("reads null as a boolean and `in` as a list", () => {
    expect(checkListQuery("filter[a][null]=1&filter[b][not_in]=x,y").query.filter).toEqual({
      a: { null: true },
      b: { not_in: ["x", "y"] },
    });
  });

  it("names the first of cursor, before, limit when paging is mixed", () => {
    expect(problemsOf("page=1&before=a&limit=5")).toEqual(["before"]);
    expect(problemsOf("per_page=5&limit=5")).toEqual(["limit"]);
    expect(problemsOf("page=1&cursor=a&before=b")).toEqual(["cursor"]);
  });

  it("leaves a second leading ? to the first parameter's name", () => {
    expect(checkListQuery("??search=a").query).toEqual({});
    expect(checkListQuery("?search=a").query).toEqual({ search: "a" });
  });

  it("reads a Next.js searchParams object, every value of a repeated key", () => {
    const { query } = checkListQuery({
      "filter[stage][in]": ["lead", "won"],
      sort: ["a", "-b"],
      tab: "open",
      search: undefined,
    });
    expect(query).toEqual({ filter: { stage: { in: ["lead", "won"] } }, sort: ["a", "-b"] });
  });

  it("trims the white space the Rust reader trims", () => {
    // U+0085 and U+3000 are white space to the Rust reader; JavaScript's own trim keeps U+0085.
    expect(checkListQuery("search=%C2%85a%C2%85").query.search).toBe("a");
    expect(checkListQuery("search=%E3%80%80a%E3%80%80").query.search).toBe("a");
  });
});

// ---- Field names that are also names on every object -----------------------------------

const ownKeys = (value: object | undefined) => Object.keys(value ?? {});

describe("names that are names on every object", () => {
  const names = ["__proto__", "constructor", "toString", "hasOwnProperty", "valueOf"];

  it("are ordinary keys when read", () => {
    const input = names.map((name) => `filter[${name}]=x`).join("&");
    const { query, problems } = checkListQuery(input);
    expect(problems).toEqual([]);
    expect(ownKeys(query.filter)).toEqual(names);
    expect(Object.getPrototypeOf(query.filter)).toBe(Object.prototype);
    expect(Object.getOwnPropertyDescriptor(query.filter, "__proto__")?.value).toBe("x");
  });

  it("do not reach the prototype of any object", () => {
    checkListQuery("filter[__proto__][in]=polluted&filter[constructor][null]=true&sort=__proto__");
    expect(({} as Record<string, unknown>).polluted).toBeUndefined();
    expect(Object.keys(Object.prototype)).toEqual([]);
    expect(Object.getPrototypeOf({})).toBe(Object.prototype);
  });

  it("are kept by the builder, by a copy and by without()", () => {
    const filter = Object.fromEntries(names.map((name) => [name, "x"]));
    const built = listQuery({ filter });
    expect(ownKeys(built.toQuery().filter)).toEqual(names);
    expect(Object.getOwnPropertyDescriptor(built.toQuery().filter, "__proto__")?.value).toBe("x");
    expect(built.toString()).toBe(names.map((name) => `filter%5B${name}%5D=x`).join("&"));

    const without = built.without("__proto__").toQuery();
    expect(ownKeys(without.filter)).toEqual(names.slice(1));
    expect(Object.hasOwn(without.filter ?? {}, "__proto__")).toBe(false);
  });

  it("take operators in the builder without reading what every object has", () => {
    const query = listQuery().where("toString", "gte", 1).where("toString", "lt", 5).where("__proto__", "in", ["a", "b"]).toQuery();
    expect(ownKeys(query.filter)).toEqual(["toString", "__proto__"]);
    expect(Object.getOwnPropertyDescriptor(query.filter, "toString")?.value).toEqual({ gte: 1, lt: 5 });
    expect(Object.getOwnPropertyDescriptor(query.filter, "__proto__")?.value).toEqual({ in: ["a", "b"] });
    expect(Object.getPrototypeOf(query.filter)).toBe(Object.prototype);
  });

  it("round trip through the string", () => {
    const text = listQuery().where("__proto__", "x").where("constructor", "in", ["a", "b"]).toString();
    expect(text).toBe("filter%5B__proto__%5D=x&filter%5Bconstructor%5D%5Bin%5D=a%2Cb");
    expect(serializeListQuery(checkListQuery(text).query).toString()).toBe(text);
  });
});

// ---- The builder writes the canonical form ---------------------------------------------

describe("the builder", () => {
  it("writes filters grouped by field, then sort, search, page, per_page", () => {
    const text = listQuery()
      .perPage(25)
      .page(2)
      .search("  agra corp ")
      .sort("-created_at", "name")
      .where("amount", "gte", 100)
      .where("stage", "won")
      .where("amount", "lte", 900)
      .toString();
    expect(text).toBe(
      "filter%5Bamount%5D%5Bgte%5D=100&filter%5Bamount%5D%5Blte%5D=900&filter%5Bstage%5D=won" +
        "&sort=-created_at%2Cname&search=agra+corp&page=2&per_page=25",
    );
  });

  it("writes cursor, before and limit last, in that order", () => {
    expect(listQuery().limit(20).cursor("abc").toString()).toBe("cursor=abc&limit=20");
    expect(listQuery().limit(20).before("abc").toString()).toBe("before=abc&limit=20");
  });

  it("page and perPage drop cursor, before and limit", () => {
    const cursor = listQuery().cursor("a").limit(10);
    expect(cursor.page(2).toQuery()).toEqual({ page: 2 });
    expect(cursor.perPage(5).toQuery()).toEqual({ per_page: 5 });
    expect(listQuery().before("b").limit(10).page(3).toQuery()).toEqual({ page: 3 });
  });

  it("cursor, before and limit drop page and per_page", () => {
    const paged = listQuery().page(4).perPage(10);
    expect(paged.cursor("a").toQuery()).toEqual({ cursor: "a" });
    expect(paged.before("b").toQuery()).toEqual({ before: "b" });
    expect(paged.limit(5).toQuery()).toEqual({ limit: 5 });
  });

  it("cursor and before replace each other, since a page is after or before a position", () => {
    expect(listQuery().cursor("a").before("b").toQuery()).toEqual({ before: "b" });
    expect(listQuery().before("b").cursor("a").toQuery()).toEqual({ cursor: "a" });
  });

  it("starts from a query without changing it", () => {
    const initial = { filter: { stage: ["a"] }, sort: ["name"], cursor: "x", before: "y" };
    const built = listQuery(initial).search("q");
    expect(initial).toEqual({ filter: { stage: ["a"] }, sort: ["name"], cursor: "x", before: "y" });
    expect(built.toQuery()).toEqual({ filter: { stage: ["a"] }, sort: ["name"], cursor: "x", before: "y", search: "q" });
    built.toQuery().sort?.push("other");
    expect(built.toQuery().sort).toEqual(["name"]);
  });

  it("copies dates, so a later change to the date does not reach the query", () => {
    const due = new Date("2026-10-27T00:00:00Z");
    const built = listQuery().where("due_at", "lte", due);
    due.setUTCFullYear(2030);
    expect(built.toString()).toBe("filter%5Bdue_at%5D%5Blte%5D=2026-10-27T00%3A00%3A00.000Z");
  });

  it("keeps an equal filter when an operator is added to the same field", () => {
    const query = listQuery().where("amount", 5).where("amount", "gte", 1).toQuery();
    expect(query.filter).toEqual({ amount: { eq: 5, gte: 1 } });
    expect(listQuery().where("amount", "eq", 5).toQuery().filter).toEqual({ amount: 5 });
  });

  it("leaves out a blank search, blank values and empty lists", () => {
    expect(listQuery().search("   ").where("a", "").where("b", []).where("c", "gte", "").toString()).toBe("");
  });

  it("writes an `in` list without its empty parts", () => {
    expect(listQuery().whereIn("a", ["x", "", "y"]).toString()).toBe("filter%5Ba%5D%5Bin%5D=x%2Cy");
  });

  it("whatever it writes, the reader reads back as the same query string", () => {
    const text = listQuery()
      .where("stage", ["lead", "won"])
      .where("owner_id", "null", false)
      .where("title", "contains", "a b,c")
      .where("amount", "gte", 1000)
      .sort("-amount", "title")
      .search("Sutlej Supply")
      .limit(30)
      .before("eyJpZCI6NDJ9")
      .toString();
    const { query, problems } = checkListQuery(text);
    expect(problems).toEqual([]);
    expect(serializeListQuery(query).toString()).toBe(text);
  });
});
