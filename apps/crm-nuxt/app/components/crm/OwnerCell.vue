<script setup lang="ts">
import UserAvatar from "./UserAvatar.vue";

defineProps<{ owner: { id: string; name: string } | null | undefined }>();
</script>

<template>
  <span v-if="!owner" class="text-fg-3">Unassigned</span>
  <span v-else class="inline-flex min-w-0 items-center gap-2">
    <UserAvatar :name="owner.name" :id="owner.id" size="xs" />
    <span class="truncate">{{ owner.name }}</span>
  </span>
</template>
