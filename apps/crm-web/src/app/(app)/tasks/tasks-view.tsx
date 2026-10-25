"use client";

import { Can } from "@casl/react";
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
    ? { kind: "task", assignee_id: assignee, completed: true }
    : { kind: "task", assignee_id: assignee, due: tab, completed: false };
}

function TabCount({ tab, assignee, active }: { tab: Tab; assignee: string | undefined; active: boolean }) {
  const count = useActivities({ ...paramsFor(tab, assignee), per_page: 1 });
  const total = count.data?.meta.total;
  if (total === undefined) return null;
  return (
    <span
      className={cn(
        "rounded-full px-1.5 text-[11px] leading-4 figures",
        tab === "overdue" && total > 0 ? "bg-destructive/10 text-destructive" : "bg-muted text-muted-foreground",
        active && tab !== "overdue" && "bg-background",
      )}
    >
      {total}
    </span>
  );
}

function TaskRow({ task, showAssignee }: { task: Activity; showAssignee: boolean }) {
  const done = Boolean(task.completed_at);
  return (
    <li className="flex items-start gap-3 px-4 py-3" data-testid="task-row">
      <span className="pt-0.5">
        <TaskCheckbox activity={task} />
      </span>
      <div className="flex min-w-0 flex-1 flex-col gap-1">
        <span className={cn("text-sm leading-snug font-medium", done && "text-muted-foreground line-through")}>
          {task.subject}
        </span>
        {task.body ? <span className="line-clamp-1 text-[13px] text-muted-foreground">{task.body}</span> : null}
        <span className="flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-muted-foreground">
          {done ? (
            <span className="figures">Completed {formatDateTime(task.completed_at)}</span>
          ) : (
            <DueLabel activity={task} />
          )}
          <Link href={subjectHref(task.subject_type, task.subject_id)} className="text-primary hover:underline">
            {task.subject_label}
          </Link>
        </span>
      </div>
      {showAssignee && task.assignee ? (
        <span className="hidden items-center gap-2 text-xs text-muted-foreground sm:flex">
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
      <div className="mb-3 flex flex-wrap items-center justify-between gap-3">
        <Tabs value={tab} onValueChange={(v) => url.set({ tab: v })}>
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
          className="bg-card"
        >
          <ToggleGroupItem value="mine" className="px-3">
            Assigned to me
          </ToggleGroupItem>
          <ToggleGroupItem value="everyone" className="px-3">
            Everyone
          </ToggleGroupItem>
        </ToggleGroup>
      </div>
      <section aria-label={`${current.label} tasks`} className="overflow-hidden rounded-lg border bg-card">
        {list.isPending ? (
          <div className="flex flex-col gap-3 p-4">
            {Array.from({ length: 5 }, (_, i) => (
              <Skeleton key={i} className="h-10 w-full" />
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
          <Empty className="py-14">
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
