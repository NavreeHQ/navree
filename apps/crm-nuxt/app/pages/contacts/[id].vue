<script setup lang="ts">
import { useAbility } from "@casl/vue";
import { Briefcase, Building2, Mail, Pencil, Phone, Trash2 } from "@lucide/vue";
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
import { Button } from "@/components/ui/button";
import RecordActivity from "@/features/activities/RecordActivity.vue";
import ContactFormDialog from "@/features/contacts/ContactFormDialog.vue";
import { useContact, useDeleteContact } from "@/features/contacts/queries";
import { useDeals } from "@/features/deals/queries";
import type { AppAbility } from "@/lib/casl/ability";
import { formatDateLong } from "@/lib/format";

const BACK = { href: "/contacts", label: "Contacts" };
const route = useRoute();
const id = computed(() => String(route.params.id));
const contact = useContact(id);
const remove = useDeleteContact();
const ability = useAbility<AppAbility>();
const editing = ref(false);

useHead({ title: () => contact.data.value?.full_name ?? "Contact" });
useRecordCrumb(() => contact.data.value?.full_name);

// Deals at the contact's company where this person is the main contact.
const companyId = computed(() => contact.data.value?.company_id ?? undefined);
const deals = useDeals(() => ({ filter: { company_id: companyId.value }, per_page: 100 }), () => Boolean(companyId.value));
const theirDeals = computed(() => deals.data.value?.data.filter((d) => d.contact_id === id.value) ?? []);

function destroy() {
  const p = contact.data.value;
  if (!p) return;
  remove.mutate(p.id, {
    onSuccess: () => {
      toast.success(`${p.full_name} deleted`);
      void navigateTo("/contacts");
    },
  });
}
</script>

<template>
  <RecordSkeleton v-if="contact.isPending.value" />
  <RecordMissing v-else-if="contact.isError.value" :error="contact.error.value" noun="contact" :back="BACK" />
  <div v-else-if="contact.data.value" class="flex flex-col">
    <RecordHeader :title="contact.data.value.full_name">
      <template #eyebrow><ContactStatusBadge :status="contact.data.value.status" class="w-fit" /></template>
      <template #meta>
        <MetaItem v-if="contact.data.value.title" :icon="Briefcase">{{ contact.data.value.title }}</MetaItem>
        <MetaItem v-if="contact.data.value.company" :icon="Building2">
          <NuxtLink :to="`/companies/${contact.data.value.company.id}`" class="link text-fg-2">{{ contact.data.value.company.name }}</NuxtLink>
        </MetaItem>
        <MetaItem v-if="contact.data.value.email" :icon="Mail">
          <a :href="`mailto:${contact.data.value.email}`" class="link text-fg-2">{{ contact.data.value.email }}</a>
        </MetaItem>
        <MetaItem v-if="contact.data.value.phone" :icon="Phone">
          <a :href="`tel:${contact.data.value.phone}`" class="link text-fg-2 figures">{{ contact.data.value.phone }}</a>
        </MetaItem>
      </template>
      <template #actions>
        <Button v-if="ability.can('update', contact.data.value)" variant="outline" @click="editing = true"><Pencil /> Edit</Button>
        <ConfirmDialog
          v-if="ability.can('delete', contact.data.value)"
          :title="`Delete ${contact.data.value.full_name}?`"
          description="The contact is removed from lists and search. Their activity history is kept."
          confirm-label="Delete contact"
          :pending="remove.isPending.value"
          @confirm="destroy"
        >
          <template #trigger>
            <Button variant="ghost" size="icon" aria-label="Delete contact"><Trash2 /></Button>
          </template>
        </ConfirmDialog>
      </template>
    </RecordHeader>
    <ContactFormDialog v-model:open="editing" :contact="contact.data.value" />

    <div class="grid gap-6 xl:grid-cols-[minmax(0,1fr)_20rem]">
      <Panel title="Activity">
        <RecordActivity subject-type="contact" :subject-id="contact.data.value.id" />
      </Panel>
      <div class="flex flex-col gap-6">
        <Panel title="Details" body-class="py-2">
          <dl>
            <DetailRow label="Owner"><OwnerCell :owner="contact.data.value.owner" /></DetailRow>
            <DetailRow label="Status"><ContactStatusBadge :status="contact.data.value.status" /></DetailRow>
            <DetailRow label="Company">
              <NuxtLink v-if="contact.data.value.company" :to="`/companies/${contact.data.value.company.id}`" class="link">{{
                contact.data.value.company.name
              }}</NuxtLink>
              <span v-else class="text-fg-3">None</span>
            </DetailRow>
            <DetailRow label="Job title" :value="contact.data.value.title" />
            <DetailRow label="Email" :value="contact.data.value.email" />
            <DetailRow label="Phone" :value="contact.data.value.phone" />
            <DetailRow label="Added" :value="formatDateLong(contact.data.value.created_at)" />
            <DetailRow label="Last updated" :value="formatDateLong(contact.data.value.updated_at)" />
          </dl>
        </Panel>
        <Panel title="Deals">
          <p v-if="!contact.data.value.company_id" class="text-[13px] text-fg-3">Link this contact to a company to track deals.</p>
          <p v-else-if="deals.isPending.value" class="text-[13px] text-fg-3">Loading deals…</p>
          <p v-else-if="theirDeals.length === 0" class="text-[13px] text-fg-3">Not the main contact on any deal.</p>
          <ul v-else class="-my-1 flex flex-col divide-y divide-line">
            <li v-for="d in theirDeals" :key="d.id" class="flex items-center justify-between gap-3 py-2.5 text-sm">
              <div class="flex min-w-0 flex-col gap-1.5">
                <NuxtLink :to="`/deals/${d.id}`" class="truncate font-medium text-fg hover:underline">{{ d.title }}</NuxtLink>
                <StageBadge :stage="d.stage" class="w-fit" />
              </div>
              <Money :cents="d.amount_cents" :currency="d.currency" class="font-medium text-fg" />
            </li>
          </ul>
        </Panel>
      </div>
    </div>
  </div>
</template>
