import { describe, expect, it } from "vitest";
import { defaultTableLabels } from "./table-labels.js";
import {
  bulkParams,
  bulkRequest,
  describeSelection,
  emptySelection,
  isSelected,
  MAX_SELECTED_IDS,
  pageSelection,
  reconcileSelection,
  rowRange,
  selectAllMatching,
  selectedCount,
  setSelected,
  type TableSelection,
} from "./table-selection.js";

const words = defaultTableLabels;
const rows = ["d1", "d2", "d3"];

describe("what is selected", () => {
  it("counts the keys, or the results except the ones left out", () => {
    expect(selectedCount(emptySelection(), 60)).toBe(0);
    expect(selectedCount({ ...emptySelection(), ids: ["a", "b"] }, 60)).toBe(2);
    expect(selectedCount(selectAllMatching("k"), 60)).toBe(60);
    expect(selectedCount({ ...selectAllMatching("k"), ex: ["a", "b"] }, 60)).toBe(58);
    expect(selectedCount({ ...selectAllMatching("k"), ex: ["a", "b"] }, 1)).toBe(0);
    expect(selectedCount(selectAllMatching("k"), undefined)).toBe(0);
  });

  it("tells a selected row in either mode", () => {
    expect(isSelected({ ...emptySelection(), ids: ["a"] }, "a")).toBe(true);
    expect(isSelected({ ...emptySelection(), ids: ["a"] }, "b")).toBe(false);
    expect(isSelected({ ...selectAllMatching("k"), ex: ["a"] }, "a")).toBe(false);
    expect(isSelected({ ...selectAllMatching("k"), ex: ["a"] }, "b")).toBe(true);
  });

  it("turns rows on and off, in 'all' mode by the rows left out, and never changes the selection it was given", () => {
    const none = emptySelection();
    const two = setSelected(none, ["a", "b"], true);
    expect(two.ids).toEqual(["a", "b"]);
    expect(none.ids).toEqual([]);
    expect(setSelected(two, ["a", "c"], true).ids).toEqual(["a", "b", "c"]);
    expect(setSelected(two, ["a"], false).ids).toEqual(["b"]);

    const all = selectAllMatching("k");
    const without = setSelected(all, ["a"], false);
    expect(without).toEqual({ mode: "all", ids: [], ex: ["a"], key: "k" });
    expect(setSelected(without, ["a"], true).ex).toEqual([]);
    expect(all.ex).toEqual([]);
  });

  it("ends a selection of all results when the search or filters change, and a selection of keys never", () => {
    const all = selectAllMatching("filter%5Bstage%5D=won");
    expect(reconcileSelection(all, "filter%5Bstage%5D=won")).toEqual({ selection: all, ended: false });
    expect(reconcileSelection(all, "filter%5Bstage%5D=lost")).toEqual({ selection: emptySelection(), ended: true });
    const keys = setSelected(emptySelection(), ["a"], true);
    expect(reconcileSelection(keys, "anything")).toEqual({ selection: keys, ended: false });
  });

  it("selects from one row to another for a shift-click, either way round", () => {
    const ids = ["a", "b", "c", "d", "e"];
    expect(rowRange(ids, "b", "d")).toEqual(["b", "c", "d"]);
    expect(rowRange(ids, "d", "b")).toEqual(["b", "c", "d"]);
    expect(rowRange(ids, undefined, "c")).toEqual(["c"]);
    expect(rowRange(ids, "gone", "c")).toEqual(["c"]);
    expect(rowRange(ids, "a", "gone")).toEqual(["gone"]);
  });
});

describe("the select-this-page box", () => {
  it("is checked when every row that can be selected is, and half-checked when some are", () => {
    const some = setSelected(emptySelection(), ["d1"], true);
    expect(pageSelection(emptySelection(), rows)).toEqual({ checked: false, indeterminate: false, disabled: false });
    expect(pageSelection(some, rows)).toEqual({ checked: false, indeterminate: true, disabled: false });
    expect(pageSelection(setSelected(some, ["d2", "d3"], true), rows)).toEqual({ checked: true, indeterminate: false, disabled: false });
  });

  it("only counts the rows that can be selected, and is disabled when there are none", () => {
    const selection = setSelected(emptySelection(), ["d1", "d3"], true);
    expect(pageSelection(selection, ["d1", "d3"]).checked).toBe(true);
    expect(pageSelection(selection, [])).toEqual({ checked: false, indeterminate: false, disabled: true });
  });

  it("follows 'all' mode: ticked until a row is left out", () => {
    expect(pageSelection(selectAllMatching("k"), rows).checked).toBe(true);
    expect(pageSelection(setSelected(selectAllMatching("k"), ["d2"], false), rows)).toEqual({
      checked: false,
      indeterminate: true,
      disabled: false,
    });
  });
});

describe("what the bar says", () => {
  const page = { rows, selectable: rows, total: 60 };

  it("counts the selected rows", () => {
    const status = describeSelection(setSelected(emptySelection(), ["d1", "d2"], true), page, words);
    expect(status).toMatchObject({ count: 2, text: "2 selected", offerAll: false });
  });

  it("says how many are on other pages", () => {
    const status = describeSelection(setSelected(emptySelection(), ["d1", "d2", "x9"], true), page, words);
    expect(status.text).toBe("3 selected · 1 not on this page");
  });

  it("offers every result once the whole page is selected and more match", () => {
    const status = describeSelection(setSelected(emptySelection(), rows, true), page, words);
    expect(status).toEqual({ count: 3, text: "All 3 on this page are selected.", offerAll: true, allText: "Select all 60 results" });
  });

  it("makes no offer when everything is on the page, nothing is counted, it is switched off, or other pages hold rows too", () => {
    const whole = setSelected(emptySelection(), rows, true);
    expect(describeSelection(whole, { ...page, total: 3 }, words).offerAll).toBe(false);
    expect(describeSelection(whole, { ...page, total: undefined }, words).offerAll).toBe(false);
    expect(describeSelection(whole, { ...page, canSelectAll: false }, words).offerAll).toBe(false);
    const more = setSelected(whole, ["x9"], true);
    expect(describeSelection(more, page, words)).toMatchObject({ offerAll: false, text: "4 selected · 1 not on this page" });
    const some = setSelected(emptySelection(), ["d1"], true);
    expect(describeSelection(some, page, words).offerAll).toBe(false);
  });

  it("says when all results are selected, and how many but the ones left out", () => {
    expect(describeSelection(selectAllMatching("k"), page, words)).toMatchObject({ count: 60, text: "All 60 results are selected." });
    const except = setSelected(selectAllMatching("k"), ["d2"], false);
    expect(describeSelection(except, page, words)).toMatchObject({ count: 59, text: "59 of 60 results are selected." });
  });

  it("groups counts in lakhs and crores when asked", () => {
    const big = { rows, selectable: rows, total: 100_000 };
    const status = describeSelection(setSelected(emptySelection(), rows, true), big, words, "indian");
    expect(status.allText).toBe("Select all 1,00,000 results");
    expect(describeSelection(selectAllMatching("k"), big, words, "indian").text).toBe("All 1,00,000 results are selected.");
    expect(describeSelection(selectAllMatching("k"), big, words).text).toBe("All 100,000 results are selected.");
  });

  it("says it in the words it is given", () => {
    const french = { ...words, selected: "{n} sélectionné(s)" };
    expect(describeSelection(setSelected(emptySelection(), ["d1"], true), page, french).text).toBe("1 sélectionné(s)");
  });
});

describe("a bulk request", () => {
  const query = { filter: { stage: "won" }, search: "agra", sort: ["-amount"], page: 3, per_page: 10 };

  it("names the keys, and the search and filters but not the page or the sort", () => {
    const keys = setSelected(emptySelection(), ["d1", "d2"], true);
    expect(bulkRequest("archive", keys, { total: 60, query })).toEqual({
      action: "archive",
      mode: "ids",
      ids: ["d1", "d2"],
      excluded: [],
      expected: 2,
      query: { filter: { stage: "won" }, search: "agra" },
    });
  });

  it("names every result except the ones left out, and how many the user saw", () => {
    const except: TableSelection = setSelected(selectAllMatching("k"), ["d3"], false);
    expect(bulkRequest("archive", except, { total: 60, version: "17", query })).toEqual({
      action: "archive",
      mode: "all",
      ids: [],
      excluded: ["d3"],
      expected: 59,
      version: "17",
      query: { filter: { stage: "won" }, search: "agra" },
    });
  });

  it("refuses a selection of all results in a list that isn't counted, and one too large to post whole", () => {
    expect(() => bulkRequest("archive", selectAllMatching("k"), { query })).toThrow(RangeError);
    const many = Array.from({ length: MAX_SELECTED_IDS + 1 }, (_, at) => `d${at}`);
    expect(() => bulkRequest("archive", setSelected(emptySelection(), many, true), { total: 20_000, query })).toThrow(RangeError);
    const limit = many.slice(0, MAX_SELECTED_IDS);
    expect(bulkRequest("archive", setSelected(emptySelection(), limit, true), { total: 20_000, query }).ids).toHaveLength(MAX_SELECTED_IDS);
  });

  it("posts with the names a Navree UI bulk route reads", () => {
    const keys = bulkParams(bulkRequest("archive", setSelected(emptySelection(), ["d1", "d2"], true), { total: 60, query }), {
      table: "deals",
      path: "/deals",
    });
    expect(keys.get("action")).toBe("archive");
    expect(keys.get("_table")).toBe("deals");
    expect(keys.get("_path")).toBe("/deals");
    expect(keys.get("_q")).toBe("filter%5Bstage%5D=won&search=agra");
    expect(keys.getAll("ids")).toEqual(["d1", "d2"]);
    expect(keys.has("_mode")).toBe(false);

    const except = setSelected(selectAllMatching("k"), ["d3", "d4"], false);
    const all = bulkParams(bulkRequest("delete", except, { total: 60, version: "17", query }));
    expect(all.get("_mode")).toBe("all");
    expect(all.get("_expected")).toBe("58");
    expect(all.get("_version")).toBe("17");
    expect(all.getAll("ex")).toEqual(["d3", "d4"]);
    expect(all.has("ids")).toBe(false);
    expect(all.has("_table")).toBe(false);
  });
});
