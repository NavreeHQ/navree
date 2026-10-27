import { useQueryClient } from "@tanstack/vue-query";
import { toast } from "vue-sonner";
import { notificationParts } from "@/features/notifications/queries";
import { keys } from "@/lib/api/keys";
import type { RealtimeConnection } from "@/lib/realtime/connection";
import {
  DEALS_CHANNEL,
  userChannel,
  type DealUpdatedPayload,
  type NotificationCreatedPayload,
} from "@/lib/realtime/events";
import { useUiStore } from "@/stores/ui";

/**
 * Subscribes to `private-crm.deals` and `private-users.{id}` for the signed-in
 * user. A `deal.updated` refreshes the board, lists, the deal and the
 * dashboard, and flashes the card that changed; a `notification.created`
 * refreshes the bell and shows a toast. Optional by design: without a socket,
 * queries still refetch on focus and the unread count polls.
 */
export function useRealtime(userId: string) {
  const config = useCrmConfig();
  const ui = useUiStore();
  const queryClient = useQueryClient();
  let connection: RealtimeConnection | null = null;
  let cancelled = false;

  function subscribe(conn: RealtimeConnection) {
    conn.listen(DEALS_CHANNEL, "deal.updated", (payload) => {
      const deal = (payload as DealUpdatedPayload | undefined)?.deal;
      if (deal?.id) {
        queryClient.setQueryData(keys.deals.detail(deal.id), deal);
        ui.flash(deal.id);
      }
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
  }

  onMounted(async () => {
    if (!config.mocks && !config.realtime) {
      ui.realtime = "off";
      return;
    }
    ui.realtime = "connecting";
    try {
      const conn = config.mocks
        ? (await import("@/lib/realtime/mock-bus")).connectMockBus()
        : (await import("@/lib/realtime/pusher")).connectPusher(
            { key: config.pusherKey, wsHost: config.wsHost, wsPort: config.wsPort },
            (state) => (ui.realtime = state),
          );
      if (cancelled) {
        conn.disconnect();
        return;
      }
      if (config.mocks) ui.realtime = "live";
      connection = conn;
      subscribe(conn);
    } catch (error) {
      ui.realtime = "unavailable";
      console.info("[navree] Real-time disabled:", error);
    }
  });

  onBeforeUnmount(() => {
    cancelled = true;
    connection?.disconnect();
    ui.realtime = "off";
  });
}
