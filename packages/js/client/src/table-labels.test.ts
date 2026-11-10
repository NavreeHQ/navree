import { describe, expect, it } from "vitest";
import { defaultTableLabels, fill, groupedCount } from "./table-labels.js";

describe("filling a text", () => {
  it("puts each value where its name is", () => {
    expect(fill("{n} of {total} results are selected.", { n: 59, total: "60" })).toBe("59 of 60 results are selected.");
    expect(fill("Remove filter {label}", { label: "Stage: Won" })).toBe("Remove filter Stage: Won");
  });

  it("leaves a name with no value as written, and names every object has", () => {
    expect(fill("{n} and {other}", { n: 1 })).toBe("1 and {other}");
    expect(fill("{constructor}", {})).toBe("{constructor}");
    expect(fill("no places", { n: 1 })).toBe("no places");
  });

  it("does not read a value as a place", () => {
    expect(fill("{label} {n}", { label: "{n}", n: 3 })).toBe("{n} 3");
  });
});

describe("counts", () => {
  it("are grouped by thousands, or in lakhs and crores", () => {
    expect(groupedCount(999)).toBe("999");
    expect(groupedCount(1_234_567)).toBe("1,234,567");
    expect(groupedCount(1_234_567, "indian")).toBe("12,34,567");
    expect(groupedCount(100_000, "indian")).toBe("1,00,000");
    expect(groupedCount(10_000_000, "indian")).toBe("1,00,00,000");
  });
});

describe("the default words", () => {
  it("are Navree UI's", () => {
    expect(defaultTableLabels.selected).toBe("{n} selected");
    expect(defaultTableLabels.selectAll).toBe("Select all {n} results");
    expect(defaultTableLabels.removeFilter).toBe("Remove filter {label}");
    expect(defaultTableLabels.grouping).toBe("thousands");
  });
});
