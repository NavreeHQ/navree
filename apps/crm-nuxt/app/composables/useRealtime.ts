import { useChannel, useRealtimeState } from "@navree/nuxt/realtime";
import { useQueryClient } from "@tanstack/vue-query";
import { toast } from "vue-sonner";
import { notificationParts } from "@/features/notifications/queries";
import { keys } from "@/lib/api/keys";
import {
  DEALS_CHANNEL,
  userChannel,
  type DealUpdatedPayload,
  type NotificationCreatedPayload,
} from "@/lib/realtime/events";
import { useUiStore, type RealtimeState } from "@/stores/ui";

/** The module's socket states, as the top bar shows them. */
function barState(state: string): RealtimeState {
  switch (state) {
    case "connected":
      return "live";
    case "initialized":
    case "connecting":
      return "connecting";
    case "unavailable":
    case "failed":
      return "unavailable";
    default:
      return "off";
  }
}

/**
 * Listens on `private-crm.deals` and `private-users.{id}` for the signed-in
 * user, through the `@navree/nuxt` module's connection (the pusher-js socket,
 * or the mock bus with the mock API). A `deal.updated` refreshes the board,
 * lists, the deal and the dashboard, and flashes the card that changed; a
 * `notification.created` refreshes the bell and shows a toast. Optional by
 * design: without a socket, queries still refetch on focus and the unread
 * count polls.
 */
export function useRealtime(userId: string) {
  const ui = useUiStore();
  const queryClient = useQueryClient();

  useChannel<DealUpdatedPayload | undefined>(DEALS_CHANNEL, "deal.updated", (payload) => {
    const deal = payload?.deal;
    if (deal?.id) {
      queryClient.setQueryData(keys.deals.detail(deal.id), deal);
      ui.flash(deal.id);
    }
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

  const state = useRealtimeState();
  watch(state, (next) => (ui.realtime = barState(next)), { immediate: true });
  onBeforeUnmount(() => {
    ui.realtime = "off";
  });
}
