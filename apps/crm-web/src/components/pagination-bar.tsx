"use client";

import { ChevronLeftIcon, ChevronRightIcon } from "lucide-react";
import { Button } from "@/components/ui/button";
import type { PageMeta } from "@/lib/api/types";
import { formatNumber } from "@/lib/format";
import { cn } from "@/lib/utils";

/** "21–40 of 52" with previous/next. Pages are 1-based like the API. */
export function PaginationBar({
  meta,
  onPageChange,
  noun,
}: {
  meta: PageMeta | undefined;
  onPageChange: (page: number) => void;
  noun: string;
}) {
  if (!meta || meta.total === 0) return null;
  const first = (meta.current_page - 1) * meta.per_page + 1;
  const last = Math.min(meta.total, meta.current_page * meta.per_page);
  const pastEnd = meta.current_page > meta.last_page;
  return (
    <nav aria-label="Pagination" className="flex items-center justify-between gap-3 border-t px-4 py-2.5 text-sm">
      <p className="min-w-0 text-fg-3 figures">
        {pastEnd
          ? `${formatNumber(meta.total)} ${noun} on ${formatNumber(meta.last_page)} ${meta.last_page === 1 ? "page" : "pages"}`
          : `${first === last ? formatNumber(first) : `${formatNumber(first)}–${formatNumber(last)}`} of ${formatNumber(meta.total)} ${noun}`}
      </p>
      <div className={cn("flex shrink-0 items-center gap-1", meta.last_page <= 1 && !pastEnd && "hidden")}>
        <Button
          variant="ghost"
          size="sm"
          disabled={meta.current_page <= 1}
          onClick={() => onPageChange(Math.min(meta.current_page - 1, meta.last_page))}
        >
          <ChevronLeftIcon data-icon="inline-start" />
          <span className="max-sm:sr-only">Previous</span>
        </Button>
        <span className="px-1 whitespace-nowrap text-fg-3 figures">
          {meta.current_page} / {meta.last_page}
        </span>
        <Button
          variant="ghost"
          size="sm"
          disabled={meta.current_page >= meta.last_page}
          onClick={() => onPageChange(meta.current_page + 1)}
        >
          <span className="max-sm:sr-only">Next</span>
          <ChevronRightIcon data-icon="inline-end" />
        </Button>
      </div>
    </nav>
  );
}
