import { defaultTableLabels, type BulkRequest, type ListQuery, type PageMeta } from "@navree/client";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { DataTable, defaultLabels, type DataTableBulk, type DataTableProps, type NavreeColumnDef, type NavreeColumnMeta } from "./data-table.js";

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

type Props = Partial<DataTableProps<Deal>>;

function renderTable(extra: Props = {}) {
  const onQueryChange = vi.fn();
  const props = (more: Props): DataTableProps<Deal> => ({
    columns,
    data: deals,
    meta,
    query: {} as ListQuery,
    onQueryChange,
    getRowId: (row) => row.id,
    ...more,
  });
  const view = render(<DataTable<Deal> {...props(extra)} />);
  return { ...view, onQueryChange, again: (more: Props) => view.rerender(<DataTable<Deal> {...props({ ...extra, ...more })} />) };
}

afterEach(() => {
  vi.useRealTimers();
});

describe("the search box", () => {
  it("tells the list query once typing pauses, back on the first page", () => {
    vi.useFakeTimers();
    const { onQueryChange } = renderTable({ search: true, query: { page: 3, sort: ["title"] } });
    const box = screen.getByRole("searchbox", { name: "Search" });
    fireEvent.change(box, { target: { value: "ag" } });
    act(() => {
      vi.advanceTimersByTime(60);
    });
    fireEvent.change(box, { target: { value: "  agra " } });
    act(() => {
      vi.advanceTimersByTime(60);
    });
    expect(onQueryChange).not.toHaveBeenCalled();
    act(() => {
      vi.advanceTimersByTime(60);
    });
    expect(onQueryChange).toHaveBeenCalledTimes(1);
    expect(onQueryChange).toHaveBeenLastCalledWith({ sort: ["title"], search: "agra" });
  });

  it("tells it at once on Enter, and takes the search off when the box is emptied", () => {
    vi.useFakeTimers();
    const { onQueryChange } = renderTable({ search: true, query: { search: "agra", filter: { stage: "won" } } });
    const box = screen.getByRole("searchbox", { name: "Search" }) as HTMLInputElement;
    expect(box.value).toBe("agra");
    fireEvent.change(box, { target: { value: "" } });
    fireEvent.keyDown(box, { key: "Enter" });
    expect(onQueryChange).toHaveBeenCalledTimes(1);
    expect(onQueryChange).toHaveBeenLastCalledWith({ filter: { stage: "won" } });
    // The timer that was waiting has nothing left to say.
    act(() => {
      vi.advanceTimersByTime(500);
    });
    expect(onQueryChange).toHaveBeenCalledTimes(1);
  });

  it("follows the list query when it changes without the box", () => {
    const { again } = renderTable({ search: true, query: { search: "agra" } });
    expect((screen.getByRole("searchbox", { name: "Search" }) as HTMLInputElement).value).toBe("agra");
    again({ query: { search: "pilot" } });
    expect((screen.getByRole("searchbox", { name: "Search" }) as HTMLInputElement).value).toBe("pilot");
    again({ query: {} });
    expect((screen.getByRole("searchbox", { name: "Search" }) as HTMLInputElement).value).toBe("");
  });

  it("has no box, and no Filters button without a filter on a column, unless asked", () => {
    renderTable({ columns: [{ accessorKey: "title", header: "Title" }] });
    expect(screen.queryByRole("searchbox")).toBeNull();
    expect(screen.queryByRole("button", { name: /Filters/ })).toBeNull();
  });
});

describe("filters in a bar", () => {
  function open(extra: Props = {}) {
    const view = renderTable(extra);
    fireEvent.click(screen.getByRole("button", { name: "Filters" }));
    return view;
  }

  it("fold away until the Filters button opens them", () => {
    renderTable();
    const button = screen.getByRole("button", { name: "Filters" });
    expect(button.getAttribute("aria-expanded")).toBe("false");
    expect(screen.queryByRole("group", { name: "Stage" })).toBeNull();
    fireEvent.click(button);
    expect(button.getAttribute("aria-expanded")).toBe("true");
    expect(screen.getByRole("group", { name: "Stage" })).toBeTruthy();
    expect(screen.getByRole("group", { name: "Amount" })).toBeTruthy();
    fireEvent.click(button);
    expect(screen.queryByRole("group", { name: "Stage" })).toBeNull();
  });

  it("write a text filter after a pause, back on the first page", () => {
    vi.useFakeTimers();
    const { onQueryChange } = open({ query: { page: 4 } });
    fireEvent.change(screen.getByRole("searchbox", { name: "Search: Title" }), { target: { value: "sutlej" } });
    act(() => {
      vi.advanceTimersByTime(100);
    });
    expect(onQueryChange).toHaveBeenLastCalledWith({ filter: { title: { contains: "sutlej" } } });
  });

  it("write a choice from a list as Navree UI does, and take it off again", () => {
    const { onQueryChange, again } = open({ query: { page: 4, sort: ["-amount"] } });
    fireEvent.click(screen.getByRole("checkbox", { name: "Won" }));
    expect(onQueryChange).toHaveBeenLastCalledWith({ sort: ["-amount"], filter: { stage: { in: ["won"] } } });
    again({ query: { sort: ["-amount"], filter: { stage: { in: ["won"] } } } });
    expect((screen.getByRole("checkbox", { name: "Won" }) as HTMLInputElement).checked).toBe(true);
    fireEvent.click(screen.getByRole("checkbox", { name: "Lead" }));
    expect(onQueryChange).toHaveBeenLastCalledWith({ sort: ["-amount"], filter: { stage: { in: ["won", "lead"] } } });
    fireEvent.click(screen.getByRole("checkbox", { name: "Won" }));
    expect(onQueryChange).toHaveBeenLastCalledWith({ sort: ["-amount"] });
  });

  it("write the bounds of a range, typed by the column's kind", () => {
    vi.useFakeTimers();
    const { onQueryChange } = open();
    const min = screen.getByRole("spinbutton", { name: "Amount Min" });
    expect(min.getAttribute("type")).toBe("number");
    fireEvent.change(min, { target: { value: "100000" } });
    act(() => {
      vi.advanceTimersByTime(100);
    });
    expect(onQueryChange).toHaveBeenLastCalledWith({ filter: { amount: { gte: "100000" } } });
  });

  it("filter for rows with no value when the column can be empty", () => {
    const { onQueryChange } = open();
    expect(screen.getAllByRole("checkbox", { name: "Empty" })).toHaveLength(1);
    fireEvent.click(screen.getByRole("checkbox", { name: "Empty" }));
    expect(onQueryChange).toHaveBeenLastCalledWith({ filter: { amount: { null: true } } });
  });

  it("show a count beside a choice, grouped as lakhs when asked", () => {
    const withCounts: NavreeColumnDef<Deal>[] = [
      columns[0]!,
      { accessorKey: "stage", header: "Stage", meta: { filter: "multi" } satisfies NavreeColumnMeta },
    ];
    open({ columns: withCounts, filterOptions: { stage: [{ value: "won", label: "Won", count: 123456 }] }, labels: { grouping: "indian" } });
    expect(screen.getByRole("checkbox", { name: "Won 1,23,456" })).toBeTruthy();
  });

  it("give a select and a yes-or-no filter a list with Any first", () => {
    const more: NavreeColumnDef<Deal>[] = [
      { accessorKey: "stage", header: "Stage", meta: { filter: "select", options: stages } satisfies NavreeColumnMeta },
      { accessorKey: "title", header: "Open", meta: { filter: "boolean", filterField: "open", kind: "bool" } satisfies NavreeColumnMeta },
    ];
    const { onQueryChange } = open({ columns: more });
    const stage = screen.getByRole("combobox", { name: "Stage" });
    expect(within(stage).getAllByRole("option").map((option) => option.textContent)).toEqual(["Any", "Lead", "Won", "Lost"]);
    fireEvent.change(stage, { target: { value: "won" } });
    expect(onQueryChange).toHaveBeenLastCalledWith({ filter: { stage: "won" } });
    fireEvent.change(screen.getByRole("combobox", { name: "Open" }), { target: { value: "false" } });
    expect(onQueryChange).toHaveBeenLastCalledWith({ filter: { open: false } });
  });

  it("count the filters in force on the button", () => {
    renderTable({ query: { filter: { stage: { in: ["won"] }, title: { contains: "a" } } } });
    expect(screen.getByRole("button", { name: "Filters (2)" })).toBeTruthy();
  });
});

describe("filters in a row", () => {
  it("sit under the headings, with no Filters button and no Empty box", () => {
    const { container, onQueryChange } = renderTable({ filterMode: "row" });
    expect(screen.queryByRole("button", { name: /Filters/ })).toBeNull();
    const row = container.querySelector("[data-slot=filter-row]")!;
    expect(row.textContent).not.toContain("Empty");
    expect(row.querySelectorAll("td")).toHaveLength(3);
    expect(within(row as HTMLElement).getByRole("searchbox", { name: "Search: Title" })).toBeTruthy();
    expect(row.querySelector("details[data-field=stage] summary")?.textContent).toBe("Stage");
    expect(row.querySelector("details[data-field=amount] summary")?.textContent).toBe("Amount");
    expect(onQueryChange).not.toHaveBeenCalled();
  });

  it("keep the selection column's place", () => {
    const { container } = renderTable({ filterMode: "row", bulk: { actions: [], onAction: vi.fn() } });
    expect(container.querySelectorAll("[data-slot=filter-row] td")).toHaveLength(4);
  });
});

describe("chips", () => {
  const query: ListQuery = { search: "agra", sort: ["-amount"], page: 2, filter: { stage: { in: ["won", "lost"] }, amount: { gte: "1000" } } };

  it("name each search and filter in force, and take one off", () => {
    const { onQueryChange } = renderTable({ query });
    fireEvent.click(screen.getByRole("button", { name: "Remove filter Stage: Won, Lost" }));
    expect(onQueryChange).toHaveBeenLastCalledWith({ search: "agra", sort: ["-amount"], filter: { amount: { gte: "1000" } } });
    fireEvent.click(screen.getByRole("button", { name: "Remove filter Amount: ≥ 1000" }));
    expect(onQueryChange).toHaveBeenLastCalledWith({ search: "agra", sort: ["-amount"], filter: { stage: { in: ["won", "lost"] } } });
    fireEvent.click(screen.getByRole("button", { name: "Remove filter Search: agra" }));
    expect(onQueryChange).toHaveBeenLastCalledWith({ sort: ["-amount"], filter: { stage: { in: ["won", "lost"] }, amount: { gte: "1000" } } });
  });

  it("clear everything but the sort", () => {
    const { onQueryChange } = renderTable({ query });
    fireEvent.click(screen.getByRole("button", { name: "Clear filters" }));
    expect(onQueryChange).toHaveBeenLastCalledWith({ sort: ["-amount"] });
  });

  it("are not there when nothing is searched or filtered", () => {
    const { container } = renderTable({ query: { sort: ["title"], page: 2 } });
    expect(container.querySelector("[data-slot=table-chips]")).toBeNull();
  });

  it("say that nothing matches, and how to fix it, when a filtered page is empty", () => {
    renderTable({ data: [], meta: { current_page: 1, per_page: 4, total: 0, last_page: 1 }, query: { search: "zzz" } });
    expect(screen.getByText("No results match these filters.")).toBeTruthy();
    expect(screen.getByText("Change or clear the search and filters.")).toBeTruthy();
    expect(screen.queryByText("No results.")).toBeNull();
  });
});

describe("selection", () => {
  const archive = { id: "archive", label: "Archive" };
  const remove = { id: "delete", label: "Delete", destructive: true, confirm: "Delete {n} deals?" };
  const status = () => screen.getByRole("status").textContent;
  const box = (title: string) => screen.getByRole("checkbox", { name: `Select ${title}` }) as HTMLInputElement;
  const page = () => screen.getByRole("checkbox", { name: "Select all on this page" }) as HTMLInputElement;

  /** A table with checkboxes whose actions end in `onAction`; `more` changes what `bulk` says. */
  function bulkTable(extra: Props = {}, more: Partial<DataTableBulk> = {}) {
    const onAction = vi.fn<(request: BulkRequest) => void | boolean | Promise<void | boolean>>();
    return { onAction, ...renderTable({ bulk: { actions: [archive, remove], onAction, ...more }, ...extra }) };
  }

  it("adds a checkbox to every row named for its first cell, and none without bulk", () => {
    const { unmount } = bulkTable();
    expect(box("Agra renewal").checked).toBe(false);
    expect(screen.getAllByRole("checkbox")).toHaveLength(deals.length + 1);
    unmount();
    renderTable();
    expect(screen.queryAllByRole("checkbox")).toHaveLength(0);
  });

  it("counts the selected rows in a status line a screen reader hears", () => {
    bulkTable();
    expect(screen.getByRole("status").getAttribute("aria-live")).toBe("polite");
    expect(status()).toBe("");
    fireEvent.click(box("Agra renewal"));
    expect(status()).toBe("1 selected");
    expect(box("Agra renewal").checked).toBe(true);
    expect(page().indeterminate).toBe(true);
    fireEvent.click(box("Agra renewal"));
    expect(status()).toBe("");
  });

  it("selects the page, and offers every result of a list that is longer", () => {
    bulkTable();
    fireEvent.click(page());
    expect(page().checked).toBe(true);
    expect(status()).toBe("All 4 on this page are selected.");
    fireEvent.click(screen.getByRole("button", { name: "Select all 40 results" }));
    expect(status()).toBe("All 40 results are selected.");
    expect(document.activeElement).toBe(screen.getByRole("button", { name: "Clear selection" }));
    fireEvent.click(box("Gulati pilot"));
    expect(status()).toBe("39 of 40 results are selected.");
    expect(page().indeterminate).toBe(true);
    fireEvent.click(screen.getByRole("button", { name: "Clear selection" }));
    expect(status()).toBe("");
    expect(page().checked).toBe(false);
  });

  it("makes no offer for a list that is not counted or when asked not to", () => {
    const { unmount } = bulkTable({ meta: undefined });
    fireEvent.click(page());
    expect(screen.queryByRole("button", { name: /Select all \d/ })).toBeNull();
    unmount();
    bulkTable({}, { selectAllMatching: false });
    fireEvent.click(page());
    expect(screen.queryByRole("button", { name: /Select all \d/ })).toBeNull();
  });

  it("groups the counts in lakhs when the words say so", () => {
    bulkTable({ total: 100000, labels: { grouping: "indian" } });
    fireEvent.click(page());
    expect(screen.getByRole("button", { name: "Select all 1,00,000 results" })).toBeTruthy();
  });

  it("selects from one row to another with shift", () => {
    bulkTable();
    fireEvent.click(box("Gulati pilot"));
    fireEvent.click(box("Sutlej supply"), { shiftKey: true });
    expect(deals.map((deal) => box(deal.title).checked)).toEqual([false, true, true, true]);
    // The range runs from the row clicked last, and takes the new state of the row clicked now.
    fireEvent.click(box("Ludhiana rollout"), { shiftKey: true });
    expect(deals.map((deal) => box(deal.title).checked)).toEqual([false, true, false, false]);
  });

  it("leaves out a row that cannot be selected", () => {
    bulkTable({ selectableIf: (deal) => deal.stage !== "lost" });
    expect(box("Ludhiana rollout").disabled).toBe(true);
    fireEvent.click(page());
    expect(deals.map((deal) => box(deal.title).checked)).toEqual([true, true, false, true]);
    expect(page().checked).toBe(true);
  });

  it("does not open the row when its checkbox is clicked", () => {
    const onRowClick = vi.fn();
    bulkTable({ onRowClick });
    fireEvent.click(box("Agra renewal"));
    expect(onRowClick).not.toHaveBeenCalled();
    fireEvent.click(screen.getByText("Gulati pilot"));
    expect(onRowClick).toHaveBeenCalledWith(deals[1]);
  });

  it("keeps a place for the checkbox in the rows that span the table", () => {
    const { container } = bulkTable({ loading: true });
    expect(container.querySelector("[data-slot=loading-row] td")?.getAttribute("colspan")).toBe("4");
  });

  it("posts the keys of the rows, then clears them", async () => {
    const { onAction } = bulkTable({ query: { search: "a", page: 2 } });
    fireEvent.click(box("Agra renewal"));
    fireEvent.click(box("Sutlej supply"));
    fireEvent.click(screen.getByRole("button", { name: "Archive" }));
    expect(onAction).toHaveBeenCalledWith({
      action: "archive",
      mode: "ids",
      ids: ["d1", "d4"],
      excluded: [],
      expected: 2,
      query: { search: "a" },
    });
    await waitFor(() => expect(status()).toBe(""));
  });

  it("posts all results but the ones left out, with how many the user saw", async () => {
    const { onAction } = bulkTable({ query: { filter: { stage: "won" }, sort: ["title"], page: 2 } }, { actions: [archive], version: "17" });
    fireEvent.click(page());
    fireEvent.click(screen.getByRole("button", { name: "Select all 40 results" }));
    fireEvent.click(box("Gulati pilot"));
    expect(status()).toBe("39 of 40 results are selected.");
    fireEvent.click(screen.getByRole("button", { name: "Archive" }));
    expect(onAction).toHaveBeenCalledWith({
      action: "archive",
      mode: "all",
      ids: [],
      excluded: ["d2"],
      expected: 39,
      version: "17",
      query: { filter: { stage: "won" } },
    });
    await waitFor(() => expect(status()).toBe(""));
  });

  it("keeps the selection when the action answers false", async () => {
    const { onAction } = bulkTable();
    onAction.mockResolvedValue(false);
    fireEvent.click(box("Agra renewal"));
    fireEvent.click(screen.getByRole("button", { name: "Archive" }));
    await waitFor(() => expect(onAction).toHaveBeenCalledTimes(1));
    await waitFor(() => expect((screen.getByRole("button", { name: "Archive" }) as HTMLButtonElement).disabled).toBe(false));
    expect(status()).toBe("1 selected");
  });

  it("asks first when an action says to, with the count in the question", async () => {
    const { onAction } = bulkTable();
    fireEvent.click(box("Agra renewal"));
    fireEvent.click(box("Gulati pilot"));
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));
    const dialog = screen.getByRole("alertdialog");
    expect(within(dialog).getByText("Are you sure?")).toBeTruthy();
    expect(within(dialog).getByText("Delete 2 deals?")).toBeTruthy();
    expect(onAction).not.toHaveBeenCalled();
    fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expect(onAction).not.toHaveBeenCalled();
    expect(status()).toBe("2 selected");

    fireEvent.click(screen.getByRole("button", { name: "Delete" }));
    fireEvent.click(within(screen.getByRole("alertdialog")).getByRole("button", { name: "Yes, go ahead" }));
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expect(onAction).toHaveBeenCalledTimes(1);
    expect(onAction).toHaveBeenLastCalledWith(expect.objectContaining({ action: "delete", ids: ["d1", "d2"], expected: 2 }));
    await waitFor(() => expect(status()).toBe(""));
  });

  it("shows an action that is not allowed disabled, with why", () => {
    bulkTable({ bulk: { actions: [{ id: "delete", label: "Delete", denied: "Only managers can delete." }], onAction: vi.fn() } });
    fireEvent.click(box("Agra renewal"));
    const button = screen.getByRole("button", { name: "Delete" }) as HTMLButtonElement;
    expect(button.disabled).toBe(true);
    expect(button.title).toBe("Only managers can delete.");
  });

  it("ends a selection of all results when the search or filters change, and says so", async () => {
    const { again } = bulkTable({ query: { filter: { stage: "won" } } });
    fireEvent.click(page());
    fireEvent.click(screen.getByRole("button", { name: "Select all 40 results" }));
    expect(status()).toBe("All 40 results are selected.");
    again({ query: { filter: { stage: "lost" } } });
    await waitFor(() => expect(status()).toBe("The filter changed, so the selection of all results ended."));
    expect(page().checked).toBe(false);
    fireEvent.click(box("Agra renewal"));
    expect(status()).toBe("1 selected");
  });

  it("keeps a selection of rows when the filters change", () => {
    const { again } = bulkTable({ query: { filter: { stage: "won" } } });
    fireEvent.click(box("Agra renewal"));
    again({ query: { filter: { stage: "lost" } } });
    expect(status()).toBe("1 selected");
  });

  it("can be held by the page", () => {
    const onSelectionChange = vi.fn();
    const { again } = bulkTable({ selection: { mode: "ids", ids: ["d2"], ex: [], key: "" }, onSelectionChange });
    expect(box("Gulati pilot").checked).toBe(true);
    expect(status()).toBe("1 selected");
    fireEvent.click(box("Agra renewal"));
    expect(onSelectionChange).toHaveBeenLastCalledWith({ mode: "ids", ids: ["d2", "d1"], ex: [], key: "" });
    // Held by the page: nothing changes until it says so.
    expect(box("Agra renewal").checked).toBe(false);
    again({ selection: { mode: "ids", ids: ["d2", "d1"], ex: [], key: "" } });
    expect(box("Agra renewal").checked).toBe(true);
  });

  it("names a checkbox from the row label you give", () => {
    bulkTable({ getRowLabel: (deal) => `${deal.title}, ${deal.stage}` });
    expect(screen.getByRole("checkbox", { name: "Select Agra renewal, won" })).toBeTruthy();
  });
});

describe("export", () => {
  it("is a link to the export carrying the search, filters and sort, and no page", () => {
    renderTable({ exportUrl: "/api/v1/deals/export", query: { filter: { stage: "won" }, search: "agra", sort: ["-amount"], page: 2, per_page: 4 } });
    const link = screen.getByRole("link", { name: "Export" });
    expect(link.getAttribute("href")).toBe("/api/v1/deals/export?filter%5Bstage%5D=won&sort=-amount&search=agra");
    expect(link.hasAttribute("download")).toBe(true);
  });

  it("says it in the words it is given", () => {
    renderTable({ exportUrl: "/export", labels: { export: "Exporter" } });
    expect(screen.getByRole("link", { name: "Exporter" }).getAttribute("href")).toBe("/export");
  });

  it("is not there without an address", () => {
    renderTable();
    expect(screen.queryByRole("link", { name: "Export" })).toBeNull();
  });
});

describe("the toolbar", () => {
  it("holds your own controls beside the search", () => {
    renderTable({ search: true, toolbar: <button type="button">New deal</button> });
    expect(screen.getByRole("button", { name: "New deal" })).toBeTruthy();
    expect(screen.getByRole("searchbox", { name: "Search" })).toBeTruthy();
  });

  it("names the table", () => {
    const { container } = renderTable({ caption: "Deals" });
    expect(container.querySelector("caption")?.textContent).toBe("Deals");
  });
});

describe("the words", () => {
  it("are Navree UI's, with the pagination's on top", () => {
    expect(defaultLabels).toMatchObject(defaultTableLabels);
    expect(defaultLabels.previous).toBe("Previous");
    expect(defaultLabels.range(1, 25, 60)).toBe("1–25 of 60");
  });

  it("can be changed one at a time", () => {
    renderTable({ search: true, labels: { search: "Khojo" } });
    expect(screen.getByRole("searchbox", { name: "Khojo" })).toBeTruthy();
  });
});
