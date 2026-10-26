"use client";

import { LockIcon, ScrollTextIcon } from "lucide-react";
import Link from "next/link";
import { useState } from "react";
import { SkeletonRows } from "@/components/data-table";
import { OptionFilter, OwnerFilter } from "@/components/filters";
import { PaginationBar } from "@/components/pagination-bar";
import { Button } from "@/components/ui/button";
import { Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from "@/components/ui/empty";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { ACTION_OPTIONS, entryLabel } from "@/features/audit/actions";
import { useAuditLog } from "@/features/audit/queries";
import { useCurrentUser } from "@/features/auth/queries";
import { useUsers } from "@/features/users/queries";
import { useUrlState } from "@/hooks/use-url-state";
import { isApiError } from "@/lib/api/errors";
import type { AuditEntry } from "@/lib/api/types";
import { formatDateTime } from "@/lib/format";

const SUBJECT_TYPES = ["Company", "Contact", "Deal", "Activity", "User"].map((s) => ({ value: s, label: s }));

function recordHref(type: string | null, id: string | null): string | null {
  if (!type || !id) return null;
  const base = { Company: "/companies", Contact: "/contacts", Deal: "/deals" }[type];
  return base ? `${base}/${id}` : null;
}

function show(value: unknown): string {
  if (value === null || value === undefined || value === "") return "empty";
  if (Array.isArray(value)) return value.length ? value.join(", ") : "none";
  if (typeof value === "object") return JSON.stringify(value);
  return String(value);
}

/** "stage: lead → proposal" lines from old/new values. */
function Changes({ entry }: { entry: AuditEntry }) {
  const [expanded, setExpanded] = useState(false);
  const keys = Array.from(new Set([...Object.keys(entry.old_values ?? {}), ...Object.keys(entry.new_values ?? {})]));
  if (keys.length === 0) return <span className="text-muted-foreground">No field changes</span>;
  const visible = expanded ? keys : keys.slice(0, 2);
  return (
    <ul className="flex flex-col gap-0.5">
      {visible.map((key) => {
        const before = entry.old_values?.[key];
        const after = entry.new_values?.[key];
        return (
          <li key={key} className="max-w-md truncate" title={`${key}: ${show(before)} → ${show(after)}`}>
            <span className="font-mono text-xs text-fg-3">{key}: </span>
            {entry.old_values && key in entry.old_values ? (
              <>
                <span className="text-muted-foreground line-through decoration-muted-foreground/50">
                  {show(before)}
                </span>
                <span aria-label="changed to"> → </span>
              </>
            ) : null}
            <span>{entry.new_values && key in entry.new_values ? show(after) : "removed"}</span>
          </li>
        );
      })}
      {keys.length > 2 ? (
        <li>
          <Button variant="link" size="xs" className="h-auto px-0" onClick={() => setExpanded((e) => !e)}>
            {expanded ? "Show less" : `${keys.length - 2} more`}
          </Button>
        </li>
      ) : null}
    </ul>
  );
}

export function AuditLog() {
  const url = useUrlState();
  const me = useCurrentUser();
  const isAdmin = Boolean(me.data?.roles.includes("admin"));
  const users = useUsers();
  const params = {
    actor_id: url.get("actor"),
    subject_type: url.get("subject_type"),
    action: url.get("action"),
    page: url.page,
    per_page: 25,
  };
  const audit = useAuditLog(params, isAdmin);
  const names = new Map(users.data?.data.map((u) => [u.id, u.name]));
  const forbidden = !isAdmin || (isApiError(audit.error) && audit.error.status === 403);

  if (forbidden) {
    return (
      <Empty className="border">
        <EmptyHeader>
          <EmptyMedia variant="icon">
            <LockIcon />
          </EmptyMedia>
          <EmptyTitle>Only admins can view the audit log</EmptyTitle>
          <EmptyDescription>The log records who changed what, for security reviews.</EmptyDescription>
        </EmptyHeader>
      </Empty>
    );
  }

  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-col gap-1">
        <h2 className="font-display text-display-sm font-semibold text-fg">Audit log</h2>
        <p className="text-sm text-muted-foreground">Every sign-in and record change, newest first.</p>
      </div>
      <div className="surface overflow-hidden">
        <div className="flex flex-wrap items-center gap-2 border-b p-3">
          <OwnerFilter label="Person" value={params.actor_id} onChange={(actor) => url.set({ actor })} />
          <OptionFilter
            value={params.subject_type}
            onChange={(subject_type) => url.set({ subject_type })}
            options={SUBJECT_TYPES}
            anyLabel="Any record type"
            ariaLabel="Filter by record type"
          />
          <OptionFilter
            value={params.action}
            onChange={(action) => url.set({ action })}
            options={ACTION_OPTIONS}
            anyLabel="Any action"
            ariaLabel="Filter by action"
          />
        </div>
        <Table className="text-[13px]">
          <TableHeader>
            <TableRow className="bg-ink-1 hover:bg-ink-1 [&>th]:h-9">
              <TableHead className="pl-4">When</TableHead>
              <TableHead>Who</TableHead>
              <TableHead>Action</TableHead>
              <TableHead>Record</TableHead>
              <TableHead>Changes</TableHead>
              <TableHead className="pr-4">Request</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {audit.isPending ? (
              <SkeletonRows columns={6} />
            ) : audit.data?.data.length ? (
              audit.data.data.map((entry) => {
                const href = recordHref(entry.subject_type, entry.subject_id);
                return (
                  <TableRow key={entry.id} className="align-top" data-testid="audit-row">
                    <TableCell className="py-2.5 pl-4 whitespace-nowrap text-fg-2 figures">
                      {formatDateTime(entry.created_at)}
                    </TableCell>
                    <TableCell className="py-2.5 whitespace-nowrap">
                      {entry.actor_id ? (
                        (names.get(entry.actor_id) ?? "Unknown user")
                      ) : (
                        <span className="text-muted-foreground">System</span>
                      )}
                    </TableCell>
                    <TableCell className="py-2.5 whitespace-nowrap">
                      <span className="font-medium" title={entry.action}>
                        {entryLabel(entry)}
                      </span>
                      <span className="block font-mono text-xs text-fg-3">{entry.kind}</span>
                    </TableCell>
                    <TableCell className="py-2.5 whitespace-nowrap">
                      {entry.subject_type ? (
                        <>
                          <span>{entry.subject_type}</span>
                          {entry.subject_id ? (
                            href ? (
                              <Link
                                href={href}
                                className="record-link block font-mono text-xs"
                                title={entry.subject_id}
                              >
                                {entry.subject_id.slice(0, 8)}
                              </Link>
                            ) : (
                              <span className="block font-mono text-xs text-fg-3" title={entry.subject_id}>
                                {entry.subject_id.slice(0, 8)}
                              </span>
                            )
                          ) : null}
                        </>
                      ) : (
                        <span className="text-muted-foreground">None</span>
                      )}
                    </TableCell>
                    <TableCell className="py-2.5 whitespace-normal">
                      <Changes entry={entry} />
                    </TableCell>
                    <TableCell
                      className="py-2.5 pr-4 font-mono text-xs text-fg-3"
                      title={entry.request_id ?? undefined}
                    >
                      {entry.request_id ? entry.request_id.slice(-8) : "None"}
                    </TableCell>
                  </TableRow>
                );
              })
            ) : (
              <TableRow className="hover:bg-transparent">
                <TableCell colSpan={6}>
                  <Empty className="py-12">
                    <EmptyHeader>
                      <EmptyMedia variant="icon">
                        <ScrollTextIcon />
                      </EmptyMedia>
                      <EmptyTitle>No entries match</EmptyTitle>
                      <EmptyDescription>Change the filters to see more of the log.</EmptyDescription>
                    </EmptyHeader>
                  </Empty>
                </TableCell>
              </TableRow>
            )}
          </TableBody>
        </Table>
        <PaginationBar meta={audit.data?.meta} onPageChange={(page) => url.set({ page })} noun="entries" />
      </div>
    </div>
  );
}
