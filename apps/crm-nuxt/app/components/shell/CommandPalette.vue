<script setup lang="ts">
import { useAbility } from "@casl/vue";
import { refDebounced } from "@vueuse/core";
import { Building2, CornerDownLeft, Plus, Search, User as UserIcon } from "@lucide/vue";
import { ListboxContent, ListboxFilter, ListboxGroup, ListboxGroupLabel, ListboxItem, ListboxRoot } from "reka-ui";
import Money from "@/components/crm/Money.vue";
import StageDot from "@/components/crm/StageDot.vue";
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "@/components/ui/dialog";
import { Spinner } from "@/components/ui/spinner";
import { useCompanies } from "@/features/companies/queries";
import { useContacts } from "@/features/contacts/queries";
import { useDeals } from "@/features/deals/queries";
import type { AppAbility } from "@/lib/casl/ability";
import { WORK_NAV } from "@/lib/navigation";
import { type CreateKind, useUiStore } from "@/stores/ui";

/**
 * The command palette (Ctrl K or ⌘ K): jump to a screen, create a record, or
 * search companies, contacts and deals on the API as you type.
 */
const ui = useUiStore();
const ability = useAbility<AppAbility>();
const query = ref("");
const term = refDebounced(computed(() => query.value.trim()), 200);
const searching = computed(() => ui.paletteOpen && term.value.length >= 2);
/** ListboxRoot is generic, so `InstanceType` can't name it; only `highlightFirstItem` is used. */
const listbox = ref<{ highlightFirstItem: () => void } | null>(null);

const companies = useCompanies(() => ({ search: term.value, per_page: 5 }), searching);
const contacts = useContacts(() => ({ search: term.value, per_page: 5 }), searching);
const deals = useDeals(() => ({ search: term.value, per_page: 5 }), searching);

const CREATE: { kind: CreateKind; label: string; subject: "Deal" | "Company" | "Contact" | "Activity"; keys: string }[] = [
  { kind: "deal", label: "New deal", subject: "Deal", keys: "C D" },
  { kind: "company", label: "New company", subject: "Company", keys: "C C" },
  { kind: "contact", label: "New contact", subject: "Contact", keys: "C P" },
  { kind: "task", label: "New task", subject: "Activity", keys: "C T" },
];

const needle = computed(() => query.value.trim().toLowerCase());
const pages = computed(() => WORK_NAV.filter((p) => !needle.value || p.label.toLowerCase().includes(needle.value)));
const actions = computed(() =>
  CREATE.filter((a) => ability.can("create", a.subject) && (!needle.value || a.label.toLowerCase().includes(needle.value))),
);

const companyHits = computed(() => (searching.value ? (companies.data.value?.data ?? []) : []));
const contactHits = computed(() => (searching.value ? (contacts.data.value?.data ?? []) : []));
const dealHits = computed(() => (searching.value ? (deals.data.value?.data ?? []) : []));
const loading = computed(
  () => searching.value && (companies.isFetching.value || contacts.isFetching.value || deals.isFetching.value),
);
const resultCount = computed(
  () => pages.value.length + actions.value.length + companyHits.value.length + contactHits.value.length + dealHits.value.length,
);

// Results arrive after the input event highlighted the first item: highlight it again.
watch([companyHits, contactHits, dealHits], () => nextTick(() => listbox.value?.highlightFirstItem()));
watch(
  () => ui.paletteOpen,
  (open) => {
    if (!open) query.value = "";
  },
);

function go(href: string) {
  ui.paletteOpen = false;
  void navigateTo(href);
}

const itemClass =
  "group/item flex h-10 cursor-default items-center gap-3 rounded-sm px-3 text-sm text-fg-1 outline-none select-none data-[highlighted]:bg-ink-3 data-[highlighted]:text-fg [&>svg]:size-4 [&>svg]:shrink-0 [&>svg]:text-fg-3 data-[highlighted]:[&>svg]:text-fg";
const labelClass = "px-3 pt-3 pb-1.5 text-xs text-fg-3";
</script>

<template>
  <Dialog v-model:open="ui.paletteOpen">
    <DialogContent
      :show-close-button="false"
      class="top-[min(12vh,7rem)] w-[min(40rem,calc(100vw-1.5rem))] max-w-none translate-y-0 gap-0 overflow-hidden p-0 sm:max-w-none"
    >
      <DialogTitle class="sr-only">Search Navree CRM</DialogTitle>
      <DialogDescription class="sr-only">Go to a screen, create a record, or search companies, contacts and deals.</DialogDescription>
      <ListboxRoot ref="listbox" highlight-on-hover class="flex flex-col">
        <div class="flex h-14 items-center gap-3 border-b border-line px-4">
          <Search class="size-5 shrink-0 text-fg-3" aria-hidden="true" />
          <ListboxFilter
            v-model="query"
            auto-focus
            aria-label="Search"
            placeholder="Search companies, contacts and deals, or jump to a screen"
            class="h-full w-full bg-transparent text-[15px] text-fg outline-none placeholder:text-fg-3"
          />
          <Spinner v-if="loading" class="text-fg-3" />
          <kbd class="kbd max-sm:hidden">Esc</kbd>
        </div>
        <ListboxContent class="max-h-[min(60vh,28rem)] overflow-y-auto p-2 scroll-thin">
          <ListboxGroup v-if="pages.length">
            <ListboxGroupLabel :class="labelClass">Go to</ListboxGroupLabel>
            <ListboxItem v-for="page in pages" :key="page.href" :value="`go:${page.href}`" :class="itemClass" @select.prevent="go(page.href)">
              <component :is="page.icon" />
              <span class="flex-1 truncate">{{ page.label }}</span>
              <span class="kbd opacity-0 group-data-[highlighted]/item:opacity-100">{{ page.shortcut }}</span>
            </ListboxItem>
          </ListboxGroup>
          <ListboxGroup v-if="actions.length">
            <ListboxGroupLabel :class="labelClass">Create</ListboxGroupLabel>
            <ListboxItem v-for="a in actions" :key="a.kind" :value="`create:${a.kind}`" :class="itemClass" @select.prevent="ui.create(a.kind)">
              <Plus />
              <span class="flex-1 truncate">{{ a.label }}</span>
              <span class="kbd opacity-0 group-data-[highlighted]/item:opacity-100">{{ a.keys }}</span>
            </ListboxItem>
          </ListboxGroup>
          <ListboxGroup v-if="companyHits.length">
            <ListboxGroupLabel :class="labelClass">Companies</ListboxGroupLabel>
            <ListboxItem v-for="c in companyHits" :key="c.id" :value="`company:${c.id}`" :class="itemClass" @select.prevent="go(`/companies/${c.id}`)">
              <Building2 />
              <span class="flex-1 truncate">{{ c.name }}</span>
              <span class="truncate text-xs text-fg-3">{{ c.domain }}</span>
            </ListboxItem>
          </ListboxGroup>
          <ListboxGroup v-if="contactHits.length">
            <ListboxGroupLabel :class="labelClass">Contacts</ListboxGroupLabel>
            <ListboxItem v-for="c in contactHits" :key="c.id" :value="`contact:${c.id}`" :class="itemClass" @select.prevent="go(`/contacts/${c.id}`)">
              <UserIcon />
              <span class="flex-1 truncate">{{ c.full_name }}</span>
              <span class="truncate text-xs text-fg-3">{{ c.company?.name }}</span>
            </ListboxItem>
          </ListboxGroup>
          <ListboxGroup v-if="dealHits.length">
            <ListboxGroupLabel :class="labelClass">Deals</ListboxGroupLabel>
            <ListboxItem v-for="d in dealHits" :key="d.id" :value="`deal:${d.id}`" :class="itemClass" @select.prevent="go(`/deals/${d.id}`)">
              <StageDot :stage="d.stage" class="mx-1" />
              <span class="flex-1 truncate">{{ d.title }}</span>
              <Money :cents="d.amount_cents" :currency="d.currency" compact class="text-xs text-fg-2" />
            </ListboxItem>
          </ListboxGroup>
          <p v-if="resultCount === 0" class="px-3 py-10 text-center text-sm text-fg-3">
            {{ loading || (needle.length >= 2 && term !== query.trim()) ? "Searching…" : `Nothing matches “${query.trim()}”.` }}
          </p>
        </ListboxContent>
      </ListboxRoot>
      <div class="flex items-center gap-4 border-t border-line px-4 py-2.5 text-xs text-fg-3 max-sm:hidden">
        <span class="inline-flex items-center gap-1.5"><kbd class="kbd">↑</kbd><kbd class="kbd">↓</kbd> to move</span>
        <span class="inline-flex items-center gap-1.5"><kbd class="kbd"><CornerDownLeft class="size-3" /></kbd> to open</span>
        <span class="ml-auto">Type two letters to search records</span>
      </div>
    </DialogContent>
  </Dialog>
</template>
