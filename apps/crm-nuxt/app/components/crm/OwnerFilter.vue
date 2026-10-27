<script setup lang="ts">
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectSeparator,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { useUsers } from "@/features/users/queries";
import { cn } from "@/lib/utils";

const ALL = "__all";

/** Filter by owner (or any person); "Mine" comes first. */
const props = withDefaults(
  defineProps<{ value: string | undefined; currentUserId?: string; label?: string }>(),
  { label: "Owner" },
);
const emit = defineEmits<{ change: [value: string | undefined] }>();
const users = useUsers();
const others = computed(() => users.data.value?.data.filter((u) => u.id !== props.currentUserId) ?? []);
</script>

<template>
  <Select
    :model-value="value ?? ALL"
    @update:model-value="(v) => emit('change', v === ALL || typeof v !== 'string' ? undefined : v)"
  >
    <SelectTrigger
      size="sm"
      :aria-label="`Filter by ${label.toLowerCase()}`"
      :class="cn('h-8 min-w-36 text-[13px]', value && 'border-fg-3/60 bg-ink-2 text-fg')"
    >
      <SelectValue :placeholder="`Any ${label.toLowerCase()}`" />
    </SelectTrigger>
    <SelectContent>
      <SelectGroup>
        <SelectItem :value="ALL">Any {{ label.toLowerCase() }}</SelectItem>
        <SelectItem v-if="currentUserId" :value="currentUserId">Mine</SelectItem>
      </SelectGroup>
      <SelectSeparator />
      <SelectGroup>
        <SelectItem v-for="u in others" :key="u.id" :value="u.id">{{ u.name }}</SelectItem>
      </SelectGroup>
    </SelectContent>
  </Select>
</template>
