import { describe, expect, it } from "vitest";
import { parseListQuery, serializeListQuery, type ListQuery } from "./list-query.js";
import {
  acceptsValue,
  activeFilters,
  clearFilters,
  filterControl,
  filterKey,
  filterText,
  inputType,
  isFiltered,
  readFilter,
  removeFilterLabel,
  withSearch,
  writeFilter,
  type FilterColumn,
} from "./table-filters.js";
import { defaultTableLabels } from "./table-labels.js";

const stage: FilterColumn = {
  field: "stage",
  label: "Stage",
  filter: "multi",
  options: [
    { value: "lead", label: "Lead" },
    { value: "won", label: "Won" },
    { value: "lost", label: "Lost" },
  ],
};
const stageOne: FilterColumn = { ...stage, filter: "select" };
const amount: FilterColumn = { field: "amount", label: "Amount", filter: "range", kind: "int", nullable: true };
const closed: FilterColumn = { field: "closed_on", label: "Closed on", filter: "range", kind: "date" };
const title: FilterColumn = { field: "title", label: "Title", filter: "text" };
const active: FilterColumn = { field: "active", label: "Active", filter: "boolean", kind: "bool" };
const owner: FilterColumn = { field: "owner_id", label: "Owner", filter: "relation", kind: "int", nullable: true };

const words = defaultTableLabels;
const params = (query: ListQuery) => serializeListQuery(query).toString();

describe("values by kind", () => {
  it("reads whole numbers within a 64-bit range", () => {
    for (const ok of ["12", "-3", "+4", "9223372036854775807", "-9223372036854775808"]) expect(acceptsValue("int", ok), ok).toBe(true);
    for (const bad of ["", "1.5", "abc", "9223372036854775808", "1e3", " 1"]) expect(acceptsValue("int", bad), bad).toBe(false);
  });

  it("reads finite decimal numbers", () => {
    for (const ok of ["1.5", ".5", "-2", "1e3", "10."]) expect(acceptsValue("float", ok), ok).toBe(true);
    for (const bad of ["", "abc", "1e999", "0x10", "1,5"]) expect(acceptsValue("decimal", bad), bad).toBe(false);
  });

  it("reads money in the grouping of either side of the world", () => {
    for (const ok of ["12500", "12,500.50", "12,34,567.50", "1,234,567", "-3.5", ".5"]) expect(acceptsValue("money", ok), ok).toBe(true);
    for (const bad of ["12.505", "1,2", "1,23,45", "abc", ""]) expect(acceptsValue("money", bad), bad).toBe(false);
  });

  it("reads days that exist", () => {
    expect(acceptsValue("date", "2026-11-07")).toBe(true);
    expect(acceptsValue("datetime", "2028-02-29")).toBe(true);
    for (const bad of ["2026-02-30", "2027-02-29", "07-11-2026", "2026-1-7", "yesterday"]) expect(acceptsValue("date", bad), bad).toBe(false);
  });

  it("reads booleans, uuids and any text", () => {
    expect(acceptsValue("bool", "true")).toBe(true);
    expect(acceptsValue("bool", "yes")).toBe(false);
    expect(acceptsValue("uuid", "0b6f1c3e-7d2a-4c11-9a55-3e0d6a8f2b71")).toBe(true);
    expect(acceptsValue("uuid", "not-a-uuid")).toBe(false);
    expect(acceptsValue("text", "")).toBe(true);
  });

  it("types the input of a range", () => {
    expect(inputType("date")).toBe("date");
    expect(inputType("datetime")).toBe("date");
    expect(inputType("int")).toBe("number");
    expect(inputType("decimal")).toBe("number");
    expect(inputType("money")).toBe("text");
    expect(inputType(undefined)).toBe("text");
  });
});

describe("reading a filter from the address", () => {
  it("reads each of the filters Navree UI writes", () => {
    const query = parseListQuery(
      "filter[title][contains]=sutlej&filter[stage][in]=won,lost&filter[amount][gte]=100&filter[amount][lte]=900&filter[active]=true&filter[owner_id][null]=true",
    );
    expect(readFilter(query, title)).toEqual({ type: "text", text: "sutlej" });
    expect(readFilter(query, stage)).toEqual({ type: "values", values: ["won", "lost"] });
    expect(readFilter(query, amount)).toEqual({ type: "range", min: "100", max: "900" });
    expect(readFilter(query, active)).toEqual({ type: "bool", value: true });
    expect(readFilter(query, owner)).toEqual({ type: "empty" });
  });

  it("reads a select as its first value and a multi filter as all of them, once each", () => {
    const query = parseListQuery("filter[stage][in]=won,lost,won");
    expect(readFilter(query, stageOne)).toEqual({ type: "values", values: ["won"] });
    expect(readFilter(query, stage)).toEqual({ type: "values", values: ["won", "lost"] });
    expect(readFilter(parseListQuery("filter[stage]=lead"), stage)).toEqual({ type: "values", values: ["lead"] });
  });

  it("leaves out what the column cannot hold, as Navree UI does", () => {
    const query = parseListQuery("filter[amount][gte]=abc&filter[amount][lte]=900&filter[closed_on][gte]=2026-02-30");
    expect(readFilter(query, amount)).toEqual({ type: "range", max: "900" });
    expect(readFilter(query, closed)).toBeUndefined();
    expect(readFilter(parseListQuery("filter[active]=maybe"), active)).toBeUndefined();
    expect(readFilter(parseListQuery("filter[owner_id][in]=1,x,3"), owner)).toEqual({ type: "values", values: ["1", "3"] });
  });

  it("knows no empty filter for a column that cannot hold NULL, and not 'is not NULL'", () => {
    expect(readFilter(parseListQuery("filter[closed_on][null]=true"), closed)).toBeUndefined();
    expect(readFilter(parseListQuery("filter[amount][null]=false"), amount)).toBeUndefined();
  });

  it("puts the empty filter before the others", () => {
    expect(readFilter(parseListQuery("filter[amount][null]=true&filter[amount][gte]=5"), amount)).toEqual({ type: "empty" });
  });

  it("finds nothing for a field that is not in the query, including names every object has", () => {
    expect(readFilter({}, title)).toBeUndefined();
    expect(readFilter({ filter: { other: "x" } }, title)).toBeUndefined();
    expect(readFilter({ filter: {} }, { field: "constructor", label: "C", filter: "text" })).toBeUndefined();
  });
});

describe("writing a filter", () => {
  it("writes the spellings Navree UI links use, and goes back to the first page", () => {
    const from: ListQuery = { page: 3, sort: ["title"] };
    expect(params(writeFilter(from, title, { type: "text", text: " sutlej " }))).toBe("filter%5Btitle%5D%5Bcontains%5D=sutlej&sort=title");
    expect(params(writeFilter(from, stage, { type: "values", values: ["won", "lost"] }))).toBe("filter%5Bstage%5D%5Bin%5D=won%2Clost&sort=title");
    expect(params(writeFilter(from, stageOne, { type: "values", values: ["won"] }))).toBe("filter%5Bstage%5D=won&sort=title");
    expect(params(writeFilter(from, active, { type: "bool", value: false }))).toBe("filter%5Bactive%5D=false&sort=title");
    expect(params(writeFilter(from, owner, { type: "empty" }))).toBe("filter%5Bowner_id%5D%5Bnull%5D=true&sort=title");
    expect(params(writeFilter(from, amount, { type: "range", min: "100", max: "900" }))).toBe(
      "filter%5Bamount%5D%5Bgte%5D=100&filter%5Bamount%5D%5Blte%5D=900&sort=title",
    );
    expect(writeFilter(from, title, { type: "text", text: "x" }).page).toBeUndefined();
  });

  it("keeps the other filters, the search and the page size", () => {
    const from: ListQuery = { search: "agra", per_page: 10, filter: { stage: "won" } };
    const next = writeFilter(from, title, { type: "text", text: "pilot" });
    expect(next).toEqual({ search: "agra", per_page: 10, filter: { stage: "won", title: { contains: "pilot" } } });
    expect(from).toEqual({ search: "agra", per_page: 10, filter: { stage: "won" } });
  });

  it("removes the filter when given nothing, or a value that holds nothing", () => {
    const from: ListQuery = { filter: { stage: { in: ["won"] }, title: { contains: "x" } } };
    expect(writeFilter(from, stage, undefined)).toEqual({ filter: { title: { contains: "x" } } });
    expect(writeFilter(from, stage, { type: "values", values: [] })).toEqual({ filter: { title: { contains: "x" } } });
    expect(writeFilter(from, title, { type: "text", text: "   " })).toEqual({ filter: { stage: { in: ["won"] } } });
    expect(writeFilter(from, amount, { type: "range" })).toEqual(from);
    expect(writeFilter({ filter: { title: { contains: "x" } } }, title, undefined)).toEqual({});
  });

  it("drops a bound the column cannot hold, and an empty filter on a column without NULL", () => {
    expect(writeFilter({}, amount, { type: "range", min: "abc", max: "9" })).toEqual({ filter: { amount: { lte: "9" } } });
    expect(writeFilter({}, amount, { type: "range", min: "abc" })).toEqual({});
    expect(writeFilter({}, closed, { type: "empty" })).toEqual({});
    expect(writeFilter({}, owner, { type: "values", values: ["1", "x", "1", "2"] })).toEqual({ filter: { owner_id: { in: ["1", "2"] } } });
  });

  it("writes what it reads back", () => {
    const columns = [title, stage, amount, active, owner];
    let query: ListQuery = { sort: ["-amount"] };
    query = writeFilter(query, title, { type: "text", text: "Ludhiana" });
    query = writeFilter(query, stage, { type: "values", values: ["lead", "won"] });
    query = writeFilter(query, amount, { type: "range", min: "100000", max: "900000" });
    query = writeFilter(query, active, { type: "bool", value: true });
    query = writeFilter(query, owner, { type: "empty" });
    const again = parseListQuery(params(query));
    expect(params(again)).toBe(params(query));
    expect(columns.map((column) => readFilter(again, column))).toEqual([
      { type: "text", text: "Ludhiana" },
      { type: "values", values: ["lead", "won"] },
      { type: "range", min: "100000", max: "900000" },
      { type: "bool", value: true },
      { type: "empty" },
    ]);
  });
});

describe("search", () => {
  it("sets the search trimmed, back on the first page, and removes it when blank", () => {
    expect(withSearch({ page: 2, sort: ["a"], search: "old" }, "  agra  ")).toEqual({ sort: ["a"], search: "agra" });
    expect(withSearch({ page: 2, search: "old", filter: { stage: "won" } }, "   ")).toEqual({ filter: { stage: "won" } });
    expect(withSearch({ cursor: "abc", limit: 20 }, "x")).toEqual({ limit: 20, search: "x" });
  });

  it("knows whether a query searches or filters", () => {
    expect(isFiltered({})).toBe(false);
    expect(isFiltered({ sort: ["a"], page: 3 })).toBe(false);
    expect(isFiltered({ search: "  " })).toBe(false);
    expect(isFiltered({ search: "agra" })).toBe(true);
    expect(isFiltered({ filter: { stage: "won" } })).toBe(true);
  });

  it("clears the search and the filters and nothing else", () => {
    expect(clearFilters({ filter: { stage: "won" }, search: "x", sort: ["-a"], page: 3, per_page: 10 })).toEqual({ sort: ["-a"], per_page: 10 });
  });

  it("names a selection of all results by the search and filters alone", () => {
    const base: ListQuery = { filter: { stage: "won" }, search: "x" };
    expect(filterKey(base)).toBe("filter%5Bstage%5D=won&search=x");
    expect(filterKey({ ...base, sort: ["-a"], page: 4, per_page: 50 })).toBe(filterKey(base));
    expect(filterKey({ ...base, search: "y" })).not.toBe(filterKey(base));
    expect(filterKey({})).toBe("");
  });
});

describe("the chip of a filter", () => {
  it("says each in Navree UI's words", () => {
    expect(filterText(title, { type: "text", text: "sutlej" }, words)).toBe("Title: sutlej");
    expect(filterText(stage, { type: "values", values: ["lead", "won"] }, words)).toBe("Stage: Lead, Won");
    expect(filterText(stage, { type: "values", values: ["lead", "won", "lost", "x"] }, words)).toBe("Stage: Lead, Won, Lost +1");
    expect(filterText(active, { type: "bool", value: false }, words)).toBe("Active: No");
    expect(filterText(owner, { type: "empty" }, words)).toBe("Owner: Empty");
    expect(filterText(amount, { type: "range", min: "1", max: "9" }, words)).toBe("Amount: 1 – 9");
    expect(filterText(amount, { type: "range", min: "1" }, words)).toBe("Amount: ≥ 1");
    expect(filterText(amount, { type: "range", max: "9" }, words)).toBe("Amount: ≤ 9");
  });

  it("lists the filters in force in the order of the columns, whatever the address says", () => {
    const query = parseListQuery("filter[stage][in]=won&filter[title][contains]=pilot&filter[other]=1");
    const list = activeFilters(query, [title, stage, amount], words);
    expect(list.map((item) => [item.field, item.text])).toEqual([
      ["title", "Title: pilot"],
      ["stage", "Stage: Won"],
    ]);
    expect(removeFilterLabel(words, list[0]!.text)).toBe("Remove filter Title: pilot");
  });
});

describe("a filter's control", () => {
  it("is a box for text and shows what the query holds", () => {
    expect(filterControl({}, title, words)).toEqual({ type: "text", value: "" });
    expect(filterControl({ filter: { title: { contains: "x" } } }, title, words)).toEqual({ type: "text", value: "x" });
  });

  it("types the inputs of a range by the column's kind", () => {
    expect(filterControl({}, amount, words)).toMatchObject({ type: "range", inputType: "number", minLabel: "Min", maxLabel: "Max" });
    expect(filterControl({}, amount, words)).not.toHaveProperty("step");
    expect(filterControl({}, { ...amount, kind: "decimal" }, words)).toMatchObject({ inputType: "number", step: "any" });
    expect(filterControl({}, { ...amount, kind: "money" }, words)).toMatchObject({ inputType: "text", inputMode: "decimal" });
    expect(filterControl({ filter: { closed_on: { gte: "2026-11-01" } } }, closed, words)).toEqual({
      type: "range",
      min: "2026-11-01",
      max: "",
      inputType: "date",
      minLabel: "From",
      maxLabel: "To",
    });
  });

  it("is a list of Any, Yes and No for a boolean, and of the options for a select", () => {
    expect(filterControl({ filter: { active: true } }, active, words)).toEqual({
      type: "select",
      value: "true",
      choices: [
        { value: "true", label: "Yes" },
        { value: "false", label: "No" },
      ],
      prompt: "Any",
    });
    expect(filterControl({ filter: { stage: "won" } }, stageOne, words)).toMatchObject({ type: "select", value: "won", prompt: "Any" });
  });

  it("is a list of ticks for a multi filter, with a Find box past eight choices, and the key itself without choices", () => {
    expect(filterControl(parseListQuery("filter[stage][in]=won"), stage, words)).toEqual({
      type: "multi",
      values: ["won"],
      choices: stage.options,
      find: false,
    });
    const many = Array.from({ length: 9 }, (_, at) => ({ value: String(at), label: `Branch ${at}` }));
    expect(filterControl({}, { ...stage, options: many }, words)).toMatchObject({ type: "multi", find: true });
    expect(filterControl(parseListQuery("filter[owner_id][in]=7"), owner, words)).toEqual({ type: "keys", value: "7" });
  });
});
