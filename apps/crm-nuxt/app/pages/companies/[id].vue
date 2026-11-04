<script setup lang="ts">
import { useAbility } from "@casl/vue";
import { Building2, Globe, MapPin, Pencil, Plus, Trash2, Users } from "@lucide/vue";
import { toast } from "vue-sonner";
import ConfirmDialog from "@/components/crm/ConfirmDialog.vue";
import ContactStatusBadge from "@/components/crm/ContactStatusBadge.vue";
import DetailRow from "@/components/crm/DetailRow.vue";
import MetaItem from "@/components/crm/MetaItem.vue";
import Money from "@/components/crm/Money.vue";
import OwnerCell from "@/components/crm/OwnerCell.vue";
import Panel from "@/components/crm/Panel.vue";
import RecordHeader from "@/components/crm/RecordHeader.vue";
import RecordMissing from "@/components/crm/RecordMissing.vue";
import RecordSkeleton from "@/components/crm/RecordSkeleton.vue";
import StageBadge from "@/components/crm/StageBadge.vue";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import RecordActivity from "@/features/activities/RecordActivity.vue";
import CompanyFormDialog from "@/features/companies/CompanyFormDialog.vue";
import { useCompany, useDeleteCompany } from "@/features/companies/queries";
import ContactFormDialog from "@/features/contacts/ContactFormDialog.vue";
import { useContacts } from "@/features/contacts/queries";
import DealFormDialog from "@/features/deals/DealFormDialog.vue";
import { useDeals } from "@/features/deals/queries";
import type { AppAbility } from "@/lib/casl/ability";
import { formatDate, formatDateLong, formatNumber } from "@/lib/format";

const BACK = { href: "/companies", label: "Companies" };
const route = useRoute();
const id = computed(() => String(route.params.id));
const company = useCompany(id);
const remove = useDeleteCompany();
const ability = useAbility<AppAbility>();
const editing = ref(false);
const addingDeal = ref(false);
const addingContact = ref(false);
const tab = ref("activity");

useHead({ title: () => company.data.value?.name ?? "Company" });
useRecordCrumb(() => company.data.value?.name);

const deals = useDeals(() => ({ filter: { company_id: id.value }, per_page: 50 }), () => tab.value === "deals");
const contacts = useContacts(() => ({ filter: { company_id: id.value }, per_page: 50 }), () => tab.value === "contacts");
const ref_ = computed(() => (company.data.value ? { id: company.data.value.id, name: company.data.value.name } : undefined));

function destroy() {
  const c = company.data.value;
  if (!c) return;
  remove.mutate(c.id, {
    onSuccess: () => {
      toast.success(`${c.name} deleted`);
      void navigateTo("/companies");
    },
  });
}
</script>

<template>
  <RecordSkeleton v-if="company.isPending.value" />
  <RecordMissing v-else-if="company.isError.value" :error="company.error.value" noun="company" :back="BACK" />
  <div v-else-if="company.data.value" class="flex flex-col">
    <RecordHeader :title="company.data.value.name">
      <template #eyebrow><Badge variant="secondary" class="h-6 gap-1.5 px-1.5"><Building2 aria-hidden="true" />Company</Badge></template>
      <template #meta>
        <MetaItem v-if="company.data.value.domain" :icon="Globe">
          <a :href="company.data.value.website ?? `https://${company.data.value.domain}`" target="_blank" rel="noreferrer" class="link text-fg-2">{{
            company.data.value.domain
          }}</a>
        </MetaItem>
        <MetaItem v-if="company.data.value.industry" :icon="Building2">{{ company.data.value.industry }}</MetaItem>
        <MetaItem v-if="company.data.value.size" :icon="Users">{{ company.data.value.size }} employees</MetaItem>
        <MetaItem v-if="company.data.value.city || company.data.value.country" :icon="MapPin">
          {{ [company.data.value.city, company.data.value.country].filter(Boolean).join(", ") }}
        </MetaItem>
      </template>
      <template #actions>
        <Button v-if="ability.can('update', company.data.value)" variant="outline" @click="editing = true"><Pencil /> Edit</Button>
        <ConfirmDialog
          v-if="ability.can('delete', company.data.value)"
          :title="`Delete ${company.data.value.name}?`"
          description="The company is removed from lists and search. Its deals and contacts stay, without a company link in reports."
          confirm-label="Delete company"
          :pending="remove.isPending.value"
          @confirm="destroy"
        >
          <template #trigger>
            <Button variant="ghost" size="icon" aria-label="Delete company"><Trash2 /></Button>
          </template>
        </ConfirmDialog>
      </template>
    </RecordHeader>
    <CompanyFormDialog v-model:open="editing" :company="company.data.value" />
    <DealFormDialog v-model:open="addingDeal" :company="ref_" />
    <ContactFormDialog v-model:open="addingContact" :company="ref_" />

    <div class="grid gap-6 xl:grid-cols-[minmax(0,1fr)_20rem]">
      <div class="flex min-w-0 flex-col gap-6">
        <dl class="surface grid grid-cols-3 overflow-hidden [&>div+div]:border-l [&>div+div]:border-line">
          <div class="flex flex-col gap-1.5 px-4 py-4">
            <dt class="text-xs text-fg-3">Open pipeline</dt>
            <dd class="font-display text-xl font-semibold tracking-[-0.02em] text-fg">
              <Money :cents="company.data.value.open_deals_amount_cents" compact />
            </dd>
          </div>
          <div class="flex flex-col gap-1.5 px-4 py-4">
            <dt class="text-xs text-fg-3">Open deals</dt>
            <dd class="font-display text-xl font-semibold tracking-[-0.02em] text-fg figures">{{ formatNumber(company.data.value.open_deals_count) }}</dd>
          </div>
          <div class="flex flex-col gap-1.5 px-4 py-4">
            <dt class="text-xs text-fg-3">Contacts</dt>
            <dd class="font-display text-xl font-semibold tracking-[-0.02em] text-fg figures">{{ formatNumber(company.data.value.contacts_count) }}</dd>
          </div>
        </dl>

        <Tabs v-model="tab" class="gap-4">
          <TabsList>
            <TabsTrigger value="activity">Activity</TabsTrigger>
            <TabsTrigger value="deals">Deals <span class="text-fg-3 figures">{{ company.data.value.open_deals_count }}</span></TabsTrigger>
            <TabsTrigger value="contacts">Contacts <span class="text-fg-3 figures">{{ company.data.value.contacts_count }}</span></TabsTrigger>
          </TabsList>
          <TabsContent value="activity">
            <Panel><RecordActivity subject-type="company" :subject-id="company.data.value.id" /></Panel>
          </TabsContent>
          <TabsContent value="deals">
            <Panel body-class="p-0">
              <div class="flex items-center justify-between gap-3 border-b border-line px-4 py-2.5">
                <p class="text-[13px] text-fg-2">Deals with {{ company.data.value.name }}</p>
                <Button v-if="ability.can('create', 'Deal')" variant="outline" size="sm" @click="addingDeal = true"><Plus /> New deal</Button>
              </div>
              <div v-if="deals.data.value?.data.length" class="overflow-x-auto scroll-thin">
                <Table>
                  <TableHeader>
                    <TableRow>
                      <TableHead class="pl-4">Deal</TableHead>
                      <TableHead>Stage</TableHead>
                      <TableHead class="text-right">Value</TableHead>
                      <TableHead class="max-sm:hidden">Owner</TableHead>
                      <TableHead class="pr-4 max-sm:hidden">Close</TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    <TableRow v-for="d in deals.data.value.data" :key="d.id">
                      <TableCell class="pl-4"><NuxtLink :to="`/deals/${d.id}`" class="font-medium text-fg hover:underline">{{ d.title }}</NuxtLink></TableCell>
                      <TableCell><StageBadge :stage="d.stage" /></TableCell>
                      <TableCell class="text-right font-medium text-fg"><Money :cents="d.amount_cents" :currency="d.currency" /></TableCell>
                      <TableCell class="text-fg-1 max-sm:hidden"><OwnerCell :owner="d.owner" /></TableCell>
                      <TableCell class="pr-4 text-fg-3 figures max-sm:hidden">{{ formatDate(d.closed_at ?? d.expected_close_date) || "Not set" }}</TableCell>
                    </TableRow>
                  </TableBody>
                </Table>
              </div>
              <p v-else class="px-4 py-6 text-[13px] text-fg-3">{{ deals.isPending.value ? "Loading deals…" : "No deals with this company yet." }}</p>
            </Panel>
          </TabsContent>
          <TabsContent value="contacts">
            <Panel body-class="p-0">
              <div class="flex items-center justify-between gap-3 border-b border-line px-4 py-2.5">
                <p class="text-[13px] text-fg-2">People at {{ company.data.value.name }}</p>
                <Button v-if="ability.can('create', 'Contact')" variant="outline" size="sm" @click="addingContact = true"><Plus /> New contact</Button>
              </div>
              <ul v-if="contacts.data.value?.data.length" class="divide-y divide-line">
                <li v-for="p in contacts.data.value.data" :key="p.id" class="flex items-center justify-between gap-3 px-4 py-3 text-sm">
                  <div class="flex min-w-0 flex-col">
                    <NuxtLink :to="`/contacts/${p.id}`" class="font-medium text-fg hover:underline">{{ p.full_name }}</NuxtLink>
                    <span class="truncate text-xs text-fg-3">{{ [p.title, p.email].filter(Boolean).join(", ") || "No title or email" }}</span>
                  </div>
                  <ContactStatusBadge :status="p.status" />
                </li>
              </ul>
              <p v-else class="px-4 py-6 text-[13px] text-fg-3">{{ contacts.isPending.value ? "Loading contacts…" : "No contacts at this company yet." }}</p>
            </Panel>
          </TabsContent>
        </Tabs>
      </div>
      <Panel title="Details" class="h-fit" body-class="py-2">
        <dl>
          <DetailRow label="Owner"><OwnerCell :owner="company.data.value.owner" /></DetailRow>
          <DetailRow label="Website">
            <a v-if="company.data.value.website" :href="company.data.value.website" target="_blank" rel="noreferrer" class="link">{{
              company.data.value.website.replace(/^https?:\/\//, "")
            }}</a>
            <span v-else class="text-fg-3">None</span>
          </DetailRow>
          <DetailRow label="Phone">
            <a v-if="company.data.value.phone" :href="`tel:${company.data.value.phone}`" class="link figures">{{ company.data.value.phone }}</a>
            <span v-else class="text-fg-3">None</span>
          </DetailRow>
          <DetailRow label="Industry" :value="company.data.value.industry" />
          <DetailRow label="Employees" :value="company.data.value.size" />
          <DetailRow label="City" :value="company.data.value.city" />
          <DetailRow label="Country" :value="company.data.value.country" />
          <DetailRow label="Added" :value="formatDateLong(company.data.value.created_at)" />
          <DetailRow label="Last updated" :value="formatDateLong(company.data.value.updated_at)" />
        </dl>
      </Panel>
    </div>
  </div>
</template>
