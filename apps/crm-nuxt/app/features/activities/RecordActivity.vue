<script setup lang="ts">
import type { ActivitySubjectType } from "@/lib/api/types";
import ActivityComposer from "./ActivityComposer.vue";
import ActivityTimeline from "./ActivityTimeline.vue";
import { useActivities } from "./queries";

/** A record's activity: the composer on top, the history below. */
const props = defineProps<{ subjectType: ActivitySubjectType; subjectId: string }>();
const activities = useActivities(() => ({
  filter: { subject_type: props.subjectType, subject_id: props.subjectId },
  per_page: 50,
}));
</script>

<template>
  <div class="flex flex-col gap-6">
    <ActivityComposer :subject-type="subjectType" :subject-id="subjectId" />
    <ActivityTimeline :activities="activities.data.value?.data" :loading="activities.isPending.value" />
  </div>
</template>
