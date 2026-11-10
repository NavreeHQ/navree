import { describe, expect, it } from "vitest";
import { csvField, CSV_BOM, exportHref, exportQuery, toCsv } from "./table-export.js";

describe("the export link", () => {
  it("keeps the search, filters and sort and leaves out the page", () => {
    const query = { filter: { stage: "won" }, search: "agra", sort: ["-amount"], page: 3, per_page: 25 };
    expect(exportQuery(query)).toEqual({ filter: { stage: "won" }, search: "agra", sort: ["-amount"] });
    expect(exportQuery({ cursor: "abc", before: "def", limit: 20, search: "x" })).toEqual({ search: "x" });
  });

  it("puts them in the address of the export", () => {
    const query = { filter: { stage: { in: ["won", "lost"] } }, search: "ludhiana", page: 2 };
    expect(exportHref("/api/v1/deals/export", query)).toBe("/api/v1/deals/export?filter%5Bstage%5D%5Bin%5D=won%2Clost&search=ludhiana");
  });

  it("is the address as given when there is nothing to carry", () => {
    expect(exportHref("/api/v1/deals/export", { page: 4, per_page: 50 })).toBe("/api/v1/deals/export");
  });

  it("joins an address that has a query already, and keeps its fragment", () => {
    expect(exportHref("/export?format=csv", { search: "x" })).toBe("/export?format=csv&search=x");
    expect(exportHref("/export#top", { search: "x" })).toBe("/export?search=x#top");
    expect(exportHref("https://api.example.in/deals/export?a=1#b", { search: "x" })).toBe("https://api.example.in/deals/export?a=1&search=x#b");
  });
});

describe("a CSV field", () => {
  it("leaves plain values and plain negative numbers alone", () => {
    for (const plain of ["Agra", "", "-12.5", "-3", "-.5", "-1e5", "-inf", "ਲੁਧਿਆਣਾ"]) expect(csvField(plain), plain).toBe(plain);
  });

  it("quotes a field that has a comma, a quote or a line break, and doubles its quotes", () => {
    expect(csvField("a,b")).toBe('"a,b"');
    expect(csvField('say "hi"')).toBe('"say ""hi"""');
    expect(csvField("two\nlines")).toBe('"two\nlines"');
    expect(csvField("two\r\nlines")).toBe('"two\r\nlines"');
  });

  it("never lets a spreadsheet run a cell as a formula", () => {
    for (const bad of ["=1+1", "+1+1", "@SUM(A1)", "-2+3", '=HYPERLINK("http://x")', "\tx", "\rx", "-", "--1"]) {
      expect(csvField(bad), bad).toMatch(/^"'/);
    }
    expect(csvField('=HYPERLINK("http://x")')).toBe('"\'=HYPERLINK(""http://x"")"');
  });
});

describe("a CSV file", () => {
  it("starts with a byte-order mark and breaks lines with CRLF, ending on one", () => {
    const file = toCsv(["Title", "Amount"], [["Agra renewal", "1,00,000"], ["Gulati pilot", "=1+1"]]);
    expect(file.startsWith(CSV_BOM)).toBe(true);
    expect(file).toBe(`${CSV_BOM}Title,Amount\r\nAgra renewal,"1,00,000"\r\nGulati pilot,"'=1+1"\r\n`);
  });

  it("can leave the mark out", () => {
    expect(toCsv(["a"], [["b"]], { bom: false })).toBe("a\r\nb\r\n");
  });

  it("is only the head for no rows", () => {
    expect(toCsv(["a", "b"], [], { bom: false })).toBe("a,b\r\n");
  });
});
