<script setup lang="ts">
import { useAbility } from "@casl/vue";
import { Building2, Plus } from "@lucide/vue";
import ListEmpty from "@/components/crm/ListEmpty.vue";
import OptionFilter from "@/components/crm/OptionFilter.vue";
import OwnerCell from "@/components/crm/OwnerCell.vue";
import OwnerFilter from "@/components/crm/OwnerFilter.vue";
import PageHeader from "@/components/crm/PageHeader.vue";
import PaginationBar from "@/components/crm/PaginationBar.vue";
import PastEndNotice from "@/components/crm/PastEndNotice.vue";
import SearchFilter from "@/components/crm/SearchFilter.vue";
import SkeletonRows from "@/components/crm/SkeletonRows.vue";
import SortableHead from "@/components/crm/SortableHead.vue";
import { Button } from "@/components/ui/button";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { useCurrentUser } from "@/features/auth/queries";
import { INDUSTRIES } from "@/features/companies/industries";
import { type CompanyListParams, useCompanies } from "@/features/companies/queries";
import type { CompanySort } from "@/lib/api/types";
import type { AppAbility } from "@/lib/casl/ability";
import { formatDate, formatNumber } from "@/lib/format";
import { useUiStore } from "@/stores/ui";

useHead({ title: "Companies" });

const SORTS: CompanySort[] = ["name", "-name", "created_at", "-created_at"];
const url = useUrlState();
const me = useCurrentUser();
const ui = useUiStore();
const ability = useAbility<AppAbility>();

const params = computed<CompanyListParams>(() => {
  const sort = url.get("sort") as CompanySort | undefined;
  return {
    search: url.get("search"),
    filter: { owner_id: url.get("owner"), industry: url.get("industry") },
    sort: sort && SORTS.includes(sort) ? sort : "name",
    page: url.page.value,
    per_page: 25,
  };
});
const companies = useCompanies(params);
const filtered = computed(() => Boolean(params.value.search || params.value.filter?.owner_id || params.value.filter?.industry));
const total = computed(() => companies.data.value?.meta.total);
const rows = computed(() => companies.data.value?.data ?? []);

function openRow(event: MouseEvent, id: string) {
  if (!(event.target as HTMLElement).closest("a,button")) void navigateTo(`/companies/${id}`);
}
</script>

<template>
  <div class="flex flex-col">
    <PageHeader
      title="Companies"
      :description="
        total === undefined ? 'Accounts your team sells to' : `${formatNumber(total)} ${total === 1 ? 'company' : 'companies'}${filtered ? ' match' : ''}`
      "
    >
      <template #actions>
        <Button v-if="ability.can('create', 'Company')" @click="ui.create('company')">
          <Plus />
          New company
        </Button>
      </template>
    </PageHeader>

    <section class="surface overflow-hidden" aria-label="Companies">
      <div class="flex flex-wrap items-center gap-2 border-b border-line p-3">
        <SearchFilter :value="params.search" placeholder="Search by name or domain" @change="(search) => url.set({ search })" />
        <OwnerFilter :value="params.filter?.owner_id" :current-user-id="me.data.value?.id" @change="(owner) => url.set({ owner })" />
        <OptionFilter
          :value="params.filter?.industry"
          :options="INDUSTRIES.map((i) => ({ value: i, label: i }))"
          any-label="Any industry"
          label="Filter by industry"
          @change="(industry) => url.set({ industry })"
        />
      </div>
      <div class="overflow-x-auto scroll-thin">
        <Table>
          <TableHeader>
            <TableRow>
              <SortableHead label="Company" field="name" :sort="params.sort" class="pl-4" @sort="(sort) => url.set({ sort })" />
              <TableHead class="max-sm:hidden">Industry</TableHead>
              <TableHead class="hidden xl:table-cell">Employees</TableHead>
              <TableHead class="max-md:hidden">Location</TableHead>
              <TableHead>Owner</TableHead>
              <SortableHead label="Added" field="created_at" :sort="params.sort" class="pr-4 max-sm:hidden" @sort="(sort) => url.set({ sort })" />
            </TableRow>
          </TableHeader>
          <TableBody>
            <SkeletonRows v-if="companies.isPending.value" :columns="6" />
            <template v-else-if="rows.length">
              <TableRow v-for="c in rows" :key="c.id" class="cursor-pointer" @click="openRow($event, c.id)">
                <TableCell class="py-2.5 pl-4">
                  <NuxtLink :to="`/companies/${c.id}`" class="font-medium text-fg hover:underline hover:decoration-fg-3 hover:underline-offset-2">{{ c.name }}</NuxtLink>
                  <div v-if="c.domain" class="text-xs text-fg-3">{{ c.domain }}</div>
                </TableCell>
                <TableCell class="text-fg-1 max-sm:hidden">
                  <template v-if="c.industry">{{ c.industry }}</template>
                  <span v-else class="text-fg-3">Not set</span>
                </TableCell>
                <TableCell class="hidden text-fg-1 figures xl:table-cell">
                  <template v-if="c.size">{{ c.size }}</template>
                  <span v-else class="text-fg-3">Unknown</span>
                </TableCell>
                <TableCell class="max-w-48 truncate text-fg-1 max-md:hidden">
                  <template v-if="c.city || c.country">{{ [c.city, c.country].filter(Boolean).join(", ") }}</template>
                  <span v-else class="text-fg-3">Not set</span>
                </TableCell>
                <TableCell class="text-fg-1"><OwnerCell :owner="c.owner" /></TableCell>
                <TableCell class="pr-4 text-fg-3 figures max-sm:hidden">{{ formatDate(c.created_at) }}</TableCell>
              </TableRow>
            </template>
            <TableRow v-else class="hover:bg-transparent">
              <TableCell :colspan="6">
                <PastEndNotice
                  v-if="companies.data.value && companies.data.value.meta.total > 0"
                  noun="companies"
                  :total="companies.data.value.meta.total"
                  @first="url.set({ page: undefined })"
                />
                <ListEmpty v-else :filtered="filtered" noun="companies" @clear="url.set({ search: undefined, owner: undefined, industry: undefined })">
                  <template #icon><Building2 /></template>
                  <template #action>
                    <Button v-if="ability.can('create', 'Company')" size="sm" @click="ui.create('company')"><Plus /> New company</Button>
                  </template>
                </ListEmpty>
              </TableCell>
            </TableRow>
          </TableBody>
        </Table>
      </div>
      <PaginationBar :meta="companies.data.value?.meta" noun="companies" @page="(page) => url.set({ page })" />
    </section>
  </div>
</template>
