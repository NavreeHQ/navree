<script setup lang="ts">
import { useAbility } from "@casl/vue";
import { useField, useForm } from "vee-validate";
import { toast } from "vue-sonner";
import { z } from "zod";
import UserSelect from "@/components/crm/UserSelect.vue";
import { Button } from "@/components/ui/button";
import { Field, FieldError, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { Spinner } from "@/components/ui/spinner";
import { Textarea } from "@/components/ui/textarea";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { useCurrentUser } from "@/features/auth/queries";
import { applyFieldErrors } from "@/lib/api/form-errors";
import type { ActivitySubjectType } from "@/lib/api/types";
import type { AppAbility } from "@/lib/casl/ability";
import { todayIso } from "@/lib/format";
import { toTypedSchema } from "@/lib/forms/zod-schema";
import { KIND_META } from "./kinds";
import { useCreateActivity } from "./queries";

/** "Add note / Log call / Add task" on a company, contact or deal. */
const props = defineProps<{ subjectType: ActivitySubjectType; subjectId: string }>();

const KINDS = ["note", "call", "task"] as const;
type ComposerKind = (typeof KINDS)[number];
const SUBMIT: Record<ComposerKind, string> = { note: "Add note", call: "Log call", task: "Add task" };
const PLACEHOLDER: Record<ComposerKind, string> = {
  note: "What happened?",
  call: "Who did you speak with, and about what?",
  task: "What needs doing?",
};

const schema = z
  .object({
    kind: z.enum(KINDS),
    subject: z.string().trim().min(1, "Add a short summary.").max(255),
    body: z.string(),
    due_at: z.string(),
    assignee_id: z.string().optional(),
  })
  .superRefine((v, ctx) => {
    if (v.kind === "task" && !v.due_at) ctx.addIssue({ code: "custom", path: ["due_at"], message: "Pick a due date." });
  });

const ability = useAbility<AppAbility>();
const me = useCurrentUser();
const create = useCreateActivity();
const rootError = ref<string | null>(null);

function defaults(kind: ComposerKind = "note") {
  return { kind, subject: "", body: "", due_at: `${todayIso()}T17:00`, assignee_id: me.data.value?.id };
}

const form = useForm({ validationSchema: toTypedSchema(schema), initialValues: defaults() });
const { value: kind } = useField<ComposerKind>("kind");
const { value: subject, errorMessage: subjectError } = useField<string>("subject");
const { value: body } = useField<string>("body");
const { value: dueAt, errorMessage: dueError } = useField<string>("due_at");
const { value: assigneeId } = useField<string | undefined>("assignee_id");

const ids = computed(() => ({
  subject: `composer-subject-${props.subjectId}`,
  body: `composer-body-${props.subjectId}`,
  due: `composer-due-${props.subjectId}`,
  assignee: `composer-assignee-${props.subjectId}`,
}));

const onSubmit = form.handleSubmit((values) => {
  rootError.value = null;
  create.mutate(
    {
      kind: values.kind,
      subject: values.subject,
      body: values.body.trim() || null,
      due_at: values.kind === "task" ? new Date(values.due_at).toISOString() : null,
      assignee_id: values.kind === "task" ? (values.assignee_id ?? me.data.value?.id ?? null) : null,
      subject_type: props.subjectType,
      subject_id: props.subjectId,
    },
    {
      onSuccess: () => {
        toast.success(values.kind === "task" ? "Task added" : values.kind === "call" ? "Call logged" : "Note added");
        form.resetForm({ values: defaults(values.kind) });
      },
      onError: (error) => {
        const root = applyFieldErrors(error, form.setFieldError, ["subject", "body", "due_at", "assignee_id"]);
        if (root) rootError.value = root;
      },
    },
  );
});
</script>

<template>
  <form
    v-if="ability.can('create', 'Activity')"
    novalidate
    aria-label="Add activity"
    class="flex flex-col gap-3 rounded-lg border border-line bg-ink-0/60 p-3"
    @submit.prevent="onSubmit"
  >
    <ToggleGroup
      type="single"
      size="sm"
      :model-value="kind"
      aria-label="Activity type"
      class="self-start"
      @update:model-value="(v) => v && (kind = v as ComposerKind)"
    >
      <ToggleGroupItem v-for="k in KINDS" :key="k" :value="k" :aria-label="KIND_META[k].label">
        <component :is="KIND_META[k].icon" />
        {{ KIND_META[k].label }}
      </ToggleGroupItem>
    </ToggleGroup>
    <Field :data-invalid="subjectError ? true : undefined">
      <FieldLabel :for="ids.subject" class="sr-only">Summary</FieldLabel>
      <Input :id="ids.subject" v-model="subject" :placeholder="PLACEHOLDER[kind]" :aria-invalid="subjectError ? true : undefined" />
      <FieldError v-if="subjectError">{{ subjectError }}</FieldError>
    </Field>
    <Field>
      <FieldLabel :for="ids.body" class="sr-only">Details</FieldLabel>
      <Textarea :id="ids.body" v-model="body" placeholder="Details (optional)" rows="2" class="min-h-14" />
    </Field>
    <div v-if="kind === 'task'" class="grid gap-3 sm:grid-cols-2">
      <Field :data-invalid="dueError ? true : undefined">
        <FieldLabel :for="ids.due">Due</FieldLabel>
        <Input :id="ids.due" v-model="dueAt" type="datetime-local" :aria-invalid="dueError ? true : undefined" />
        <FieldError v-if="dueError">{{ dueError }}</FieldError>
      </Field>
      <Field>
        <FieldLabel :for="ids.assignee">Assignee</FieldLabel>
        <UserSelect :id="ids.assignee" v-model="assigneeId" />
      </Field>
    </div>
    <p v-if="rootError" class="text-[13px] text-danger">{{ rootError }}</p>
    <div class="flex justify-end">
      <Button type="submit" size="sm" :disabled="create.isPending.value">
        <Spinner v-if="create.isPending.value" />
        {{ SUBMIT[kind] }}
      </Button>
    </div>
  </form>
</template>
