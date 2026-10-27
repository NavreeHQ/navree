<script setup lang="ts">
/** Holds the real-time connection and the keyboard shortcuts for as long as the shell is mounted. */
const props = defineProps<{ userId: string }>();
useRealtime(props.userId);
useShortcuts();
</script>

<template>
  <span hidden />
</template>
