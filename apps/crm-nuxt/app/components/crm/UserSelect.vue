<script setup lang="ts">
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { useUsers } from "@/features/users/queries";

const NONE = "__none";

/** Pick a teammate (owner or assignee). */
withDefaults(
  defineProps<{ id?: string; invalid?: boolean; allowEmpty?: boolean; emptyLabel?: string }>(),
  { emptyLabel: "Nobody" },
);
const model = defineModel<string | undefined>();
const users = useUsers();
</script>

<template>
  <Select
    :model-value="model ?? NONE"
    @update:model-value="(v) => (model = v === NONE || typeof v !== 'string' ? undefined : v)"
  >
    <SelectTrigger :id="id" class="w-full" :aria-invalid="invalid || undefined">
      <SelectValue placeholder="Choose a teammate" />
    </SelectTrigger>
    <SelectContent>
      <SelectGroup>
        <SelectItem v-if="allowEmpty" :value="NONE">{{ emptyLabel }}</SelectItem>
        <SelectItem v-for="u in users.data.value?.data ?? []" :key="u.id" :value="u.id">{{ u.name }}</SelectItem>
      </SelectGroup>
    </SelectContent>
  </Select>
</template>
