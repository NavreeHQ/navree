<script setup lang="ts">
import { useField, useForm } from "vee-validate";
import { toast } from "vue-sonner";
import { z } from "zod";
import FormDialog from "@/components/crm/FormDialog.vue";
import RecordCombobox from "@/components/crm/RecordCombobox.vue";
import TextField from "@/components/crm/TextField.vue";
import type { PickedRecord } from "@/components/crm/types";
import UserSelect from "@/components/crm/UserSelect.vue";
import { Field, FieldError, FieldLabel } from "@/components/ui/field";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { useCurrentUser } from "@/features/auth/queries";
import { applyFieldErrors } from "@/lib/api/form-errors";
import type { ActivitySubjectType } from "@/lib/api/types";
import { todayIso } from "@/lib/format";
import { toTypedSchema } from "@/lib/forms/zod-schema";
import { useCreateActivity } from "./queries";

/** A new task from the Tasks page or the palette: a task always hangs off a deal, company or contact. */
const open = defineModel<boolean>("open", { default: false });

const schema = z.object({
  subject: z.string().trim().min(1, "Say what needs doing.").max(255),
  record_type: z.enum(["deal", "company", "contact"]),
  record: z
    .object({ id: z.string(), label: z.string() })
    .nullable()
    .refine((v) => v !== null, "Choose what this task is about."),
  due_at: z.string().min(1, "Pick a due date."),
  assignee_id: z.string().optional(),
  body: z.string(),
});

const TYPE_LABEL: Record<ActivitySubjectType, string> = { deal: "Deal", company: "Company", contact: "Contact" };

const me = useCurrentUser();
const create = useCreateActivity();
const rootError = ref<string | null>(null);

function defaults() {
  return {
    subject: "",
    record_type: "deal" as ActivitySubjectType,
    record: null as PickedRecord | null,
    due_at: `${todayIso()}T17:00`,
    assignee_id: me.data.value?.id,
    body: "",
  };
}

const form = useForm({ validationSchema: toTypedSchema(schema), initialValues: defaults() });
const { value: recordType } = useField<ActivitySubjectType>("record_type");
const { value: record, errorMessage: recordError } = useField<PickedRecord | null>("record");
const { value: assigneeId, errorMessage: assigneeError } = useField<string | undefined>("assignee_id");

watch(open, (isOpen) => {
  if (!isOpen) return;
  form.resetForm({ values: defaults() });
  rootError.value = null;
});

function setType(value: unknown) {
  if (typeof value !== "string" || !value) return;
  recordType.value = value as ActivitySubjectType;
  record.value = null;
}

const onSubmit = form.handleSubmit((values) => {
  rootError.value = null;
  create.mutate(
    {
      kind: "task",
      subject: values.subject,
      body: values.body.trim() || null,
      due_at: new Date(values.due_at).toISOString(),
      subject_type: values.record_type,
      subject_id: values.record!.id,
      assignee_id: values.assignee_id ?? me.data.value?.id ?? null,
    },
    {
      onSuccess: () => {
        toast.success("Task added");
        open.value = false;
      },
      onError: (error) => {
        const root = applyFieldErrors(error, form.setFieldError, {
          subject: "subject",
          subject_id: "record",
          subject_type: "record",
          due_at: "due_at",
          assignee_id: "assignee_id",
          body: "body",
        });
        if (root) rootError.value = root;
      },
    },
  );
});
</script>

<template>
  <FormDialog
    v-model:open="open"
    title="New task"
    description="Tasks show up for their assignee under Today, Upcoming or Overdue."
    submit-label="Add task"
    :pending="create.isPending.value"
    :root-error="rootError"
    @submit="onSubmit"
  >
    <template v-if="$slots.trigger" #trigger><slot name="trigger" /></template>
    <TextField name="subject" label="Task" required autofocus placeholder="Send revised proposal" />
    <Field :data-invalid="recordError ? true : undefined">
      <FieldLabel for="task-record">Related to <span class="font-normal text-fg-3">(required)</span></FieldLabel>
      <div class="flex flex-col gap-2 sm:flex-row sm:items-center">
        <ToggleGroup type="single" size="sm" :model-value="recordType" aria-label="Record type" class="shrink-0" @update:model-value="setType">
          <ToggleGroupItem v-for="t in ['deal', 'company', 'contact'] as const" :key="t" :value="t">{{ TYPE_LABEL[t] }}</ToggleGroupItem>
        </ToggleGroup>
        <RecordCombobox
          id="task-record"
          v-model="record"
          :type="recordType"
          :placeholder="`Choose a ${TYPE_LABEL[recordType].toLowerCase()}`"
          :invalid="Boolean(recordError)"
        />
      </div>
      <FieldError v-if="recordError">{{ recordError }}</FieldError>
    </Field>
    <div class="grid gap-5 sm:grid-cols-2">
      <TextField name="due_at" label="Due" type="datetime-local" required />
      <Field :data-invalid="assigneeError ? true : undefined">
        <FieldLabel for="task-assignee">Assignee</FieldLabel>
        <UserSelect id="task-assignee" v-model="assigneeId" :invalid="Boolean(assigneeError)" />
        <FieldError v-if="assigneeError">{{ assigneeError }}</FieldError>
      </Field>
    </div>
    <TextField name="body" label="Notes" multiline />
  </FormDialog>
</template>
