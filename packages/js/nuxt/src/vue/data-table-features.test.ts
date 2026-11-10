import { defaultTableLabels, type BulkRequest, type ListQuery, type PageMeta } from "@navree/client";
import { flushPromises, mount, type VueWrapper } from "@vue/test-utils";
import { h, nextTick } from "vue";
import { afterEach, describe, expect, it, vi } from "vitest";
import { defaultLabels, NavreeDataTable, type DataTableBulk, type NavreeColumnDef, type NavreeColumnMeta } from "./data-table.js";

type Deal = { id: string; title: string; stage: string; amount: number | null };

const stages = [
  { value: "lead", label: "Lead" },
  { value: "won", label: "Won" },
  { value: "lost", label: "Lost" },
];

const columns: NavreeColumnDef<Deal>[] = [
  { accessorKey: "title", header: "Title", meta: { sortField: "title", filter: "text" } satisfies NavreeColumnMeta },
  { accessorKey: "stage", header: "Stage", meta: { filter: "multi", options: stages } satisfies NavreeColumnMeta },
  {
    accessorKey: "amount",
    header: "Amount",
    meta: { sortField: "amount", align: "end", filter: "range", kind: "int", nullable: true } satisfies NavreeColumnMeta,
  },
];

const deals: Deal[] = [
  { id: "d1", title: "Agra renewal", stage: "won", amount: 120000 },
  { id: "d2", title: "Gulati pilot", stage: "lead", amount: 30000 },
  { id: "d3", title: "Ludhiana rollout", stage: "lost", amount: null },
  { id: "d4", title: "Sutlej supply", stage: "won", amount: 900000 },
];
const meta: PageMeta = { current_page: 1, per_page: 4, total: 40, last_page: 10 };

const mounted: VueWrapper[] = [];

function table(props: Record<string, unknown> = {}) {
  const wrapper = mount(NavreeDataTable, {
    props: { columns, data: deals, meta, query: {} as ListQuery, getRowId: (row: Deal) => row.id, ...props },
    attachTo: document.body,
  });
  mounted.push(wrapper);
  return wrapper;
}

afterEach(() => {
  vi.useRealTimers();
  for (const wrapper of mounted.splice(0)) wrapper.unmount();
});

const last = (wrapper: VueWrapper, event = "update:query") => wrapper.emitted(event)!.at(-1);
const control = (wrapper: VueWrapper, label: string) => wrapper.find(`[aria-label="${label}"]`);
const box = (wrapper: VueWrapper, title: string) => control(wrapper, `Select ${title}`);
const checked = (wrapper: VueWrapper, title: string) => (box(wrapper, title).element as HTMLInputElement).checked;
const pageBox = (wrapper: VueWrapper) => control(wrapper, "Select all on this page");
const status = (wrapper: VueWrapper) => wrapper.find("[data-slot=bulk-count]").text();
const choice = (wrapper: VueWrapper, field: string, label: string) =>
  wrapper
    .findAll(`[data-field=${field}] label`)
    .find((item) => item.text().startsWith(label))!
    .find("input");
const named = (wrapper: VueWrapper, name: string) =>
  wrapper.findAll("button").find((item) => (item.attributes("aria-label") ?? item.text()) === name)!;

describe("the search box", () => {
  it("tells the list query once typing pauses, back on the first page", async () => {
    vi.useFakeTimers();
    const wrapper = table({ search: true, query: { page: 3, sort: ["title"] } });
    const search = control(wrapper, "Search");
    await search.setValue("ag");
    await vi.advanceTimersByTimeAsync(60);
    await search.setValue("  agra ");
    await vi.advanceTimersByTimeAsync(60);
    expect(wrapper.emitted("update:query")).toBeUndefined();
    await vi.advanceTimersByTimeAsync(60);
    expect(wrapper.emitted("update:query")).toHaveLength(1);
    expect(last(wrapper)).toEqual([{ sort: ["title"], search: "agra" }]);
  });

  it("tells it at once on Enter, and takes the search off when the box is emptied", async () => {
    vi.useFakeTimers();
    const wrapper = table({ search: true, query: { search: "agra", filter: { stage: "won" } } });
    const search = control(wrapper, "Search");
    expect((search.element as HTMLInputElement).value).toBe("agra");
    await search.setValue("");
    await search.trigger("keydown", { key: "Enter" });
    expect(wrapper.emitted("update:query")).toHaveLength(1);
    expect(last(wrapper)).toEqual([{ filter: { stage: "won" } }]);
    await vi.advanceTimersByTimeAsync(500);
    expect(wrapper.emitted("update:query")).toHaveLength(1);
  });

  it("follows the list query when it changes without the box", async () => {
    const wrapper = table({ search: true, query: { search: "agra" } });
    expect((control(wrapper, "Search").element as HTMLInputElement).value).toBe("agra");
    await wrapper.setProps({ query: { search: "pilot" } });
    await nextTick();
    expect((control(wrapper, "Search").element as HTMLInputElement).value).toBe("pilot");
    await wrapper.setProps({ query: {} });
    await nextTick();
    expect((control(wrapper, "Search").element as HTMLInputElement).value).toBe("");
  });

  it("has no box, and no Filters button without a filter on a column, unless asked", () => {
    const wrapper = table({ columns: [{ accessorKey: "title", header: "Title" }] });
    expect(wrapper.find("input[type=search]").exists()).toBe(false);
    expect(wrapper.findAll("button").some((item) => item.text().startsWith("Filters"))).toBe(false);
  });
});

describe("filters in a bar", () => {
  it("fold away until the Filters button opens them", async () => {
    const wrapper = table();
    const button = named(wrapper, "Filters");
    const panel = wrapper.find("[data-slot=filter-bar]").element as HTMLElement;
    expect(button.attributes("aria-expanded")).toBe("false");
    expect(panel.hidden).toBe(true);
    await button.trigger("click");
    expect(button.attributes("aria-expanded")).toBe("true");
    expect(panel.hidden).toBe(false);
    expect(wrapper.find('[role=group][aria-label="Stage"]').exists()).toBe(true);
    await button.trigger("click");
    expect(panel.hidden).toBe(true);
  });

  it("write a text filter after a pause, back on the first page", async () => {
    vi.useFakeTimers();
    const wrapper = table({ query: { page: 4 } });
    await control(wrapper, "Search: Title").setValue("sutlej");
    await vi.advanceTimersByTimeAsync(100);
    expect(last(wrapper)).toEqual([{ filter: { title: { contains: "sutlej" } } }]);
  });

  it("write a choice from a list as Navree UI does, and take it off again", async () => {
    const wrapper = table({ query: { page: 4, sort: ["-amount"] } });
    await choice(wrapper, "stage", "Won").setValue(true);
    expect(last(wrapper)).toEqual([{ sort: ["-amount"], filter: { stage: { in: ["won"] } } }]);
    await wrapper.setProps({ query: { sort: ["-amount"], filter: { stage: { in: ["won"] } } } });
    expect((choice(wrapper, "stage", "Won").element as HTMLInputElement).checked).toBe(true);
    await choice(wrapper, "stage", "Lead").setValue(true);
    expect(last(wrapper)).toEqual([{ sort: ["-amount"], filter: { stage: { in: ["won", "lead"] } } }]);
    await choice(wrapper, "stage", "Won").setValue(false);
    expect(last(wrapper)).toEqual([{ sort: ["-amount"] }]);
  });

  it("write the bounds of a range, typed by the column's kind", async () => {
    vi.useFakeTimers();
    const wrapper = table();
    const min = control(wrapper, "Amount Min");
    expect(min.attributes("type")).toBe("number");
    await min.setValue("100000");
    await vi.advanceTimersByTimeAsync(100);
    expect(last(wrapper)).toEqual([{ filter: { amount: { gte: "100000" } } }]);
  });

  it("filter for rows with no value when the column can be empty", async () => {
    const wrapper = table();
    const empty = wrapper.findAll("label").filter((item) => item.text() === "Empty");
    expect(empty).toHaveLength(1);
    await empty[0]!.find("input").setValue(true);
    expect(last(wrapper)).toEqual([{ filter: { amount: { null: true } } }]);
  });

  it("show a count beside a choice, grouped as lakhs when asked", () => {
    const withCounts: NavreeColumnDef<Deal>[] = [columns[0]!, { accessorKey: "stage", header: "Stage", meta: { filter: "multi" } satisfies NavreeColumnMeta }];
    const wrapper = table({
      columns: withCounts,
      filterOptions: { stage: [{ value: "won", label: "Won", count: 123456 }] },
      labels: { grouping: "indian" },
    });
    expect(wrapper.find("[data-field=stage] label").text()).toBe("Won 1,23,456");
  });

  it("give a select and a yes-or-no filter a list with Any first", async () => {
    const more: NavreeColumnDef<Deal>[] = [
      { accessorKey: "stage", header: "Stage", meta: { filter: "select", options: stages } satisfies NavreeColumnMeta },
      { accessorKey: "title", header: "Open", meta: { filter: "boolean", filterField: "open", kind: "bool" } satisfies NavreeColumnMeta },
    ];
    const wrapper = table({ columns: more });
    const stage = wrapper.find('select[aria-label="Stage"]');
    expect(stage.findAll("option").map((option) => option.text())).toEqual(["Any", "Lead", "Won", "Lost"]);
    await stage.setValue("won");
    expect(last(wrapper)).toEqual([{ filter: { stage: "won" } }]);
    await wrapper.find('select[aria-label="Open"]').setValue("false");
    expect(last(wrapper)).toEqual([{ filter: { open: false } }]);
  });

  it("count the filters in force on the button", () => {
    const wrapper = table({ query: { filter: { stage: { in: ["won"] }, title: { contains: "a" } } } });
    expect(named(wrapper, "Filters (2)").exists()).toBe(true);
  });
});

describe("filters in a row", () => {
  it("sit under the headings, with no Filters button and no Empty box", () => {
    const wrapper = table({ filterMode: "row" });
    expect(wrapper.findAll("button").some((item) => item.text().startsWith("Filters"))).toBe(false);
    const row = wrapper.find("[data-slot=filter-row]");
    expect(row.text()).not.toContain("Empty");
    expect(row.findAll("td")).toHaveLength(3);
    expect(row.find('[aria-label="Search: Title"]').exists()).toBe(true);
    expect(row.find("details[data-field=stage] summary").text()).toBe("Stage");
    expect(row.find("details[data-field=amount] summary").text()).toBe("Amount");
    expect(wrapper.emitted("update:query")).toBeUndefined();
  });

  it("keep the selection column's place", () => {
    const wrapper = table({ filterMode: "row", bulk: { actions: [], onAction: vi.fn() } });
    expect(wrapper.find("[data-slot=filter-row]").findAll("td")).toHaveLength(4);
  });
});

describe("chips", () => {
  const query: ListQuery = { search: "agra", sort: ["-amount"], page: 2, filter: { stage: { in: ["won", "lost"] }, amount: { gte: "1000" } } };

  it("name each search and filter in force, and take one off", async () => {
    const wrapper = table({ query });
    await control(wrapper, "Remove filter Stage: Won, Lost").trigger("click");
    expect(last(wrapper)).toEqual([{ search: "agra", sort: ["-amount"], filter: { amount: { gte: "1000" } } }]);
    await control(wrapper, "Remove filter Amount: ≥ 1000").trigger("click");
    expect(last(wrapper)).toEqual([{ search: "agra", sort: ["-amount"], filter: { stage: { in: ["won", "lost"] } } }]);
    await control(wrapper, "Remove filter Search: agra").trigger("click");
    expect(last(wrapper)).toEqual([{ sort: ["-amount"], filter: { stage: { in: ["won", "lost"] }, amount: { gte: "1000" } } }]);
  });

  it("clear everything but the sort", async () => {
    const wrapper = table({ query });
    await wrapper.find("[data-slot=chips-clear]").trigger("click");
    expect(last(wrapper)).toEqual([{ sort: ["-amount"] }]);
  });

  it("are not there when nothing is searched or filtered", () => {
    expect(table({ query: { sort: ["title"], page: 2 } }).find("[data-slot=table-chips]").exists()).toBe(false);
  });

  it("say that nothing matches, and how to fix it, when a filtered page is empty", () => {
    const wrapper = table({ data: [], meta: { current_page: 1, per_page: 4, total: 0, last_page: 1 }, query: { search: "zzz" } });
    expect(wrapper.text()).toContain("No results match these filters.");
    expect(wrapper.text()).toContain("Change or clear the search and filters.");
    expect(wrapper.text()).not.toContain("No results.");
  });
});

describe("selection", () => {
  const archive = { id: "archive", label: "Archive" };
  const remove = { id: "delete", label: "Delete", destructive: true, confirm: "Delete {n} deals?" };

  /** A table with checkboxes whose actions end in `onAction`; `more` changes what `bulk` says. */
  function bulkTable(extra: Record<string, unknown> = {}, more: Partial<DataTableBulk> = {}) {
    const onAction = vi.fn<(request: BulkRequest) => void | boolean | Promise<void | boolean>>();
    const wrapper = table({ bulk: { actions: [archive, remove], onAction, ...more }, ...extra });
    return { wrapper, onAction };
  }

  it("adds a checkbox to every row named for its first cell, and none without bulk", () => {
    const { wrapper } = bulkTable();
    expect(checked(wrapper, "Agra renewal")).toBe(false);
    expect(wrapper.findAll("input[type=checkbox]").filter((item) => item.attributes("aria-label")?.startsWith("Select"))).toHaveLength(deals.length + 1);
    expect(table().findAll("input[aria-label^=Select]")).toHaveLength(0);
  });

  it("counts the selected rows in a status line a screen reader hears", async () => {
    const { wrapper } = bulkTable();
    expect(wrapper.find("[data-slot=bulk-count]").attributes("aria-live")).toBe("polite");
    expect(status(wrapper)).toBe("");
    await box(wrapper, "Agra renewal").trigger("click");
    expect(status(wrapper)).toBe("1 selected");
    expect(checked(wrapper, "Agra renewal")).toBe(true);
    expect((pageBox(wrapper).element as HTMLInputElement).indeterminate).toBe(true);
    await box(wrapper, "Agra renewal").trigger("click");
    expect(status(wrapper)).toBe("");
  });

  it("selects the page, and offers every result of a list that is longer", async () => {
    const { wrapper } = bulkTable();
    await pageBox(wrapper).trigger("click");
    expect((pageBox(wrapper).element as HTMLInputElement).checked).toBe(true);
    expect(status(wrapper)).toBe("All 4 on this page are selected.");
    await named(wrapper, "Select all 40 results").trigger("click");
    expect(status(wrapper)).toBe("All 40 results are selected.");
    expect(document.activeElement).toBe(named(wrapper, "Clear selection").element);
    await box(wrapper, "Gulati pilot").trigger("click");
    expect(status(wrapper)).toBe("39 of 40 results are selected.");
    expect((pageBox(wrapper).element as HTMLInputElement).indeterminate).toBe(true);
    await named(wrapper, "Clear selection").trigger("click");
    expect(status(wrapper)).toBe("");
    expect((pageBox(wrapper).element as HTMLInputElement).checked).toBe(false);
  });

  it("makes no offer for a list that is not counted or when asked not to", async () => {
    const uncounted = bulkTable({ meta: undefined }).wrapper;
    await pageBox(uncounted).trigger("click");
    expect(uncounted.findAll("button").some((item) => /^Select all \d/.test(item.text()))).toBe(false);
    const off = bulkTable({}, { selectAllMatching: false }).wrapper;
    await pageBox(off).trigger("click");
    expect(off.findAll("button").some((item) => /^Select all \d/.test(item.text()))).toBe(false);
  });

  it("groups the counts in lakhs when the words say so", async () => {
    const { wrapper } = bulkTable({ total: 100000, labels: { grouping: "indian" } });
    await pageBox(wrapper).trigger("click");
    expect(named(wrapper, "Select all 1,00,000 results").exists()).toBe(true);
  });

  it("selects from one row to another with shift", async () => {
    const { wrapper } = bulkTable();
    await box(wrapper, "Gulati pilot").trigger("click");
    await box(wrapper, "Sutlej supply").trigger("click", { shiftKey: true });
    expect(deals.map((deal) => checked(wrapper, deal.title))).toEqual([false, true, true, true]);
    // The range runs from the row clicked last, and takes the new state of the row clicked now.
    await box(wrapper, "Ludhiana rollout").trigger("click", { shiftKey: true });
    expect(deals.map((deal) => checked(wrapper, deal.title))).toEqual([false, true, false, false]);
  });

  it("leaves out a row that cannot be selected", async () => {
    const { wrapper } = bulkTable({ selectableIf: (deal: Deal) => deal.stage !== "lost" });
    expect((box(wrapper, "Ludhiana rollout").element as HTMLInputElement).disabled).toBe(true);
    await pageBox(wrapper).trigger("click");
    expect(deals.map((deal) => checked(wrapper, deal.title))).toEqual([true, true, false, true]);
    expect((pageBox(wrapper).element as HTMLInputElement).checked).toBe(true);
  });

  it("does not open the row when its checkbox is clicked", async () => {
    const { wrapper } = bulkTable({ clickable: true });
    await box(wrapper, "Agra renewal").trigger("click");
    expect(wrapper.emitted("row-click")).toBeUndefined();
    await wrapper.findAll("tbody tr")[1]!.find("td:nth-child(2)").trigger("click");
    expect(wrapper.emitted("row-click")![0]).toEqual([deals[1]]);
  });

  it("keeps a place for the checkbox in the rows that span the table", () => {
    const { wrapper } = bulkTable({ loading: true });
    expect(wrapper.find("[data-slot=loading-row] td").attributes("colspan")).toBe("4");
  });

  it("posts the keys of the rows, then clears them", async () => {
    const { wrapper, onAction } = bulkTable({ query: { search: "a", page: 2 } });
    await box(wrapper, "Agra renewal").trigger("click");
    await box(wrapper, "Sutlej supply").trigger("click");
    await named(wrapper, "Archive").trigger("click");
    expect(onAction).toHaveBeenCalledWith({
      action: "archive",
      mode: "ids",
      ids: ["d1", "d4"],
      excluded: [],
      expected: 2,
      query: { search: "a" },
    });
    await flushPromises();
    expect(status(wrapper)).toBe("");
  });

  it("posts all results but the ones left out, with how many the user saw", async () => {
    const { wrapper, onAction } = bulkTable({ query: { filter: { stage: "won" }, sort: ["title"], page: 2 } }, { actions: [archive], version: "17" });
    await pageBox(wrapper).trigger("click");
    await named(wrapper, "Select all 40 results").trigger("click");
    await box(wrapper, "Gulati pilot").trigger("click");
    expect(status(wrapper)).toBe("39 of 40 results are selected.");
    await named(wrapper, "Archive").trigger("click");
    expect(onAction).toHaveBeenCalledWith({
      action: "archive",
      mode: "all",
      ids: [],
      excluded: ["d2"],
      expected: 39,
      version: "17",
      query: { filter: { stage: "won" } },
    });
    await flushPromises();
    expect(status(wrapper)).toBe("");
  });

  it("keeps the selection when the action answers false", async () => {
    const { wrapper, onAction } = bulkTable();
    onAction.mockResolvedValue(false);
    await box(wrapper, "Agra renewal").trigger("click");
    await named(wrapper, "Archive").trigger("click");
    await flushPromises();
    expect(onAction).toHaveBeenCalledTimes(1);
    expect(status(wrapper)).toBe("1 selected");
    expect((named(wrapper, "Archive").element as HTMLButtonElement).disabled).toBe(false);
  });

  it("asks first when an action says to, with the count in the question", async () => {
    const { wrapper, onAction } = bulkTable();
    await box(wrapper, "Agra renewal").trigger("click");
    await box(wrapper, "Gulati pilot").trigger("click");
    await named(wrapper, "Delete").trigger("click");
    const dialog = wrapper.find("dialog");
    expect(dialog.text()).toContain("Are you sure?");
    expect(dialog.text()).toContain("Delete 2 deals?");
    expect(onAction).not.toHaveBeenCalled();
    await dialog.findAll("button").find((item) => item.text() === "Cancel")!.trigger("click");
    expect(wrapper.find("dialog").exists()).toBe(false);
    expect(onAction).not.toHaveBeenCalled();
    expect(status(wrapper)).toBe("2 selected");

    await named(wrapper, "Delete").trigger("click");
    await wrapper.find("dialog").findAll("button").find((item) => item.text() === "Yes, go ahead")!.trigger("click");
    expect(wrapper.find("dialog").exists()).toBe(false);
    expect(onAction).toHaveBeenCalledTimes(1);
    expect(onAction).toHaveBeenLastCalledWith(expect.objectContaining({ action: "delete", ids: ["d1", "d2"], expected: 2 }));
    await flushPromises();
    expect(status(wrapper)).toBe("");
  });

  it("shows an action that is not allowed disabled, with why", async () => {
    const { wrapper } = bulkTable({}, { actions: [{ id: "delete", label: "Delete", denied: "Only managers can delete." }] });
    await box(wrapper, "Agra renewal").trigger("click");
    const button = named(wrapper, "Delete");
    expect((button.element as HTMLButtonElement).disabled).toBe(true);
    expect(button.attributes("title")).toBe("Only managers can delete.");
  });

  it("ends a selection of all results when the search or filters change, and says so", async () => {
    const { wrapper } = bulkTable({ query: { filter: { stage: "won" } } });
    await pageBox(wrapper).trigger("click");
    await named(wrapper, "Select all 40 results").trigger("click");
    expect(status(wrapper)).toBe("All 40 results are selected.");
    await wrapper.setProps({ query: { filter: { stage: "lost" } } });
    await nextTick();
    expect(status(wrapper)).toBe("The filter changed, so the selection of all results ended.");
    expect((pageBox(wrapper).element as HTMLInputElement).checked).toBe(false);
    await box(wrapper, "Agra renewal").trigger("click");
    expect(status(wrapper)).toBe("1 selected");
  });

  it("keeps a selection of rows when the filters change", async () => {
    const { wrapper } = bulkTable({ query: { filter: { stage: "won" } } });
    await box(wrapper, "Agra renewal").trigger("click");
    await wrapper.setProps({ query: { filter: { stage: "lost" } } });
    expect(status(wrapper)).toBe("1 selected");
  });

  it("can be held by the page", async () => {
    const { wrapper } = bulkTable({ selection: { mode: "ids", ids: ["d2"], ex: [], key: "" } });
    expect(checked(wrapper, "Gulati pilot")).toBe(true);
    expect(status(wrapper)).toBe("1 selected");
    await box(wrapper, "Agra renewal").trigger("click");
    expect(last(wrapper, "update:selection")).toEqual([{ mode: "ids", ids: ["d2", "d1"], ex: [], key: "" }]);
    // Held by the page: nothing changes until it says so.
    expect(checked(wrapper, "Agra renewal")).toBe(false);
    await wrapper.setProps({ selection: { mode: "ids", ids: ["d2", "d1"], ex: [], key: "" } });
    expect(checked(wrapper, "Agra renewal")).toBe(true);
  });

  it("names a checkbox from the row label you give", () => {
    const { wrapper } = bulkTable({ getRowLabel: (deal: Deal) => `${deal.title}, ${deal.stage}` });
    expect(control(wrapper, "Select Agra renewal, won").exists()).toBe(true);
  });
});

describe("export", () => {
  it("is a link to the export carrying the search, filters and sort, and no page", () => {
    const wrapper = table({ exportUrl: "/api/v1/deals/export", query: { filter: { stage: "won" }, search: "agra", sort: ["-amount"], page: 2, per_page: 4 } });
    const link = wrapper.find("a[data-slot=export]");
    expect(link.text()).toBe("Export");
    expect(link.attributes("href")).toBe("/api/v1/deals/export?filter%5Bstage%5D=won&sort=-amount&search=agra");
    expect(link.attributes("download")).toBeDefined();
  });

  it("says it in the words it is given", () => {
    const wrapper = table({ exportUrl: "/export", labels: { export: "Exporter" } });
    expect(wrapper.find("a[data-slot=export]").text()).toBe("Exporter");
    expect(wrapper.find("a[data-slot=export]").attributes("href")).toBe("/export");
  });

  it("is not there without an address", () => {
    expect(table().find("a[data-slot=export]").exists()).toBe(false);
  });
});

describe("the toolbar", () => {
  it("holds your own controls beside the search", () => {
    const wrapper = mount(NavreeDataTable, {
      props: { columns, data: deals, meta, query: {} as ListQuery, search: true },
      slots: { toolbar: () => h("button", { type: "button" }, "New deal") },
    });
    expect(wrapper.find("[data-slot=table-toolbar]").text()).toContain("New deal");
    expect(wrapper.find("input[type=search]").exists()).toBe(true);
  });

  it("names the table", () => {
    expect(table({ caption: "Deals" }).find("caption").text()).toBe("Deals");
  });
});

describe("the words", () => {
  it("are Navree UI's, with the pagination's on top", () => {
    expect(defaultLabels).toMatchObject(defaultTableLabels);
    expect(defaultLabels.previous).toBe("Previous");
    expect(defaultLabels.range(1, 25, 60)).toBe("1–25 of 60");
  });

  it("can be changed one at a time", () => {
    const wrapper = table({ search: true, labels: { search: "Khojo" } });
    expect(control(wrapper, "Khojo").exists()).toBe(true);
  });
});
