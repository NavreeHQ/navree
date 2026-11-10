import type { ListQuery, PageMeta } from "@navree/client";
import { fireEvent, render, renderHook, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { DataTable, useDataTable, type NavreeColumnDef, type NavreeColumnMeta } from "./data-table.js";

type Deal = { id: string; title: string; stage: string; amount: number };

const columns: NavreeColumnDef<Deal>[] = [
  { accessorKey: "title", header: "Title", meta: { sortField: "title" } satisfies NavreeColumnMeta },
  { accessorKey: "stage", header: "Stage", meta: { className: "hidden md:table-cell" } satisfies NavreeColumnMeta },
  {
    accessorKey: "amount",
    header: "Amount",
    cell: (ctx) => `$${ctx.getValue<number>()}`,
    meta: { sortField: "amount_cents", align: "end" } satisfies NavreeColumnMeta,
  },
];

const deals: Deal[] = [
  { id: "d1", title: "Agra renewal", stage: "won", amount: 1200 },
  { id: "d2", title: "Gulati pilot", stage: "lead", amount: 300 },
];
const meta: PageMeta = { current_page: 2, per_page: 2, total: 5, last_page: 3 };

function renderTable(query: ListQuery = {}, extra: Partial<Parameters<typeof DataTable<Deal>>[0]> = {}) {
  const onQueryChange = vi.fn();
  const view = render(
    <DataTable<Deal>
      columns={columns}
      data={deals}
      meta={meta}
      query={query}
      onQueryChange={onQueryChange}
      getRowId={(row) => row.id}
      {...extra}
    />,
  );
  return { ...view, onQueryChange };
}

describe("DataTable", () => {
  it("renders one page of rows with the column cells and classes", () => {
    renderTable();
    const rows = screen.getAllByRole("row");
    expect(rows).toHaveLength(3);
    expect(within(rows[1]!).getByText("Agra renewal")).toBeTruthy();
    expect(within(rows[1]!).getByText("$1200").className).toContain("text-end");
    expect(within(rows[1]!).getByText("won").className).toContain("hidden md:table-cell");
    expect(rows[1]!.getAttribute("data-row-id")).toBe("d1");
  });

  it("turns a header click into a new sort on page 1 (ascending, descending, off)", () => {
    const { onQueryChange, rerender } = renderTable({ page: 2, filter: { stage: "won" } });
    fireEvent.click(screen.getByRole("button", { name: /Title/ }));
    expect(onQueryChange).toHaveBeenLastCalledWith({ filter: { stage: "won" }, sort: ["title"] });

    rerender(<DataTable<Deal> columns={columns} data={deals} meta={meta} query={{ sort: ["title"] }} onQueryChange={onQueryChange} />);
    expect(screen.getByRole("columnheader", { name: /Title/ }).getAttribute("aria-sort")).toBe("ascending");
    fireEvent.click(screen.getByRole("button", { name: /Title/ }));
    expect(onQueryChange).toHaveBeenLastCalledWith({ sort: ["-title"] });

    rerender(<DataTable<Deal> columns={columns} data={deals} meta={meta} query={{ sort: ["-title"] }} onQueryChange={onQueryChange} />);
    expect(screen.getByRole("columnheader", { name: /Title/ }).getAttribute("aria-sort")).toBe("descending");
    fireEvent.click(screen.getByRole("button", { name: /Title/ }));
    expect(onQueryChange).toHaveBeenLastCalledWith({});
  });

  it("starts a cursor list over on a sort click: no cursor, no page before, the page size stays", () => {
    const { onQueryChange } = renderTable({ cursor: "abc", before: "def", limit: 20, filter: { stage: { in: ["lead", "won"] } } });
    fireEvent.click(screen.getByRole("button", { name: /Title/ }));
    expect(onQueryChange).toHaveBeenLastCalledWith({ limit: 20, filter: { stage: { in: ["lead", "won"] } }, sort: ["title"] });
  });

  it("sorts by the column's sort field, and by several with shift", () => {
    const { onQueryChange } = renderTable({ sort: ["title"] });
    fireEvent.click(screen.getByRole("button", { name: /Amount/ }), { shiftKey: true });
    expect(onQueryChange).toHaveBeenLastCalledWith({ sort: ["title", "amount_cents"] });
    expect(screen.queryByRole("button", { name: /Stage/ })).toBeNull();
  });

  it("pages with the pagination bar", () => {
    const { onQueryChange } = renderTable({ page: 2 });
    expect(screen.getByText("3–4 of 5")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Next" }));
    expect(onQueryChange).toHaveBeenLastCalledWith({ page: 3 });
    fireEvent.click(screen.getByRole("button", { name: "Previous" }));
    expect(onQueryChange).toHaveBeenLastCalledWith({});
  });

  it("disables previous on the first page and next on the last", () => {
    renderTable({}, { meta: { current_page: 1, per_page: 2, total: 2, last_page: 1 } });
    expect((screen.getByRole("button", { name: "Previous" }) as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByRole("button", { name: "Next" }) as HTMLButtonElement).disabled).toBe(true);
  });

  it("shows loading rows, the empty state and translated labels", () => {
    const { rerender, onQueryChange } = renderTable({}, { loading: true });
    expect(screen.getAllByRole("row").filter((r) => r.dataset.slot === "loading-row")).toHaveLength(5);
    expect(screen.getByText("Loading…")).toBeTruthy();

    rerender(
      <DataTable<Deal>
        columns={columns}
        data={[]}
        meta={{ current_page: 1, per_page: 2, total: 0, last_page: 1 }}
        query={{}}
        onQueryChange={onQueryChange}
        labels={{ empty: "Aucun résultat.", previous: "Précédent", next: "Suivant" }}
      />,
    );
    expect(screen.getByText("Aucun résultat.")).toBeTruthy();
    // No pagination bar for an empty list.
    expect(screen.queryByRole("navigation")).toBeNull();
  });

  it("opens a row on click", () => {
    const onRowClick = vi.fn();
    renderTable({}, { onRowClick });
    fireEvent.click(screen.getByText("Gulati pilot"));
    expect(onRowClick).toHaveBeenCalledWith(deals[1]);
  });
});

describe("useDataTable", () => {
  it("gives TanStack Table the list query's sort and the page, and keeps the server's row order", () => {
    const { result, rerender } = renderHook(
      ({ query }: { query: ListQuery }) => useDataTable<Deal>({ columns, data: deals, meta, query, onQueryChange: vi.fn() }),
      { initialProps: { query: { sort: ["-amount_cents"], page: 2 } as ListQuery } },
    );
    expect(result.current.state.sorting).toEqual([{ id: "amount_cents", desc: true }]);
    expect(result.current.state.pagination).toEqual({ pageIndex: 1, pageSize: 2 });
    expect(result.current.getPageCount()).toBe(3);
    expect(result.current.getCanNextPage()).toBe(true);
    expect(result.current.getRowModel().rows.map((row) => row.original.id)).toEqual(["d1", "d2"]);

    rerender({ query: { sort: ["title"] } });
    expect(result.current.state.sorting).toEqual([{ id: "title", desc: false }]);
  });

  it("takes the page size of a cursor list from `limit` until the server reports one", () => {
    const { result } = renderHook(() =>
      useDataTable<Deal>({ columns, data: deals, query: { cursor: "abc", limit: 40 }, onQueryChange: vi.fn() }),
    );
    expect(result.current.state.pagination).toEqual({ pageIndex: 0, pageSize: 40 });
  });
});
