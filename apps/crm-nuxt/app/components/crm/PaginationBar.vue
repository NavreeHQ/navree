<script setup lang="ts">
import { ChevronLeft, ChevronRight } from "@lucide/vue";
import { Button } from "@/components/ui/button";
import type { PageMeta } from "@/lib/api/types";
import { formatNumber } from "@/lib/format";

/** "21–40 of 52 companies" with previous and next. Pages are 1-based like the API. */
const props = defineProps<{ meta: PageMeta | undefined; noun: string }>();
const emit = defineEmits<{ page: [page: number] }>();

const summary = computed(() => {
  const meta = props.meta;
  if (!meta) return "";
  if (meta.current_page > meta.last_page) {
    return `${formatNumber(meta.total)} ${props.noun} on ${formatNumber(meta.last_page)} ${meta.last_page === 1 ? "page" : "pages"}`;
  }
  const first = (meta.current_page - 1) * meta.per_page + 1;
  const last = Math.min(meta.total, meta.current_page * meta.per_page);
  const range = first === last ? formatNumber(first) : `${formatNumber(first)}–${formatNumber(last)}`;
  return `${range} of ${formatNumber(meta.total)} ${props.noun}`;
});
const showControls = computed(
  () => props.meta !== undefined && (props.meta.last_page > 1 || props.meta.current_page > props.meta.last_page),
);
</script>

<template>
  <nav
    v-if="meta && meta.total > 0"
    aria-label="Pagination"
    class="flex items-center justify-between gap-4 border-t border-line px-4 py-2.5 text-[13px]"
  >
    <p class="text-fg-3 figures">{{ summary }}</p>
    <div v-if="showControls" class="flex items-center gap-1">
      <Button
        variant="ghost"
        size="sm"
        :disabled="meta.current_page <= 1"
        @click="emit('page', Math.min(meta.current_page - 1, meta.last_page))"
      >
        <ChevronLeft />
        <span class="max-sm:sr-only">Previous</span>
      </Button>
      <span class="px-1 text-fg-3 figures">{{ meta.current_page }} / {{ meta.last_page }}</span>
      <Button variant="ghost" size="sm" :disabled="meta.current_page >= meta.last_page" @click="emit('page', meta.current_page + 1)">
        <span class="max-sm:sr-only">Next</span>
        <ChevronRight />
      </Button>
    </div>
  </nav>
</template>
