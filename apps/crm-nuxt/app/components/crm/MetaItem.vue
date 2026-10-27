<script setup lang="ts">
import type { Component } from "vue";

/** One fact in a record header's meta line, with its icon. */
defineProps<{ icon: Component }>();
</script>

<template>
  <span class="inline-flex min-w-0 items-center gap-1.5">
    <component :is="icon" class="size-3.5 shrink-0 text-fg-3" aria-hidden="true" />
    <slot />
  </span>
</template>
