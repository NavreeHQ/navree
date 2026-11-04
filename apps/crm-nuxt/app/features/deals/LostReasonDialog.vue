<script setup lang="ts">
import { errorMessage, isNavreeError } from "@navree/client";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Field, FieldError, FieldLabel } from "@/components/ui/field";
import { Spinner } from "@/components/ui/spinner";
import { Textarea } from "@/components/ui/textarea";

/**
 * The API requires `lost_reason` when a deal moves to lost (400 otherwise), so
 * dropping a card on Lost, or picking Lost on the stage bar, asks for it first.
 */
const props = defineProps<{ open: boolean; dealTitle: string; pending?: boolean; error?: unknown }>();
const emit = defineEmits<{ cancel: []; confirm: [reason: string] }>();

const SUGGESTIONS = [
  "Went with a competitor",
  "No budget this year",
  "Price too high",
  "No decision made",
  "Timing: revisit next quarter",
];

const reason = ref("");
const touched = ref(false);

watch(
  () => props.open,
  (open) => {
    if (open) {
      reason.value = "";
      touched.value = false;
    }
  },
);

const serverError = computed(() =>
  isNavreeError(props.error) && props.error.isValidation
    ? (props.error.errors.lost_reason?.[0] ?? errorMessage(props.error))
    : undefined,
);
const otherError = computed(() => (props.error && !serverError.value ? errorMessage(props.error) : undefined));
const message = computed(() =>
  touched.value && reason.value.trim() === "" ? "Say why the deal was lost; it helps the team's reporting." : serverError.value,
);

function submit() {
  touched.value = true;
  if (reason.value.trim()) emit("confirm", reason.value.trim());
}
</script>

<template>
  <Dialog :open="open" @update:open="(next) => !next && emit('cancel')">
    <DialogContent class="sm:max-w-md">
      <form class="flex flex-col gap-4" novalidate @submit.prevent="submit">
        <DialogHeader class="text-left">
          <DialogTitle class="font-sans text-lg font-semibold tracking-[-0.01em]">Mark “{{ dealTitle }}” as lost</DialogTitle>
          <DialogDescription class="text-sm text-fg-2">
            Lost deals leave the open pipeline. Add the reason so it shows up in reports.
          </DialogDescription>
        </DialogHeader>
        <Alert v-if="otherError" variant="destructive">
          <AlertDescription>{{ otherError }}</AlertDescription>
        </Alert>
        <Field :data-invalid="message ? true : undefined">
          <FieldLabel for="lost-reason">Reason</FieldLabel>
          <Textarea
            id="lost-reason"
            v-model="reason"
            autofocus
            rows="3"
            placeholder="What made them say no?"
            :aria-invalid="message ? true : undefined"
          />
          <FieldError v-if="message">{{ message }}</FieldError>
        </Field>
        <div class="flex flex-wrap gap-1.5" role="group" aria-label="Common reasons">
          <Button v-for="s in SUGGESTIONS" :key="s" type="button" variant="outline" size="xs" @click="reason = s">{{ s }}</Button>
        </div>
        <DialogFooter class="gap-2">
          <Button type="button" variant="ghost" @click="emit('cancel')">Keep it open</Button>
          <Button type="submit" variant="destructive" :disabled="pending">
            <Spinner v-if="pending" />
            Mark as lost
          </Button>
        </DialogFooter>
      </form>
    </DialogContent>
  </Dialog>
</template>
