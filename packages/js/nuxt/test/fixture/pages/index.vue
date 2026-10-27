<script setup lang="ts">
import type { ColumnDef } from "@tanstack/vue-table";

type Deal = { id: string; title: string };

const auth = useNavreeAuth<{ name: string }>();
const { workspace } = useWorkspace();
const list = useListQuery({ per_page: 25 });
const ability = useAbility();
const state = useRealtimeState();
useChannel("crm.deals", "deal.updated", () => undefined);
const { operation } = useOperation(null);
const columns: ColumnDef<Deal, any>[] = [{ accessorKey: "title", header: "Title", meta: { sortField: "title" } }];
const deals: Deal[] = [{ id: "d1", title: "Acme renewal" }];

// Server-side rendering calls the BFF in-process with the browser's cookies.
const me = await auth.load().catch(() => null);
const api = useNavreeClient();
const echo = await api
  .raw("/api/v1/deals?page=2")
  .then((response) => (response.ok ? response.json() : null))
  .catch(() => null);
</script>

<template>
  <main>
    <h1>Deals</h1>
    <p id="status">{{ auth.status.value }} {{ workspace ?? "none" }} {{ state }} {{ operation?.status ?? "idle" }}</p>
    <p id="me">{{ me ? `user:${me.name}` : "no user" }}</p>
    <p id="echo">{{ echo ? `ws:${echo.workspace} auth:${echo.auth}` : "no deals" }}</p>
    <p id="can">{{ ability.can("read", "Deal") ? "can read" : "cannot read" }}</p>
    <Can I="read" a="Deal"><span>visible</span></Can>
    <NavreeDataTable :columns="columns" :data="deals" :query="list.query.value" @update:query="list.setQuery" />
  </main>
</template>
