"use client";

import { DataTable, useListQuery, type NavreeColumnDef, type NavreeColumnMeta } from "@navree/next";
import { AbilityProvider, Can } from "@navree/next/abilities";
import { RealtimeProvider, useChannel } from "@navree/next/realtime";

type Deal = { id: string; title: string };

const columns: NavreeColumnDef<Deal, unknown>[] = [
  { accessorKey: "title", header: "Title", meta: { sortField: "title", filter: "text" } satisfies NavreeColumnMeta },
];

function Live() {
  useChannel("crm.deals", "deal.updated", () => undefined);
  return null;
}

export function DealsTable() {
  const { query, setQuery } = useListQuery({ per_page: 25 });
  return (
    <AbilityProvider rules={[{ action: "read", subject: "Deal" }]}>
      <RealtimeProvider enabled={false}>
        <Live />
        <Can I="read" a="Deal">
          <p>can read</p>
        </Can>
        <DataTable<Deal>
          columns={columns}
          data={[{ id: "d1", title: "Agra renewal" }]}
          meta={{ current_page: 1, per_page: 25, total: 1, last_page: 1 }}
          query={query}
          onQueryChange={setQuery}
          getRowId={(row) => row.id}
          search
          exportUrl="/api/v1/deals/export"
          bulk={{ actions: [{ id: "archive", label: "Archive" }], onAction: () => undefined }}
        />
      </RealtimeProvider>
    </AbilityProvider>
  );
}
