<script setup lang="ts">
import { useAbility } from "@casl/vue";
import { Building2, Calendar, Pencil, Trash2, User } from "@lucide/vue";
import { toast } from "vue-sonner";
import ConfirmDialog from "@/components/crm/ConfirmDialog.vue";
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
import DealFormDialog from "@/features/deals/DealFormDialog.vue";
import { useDeal, useDeleteDeal } from "@/features/deals/queries";
import StageBar from "@/features/deals/StageBar.vue";
import type { AppAbility } from "@/lib/casl/ability";
import { formatDateLong, formatPercent } from "@/lib/format";
import { isClosed } from "@/lib/stages";

const BACK = { href: "/deals", label: "Deals" };
const route = useRoute();
const id = computed(() => String(route.params.id));
const deal = useDeal(id);
const remove = useDeleteDeal();
const ability = useAbility<AppAbility>();
const editing = ref(false);

useHead({ title: () => deal.data.value?.title ?? "Deal" });
useRecordCrumb(() => deal.data.value?.title);

const weighted = computed(() =>
  deal.data.value ? Math.round((deal.data.value.amount_cents * deal.data.value.probability) / 100) : 0,
);

function destroy() {
  const d = deal.data.value;
  if (!d) return;
  remove.mutate(d.id, {
    onSuccess: () => {
      toast.success(`${d.title} deleted`);
      void navigateTo("/deals");
    },
  });
}
</script>

<template>
  <RecordSkeleton v-if="deal.isPending.value" />
  <RecordMissing v-else-if="deal.isError.value" :error="deal.error.value" noun="deal" :back="BACK" />
  <div v-else-if="deal.data.value" class="flex flex-col">
    <RecordHeader :title="deal.data.value.title">
      <template #eyebrow><StageBadge :stage="deal.data.value.stage" class="w-fit" /></template>
      <template #meta>
        <MetaItem :icon="Building2">
          <NuxtLink :to="`/companies/${deal.data.value.company.id}`" class="link text-fg-2">{{ deal.data.value.company.name }}</NuxtLink>
        </MetaItem>
        <MetaItem v-if="deal.data.value.contact" :icon="User">
          <NuxtLink :to="`/contacts/${deal.data.value.contact.id}`" class="link text-fg-2">{{ deal.data.value.contact.full_name }}</NuxtLink>
        </MetaItem>
        <MetaItem v-if="deal.data.value.expected_close_date && !deal.data.value.closed_at" :icon="Calendar">
          Expected to close {{ formatDateLong(deal.data.value.expected_close_date) }}
        </MetaItem>
      </template>
      <template #actions>
        <Button v-if="ability.can('update', deal.data.value)" variant="outline" @click="editing = true">
          <Pencil />
          Edit
        </Button>
        <ConfirmDialog
          v-if="ability.can('delete', deal.data.value)"
          :title="`Delete ${deal.data.value.title}?`"
          description="The deal is removed from the board and reports. This can't be undone from the app."
          confirm-label="Delete deal"
          :pending="remove.isPending.value"
          @confirm="destroy"
        >
          <template #trigger>
            <Button variant="ghost" size="icon" aria-label="Delete deal"><Trash2 /></Button>
          </template>
        </ConfirmDialog>
      </template>
    </RecordHeader>
    <DealFormDialog v-model:open="editing" :deal="deal.data.value" />

    <div class="flex flex-col gap-6">
      <Panel body-class="p-2.5">
        <StageBar :deal="deal.data.value" />
      </Panel>
      <div class="grid gap-6 xl:grid-cols-[minmax(0,1fr)_20rem]">
        <div class="flex min-w-0 flex-col gap-6">
          <dl class="surface grid grid-cols-2 overflow-hidden sm:grid-cols-4 [&>div]:border-line max-sm:[&>div:nth-child(even)]:border-l max-sm:[&>div:nth-child(n+3)]:border-t sm:[&>div:nth-child(n+2)]:border-l">
            <div class="flex flex-col gap-1.5 px-4 py-4">
              <dt class="text-xs text-fg-3">Value</dt>
              <dd class="font-display text-xl font-semibold tracking-[-0.02em] text-fg">
                <Money :cents="deal.data.value.amount_cents" :currency="deal.data.value.currency" />
              </dd>
            </div>
            <div class="flex flex-col gap-1.5 px-4 py-4">
              <dt class="text-xs text-fg-3">Probability</dt>
              <dd class="font-display text-xl font-semibold tracking-[-0.02em] text-fg figures">{{ formatPercent(deal.data.value.probability) }}</dd>
            </div>
            <div class="flex flex-col gap-1.5 px-4 py-4">
              <dt class="text-xs text-fg-3">Weighted</dt>
              <dd class="font-display text-xl font-semibold tracking-[-0.02em] text-fg">
                <Money :cents="weighted" :currency="deal.data.value.currency" />
              </dd>
            </div>
            <div class="flex flex-col gap-1.5 px-4 py-4">
              <dt class="text-xs text-fg-3">{{ isClosed(deal.data.value.stage) ? "Closed" : "Expected close" }}</dt>
              <dd class="font-display text-xl font-semibold tracking-[-0.02em] text-fg figures">
                <template v-if="isClosed(deal.data.value.stage) ? deal.data.value.closed_at : deal.data.value.expected_close_date">
                  {{ formatDateLong(isClosed(deal.data.value.stage) ? deal.data.value.closed_at : deal.data.value.expected_close_date) }}
                </template>
                <span v-else class="text-fg-3">Not set</span>
              </dd>
            </div>
          </dl>
          <Panel v-if="deal.data.value.stage === 'lost' && deal.data.value.lost_reason" title="Why it was lost">
            <p class="text-sm text-fg-1">{{ deal.data.value.lost_reason }}</p>
          </Panel>
          <Panel title="Activity">
            <RecordActivity subject-type="deal" :subject-id="deal.data.value.id" />
          </Panel>
        </div>
        <Panel title="Details" class="h-fit" body-class="py-2">
          <dl>
            <DetailRow label="Owner"><OwnerCell :owner="deal.data.value.owner" /></DetailRow>
            <DetailRow label="Company">
              <NuxtLink :to="`/companies/${deal.data.value.company.id}`" class="link">{{ deal.data.value.company.name }}</NuxtLink>
            </DetailRow>
            <DetailRow label="Main contact">
              <NuxtLink v-if="deal.data.value.contact" :to="`/contacts/${deal.data.value.contact.id}`" class="link">{{
                deal.data.value.contact.full_name
              }}</NuxtLink>
              <span v-else class="text-fg-3">None</span>
            </DetailRow>
            <DetailRow label="Currency" :value="deal.data.value.currency" />
            <DetailRow label="Created" :value="formatDateLong(deal.data.value.created_at)" />
            <DetailRow label="Last updated" :value="formatDateLong(deal.data.value.updated_at)" />
          </dl>
        </Panel>
      </div>
    </div>
  </div>
</template>
