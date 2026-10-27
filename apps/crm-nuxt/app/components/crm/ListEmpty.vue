<script setup lang="ts">
import { SearchX } from "@lucide/vue";
import { Button } from "@/components/ui/button";
import { Empty, EmptyContent, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from "@/components/ui/empty";

/**
 * An empty list: "nothing yet" (with the create action in the `action` slot and
 * the resource's icon in `icon`) or "nothing matches" (with a way back).
 */
defineProps<{ filtered: boolean; noun: string }>();
const emit = defineEmits<{ clear: [] }>();
</script>

<template>
  <Empty class="py-14">
    <EmptyHeader>
      <EmptyMedia variant="icon">
        <SearchX v-if="filtered" />
        <slot v-else name="icon" />
      </EmptyMedia>
      <EmptyTitle>{{ filtered ? `No ${noun} match these filters` : `No ${noun} yet` }}</EmptyTitle>
      <EmptyDescription>
        {{ filtered ? "Try a different search, or clear the filters to see everything." : "Add the first one to get started." }}
      </EmptyDescription>
    </EmptyHeader>
    <EmptyContent>
      <Button v-if="filtered" variant="outline" size="sm" @click="emit('clear')">Clear filters</Button>
      <slot v-else name="action" />
    </EmptyContent>
  </Empty>
</template>
