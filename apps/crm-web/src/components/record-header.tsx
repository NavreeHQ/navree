"use client";

import { ChevronLeftIcon, FileQuestionIcon } from "lucide-react";
import Link from "next/link";
import type { ReactNode } from "react";
import { Button } from "@/components/ui/button";
import { Empty, EmptyContent, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from "@/components/ui/empty";
import { Skeleton } from "@/components/ui/skeleton";
import { isApiError } from "@/lib/api/errors";

export function RecordHeader({
  back,
  title,
  meta,
  actions,
  eyebrow,
}: {
  back: { href: string; label: string };
  title: ReactNode;
  meta?: ReactNode;
  actions?: ReactNode;
  eyebrow?: ReactNode;
}) {
  return (
    <header className="flex flex-col gap-3 pb-6">
      <Link
        href={back.href}
        className="-ml-1 inline-flex w-fit items-center gap-1 rounded px-1 text-sm text-muted-foreground hover:text-foreground"
      >
        <ChevronLeftIcon className="size-4" aria-hidden="true" />
        {back.label}
      </Link>
      <div className="flex flex-wrap items-start justify-between gap-x-6 gap-y-3">
        <div className="flex min-w-0 flex-col gap-2">
          {eyebrow}
          <h1 className="text-[28px] leading-tight font-semibold tracking-[-0.015em] break-words font-condensed">
            {title}
          </h1>
          {meta ? (
            <div className="flex flex-wrap items-center gap-x-4 gap-y-1.5 text-sm text-muted-foreground">{meta}</div>
          ) : null}
        </div>
        {actions ? <div className="flex flex-wrap items-center gap-2">{actions}</div> : null}
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
      <Icon className="size-3.5 shrink-0 opacity-70" aria-hidden="true" />
      {children}
    </span>
  );
}

export function RecordSkeleton() {
  return (
    <div className="flex flex-col gap-4" aria-busy="true">
      <Skeleton className="h-4 w-24" />
      <Skeleton className="h-8 w-72" />
      <Skeleton className="h-4 w-96" />
      <div className="mt-4 grid gap-6 xl:grid-cols-[minmax(0,1fr)_19rem]">
        <Skeleton className="h-96" />
        <Skeleton className="h-96" />
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
  return (
    <Empty className="mt-10 border">
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
          <Link href={back.href}>{back.label}</Link>
        </Button>
      </EmptyContent>
    </Empty>
  );
}
