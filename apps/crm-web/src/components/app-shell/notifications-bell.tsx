"use client";

import { BellIcon, CheckCheckIcon } from "lucide-react";
import { useRouter } from "next/navigation";
import { useState } from "react";
import { Button } from "@/components/ui/button";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Skeleton } from "@/components/ui/skeleton";
import {
  useMarkAllNotificationsRead,
  useMarkNotificationRead,
  useNotifications,
  useUnreadCount,
} from "@/features/notifications/queries";
import type { AppNotification } from "@/lib/api/types";
import { formatRelative } from "@/lib/format";
import { cn } from "@/lib/utils";

/**
 * Title and message for a notification. Navree sends `data: {title, message, url, deal_id?}`
 * (spec §9.7); fall back to the type for anything else.
 */
export function notificationParts(n: Pick<AppNotification, "type" | "data">): { title: string; message?: string } {
  const data = n.data ?? {};
  const text = (key: string) => (typeof data[key] === "string" && data[key] ? (data[key] as string) : undefined);
  const fallback = n.type.replace(/[._]/g, " ");
  const title = text("title");
  const message = text("message") ?? text("body");
  if (title) return { title, message };
  return { title: message ?? fallback.charAt(0).toUpperCase() + fallback.slice(1) };
}

export function notificationHref(n: AppNotification): string | null {
  const data = n.data ?? {};
  if (typeof data.url === "string" && data.url.startsWith("/")) return data.url;
  if (typeof data.deal_id === "string") return `/deals/${data.deal_id}`;
  if (typeof data.company_id === "string") return `/companies/${data.company_id}`;
  if (typeof data.contact_id === "string") return `/contacts/${data.contact_id}`;
  return null;
}

export function NotificationsBell() {
  const [open, setOpen] = useState(false);
  const router = useRouter();
  const unread = useUnreadCount();
  const list = useNotifications(open);
  const markRead = useMarkNotificationRead();
  const markAll = useMarkAllNotificationsRead();
  const count = unread.data?.count ?? 0;

  const openNotification = (n: AppNotification) => {
    if (!n.read_at) markRead.mutate(n.id);
    const href = notificationHref(n);
    setOpen(false);
    if (href) router.push(href);
  };

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <Button
          variant="ghost"
          size="icon"
          className="relative"
          aria-label={count > 0 ? `Notifications, ${count} unread` : "Notifications"}
        >
          <BellIcon />
          {count > 0 ? (
            <span
              data-testid="unread-count"
              className="absolute -top-0.5 -right-0.5 flex h-4 min-w-4 items-center justify-center rounded-full bg-primary px-1 text-[10px] leading-none font-semibold text-primary-foreground figures ring-2 ring-background"
            >
              {count > 99 ? "99+" : count}
            </span>
          ) : null}
        </Button>
      </PopoverTrigger>
      <PopoverContent align="end" className="w-96 gap-0 p-0">
        <div className="flex items-center justify-between border-b px-4 py-2.5">
          <h2 className="text-sm font-semibold">Notifications</h2>
          <Button
            variant="ghost"
            size="xs"
            disabled={count === 0 || markAll.isPending}
            onClick={() => markAll.mutate()}
          >
            <CheckCheckIcon data-icon="inline-start" />
            Mark all read
          </Button>
        </div>
        <ScrollArea className="max-h-96">
          {list.isPending ? (
            <div className="flex flex-col gap-3 p-4">
              <Skeleton className="h-4 w-3/4" />
              <Skeleton className="h-4 w-2/3" />
              <Skeleton className="h-4 w-1/2" />
            </div>
          ) : list.data && list.data.data.length > 0 ? (
            <ul className="divide-y">
              {list.data.data.map((n) => (
                <li key={n.id}>
                  <button
                    type="button"
                    onClick={() => openNotification(n)}
                    className="flex w-full items-start gap-3 px-4 py-3 text-left text-sm hover:bg-muted/60 focus-visible:bg-muted/60 focus-visible:outline-none"
                  >
                    <span
                      aria-hidden="true"
                      className={cn(
                        "mt-1.5 size-1.5 shrink-0 rounded-full",
                        n.read_at ? "bg-transparent" : "bg-primary",
                      )}
                    />
                    <span className="flex min-w-0 flex-col gap-0.5">
                      <span className={cn("leading-snug", !n.read_at && "font-medium")}>
                        {notificationParts(n).title}
                      </span>
                      {notificationParts(n).message ? (
                        <span className="text-[13px] leading-snug text-muted-foreground">
                          {notificationParts(n).message}
                        </span>
                      ) : null}
                      <span className="text-xs text-muted-foreground">
                        {formatRelative(n.created_at)}
                        {n.read_at ? "" : <span className="sr-only">, unread</span>}
                      </span>
                    </span>
                  </button>
                </li>
              ))}
            </ul>
          ) : (
            <p className="px-4 py-8 text-center text-sm text-muted-foreground">
              Nothing new. Deal assignments, wins and task reminders show up here.
            </p>
          )}
        </ScrollArea>
      </PopoverContent>
    </Popover>
  );
}
