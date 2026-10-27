"use client";

import { ChevronLeftIcon, FileQuestionIcon } from "lucide-react";
import Link from "next/link";
import type { ReactNode } from "react";
import { PageActions, useRegisterRecordTitle } from "@/components/app-shell/shell-slots";
import { Button } from "@/components/ui/button";
import { Empty, EmptyContent, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from "@/components/ui/empty";
import { Skeleton } from "@/components/ui/skeleton";
import { isApiError } from "@/lib/api/errors";

/**
 * A record's title block. The breadcrumb in the top bar names the record and
 * links back to its list; `actions` (Edit, Delete) go to the top bar from `md` up.
 */
export function RecordHeader({
  title,
  meta,
  actions,
  eyebrow,
}: {
  title: string;
  meta?: ReactNode;
  actions?: ReactNode;
  eyebrow?: ReactNode;
}) {
  useRegisterRecordTitle(title);
  return (
    <header className="flex flex-col gap-4 pb-5">
      <div className="flex flex-wrap items-start justify-between gap-x-6 gap-y-3">
        <div className="flex min-w-0 flex-col gap-2">
          {eyebrow}
          <h1 className="text-title font-semibold break-words text-fg">{title}</h1>
          {meta ? <div className="flex flex-wrap items-center gap-x-4 gap-y-1.5 text-sm text-fg-2">{meta}</div> : null}
        </div>
        {actions ? <PageActions>{actions}</PageActions> : null}
      </div>
    </header>
  );
}

/** One fact in the header meta line, with its icon. */
export function MetaItem({
  icon: Icon,
  children,
}: {
  icon: React.ComponentType<{ className?: string }>;
  children: ReactNode;
}) {
  return (
    <span className="inline-flex items-center gap-1.5">
      <Icon className="size-3.5 shrink-0 text-fg-3" aria-hidden="true" />
      {children}
    </span>
  );
}

export function RecordSkeleton() {
  return (
    <div className="flex flex-col gap-4" aria-busy="true">
      <Skeleton className="h-5 w-20" />
      <Skeleton className="h-7 w-72 max-w-full" />
      <Skeleton className="h-4 w-96 max-w-full" />
      <div className="mt-4 grid gap-6 xl:grid-cols-[minmax(0,1fr)_20rem]">
        <Skeleton className="h-96 rounded-xl" />
        <Skeleton className="h-96 rounded-xl" />
      </div>
    </div>
  );
}

export function RecordMissing({
  error,
  noun,
  back,
}: {
  error: unknown;
  noun: string;
  back: { href: string; label: string };
}) {
  const notFound = isApiError(error) && error.status === 404;
  useRegisterRecordTitle(notFound ? "Not found" : "Unavailable");
  return (
    <Empty className="surface mx-auto mt-10 w-full max-w-xl flex-none border-solid py-12">
      <EmptyHeader>
        <EmptyMedia variant="icon">
          <FileQuestionIcon />
        </EmptyMedia>
        <EmptyTitle>{notFound ? `This ${noun} doesn't exist` : `This ${noun} couldn't be loaded`}</EmptyTitle>
        <EmptyDescription>
          {notFound ? `It may have been deleted, or the link is wrong.` : "Check your connection and refresh the page."}
        </EmptyDescription>
      </EmptyHeader>
      <EmptyContent>
        <Button asChild variant="outline">
          <Link href={back.href}>
            <ChevronLeftIcon data-icon="inline-start" />
            {back.label}
          </Link>
        </Button>
      </EmptyContent>
    </Empty>
  );
}
