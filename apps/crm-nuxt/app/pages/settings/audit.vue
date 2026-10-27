<script setup lang="ts">
import { Lock, ScrollText } from "@lucide/vue";
import AuditChanges from "@/components/crm/AuditChanges.vue";
import OptionFilter from "@/components/crm/OptionFilter.vue";
import OwnerFilter from "@/components/crm/OwnerFilter.vue";
import PaginationBar from "@/components/crm/PaginationBar.vue";
import SkeletonRows from "@/components/crm/SkeletonRows.vue";
import { Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from "@/components/ui/empty";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { ACTION_OPTIONS, entryLabel } from "@/features/audit/actions";
import { useAuditLog } from "@/features/audit/queries";
import { useCurrentUser } from "@/features/auth/queries";
import { useUsers } from "@/features/users/queries";
import { isApiError } from "@/lib/api/errors";
import { formatDateTime } from "@/lib/format";

useHead({ title: "Audit log" });

const SUBJECT_TYPES = ["Company", "Contact", "Deal", "Activity", "User"].map((s) => ({ value: s, label: s }));

const url = useUrlState();
const me = useCurrentUser();
const users = useUsers();
const isAdmin = computed(() => Boolean(me.data.value?.roles.includes("admin")));
const params = computed(() => ({
  actor_id: url.get("actor"),
  subject_type: url.get("subject_type"),
  action: url.get("action"),
  page: url.page.value,
  per_page: 25,
}));
const audit = useAuditLog(params, isAdmin);
const names = computed(() => new Map(users.data.value?.data.map((u) => [u.id, u.name])));
const forbidden = computed(() => !isAdmin.value || (isApiError(audit.error.value) && audit.error.value.status === 403));

function recordHref(type: string | null, id: string | null): string | null {
  if (!type || !id) return null;
  const base = ({ Company: "/companies", Contact: "/contacts", Deal: "/deals" } as Record<string, string>)[type];
  return base ? `${base}/${id}` : null;
}
</script>

<template>
  <Empty v-if="forbidden" class="surface border">
    <EmptyHeader>
      <EmptyMedia variant="icon"><Lock /></EmptyMedia>
      <EmptyTitle>Only admins can view the audit log</EmptyTitle>
      <EmptyDescription>The log records who changed what, for security reviews.</EmptyDescription>
    </EmptyHeader>
  </Empty>
  <div v-else class="flex flex-col gap-4">
    <div class="flex flex-col gap-1">
      <h2 class="text-base font-semibold text-fg">Audit log</h2>
      <p class="text-[13px] text-fg-2">Every sign-in and record change, newest first.</p>
    </div>
    <section class="surface overflow-hidden" aria-label="Audit log">
      <div class="flex flex-wrap items-center gap-2 border-b border-line p-3">
        <OwnerFilter label="Person" :value="params.actor_id" @change="(actor) => url.set({ actor })" />
        <OptionFilter
          :value="params.subject_type"
          :options="SUBJECT_TYPES"
          any-label="Any record type"
          label="Filter by record type"
          @change="(subject_type) => url.set({ subject_type })"
        />
        <OptionFilter :value="params.action" :options="ACTION_OPTIONS" any-label="Any action" label="Filter by action" @change="(action) => url.set({ action })" />
      </div>
      <div class="overflow-x-auto scroll-thin">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead class="pl-4">When</TableHead>
              <TableHead>Who</TableHead>
              <TableHead>Action</TableHead>
              <TableHead>Record</TableHead>
              <TableHead>Changes</TableHead>
              <TableHead class="pr-4">Request</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            <SkeletonRows v-if="audit.isPending.value" :columns="6" />
            <template v-else-if="audit.data.value?.data.length">
              <TableRow v-for="entry in audit.data.value.data" :key="entry.id" class="align-top" data-testid="audit-row">
                <TableCell class="py-2.5 pl-4 text-fg-3 figures">{{ formatDateTime(entry.created_at) }}</TableCell>
                <TableCell class="py-2.5 text-fg-1">
                  <template v-if="entry.actor_id">{{ names.get(entry.actor_id) ?? "Unknown user" }}</template>
                  <span v-else class="text-fg-3">System</span>
                </TableCell>
                <TableCell class="py-2.5">
                  <span class="font-medium text-fg" :title="entry.action">{{ entryLabel(entry) }}</span>
                  <span class="block font-mono text-xs text-fg-3">{{ entry.kind }}</span>
                </TableCell>
                <TableCell class="py-2.5">
                  <template v-if="entry.subject_type">
                    <span class="text-fg-1">{{ entry.subject_type }}</span>
                    <template v-if="entry.subject_id">
                      <NuxtLink
                        v-if="recordHref(entry.subject_type, entry.subject_id)"
                        :to="recordHref(entry.subject_type, entry.subject_id)!"
                        class="link block font-mono text-xs"
                        :title="entry.subject_id"
                        >{{ entry.subject_id.slice(0, 8) }}</NuxtLink
                      >
                      <span v-else class="block font-mono text-xs text-fg-3" :title="entry.subject_id">{{ entry.subject_id.slice(0, 8) }}</span>
                    </template>
                  </template>
                  <span v-else class="text-fg-3">None</span>
                </TableCell>
                <TableCell class="py-2.5 whitespace-normal"><AuditChanges :entry="entry" /></TableCell>
                <TableCell class="py-2.5 pr-4 font-mono text-xs text-fg-3" :title="entry.request_id ?? undefined">
                  {{ entry.request_id ? entry.request_id.slice(-8) : "None" }}
                </TableCell>
              </TableRow>
            </template>
            <TableRow v-else class="hover:bg-transparent">
              <TableCell :colspan="6">
                <Empty class="py-12">
                  <EmptyHeader>
                    <EmptyMedia variant="icon"><ScrollText /></EmptyMedia>
                    <EmptyTitle>No entries match</EmptyTitle>
                    <EmptyDescription>Change the filters to see more of the log.</EmptyDescription>
                  </EmptyHeader>
                </Empty>
              </TableCell>
            </TableRow>
          </TableBody>
        </Table>
      </div>
      <PaginationBar :meta="audit.data.value?.meta" noun="entries" @page="(page) => url.set({ page })" />
    </section>
  </div>
</template>
