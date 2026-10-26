"use client";

import { useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";
import { toast } from "sonner";
import { notificationParts } from "@/components/app-shell/notifications-bell";
import { keys } from "@/lib/api/keys";
import { env } from "@/lib/env";
import type { RealtimeConnection } from "@/lib/realtime/connection";
import {
  DEALS_CHANNEL,
  userChannel,
  type DealUpdatedPayload,
  type NotificationCreatedPayload,
} from "@/lib/realtime/events";

/**
 * Subscribes to `private-crm.deals` and `private-users.{id}`. Optional by
 * design: if the socket can't connect, queries still refetch on focus and the
 * unread count polls.
 */
export function RealtimeBridge({ userId }: { userId: string }) {
  const queryClient = useQueryClient();

  useEffect(() => {
    if (!env.mocks && !env.realtime) return;
    let connection: RealtimeConnection | null = null;
    let cancelled = false;

    const subscribe = (conn: RealtimeConnection) => {
      conn.listen(DEALS_CHANNEL, "deal.updated", (payload) => {
        const deal = (payload as DealUpdatedPayload | undefined)?.deal;
        if (deal?.id) void queryClient.invalidateQueries({ queryKey: keys.deals.detail(deal.id) });
        void queryClient.invalidateQueries({ queryKey: [...keys.deals.all, "board"] });
        void queryClient.invalidateQueries({ queryKey: [...keys.deals.all, "list"] });
        void queryClient.invalidateQueries({ queryKey: keys.dashboard });
      });
      conn.listen(userChannel(userId), "notification.created", (payload) => {
        void queryClient.invalidateQueries({ queryKey: keys.notifications.all });
        const n = payload as NotificationCreatedPayload | undefined;
        if (!n || typeof n !== "object") return;
        const data = ("data" in n && n.data ? n.data : n) as Record<string, unknown>;
        const type = "type" in n && typeof n.type === "string" ? n.type : "notification";
        const { title, message } = notificationParts({ type, data });
        toast(title, { description: message });
      });
    };

    const start = async () => {
      try {
        const conn = env.mocks
          ? (await import("@/lib/realtime/mock-bus")).connectMockBus()
          : (await import("@/lib/realtime/echo")).connectEcho();
        if (cancelled) {
          conn.disconnect();
          return;
        }
        connection = conn;
        subscribe(conn);
      } catch (error) {
        console.info("[navis] Real-time disabled:", error);
      }
    };
    void start();

    return () => {
      cancelled = true;
      connection?.disconnect();
    };
  }, [queryClient, userId]);

  return null;
}
