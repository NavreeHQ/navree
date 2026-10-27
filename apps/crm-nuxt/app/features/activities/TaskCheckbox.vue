<script setup lang="ts">
import { useAbility } from "@casl/vue";
import { toast } from "vue-sonner";
import { Checkbox } from "@/components/ui/checkbox";
import type { Activity } from "@/lib/api/types";
import type { AppAbility } from "@/lib/casl/ability";
import { useCompleteActivity } from "./queries";

/** Completes a task. Disabled where the API would refuse (CASL `complete`). */
const props = defineProps<{ activity: Activity; label?: string }>();
const ability = useAbility<AppAbility>();
const complete = useCompleteActivity();
const done = computed(() => Boolean(props.activity.completed_at));
const allowed = computed(() => ability.can("complete", props.activity));
const name = computed(() => props.label ?? props.activity.subject);

function onChange(checked: boolean | "indeterminate") {
  if (checked !== true || done.value) return;
  complete.mutate(props.activity.id, {
    onSuccess: () => toast.success(`Completed “${props.activity.subject}”`),
  });
}
</script>

<template>
  <Checkbox
    :model-value="done || complete.isPending.value"
    :disabled="done || !allowed || complete.isPending.value"
    :aria-label="done ? `${name} is complete` : `Complete ${name}`"
    :title="!allowed && !done ? 'Only the assignee, the task owner or a manager can complete it' : undefined"
    class="size-[18px]"
    @update:model-value="onChange"
  />
</template>
