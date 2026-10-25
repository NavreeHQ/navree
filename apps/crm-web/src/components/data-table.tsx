"use client";

import { ArrowDownIcon, ArrowUpIcon, ChevronsUpDownIcon, SearchXIcon } from "lucide-react";
import type { ReactNode } from "react";
import { Button } from "@/components/ui/button";
import { Empty, EmptyContent, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from "@/components/ui/empty";
import { Skeleton } from "@/components/ui/skeleton";
import { TableCell, TableHead, TableRow } from "@/components/ui/table";
import { cn } from "@/lib/utils";

/** Column header that toggles `field` / `-field` sorting. */
export function SortableHead({
  label,
  field,
  sort,
  onSort,
  className,
}: {
  label: string;
  field: string;
  sort: string | undefined;
  onSort: (sort: string) => void;
  className?: string;
}) {
  const active = sort === field || sort === `-${field}`;
  const desc = sort === `-${field}`;
  const Icon = !active ? ChevronsUpDownIcon : desc ? ArrowDownIcon : ArrowUpIcon;
  return (
    <TableHead className={className} aria-sort={active ? (desc ? "descending" : "ascending") : "none"}>
      <button
        type="button"
        onClick={() => onSort(active && !desc ? `-${field}` : field)}
        className={cn(
          "-ml-1 inline-flex items-center gap-1 rounded px-1 py-0.5 hover:text-foreground focus-visible:outline-2",
          active ? "text-foreground" : "",
        )}
      >
        {label}
        <Icon className={cn("size-3.5", !active && "opacity-40")} aria-hidden="true" />
      </button>
    </TableHead>
  );
}

export function SkeletonRows({ columns, rows = 8 }: { columns: number; rows?: number }) {
  return (
    <>
      {Array.from({ length: rows }, (_, r) => (
        <TableRow key={r} className="hover:bg-transparent">
          {Array.from({ length: columns }, (_, c) => (
            <TableCell key={c} className="py-3">
              <Skeleton className={cn("h-4", c === 0 ? "w-40" : "w-20")} />
            </TableCell>
          ))}
        </TableRow>
      ))}
    </>
  );
}

/** Shown when a list is empty: different copy for "nothing yet" and "nothing matches". */
export function ListEmpty({
  filtered,
  noun,
  onClear,
  action,
  icon,
}: {
  filtered: boolean;
  noun: string;
  onClear: () => void;
  action?: ReactNode;
  icon: ReactNode;
}) {
  return (
    <Empty className="py-14">
      <EmptyHeader>
        <EmptyMedia variant="icon">{filtered ? <SearchXIcon /> : icon}</EmptyMedia>
        <EmptyTitle>{filtered ? `No ${noun} match these filters` : `No ${noun} yet`}</EmptyTitle>
        <EmptyDescription>
          {filtered
            ? "Try a different search, or clear the filters to see everything."
            : `Add the first one to get started.`}
        </EmptyDescription>
      </EmptyHeader>
      <EmptyContent>
        {filtered ? (
          <Button variant="outline" size="sm" onClick={onClear}>
            Clear filters
          </Button>
        ) : (
          action
        )}
      </EmptyContent>
    </Empty>
  );
}

/** The bordered surface that holds a filter bar, table and pagination. */
export function TablePanel({ children, className }: { children: ReactNode; className?: string }) {
  return <div className={cn("overflow-hidden rounded-lg border bg-card", className)}>{children}</div>;
}
