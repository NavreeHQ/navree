"use client";

import { zodResolver } from "@hookform/resolvers/zod";
import { useState, type ReactNode } from "react";
import { Controller, useForm, useWatch } from "react-hook-form";
import { toast } from "sonner";
import { z } from "zod";
import { FormDialog } from "@/components/form-dialog";
import { TextField } from "@/components/form-fields";
import { RecordCombobox, UserSelect, type PickedRecord } from "@/components/record-pickers";
import { Field, FieldError, FieldGroup, FieldLabel } from "@/components/ui/field";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { useCurrentUser } from "@/features/auth/queries";
import { useCreateIntent } from "@/hooks/use-create-intent";
import { applyFieldErrors } from "@/lib/api/form-errors";
import type { ActivitySubjectType } from "@/lib/api/types";
import { todayIso } from "@/lib/format";
import { useCreateActivity } from "./queries";

const schema = z
  .object({
    subject: z.string().trim().min(1, "Say what needs doing.").max(255),
    record_type: z.enum(["deal", "company", "contact"]),
    record: z.object({ id: z.string(), label: z.string() }).nullable(),
    due_at: z.string().min(1, "Pick a due date."),
    assignee_id: z.string().optional(),
    body: z.string(),
  })
  .superRefine((v, ctx) => {
    if (!v.record) ctx.addIssue({ code: "custom", path: ["record"], message: "Choose what this task is about." });
  });
type Values = z.infer<typeof schema>;

const TYPE_LABEL: Record<ActivitySubjectType, string> = { deal: "Deal", company: "Company", contact: "Contact" };

function defaults(userId?: string): Values {
  return {
    subject: "",
    record_type: "deal",
    record: null,
    due_at: `${todayIso()}T17:00`,
    assignee_id: userId,
    body: "",
  };
}

/** New task from the Tasks page: a task always hangs off a deal, company or contact. */
export function TaskFormDialog({
  trigger,
  openOnCreateIntent,
}: {
  trigger: ReactNode;
  /** Open when the command menu asks for a new task (set on one dialog per screen). */
  openOnCreateIntent?: boolean;
}) {
  const [open, setOpen] = useState(false);
  const me = useCurrentUser();
  const create = useCreateActivity();
  const form = useForm<Values>({ resolver: zodResolver(schema), defaultValues: defaults(me.data?.id) });
  const recordType = useWatch({ control: form.control, name: "record_type" });
  const onOpenChange = (next: boolean) => {
    if (next) form.reset(defaults(me.data?.id));
    setOpen(next);
  };
  useCreateIntent(openOnCreateIntent ? "task" : undefined, () => onOpenChange(true));

  const onSubmit = form.handleSubmit((values) => {
    create.mutate(
      {
        kind: "task",
        subject: values.subject,
        body: values.body.trim() || null,
        due_at: new Date(values.due_at).toISOString(),
        subject_type: values.record_type,
        subject_id: values.record!.id,
        assignee_id: values.assignee_id ?? me.data?.id ?? null,
      },
      {
        onSuccess: () => {
          toast.success("Task added");
          setOpen(false);
        },
        onError: (error) =>
          applyFieldErrors(
            error,
            (name, err) =>
              form.setError(name === "subject_id" || name === "subject_type" ? "record" : (name as keyof Values), err),
            ["subject", "subject_id", "subject_type", "due_at", "assignee_id", "body"],
          ),
      },
    );
  });

  return (
    <FormDialog
      open={open}
      onOpenChange={onOpenChange}
      trigger={trigger}
      title="New task"
      description="Tasks show up for their assignee under Today, Upcoming or Overdue."
      submitLabel="Add task"
      pending={create.isPending}
      rootError={form.formState.errors.root?.server?.message}
      onSubmit={onSubmit}
    >
      <FieldGroup className="gap-4">
        <TextField
          control={form.control}
          name="subject"
          label="Task"
          required
          autoFocus
          placeholder="Send revised proposal"
        />
        <Controller
          control={form.control}
          name="record"
          render={({ field, fieldState }) => (
            <Field data-invalid={fieldState.invalid || undefined}>
              <FieldLabel htmlFor="task-record">
                Related to <span className="text-muted-foreground">(required)</span>
              </FieldLabel>
              <div className="flex flex-col gap-2 sm:flex-row">
                <ToggleGroup
                  type="single"
                  variant="outline"
                  size="sm"
                  value={recordType}
                  onValueChange={(v) => {
                    if (!v) return;
                    form.setValue("record_type", v as ActivitySubjectType);
                    field.onChange(null);
                  }}
                  aria-label="Record type"
                  className="shrink-0"
                >
                  {(["deal", "company", "contact"] as const).map((t) => (
                    <ToggleGroupItem key={t} value={t} className="px-2.5">
                      {TYPE_LABEL[t]}
                    </ToggleGroupItem>
                  ))}
                </ToggleGroup>
                <RecordCombobox
                  id="task-record"
                  type={recordType}
                  value={field.value as PickedRecord | null}
                  onChange={field.onChange}
                  placeholder={`Choose a ${TYPE_LABEL[recordType].toLowerCase()}`}
                  invalid={fieldState.invalid}
                />
              </div>
              <FieldError errors={[fieldState.error]} />
            </Field>
          )}
        />
        <div className="grid gap-4 sm:grid-cols-2">
          <TextField control={form.control} name="due_at" label="Due" type="datetime-local" required />
          <Controller
            control={form.control}
            name="assignee_id"
            render={({ field, fieldState }) => (
              <Field data-invalid={fieldState.invalid || undefined}>
                <FieldLabel htmlFor="task-assignee">Assignee</FieldLabel>
                <UserSelect
                  id="task-assignee"
                  value={field.value}
                  onChange={field.onChange}
                  invalid={fieldState.invalid}
                />
                <FieldError errors={[fieldState.error]} />
              </Field>
            )}
          />
        </div>
        <TextField control={form.control} name="body" label="Notes" multiline />
      </FieldGroup>
    </FormDialog>
  );
}
