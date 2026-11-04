"use client";

import { useChannel } from "@navree/next/realtime";
import { useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { notificationParts } from "@/components/app-shell/notifications-bell";
import { keys } from "@/lib/api/keys";
import {
  DEALS_CHANNEL,
  userChannel,
  type DealUpdatedPayload,
  type NotificationCreatedPayload,
} from "@/lib/realtime/events";

/**
 * Refreshes what the screens show when the API broadcasts: `private-crm.deals`
 * (`deal.updated`) and `private-users.{id}` (`notification.created`). It sits
 * under the shell's `RealtimeProvider`. Optional by design: if the socket can't
 * connect, queries still refetch on focus and the unread count polls.
 */
export function RealtimeBridge({ userId }: { userId: string }) {
  const queryClient = useQueryClient();

  useChannel<DealUpdatedPayload | undefined>(DEALS_CHANNEL, "deal.updated", (payload) => {
    const deal = payload?.deal;
    if (deal?.id) void queryClient.invalidateQueries({ queryKey: keys.deals.detail(deal.id) });
    void queryClient.invalidateQueries({ queryKey: [...keys.deals.all, "board"] });
    void queryClient.invalidateQueries({ queryKey: [...keys.deals.all, "list"] });
    void queryClient.invalidateQueries({ queryKey: keys.dashboard });
  });

  useChannel<NotificationCreatedPayload | undefined>(userChannel(userId), "notification.created", (n) => {
    void queryClient.invalidateQueries({ queryKey: keys.notifications.all });
    if (!n || typeof n !== "object") return;
    const data = ("data" in n && n.data ? n.data : n) as Record<string, unknown>;
    const type = "type" in n && typeof n.type === "string" ? n.type : "notification";
    const { title, message } = notificationParts({ type, data });
    toast(title, { description: message });
  });

  return null;
}
