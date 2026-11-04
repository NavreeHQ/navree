import type { ListQuery, PageMeta } from "@navree/client";
import { mount } from "@vue/test-utils";
import { effectScope, nextTick, ref } from "vue";
import { describe, expect, it } from "vitest";
import { NavreeDataTable, useDataTable, type NavreeColumnDef, type NavreeColumnMeta } from "./data-table.js";

type Deal = { id: string; title: string; stage: string; amount: number };

const columns: NavreeColumnDef<Deal>[] = [
  { accessorKey: "title", header: "Title", meta: { sortField: "title" } satisfies NavreeColumnMeta },
  { accessorKey: "stage", header: "Stage", meta: { className: "hidden md:table-cell" } satisfies NavreeColumnMeta },
  { accessorKey: "amount", header: "Amount", cell: (ctx) => `$${ctx.getValue()}`, meta: { sortField: "amount_cents", align: "end" } satisfies NavreeColumnMeta },
];
const deals: Deal[] = [
  { id: "d1", title: "Acme renewal", stage: "won", amount: 1200 },
  { id: "d2", title: "Globex pilot", stage: "lead", amount: 300 },
];
const meta: PageMeta = { current_page: 2, per_page: 2, total: 5, last_page: 3 };

function table(props: Record<string, unknown> = {}) {
  return mount(NavreeDataTable, {
    props: { columns, data: deals, meta, query: {} as ListQuery, getRowId: (row: Deal) => row.id, ...props },
  });
}

describe("NavreeDataTable", () => {
  it("renders one page with shadcn-vue's table slots", () => {
    const wrapper = table();
    const rows = wrapper.findAll("tbody tr");
    expect(rows).toHaveLength(2);
    expect(rows[0]!.attributes("data-row-id")).toBe("d1");
    expect(rows[0]!.text()).toContain("Acme renewal");
    expect(wrapper.find('[data-slot="table"]').exists()).toBe(true);
    const amount = rows[0]!.findAll("td")[2]!;
    expect(amount.text()).toBe("$1200");
    expect(amount.classes()).toContain("text-end");
    expect(rows[0]!.findAll("td")[1]!.classes()).toContain("md:table-cell");
  });

  it("emits the next list query on a header click: a new sort on page 1", async () => {
    const wrapper = table({ query: { page: 2, filter: { stage: "won" } } });
    await wrapper.findAll("th button")[0]!.trigger("click");
    expect(wrapper.emitted("update:query")!.at(-1)).toEqual([{ filter: { stage: "won" }, sort: ["title"] }]);

    await wrapper.setProps({ query: { sort: ["title"] } });
    expect(wrapper.findAll("th")[0]!.attributes("aria-sort")).toBe("ascending");
    await wrapper.findAll("th button")[0]!.trigger("click");
    expect(wrapper.emitted("update:query")!.at(-1)).toEqual([{ sort: ["-title"] }]);

    await wrapper.findAll("th button")[1]!.trigger("click", { shiftKey: true });
    expect(wrapper.emitted("update:query")!.at(-1)).toEqual([{ sort: ["title", "amount_cents"] }]);
    expect(wrapper.findAll("th")[1]!.find("button").exists()).toBe(false);
  });

  it("starts a cursor list over on a sort click: no cursor, no page before, the page size stays", async () => {
    const wrapper = table({ query: { cursor: "abc", before: "def", limit: 20, filter: { stage: { in: ["lead", "won"] } } } });
    await wrapper.findAll("th button")[0]!.trigger("click");
    expect(wrapper.emitted("update:query")!.at(-1)).toEqual([{ limit: 20, filter: { stage: { in: ["lead", "won"] } }, sort: ["title"] }]);
  });

  it("pages", async () => {
    const wrapper = table({ query: { page: 2 } });
    expect(wrapper.find("nav").text()).toContain("3–4 of 5");
    const [previous, next] = wrapper.findAll("nav button");
    await next!.trigger("click");
    expect(wrapper.emitted("update:query")!.at(-1)).toEqual([{ page: 3 }]);
    await previous!.trigger("click");
    expect(wrapper.emitted("update:query")!.at(-1)).toEqual([{}]);
  });

  it("shows loading rows, the empty state and translated labels, and emits row clicks", async () => {
    const loading = table({ loading: true });
    expect(loading.findAll('[data-slot="loading-row"]')).toHaveLength(5);
    expect(loading.text()).toContain("Loading…");

    const empty = table({ data: [], meta: { current_page: 1, per_page: 2, total: 0, last_page: 1 }, labels: { empty: "Aucun résultat." } });
    expect(empty.text()).toContain("Aucun résultat.");
    expect(empty.find("nav").exists()).toBe(false);

    const clickable = table({ clickable: true });
    await clickable.findAll("tbody tr")[1]!.trigger("click");
    expect(clickable.emitted("row-click")![0]).toEqual([deals[1]]);
  });
});

describe("useDataTable", () => {
  it("gives TanStack Table the list query's sort and the page count, and keeps the server's row order", async () => {
    const query = ref<ListQuery>({ sort: ["-amount_cents"] });
    const scope = effectScope();
    const navree = scope.run(() =>
      useDataTable<Deal>({
        columns: () => columns,
        data: () => deals,
        query: () => query.value,
        meta: () => meta,
        onQueryChange: () => undefined,
      }),
    )!;
    expect(navree.table.atoms.sorting.get()).toEqual([{ id: "amount_cents", desc: true }]);
    expect(navree.table.getPageCount()).toBe(3);
    expect(navree.table.getRowModel().rows.map((row) => row.original.id)).toEqual(["d1", "d2"]);

    query.value = { sort: ["title"] };
    await nextTick();
    expect(navree.table.atoms.sorting.get()).toEqual([{ id: "title", desc: false }]);
    scope.stop();
  });
});
