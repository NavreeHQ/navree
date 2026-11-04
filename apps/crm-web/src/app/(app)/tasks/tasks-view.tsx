"use client";

import { Can } from "@navree/next/abilities";
import { ListChecksIcon, PlusIcon } from "lucide-react";
import Link from "next/link";
import { PastEndNotice } from "@/components/data-table";
import { PageHeader } from "@/components/page-header";
import { PaginationBar } from "@/components/pagination-bar";
import { Button } from "@/components/ui/button";
import { Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from "@/components/ui/empty";
import { Skeleton } from "@/components/ui/skeleton";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { UserAvatar } from "@/components/user-avatar";
import { DueLabel, subjectHref, TaskCheckbox } from "@/features/activities/components";
import { useActivities, type ActivityListParams } from "@/features/activities/queries";
import { TaskFormDialog } from "@/features/activities/task-form";
import { useCurrentUser } from "@/features/auth/queries";
import { useUrlState } from "@/hooks/use-url-state";
import type { Activity } from "@/lib/api/types";
import { formatDateTime } from "@/lib/format";
import { cn } from "@/lib/utils";

const TABS = [
  { value: "overdue", label: "Overdue", empty: "Nothing overdue. Nice work." },
  { value: "today", label: "Today", empty: "Nothing due today. Add a task or check what's coming up." },
  { value: "upcoming", label: "Upcoming", empty: "Nothing scheduled after today." },
  { value: "completed", label: "Completed", empty: "Completed tasks will be listed here." },
] as const;
type Tab = (typeof TABS)[number]["value"];

function paramsFor(tab: Tab, assignee: string | undefined): ActivityListParams {
  return tab === "completed"
    ? { filter: { kind: "task", assignee_id: assignee, completed: true } }
    : { filter: { kind: "task", assignee_id: assignee, due: tab, completed: false } };
}

function TabCount({ tab, assignee, active }: { tab: Tab; assignee: string | undefined; active: boolean }) {
  const count = useActivities({ ...paramsFor(tab, assignee), per_page: 1 });
  const total = count.data?.meta.total;
  if (total === undefined) return null;
  return (
    <span
      className={cn(
        "font-mono text-xs leading-4",
        tab === "overdue" && total > 0 ? "text-destructive" : active ? "text-fg-2" : "text-fg-3",
      )}
    >
      {total}
    </span>
  );
}

function TaskRow({ task, showAssignee }: { task: Activity; showAssignee: boolean }) {
  const done = Boolean(task.completed_at);
  return (
    <li className="flex items-start gap-3 px-4 py-2.5 transition-colors hover:bg-ink-2/50" data-testid="task-row">
      <span className="pt-0.5">
        <TaskCheckbox activity={task} />
      </span>
      <div className="flex min-w-0 flex-1 flex-col gap-1">
        <span className={cn("text-sm leading-snug font-medium text-fg", done && "text-fg-3 line-through")}>
          {task.subject}
        </span>
        {task.body ? <span className="line-clamp-1 text-[0.8125rem] text-fg-2">{task.body}</span> : null}
        <span className="flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-fg-3">
          {done ? (
            <span className="figures">Completed {formatDateTime(task.completed_at)}</span>
          ) : (
            <DueLabel activity={task} />
          )}
          <Link href={subjectHref(task.subject_type, task.subject_id)} className="record-link">
            {task.subject_label}
          </Link>
        </span>
      </div>
      {showAssignee && task.assignee ? (
        <span className="hidden items-center gap-2 text-xs text-fg-2 sm:flex">
          <UserAvatar name={task.assignee.name} id={task.assignee.id} size="xs" />
          {task.assignee.name}
        </span>
      ) : null}
    </li>
  );
}

export function TasksView() {
  const url = useUrlState();
  const me = useCurrentUser();
  const rawTab = url.get("tab");
  const tab: Tab = TABS.some((t) => t.value === rawTab) ? (rawTab as Tab) : "today";
  const everyone = url.get("scope") === "everyone";
  const assignee = everyone ? undefined : me.data?.id;
  const list = useActivities({ ...paramsFor(tab, assignee), page: url.page, per_page: 25 });
  const current = TABS.find((t) => t.value === tab)!;

  return (
    <div className="flex flex-col">
      <PageHeader
        title="Tasks"
        description="Follow-ups across your deals, companies and contacts."
        actions={
          <Can I="create" a="Activity">
            <TaskFormDialog
              openOnCreateIntent
              trigger={
                <Button>
                  <PlusIcon data-icon="inline-start" />
                  New task
                </Button>
              }
            />
          </Can>
        }
      />
      <div className="mb-4 flex flex-wrap items-center justify-between gap-3">
        <Tabs
          value={tab}
          onValueChange={(v) => url.set({ tab: v })}
          className="-mx-4 max-w-[calc(100%+2rem)] scroll-thin overflow-x-auto px-4 pb-1 sm:mx-0 sm:max-w-full sm:px-0 sm:pb-0"
        >
          <TabsList>
            {TABS.map((t) => (
              <TabsTrigger key={t.value} value={t.value} className="gap-1.5">
                {t.label}
                <TabCount tab={t.value} assignee={assignee} active={t.value === tab} />
              </TabsTrigger>
            ))}
          </TabsList>
        </Tabs>
        <ToggleGroup
          type="single"
          variant="outline"
          size="sm"
          value={everyone ? "everyone" : "mine"}
          onValueChange={(v) => v && url.set({ scope: v === "everyone" ? "everyone" : undefined })}
          aria-label="Whose tasks"
          className="bg-ink-1"
        >
          <ToggleGroupItem value="mine" className="px-3">
            Assigned to me
          </ToggleGroupItem>
          <ToggleGroupItem value="everyone" className="px-3">
            Everyone
          </ToggleGroupItem>
        </ToggleGroup>
      </div>
      <section aria-label={`${current.label} tasks`} className="surface overflow-hidden">
        {list.isPending ? (
          <div className="flex flex-col divide-y">
            {Array.from({ length: 5 }, (_, i) => (
              <div key={i} className="flex items-start gap-3 px-4 py-2.5" style={{ opacity: 1 - i * 0.15 }}>
                <Skeleton className="size-4.5 rounded-sm" />
                <div className="flex flex-1 flex-col gap-2">
                  <Skeleton className="h-3.5 w-1/2" />
                  <Skeleton className="h-3 w-1/3" />
                </div>
              </div>
            ))}
          </div>
        ) : list.data?.data.length ? (
          <ul className="divide-y">
            {list.data.data.map((task) => (
              <TaskRow key={task.id} task={task} showAssignee={everyone} />
            ))}
          </ul>
        ) : list.data && list.data.meta.total > 0 ? (
          <PastEndNotice noun="tasks" total={list.data.meta.total} onFirstPage={() => url.set({ page: undefined })} />
        ) : (
          <Empty className="py-12">
            <EmptyHeader>
              <EmptyMedia variant="icon">
                <ListChecksIcon />
              </EmptyMedia>
              <EmptyTitle>No {current.label.toLowerCase()} tasks</EmptyTitle>
              <EmptyDescription>{current.empty}</EmptyDescription>
            </EmptyHeader>
          </Empty>
        )}
        <PaginationBar meta={list.data?.meta} onPageChange={(page) => url.set({ page })} noun="tasks" />
      </section>
    </div>
  );
}
