"use client";

import { Can } from "@navree/next/abilities";
import { zodResolver } from "@hookform/resolvers/zod";
import {
  CalendarDaysIcon,
  CheckSquareIcon,
  MailIcon,
  PhoneIcon,
  StickyNoteIcon,
  Trash2Icon,
  type LucideIcon,
} from "lucide-react";
import Link from "next/link";
import { Controller, useForm, useWatch } from "react-hook-form";
import { toast } from "sonner";
import { z } from "zod";
import { ConfirmDialog } from "@/components/confirm-dialog";
import { UserSelect } from "@/components/record-pickers";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Field, FieldError, FieldGroup, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import { Spinner } from "@/components/ui/spinner";
import { Textarea } from "@/components/ui/textarea";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { useCurrentUser } from "@/features/auth/queries";
import { applyFieldErrors } from "@/lib/api/form-errors";
import type { Activity, ActivityKind, ActivitySubjectType } from "@/lib/api/types";
import { dueBucket, formatDateTime, formatRelative, todayIso } from "@/lib/format";
import { cn } from "@/lib/utils";
import { useActivities, useCompleteActivity, useCreateActivity, useDeleteActivity } from "./queries";

export const KIND_META: Record<ActivityKind, { label: string; icon: LucideIcon }> = {
  note: { label: "Note", icon: StickyNoteIcon },
  call: { label: "Call", icon: PhoneIcon },
  email: { label: "Email", icon: MailIcon },
  meeting: { label: "Meeting", icon: CalendarDaysIcon },
  task: { label: "Task", icon: CheckSquareIcon },
};

export function subjectHref(type: ActivitySubjectType, id: string): string {
  return type === "company" ? `/companies/${id}` : type === "contact" ? `/contacts/${id}` : `/deals/${id}`;
}

export function ActivityKindIcon({ kind, className }: { kind: ActivityKind; className?: string }) {
  const Icon = KIND_META[kind].icon;
  return (
    <span
      className={cn(
        "flex size-7 shrink-0 items-center justify-center rounded-lg border border-line-2 bg-ink-2 text-fg-2",
        kind === "task" && "text-fg-1",
        className,
      )}
      aria-hidden="true"
    >
      <Icon className="size-3.5" />
    </span>
  );
}

/** Due date text with overdue emphasis. */
export function DueLabel({ activity }: { activity: Activity }) {
  if (!activity.due_at) return null;
  if (activity.kind !== "task") {
    // Meetings and calls carry their scheduled time in due_at; they are never "overdue".
    return (
      <span className="text-xs text-muted-foreground figures">Scheduled for {formatDateTime(activity.due_at)}</span>
    );
  }
  const bucket = activity.completed_at ? null : dueBucket(activity.due_at);
  return (
    <span
      className={cn(
        "text-xs figures",
        bucket === "overdue"
          ? "font-medium text-destructive"
          : bucket === "today"
            ? "font-medium text-warning"
            : "text-muted-foreground",
      )}
    >
      {bucket === "overdue" ? "Overdue, " : "Due "}
      {formatDateTime(activity.due_at)}
    </span>
  );
}

/** Complete checkbox for a task; disabled when the API would refuse (CASL `complete`). */
export function TaskCheckbox({ activity, label }: { activity: Activity; label?: string }) {
  const complete = useCompleteActivity();
  const done = Boolean(activity.completed_at);
  return (
    <Can I="complete" this={activity} passThrough>
      {({ isAllowed }) => (
        <Checkbox
          checked={done || complete.isPending}
          disabled={done || !isAllowed || complete.isPending}
          aria-label={done ? `${label ?? activity.subject} is complete` : `Complete ${label ?? activity.subject}`}
          title={!isAllowed && !done ? "Only the assignee, the task's owner or a manager can complete it" : undefined}
          onCheckedChange={(checked) => {
            if (checked === true && !done) {
              complete.mutate(activity.id, {
                onSuccess: () => toast.success(`Completed “${activity.subject}”`),
              });
            }
          }}
          className="size-4.5"
        />
      )}
    </Can>
  );
}

function TimelineItem({ activity, showSubject }: { activity: Activity; showSubject?: boolean }) {
  const remove = useDeleteActivity();
  return (
    <li className="group relative flex gap-3 pb-5 last:pb-0">
      <span aria-hidden="true" className="absolute top-8 bottom-0 left-3.5 w-px bg-border group-last:hidden" />
      {activity.kind === "task" ? (
        <span className="flex size-7 shrink-0 items-center justify-center">
          <TaskCheckbox activity={activity} />
        </span>
      ) : (
        <ActivityKindIcon kind={activity.kind} />
      )}
      <div className="flex min-w-0 flex-1 flex-col gap-1 pt-0.5">
        <div className="flex items-start justify-between gap-2">
          <p
            className={cn(
              "text-sm leading-snug font-medium",
              activity.completed_at && "text-muted-foreground line-through",
            )}
          >
            {activity.subject}
          </p>
          <Can I="delete" this={activity}>
            <ConfirmDialog
              trigger={
                <Button
                  variant="ghost"
                  size="icon-xs"
                  className="-my-1 opacity-0 group-focus-within:opacity-100 group-hover:opacity-100"
                  aria-label={`Delete ${activity.subject}`}
                >
                  <Trash2Icon />
                </Button>
              }
              title="Delete this activity?"
              description={`“${activity.subject}” will be removed from the timeline.`}
              confirmLabel="Delete activity"
              pending={remove.isPending}
              onConfirm={() => remove.mutate(activity.id, { onSuccess: () => toast.success("Activity deleted") })}
            />
          </Can>
        </div>
        {activity.body ? (
          <p className="text-sm leading-relaxed whitespace-pre-line text-muted-foreground">{activity.body}</p>
        ) : null}
        <p className="flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-muted-foreground">
          <span>
            {KIND_META[activity.kind].label} by {activity.owner.name}
          </span>
          <time dateTime={activity.created_at} title={formatDateTime(activity.created_at)}>
            {formatRelative(activity.created_at)}
          </time>
          {activity.kind === "task" && activity.assignee ? <span>Assigned to {activity.assignee.name}</span> : null}
          <DueLabel activity={activity} />
          {showSubject ? (
            <Link href={subjectHref(activity.subject_type, activity.subject_id)} className="record-link">
              {activity.subject_label}
            </Link>
          ) : null}
        </p>
      </div>
    </li>
  );
}

export function ActivityTimeline({
  activities,
  loading,
  showSubject,
  emptyText = "No activity yet. Log a call or add a note to start the history.",
}: {
  activities: Activity[] | undefined;
  loading?: boolean;
  showSubject?: boolean;
  emptyText?: string;
}) {
  if (loading) {
    return (
      <div className="flex flex-col gap-4">
        {Array.from({ length: 3 }, (_, i) => (
          <div key={i} className="flex gap-3">
            <Skeleton className="size-7" />
            <div className="flex flex-1 flex-col gap-2">
              <Skeleton className="h-4 w-1/2" />
              <Skeleton className="h-3 w-1/3" />
            </div>
          </div>
        ))}
      </div>
    );
  }
  if (!activities || activities.length === 0) {
    return (
      <p className="rounded-xl border border-dashed border-line-2 px-4 py-6 text-center text-sm text-fg-3">
        {emptyText}
      </p>
    );
  }
  return (
    <ol className="flex flex-col">
      {activities.map((a) => (
        <TimelineItem key={a.id} activity={a} showSubject={showSubject} />
      ))}
    </ol>
  );
}

const COMPOSER_KINDS = ["note", "call", "task"] as const;
type ComposerKind = (typeof COMPOSER_KINDS)[number];

const composerSchema = z
  .object({
    kind: z.enum(COMPOSER_KINDS),
    subject: z.string().trim().min(1, "Add a short summary.").max(255),
    body: z.string(),
    due_at: z.string(),
    assignee_id: z.string().optional(),
  })
  .superRefine((v, ctx) => {
    if (v.kind === "task" && !v.due_at) ctx.addIssue({ code: "custom", path: ["due_at"], message: "Pick a due date." });
  });
type ComposerValues = z.infer<typeof composerSchema>;

const SUBMIT_LABEL: Record<ComposerKind, string> = { note: "Add note", call: "Log call", task: "Add task" };
const SUBJECT_PLACEHOLDER: Record<ComposerKind, string> = {
  note: "What happened?",
  call: "Who did you speak with, and about what?",
  task: "What needs doing?",
};

function defaultDue(): string {
  return `${todayIso()}T17:00`;
}

/** "Add note / Log call / Add task" on a company, contact or deal. */
export function ActivityComposer({ subjectType, subjectId }: { subjectType: ActivitySubjectType; subjectId: string }) {
  const me = useCurrentUser();
  const create = useCreateActivity();
  const form = useForm<ComposerValues>({
    resolver: zodResolver(composerSchema),
    defaultValues: { kind: "note", subject: "", body: "", due_at: defaultDue(), assignee_id: me.data?.id },
  });
  const kind = useWatch({ control: form.control, name: "kind" });

  const onSubmit = form.handleSubmit((values) => {
    create.mutate(
      {
        kind: values.kind,
        subject: values.subject,
        body: values.body.trim() || null,
        due_at: values.kind === "task" ? new Date(values.due_at).toISOString() : null,
        assignee_id: values.kind === "task" ? (values.assignee_id ?? me.data?.id ?? null) : null,
        subject_type: subjectType,
        subject_id: subjectId,
      },
      {
        onSuccess: () => {
          toast.success(values.kind === "task" ? "Task added" : values.kind === "call" ? "Call logged" : "Note added");
          form.reset({ kind: values.kind, subject: "", body: "", due_at: defaultDue(), assignee_id: me.data?.id });
        },
        onError: (error) => applyFieldErrors(error, form.setError, ["subject", "body", "due_at", "assignee_id"]),
      },
    );
  });

  return (
    <Can I="create" a="Activity">
      <form
        onSubmit={onSubmit}
        className="flex flex-col gap-3 rounded-xl border bg-ink-0/50 p-3"
        aria-label="Add activity"
      >
        <Controller
          control={form.control}
          name="kind"
          render={({ field }) => (
            <ToggleGroup
              type="single"
              size="sm"
              variant="outline"
              value={field.value}
              onValueChange={(v) => v && field.onChange(v)}
              aria-label="Activity type"
              className="self-start bg-card"
            >
              {COMPOSER_KINDS.map((k) => {
                const Icon = KIND_META[k].icon;
                return (
                  <ToggleGroupItem key={k} value={k} aria-label={KIND_META[k].label} className="gap-1.5 px-2.5">
                    <Icon />
                    {KIND_META[k].label}
                  </ToggleGroupItem>
                );
              })}
            </ToggleGroup>
          )}
        />
        <FieldGroup className="gap-3">
          <Controller
            control={form.control}
            name="subject"
            render={({ field, fieldState }) => (
              <Field data-invalid={fieldState.invalid || undefined}>
                <FieldLabel htmlFor={`composer-subject-${subjectId}`} className="sr-only">
                  Summary
                </FieldLabel>
                <Input
                  {...field}
                  id={`composer-subject-${subjectId}`}
                  placeholder={SUBJECT_PLACEHOLDER[kind]}
                  aria-invalid={fieldState.invalid || undefined}
                  className="bg-card"
                />
                <FieldError errors={[fieldState.error]} />
              </Field>
            )}
          />
          <Controller
            control={form.control}
            name="body"
            render={({ field }) => (
              <Field>
                <FieldLabel htmlFor={`composer-body-${subjectId}`} className="sr-only">
                  Details
                </FieldLabel>
                <Textarea
                  {...field}
                  id={`composer-body-${subjectId}`}
                  placeholder="Details (optional)"
                  rows={2}
                  className="min-h-14 bg-card"
                />
              </Field>
            )}
          />
          {kind === "task" ? (
            <div className="grid gap-3 sm:grid-cols-2">
              <Controller
                control={form.control}
                name="due_at"
                render={({ field, fieldState }) => (
                  <Field data-invalid={fieldState.invalid || undefined}>
                    <FieldLabel htmlFor={`composer-due-${subjectId}`}>Due</FieldLabel>
                    <Input
                      {...field}
                      id={`composer-due-${subjectId}`}
                      type="datetime-local"
                      aria-invalid={fieldState.invalid || undefined}
                      className="bg-card"
                    />
                    <FieldError errors={[fieldState.error]} />
                  </Field>
                )}
              />
              <Controller
                control={form.control}
                name="assignee_id"
                render={({ field }) => (
                  <Field>
                    <FieldLabel htmlFor={`composer-assignee-${subjectId}`}>Assignee</FieldLabel>
                    <UserSelect id={`composer-assignee-${subjectId}`} value={field.value} onChange={field.onChange} />
                  </Field>
                )}
              />
            </div>
          ) : null}
        </FieldGroup>
        {form.formState.errors.root?.server ? (
          <p className="text-sm text-destructive">{form.formState.errors.root.server.message}</p>
        ) : null}
        <div className="flex justify-end">
          <Button type="submit" size="sm" disabled={create.isPending}>
            {create.isPending ? <Spinner data-icon="inline-start" /> : null}
            {SUBMIT_LABEL[kind]}
          </Button>
        </div>
      </form>
    </Can>
  );
}

/** Timeline panel for a record: composer on top, history below. */
export function RecordActivity({ subjectType, subjectId }: { subjectType: ActivitySubjectType; subjectId: string }) {
  const activities = useActivities({ filter: { subject_type: subjectType, subject_id: subjectId }, per_page: 50 });
  return (
    <div className="flex flex-col gap-5">
      <ActivityComposer subjectType={subjectType} subjectId={subjectId} />
      <ActivityTimeline activities={activities.data?.data} loading={activities.isPending} />
    </div>
  );
}
