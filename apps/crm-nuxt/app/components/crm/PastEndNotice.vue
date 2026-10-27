<script setup lang="ts">
import { SearchX } from "@lucide/vue";
import { Button } from "@/components/ui/button";
import { Empty, EmptyContent, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from "@/components/ui/empty";

/** A page number past the last page (an old link, or a list that shrank) is not an empty list. */
defineProps<{ noun: string; total: number }>();
const emit = defineEmits<{ first: [] }>();
</script>

<template>
  <Empty class="py-14">
    <EmptyHeader>
      <EmptyMedia variant="icon"><SearchX /></EmptyMedia>
      <EmptyTitle>This page is past the end of the list</EmptyTitle>
      <EmptyDescription>There are {{ total }} {{ noun }} in all. Go back to the first page to see them.</EmptyDescription>
    </EmptyHeader>
    <EmptyContent>
      <Button variant="outline" size="sm" @click="emit('first')">Go to the first page</Button>
    </EmptyContent>
  </Empty>
</template>
