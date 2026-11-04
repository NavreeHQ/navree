<script setup lang="ts">
import { FileQuestion } from "@lucide/vue";
import { isNavreeError } from "@navree/client";
import { Button } from "@/components/ui/button";
import { Empty, EmptyContent, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from "@/components/ui/empty";

const props = defineProps<{ error: unknown; noun: string; back: { href: string; label: string } }>();
const notFound = computed(() => isNavreeError(props.error) && props.error.status === 404);
</script>

<template>
  <Empty class="surface mt-10 border">
    <EmptyHeader>
      <EmptyMedia variant="icon"><FileQuestion /></EmptyMedia>
      <EmptyTitle>{{ notFound ? `This ${noun} doesn't exist` : `This ${noun} couldn't be loaded` }}</EmptyTitle>
      <EmptyDescription>
        {{ notFound ? "It may have been deleted, or the link is wrong." : "Check your connection and refresh the page." }}
      </EmptyDescription>
    </EmptyHeader>
    <EmptyContent>
      <Button as-child variant="outline">
        <NuxtLink :to="back.href">{{ back.label }}</NuxtLink>
      </Button>
    </EmptyContent>
  </Empty>
</template>
