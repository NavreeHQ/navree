<script setup lang="ts">
import { refDebounced } from "@vueuse/core";
import { Check, ChevronsUpDown, Search } from "@lucide/vue";
import { ListboxContent, ListboxFilter, ListboxItem, ListboxRoot } from "reka-ui";
import { Button } from "@/components/ui/button";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Spinner } from "@/components/ui/spinner";
import { useCompanies } from "@/features/companies/queries";
import { useContacts } from "@/features/contacts/queries";
import { useDeals } from "@/features/deals/queries";
import type { ActivitySubjectType } from "@/lib/api/types";
import { cn } from "@/lib/utils";
import type { PickedRecord } from "./types";

/**
 * A searchable, server-backed picker for a company, contact or deal. The
 * trigger is a `combobox` named by its field label; the list searches the API
 * as you type (200 ms debounce) and keyboard-navigates like any listbox.
 */
const props = defineProps<{
  type: ActivitySubjectType;
  placeholder: string;
  /** Limits contacts and deals to one company. */
  companyId?: string;
  invalid?: boolean;
  id?: string;
  disabled?: boolean;
}>();
const model = defineModel<PickedRecord | null>({ default: null });

const open = ref(false);
const search = ref("");
const term = refDebounced(search, 200);
const query = computed(() => term.value.trim() || undefined);

const companies = useCompanies(
  () => ({ search: query.value, per_page: 8, sort: "name" as const }),
  () => open.value && props.type === "company",
);
const contacts = useContacts(
  () => ({ search: query.value, filter: { company_id: props.companyId }, per_page: 8 }),
  () => open.value && props.type === "contact",
);
const deals = useDeals(
  () => ({ search: query.value, filter: { company_id: props.companyId }, per_page: 8 }),
  () => open.value && props.type === "deal",
);

const options = computed<PickedRecord[]>(() => {
  if (props.type === "company") return companies.data.value?.data.map((c) => ({ id: c.id, label: c.name })) ?? [];
  if (props.type === "contact") return contacts.data.value?.data.map((c) => ({ id: c.id, label: c.full_name })) ?? [];
  return deals.data.value?.data.map((d) => ({ id: d.id, label: d.title })) ?? [];
});
const loading = computed(
  () => companies.isFetching.value || contacts.isFetching.value || deals.isFetching.value,
);
const plural = computed(() => (props.type === "company" ? "companies" : `${props.type}s`));

watch(open, (isOpen) => {
  if (!isOpen) search.value = "";
});

function pick(option: PickedRecord) {
  model.value = option.id === model.value?.id ? null : option;
  open.value = false;
}
</script>

<template>
  <Popover v-model:open="open">
    <PopoverTrigger as-child>
      <Button
        :id="id"
        type="button"
        variant="outline"
        role="combobox"
        :aria-expanded="open"
        :aria-invalid="invalid || undefined"
        :disabled="disabled"
        :class="
          cn(
            'h-9 w-full justify-between rounded-md bg-ink-1 px-3 font-normal hover:bg-ink-1',
            !model && 'text-fg-3',
            invalid && 'border-danger/70',
          )
        "
      >
        <span class="truncate">{{ model?.label ?? placeholder }}</span>
        <ChevronsUpDown class="text-fg-3" />
      </Button>
    </PopoverTrigger>
    <PopoverContent align="start" class="w-(--reka-popover-trigger-width) min-w-64 p-0">
      <ListboxRoot :model-value="model?.id" highlight-on-hover class="flex flex-col">
        <div class="flex h-10 items-center gap-2 border-b border-line px-3">
          <Search class="size-4 shrink-0 text-fg-3" aria-hidden="true" />
          <ListboxFilter
            v-model="search"
            auto-focus
            :placeholder="`Search ${plural}`"
            class="h-full w-full bg-transparent text-sm text-fg outline-none placeholder:text-fg-3"
          />
          <Spinner v-if="loading" class="text-fg-3" />
        </div>
        <ListboxContent class="max-h-64 overflow-y-auto p-1 scroll-thin">
          <ListboxItem
            v-for="option in options"
            :key="option.id"
            :value="option.id"
            class="flex cursor-default items-center gap-2 rounded-sm px-2 py-1.5 text-sm text-fg-1 outline-none select-none data-[highlighted]:bg-ink-3 data-[highlighted]:text-fg"
            @select.prevent="pick(option)"
          >
            <Check :class="cn('size-4 shrink-0 text-accent', option.id === model?.id ? 'opacity-100' : 'opacity-0')" />
            <span class="truncate">{{ option.label }}</span>
          </ListboxItem>
          <p v-if="options.length === 0" class="px-2 py-6 text-center text-[13px] text-fg-3">
            {{ loading ? "Searching…" : "No matches." }}
          </p>
        </ListboxContent>
      </ListboxRoot>
    </PopoverContent>
  </Popover>
</template>
