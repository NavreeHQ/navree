<script setup lang="ts">
import { useAbility } from "@casl/vue";
import { Download, Plus, Users, X } from "@lucide/vue";
import { errorMessage } from "@navree/client";
import { toast } from "vue-sonner";
import ContactStatusBadge from "@/components/crm/ContactStatusBadge.vue";
import ListEmpty from "@/components/crm/ListEmpty.vue";
import OptionFilter from "@/components/crm/OptionFilter.vue";
import OwnerCell from "@/components/crm/OwnerCell.vue";
import OwnerFilter from "@/components/crm/OwnerFilter.vue";
import PageHeader from "@/components/crm/PageHeader.vue";
import PaginationBar from "@/components/crm/PaginationBar.vue";
import PastEndNotice from "@/components/crm/PastEndNotice.vue";
import SearchFilter from "@/components/crm/SearchFilter.vue";
import SkeletonRows from "@/components/crm/SkeletonRows.vue";
import { Button } from "@/components/ui/button";
import { Spinner } from "@/components/ui/spinner";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { useCurrentUser } from "@/features/auth/queries";
import { useCompany } from "@/features/companies/queries";
import { type ContactListParams, exportContactsCsv, useContacts } from "@/features/contacts/queries";
import { STATUS_LABEL } from "@/features/contacts/status";
import type { ContactStatus } from "@/lib/api/types";
import { CONTACT_STATUSES } from "@/lib/api/types";
import type { AppAbility } from "@/lib/casl/ability";
import { formatNumber } from "@/lib/format";
import { useUiStore } from "@/stores/ui";

useHead({ title: "Contacts" });

const STATUS_OPTIONS = CONTACT_STATUSES.map((status) => ({ value: status, label: STATUS_LABEL[status] }));

const url = useUrlState();
const me = useCurrentUser();
const ui = useUiStore();
const ability = useAbility<AppAbility>();

const params = computed<ContactListParams>(() => {
  const status = url.get("status") as ContactStatus | undefined;
  return {
    search: url.get("search"),
    filter: {
      status: status && CONTACT_STATUSES.includes(status) ? status : undefined,
      owner_id: url.get("owner"),
      company_id: url.get("company"),
    },
    page: url.page.value,
    per_page: 25,
  };
});
const contacts = useContacts(params);
const companyFilter = useCompany(
  () => params.value.filter?.company_id ?? "",
  () => Boolean(params.value.filter?.company_id),
);
const filtered = computed(() => {
  const { search, filter } = params.value;
  return Boolean(search || filter?.status || filter?.owner_id || filter?.company_id);
});
const total = computed(() => contacts.data.value?.meta.total);
const rows = computed(() => contacts.data.value?.data ?? []);

const exporting = ref(false);
async function exportCsv() {
  exporting.value = true;
  try {
    const { filter, search } = params.value;
    await exportContactsCsv({ filter, search });
    toast.success("Contacts exported");
  } catch (error) {
    toast.error(errorMessage(error, "The export failed. Try again."));
  } finally {
    exporting.value = false;
  }
}

function openRow(event: MouseEvent, id: string) {
  if (!(event.target as HTMLElement).closest("a,button")) void navigateTo(`/contacts/${id}`);
}
</script>

<template>
  <div class="flex flex-col">
    <PageHeader
      title="Contacts"
      :description="
        total === undefined ? 'People at the companies you sell to' : `${formatNumber(total)} ${total === 1 ? 'contact' : 'contacts'}${filtered ? ' match' : ''}`
      "
    >
      <template #actions>
        <Button v-if="ability.can('export', 'Contact')" variant="outline" :disabled="exporting" @click="exportCsv">
          <Spinner v-if="exporting" />
          <Download v-else />
          Export CSV
        </Button>
        <Button v-if="ability.can('create', 'Contact')" @click="ui.create('contact')"><Plus /> New contact</Button>
      </template>
    </PageHeader>

    <section class="surface overflow-hidden" aria-label="Contacts">
      <div class="flex flex-wrap items-center gap-2 border-b border-line p-3">
        <SearchFilter :value="params.search" placeholder="Search by name, email or title" @change="(search) => url.set({ search })" />
        <OptionFilter
          :value="params.filter?.status"
          :options="STATUS_OPTIONS"
          any-label="Any status"
          label="Filter by status"
          @change="(status) => url.set({ status })"
        />
        <OwnerFilter :value="params.filter?.owner_id" :current-user-id="me.data.value?.id" @change="(owner) => url.set({ owner })" />
        <span
          v-if="params.filter?.company_id"
          class="inline-flex h-8 items-center gap-1 rounded-md border border-fg-3/60 bg-ink-2 pr-1 pl-2.5 text-[13px]"
        >
          <span class="text-fg-3">Company:</span>
          <span class="font-medium text-fg">{{ companyFilter.data.value?.name ?? "…" }}</span>
          <Button variant="ghost" size="icon-xs" aria-label="Clear company filter" @click="url.set({ company: undefined })"><X /></Button>
        </span>
      </div>
      <div class="overflow-x-auto scroll-thin">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead class="pl-4">Name</TableHead>
              <TableHead class="max-sm:hidden">Company</TableHead>
              <TableHead class="max-md:hidden">Email</TableHead>
              <TableHead class="hidden xl:table-cell">Phone</TableHead>
              <TableHead>Status</TableHead>
              <TableHead class="pr-4 max-lg:hidden">Owner</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            <SkeletonRows v-if="contacts.isPending.value" :columns="6" />
            <template v-else-if="rows.length">
              <TableRow v-for="p in rows" :key="p.id" class="cursor-pointer" @click="openRow($event, p.id)">
                <TableCell class="py-2.5 pl-4">
                  <NuxtLink :to="`/contacts/${p.id}`" class="font-medium text-fg hover:underline hover:decoration-fg-3 hover:underline-offset-2">{{ p.full_name }}</NuxtLink>
                  <div v-if="p.title" class="text-xs text-fg-3">{{ p.title }}</div>
                </TableCell>
                <TableCell class="text-fg-1 max-sm:hidden">
                  <NuxtLink v-if="p.company" :to="`/companies/${p.company.id}`" class="hover:underline">{{ p.company.name }}</NuxtLink>
                  <span v-else class="text-fg-3">No company</span>
                </TableCell>
                <TableCell class="max-w-56 truncate text-fg-1 max-md:hidden">
                  <a v-if="p.email" :href="`mailto:${p.email}`" class="hover:underline">{{ p.email }}</a>
                  <span v-else class="text-fg-3">Not set</span>
                </TableCell>
                <TableCell class="hidden whitespace-nowrap text-fg-1 figures xl:table-cell">
                  <template v-if="p.phone">{{ p.phone }}</template>
                  <span v-else class="text-fg-3">Not set</span>
                </TableCell>
                <TableCell><ContactStatusBadge :status="p.status" /></TableCell>
                <TableCell class="pr-4 text-fg-1 max-lg:hidden"><OwnerCell :owner="p.owner" /></TableCell>
              </TableRow>
            </template>
            <TableRow v-else class="hover:bg-transparent">
              <TableCell :colspan="6">
                <PastEndNotice
                  v-if="contacts.data.value && contacts.data.value.meta.total > 0"
                  noun="contacts"
                  :total="contacts.data.value.meta.total"
                  @first="url.set({ page: undefined })"
                />
                <ListEmpty
                  v-else
                  :filtered="filtered"
                  noun="contacts"
                  @clear="url.set({ search: undefined, status: undefined, owner: undefined, company: undefined })"
                >
                  <template #icon><Users /></template>
                  <template #action>
                    <Button v-if="ability.can('create', 'Contact')" size="sm" @click="ui.create('contact')"><Plus /> New contact</Button>
                  </template>
                </ListEmpty>
              </TableCell>
            </TableRow>
          </TableBody>
        </Table>
      </div>
      <PaginationBar :meta="contacts.data.value?.meta" noun="contacts" @page="(page) => url.set({ page })" />
    </section>
  </div>
</template>
